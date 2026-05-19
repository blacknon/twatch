// Copyright (c) 2026 Blacknon. All rights reserved.
// Use of this source code is governed by an MIT license
// that can be found in the LICENSE file.

use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, RwLock};
use std::thread;

use anyhow::Result;
use crossterm::event::{KeyEvent, MouseEvent};

use crate::runner::{CaptureFrame, FrameSource, SourceEvent, time_label, unix_timestamp_millis};
use crate::screen::{Cell, ScreenSnapshot, Style, Symbol, TermColor};

const SCROLLBACK_LINES: usize = 10_000;

pub struct PipeRunner {
    state: Arc<RwLock<TerminalState>>,
    dirty: Arc<AtomicBool>,
    last_snapshot: Option<ScreenSnapshot>,
    last_size: (u16, u16),
    update_rx: Option<Receiver<SourceEvent>>,
}

struct TerminalState {
    parser: vt100::Parser,
}

impl PipeRunner {
    pub fn from_stdin(width: u16, height: u16) -> Self {
        Self::from_reader(Box::new(std::io::stdin()), width, height)
    }

    pub(crate) fn from_reader(reader: Box<dyn Read + Send>, width: u16, height: u16) -> Self {
        let state = Arc::new(RwLock::new(TerminalState {
            parser: vt100::Parser::new(height.max(1), width.max(1), SCROLLBACK_LINES),
        }));
        let dirty = Arc::new(AtomicBool::new(true));
        let (update_tx, update_rx) = mpsc::sync_channel(2);
        start_reader_thread(state.clone(), dirty.clone(), reader, update_tx);

        Self {
            state,
            dirty,
            last_snapshot: None,
            last_size: (width.max(1), height.max(1)),
            update_rx: Some(update_rx),
        }
    }

    fn snapshot(&self) -> ScreenSnapshot {
        self.snapshot_with_scrollback(None)
    }

    fn snapshot_with_scrollback(&self, scrollback_offset: Option<usize>) -> ScreenSnapshot {
        let mut state = self.state.write().expect("terminal state poisoned");
        let previous_scrollback = state.parser.screen().scrollback();
        if let Some(offset) = scrollback_offset {
            state.parser.screen_mut().set_scrollback(offset);
        }
        let screen = state.parser.screen();
        let (rows, cols) = screen.size();
        let mut snapshot = ScreenSnapshot::new(cols, rows);
        let (cursor_row, cursor_col) = screen.cursor_position();
        snapshot.set_cursor_state(cursor_col, cursor_row, !screen.hide_cursor());
        snapshot.set_screen_mode(screen.alternate_screen(), screen.scrollback());
        snapshot.set_mouse_reporting(!matches!(
            screen.mouse_protocol_mode(),
            vt100::MouseProtocolMode::None
        ));

        for row in 0..rows {
            for col in 0..cols {
                let Some(cell) = screen.cell(row, col) else {
                    continue;
                };
                if cell.is_wide_continuation() {
                    snapshot.set_cell(col, row, Cell::blank());
                    continue;
                }
                let symbol = if cell.has_contents() {
                    cell.contents()
                } else {
                    " "
                };
                snapshot.set_cell(
                    col,
                    row,
                    Cell {
                        symbol: Symbol::from(symbol),
                        style: Style {
                            fg: map_color(cell.fgcolor()),
                            bg: map_color(cell.bgcolor()),
                            bold: cell.bold(),
                            italic: cell.italic(),
                            underline: cell.underline(),
                            inverted: cell.inverse(),
                        },
                    },
                );
            }
        }

        if scrollback_offset.is_some() {
            state
                .parser
                .screen_mut()
                .set_scrollback(previous_scrollback);
        }

        snapshot
    }
}

impl FrameSource for PipeRunner {
    fn capture(&mut self, width: u16, height: u16) -> Result<CaptureFrame> {
        self.resize(width, height)?;
        let dirty = self.dirty.swap(false, Ordering::Relaxed);
        let snapshot = if dirty || self.last_snapshot.is_none() {
            self.snapshot()
        } else {
            self.last_snapshot.clone().expect("snapshot must exist")
        };
        let raw_output = snapshot.lines().join("\n");
        let changed = self.last_snapshot.as_ref() != Some(&snapshot);
        self.last_snapshot = Some(snapshot.clone());

        Ok(CaptureFrame {
            label: time_label(),
            timestamp_unix_ms: unix_timestamp_millis(),
            snapshot,
            raw_output,
            changed,
        })
    }

    fn view_snapshot(
        &mut self,
        width: u16,
        height: u16,
        scrollback_offset: usize,
    ) -> Result<Option<ScreenSnapshot>> {
        self.resize(width, height)?;
        Ok(Some(self.snapshot_with_scrollback(Some(scrollback_offset))))
    }

    fn resize(&mut self, width: u16, height: u16) -> Result<()> {
        let size = (width.max(1), height.max(1));
        if self.last_size == size {
            return Ok(());
        }
        let mut state = self.state.write().expect("terminal state poisoned");
        state.parser.screen_mut().set_size(size.1, size.0);
        self.last_size = size;
        self.dirty.store(true, Ordering::Relaxed);
        Ok(())
    }

    fn send_key(&mut self, _key: KeyEvent) -> Result<()> {
        Ok(())
    }

    fn send_mouse(&mut self, _event: MouseEvent, _body_row_offset: u16) -> Result<bool> {
        Ok(false)
    }

    fn has_pending_update(&self) -> bool {
        self.dirty.load(Ordering::Relaxed)
    }

    fn is_event_driven(&self) -> bool {
        true
    }

    fn take_update_receiver(&mut self) -> Option<Receiver<SourceEvent>> {
        self.update_rx.take()
    }

    fn terminate(&mut self) -> Result<()> {
        Ok(())
    }
}

fn start_reader_thread(
    state: Arc<RwLock<TerminalState>>,
    dirty: Arc<AtomicBool>,
    mut reader: Box<dyn Read + Send>,
    update_tx: SyncSender<SourceEvent>,
) {
    thread::spawn(move || {
        let mut buffer = [0u8; 8192];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(count) => {
                    process_parser_bytes(&state, &buffer[..count]);
                    dirty.store(true, Ordering::Relaxed);
                    match update_tx.try_send(SourceEvent::Updated) {
                        Ok(()) | Err(TrySendError::Full(_)) => {}
                        Err(TrySendError::Disconnected(_)) => break,
                    }
                }
                Err(err) => {
                    let _ = update_tx.send(SourceEvent::ClosedWithError(err.to_string()));
                    return;
                }
            }
        }
        let _ = update_tx.send(SourceEvent::Closed);
    });
}

fn process_parser_bytes(state: &Arc<RwLock<TerminalState>>, bytes: &[u8]) {
    if bytes.is_empty() {
        return;
    }
    let mut state = state.write().expect("terminal state poisoned");
    state.parser.process(bytes);
}

fn map_color(color: vt100::Color) -> TermColor {
    match color {
        vt100::Color::Default => TermColor::Default,
        vt100::Color::Idx(index) => TermColor::Indexed(index),
        vt100::Color::Rgb(r, g, b) => TermColor::Rgb(r, g, b),
    }
}
