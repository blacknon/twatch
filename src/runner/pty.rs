use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, RwLock};
use std::thread;

use anyhow::{Context, Result};
use crossterm::event::{KeyEvent, MouseEvent};
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};

use crate::process_control;
use crate::runner::capture_hook::CaptureHook;
use crate::runner::encode::{key_to_bytes, mouse_to_bytes};
use crate::runner::{CaptureFrame, FrameSource, SourceEvent, time_label, unix_timestamp_millis};
use crate::screen::{Cell, ScreenSnapshot, Style, TermColor};

const SCROLLBACK_LINES: usize = 10_000;

pub struct PtyRunner {
    master: Box<dyn MasterPty + Send>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    child: Box<dyn Child + Send + Sync>,
    state: Arc<RwLock<TerminalState>>,
    dirty: Arc<AtomicBool>,
    capture_hook: Option<CaptureHook>,
    last_snapshot: Option<ScreenSnapshot>,
    last_size: (u16, u16),
    child_paused: bool,
    update_rx: Option<Receiver<SourceEvent>>,
}

struct TerminalState {
    parser: vt100::Parser,
}

impl PtyRunner {
    pub fn spawn(
        shell: &str,
        command: String,
        aftercommand: Option<String>,
        width: u16,
        height: u16,
    ) -> Result<Self> {
        let size = pty_size(width, height);
        let pty_system = native_pty_system();
        let pair = pty_system.openpty(size).context("failed to open PTY")?;
        let mut command_builder = build_command(shell, &command)?;
        command_builder.env("TERM", "xterm-256color");
        let child = pair
            .slave
            .spawn_command(command_builder)
            .context("failed to spawn child in PTY")?;
        drop(pair.slave);

        let reader = pair
            .master
            .try_clone_reader()
            .context("failed to clone PTY reader")?;
        let writer = pair
            .master
            .take_writer()
            .context("failed to take PTY writer")?;

        let state = Arc::new(RwLock::new(TerminalState {
            parser: vt100::Parser::new(height.max(1), width.max(1), SCROLLBACK_LINES),
        }));
        let dirty = Arc::new(AtomicBool::new(true));
        let (update_tx, update_rx) = mpsc::sync_channel(2);
        start_reader_thread(state.clone(), dirty.clone(), reader, update_tx);

        Ok(Self {
            master: pair.master,
            writer: Arc::new(Mutex::new(writer)),
            child,
            state,
            dirty,
            capture_hook: aftercommand.map(|hook| CaptureHook::new(shell, command.clone(), hook)),
            last_snapshot: None,
            last_size: (width.max(1), height.max(1)),
            child_paused: false,
            update_rx: Some(update_rx),
        })
    }

    fn snapshot(&self) -> ScreenSnapshot {
        let state = self.state.read().expect("terminal state poisoned");
        let screen = state.parser.screen();
        let (rows, cols) = screen.size();
        let mut snapshot = ScreenSnapshot::new(cols, rows);

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
                    " ".to_string()
                };
                snapshot.set_cell(
                    col,
                    row,
                    Cell {
                        symbol,
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

        snapshot
    }

    fn maybe_run_aftercommand(&self, output: &str, changed: bool) -> Result<()> {
        self.capture_hook
            .as_ref()
            .map_or(Ok(()), |hook| hook.maybe_run(output, changed))
    }
}

impl FrameSource for PtyRunner {
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
        self.maybe_run_aftercommand(&raw_output, changed)?;

        Ok(CaptureFrame {
            label: time_label(),
            timestamp_unix_ms: unix_timestamp_millis(),
            snapshot,
            raw_output,
            changed,
        })
    }

    fn resize(&mut self, width: u16, height: u16) -> Result<()> {
        let size = (width.max(1), height.max(1));
        if self.last_size == size {
            return Ok(());
        }
        self.master
            .resize(pty_size(size.0, size.1))
            .context("failed to resize PTY")?;
        let mut state = self.state.write().expect("terminal state poisoned");
        state.parser.set_size(size.1, size.0);
        self.last_size = size;
        self.dirty.store(true, Ordering::Relaxed);
        Ok(())
    }

    fn send_key(&mut self, key: KeyEvent) -> Result<()> {
        let bytes = {
            let state = self.state.read().expect("terminal state poisoned");
            key_to_bytes(key, state.parser.screen().application_cursor())
        };
        if bytes.is_empty() {
            return Ok(());
        }
        let mut writer = self.writer.lock().expect("writer poisoned");
        writer
            .write_all(&bytes)
            .context("failed to write key to PTY")?;
        writer.flush().ok();
        Ok(())
    }

    fn send_mouse(&mut self, event: MouseEvent, body_row_offset: u16) -> Result<()> {
        let encoded = {
            let state = self.state.read().expect("terminal state poisoned");
            mouse_to_bytes(
                state.parser.screen().mouse_protocol_mode(),
                state.parser.screen().mouse_protocol_encoding(),
                event,
                body_row_offset,
            )
        };
        let Some(bytes) = encoded else {
            return Ok(());
        };
        let mut writer = self.writer.lock().expect("writer poisoned");
        writer
            .write_all(&bytes)
            .context("failed to write mouse event to PTY")?;
        writer.flush().ok();
        Ok(())
    }

    fn toggle_child_pause(&mut self) -> Result<Option<bool>> {
        let pid = self
            .child
            .process_id()
            .context("PTY child process id is unavailable")?;
        if self.child_paused {
            process_control::resume_process(pid).context("failed to resume child process")?;
        } else {
            process_control::suspend_process(pid).context("failed to pause child process")?;
        }
        self.child_paused = !self.child_paused;
        Ok(Some(self.child_paused))
    }

    fn supports_child_pause(&self) -> bool {
        true
    }

    fn terminate(&mut self) -> Result<()> {
        match self.child.kill() {
            Ok(()) => Ok(()),
            Err(err) if is_child_exit_error(&err) => Ok(()),
            Err(err) => Err(err).context("failed to terminate child PTY process"),
        }
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
                    let mut state = state.write().expect("terminal state poisoned");
                    state.parser.process(&buffer[..count]);
                    dirty.store(true, Ordering::Relaxed);
                    match update_tx.try_send(SourceEvent::Updated) {
                        Ok(()) | Err(TrySendError::Full(_)) => {}
                        Err(TrySendError::Disconnected(_)) => break,
                    }
                }
                Err(_) => break,
            }
        }
        let _ = update_tx.send(SourceEvent::Closed);
    });
}

fn is_child_exit_error(err: &std::io::Error) -> bool {
    matches!(
        err.kind(),
        std::io::ErrorKind::NotFound
            | std::io::ErrorKind::BrokenPipe
            | std::io::ErrorKind::UnexpectedEof
            | std::io::ErrorKind::ConnectionAborted
            | std::io::ErrorKind::ConnectionReset
            | std::io::ErrorKind::NotConnected
    ) || matches!(err.raw_os_error(), Some(3) | Some(5))
}

fn build_command(shell: &str, command: &str) -> Result<CommandBuilder> {
    let parts = shell_words::split(shell).context("failed to parse shell command")?;
    if parts.is_empty() {
        return Ok(CommandBuilder::new(command));
    }

    let mut builder = CommandBuilder::new(&parts[0]);
    if shell.contains("{COMMAND}") {
        for arg in parts.iter().skip(1) {
            builder.arg(arg.replace("{COMMAND}", command));
        }
    } else {
        for arg in parts.iter().skip(1) {
            builder.arg(arg);
        }
        builder.arg(command);
    }
    Ok(builder)
}

fn map_color(color: vt100::Color) -> TermColor {
    match color {
        vt100::Color::Default => TermColor::Default,
        vt100::Color::Idx(index) => TermColor::Indexed(index),
        vt100::Color::Rgb(r, g, b) => TermColor::Rgb(r, g, b),
    }
}

fn pty_size(width: u16, height: u16) -> PtySize {
    PtySize {
        rows: height.max(1),
        cols: width.max(1),
        pixel_width: 0,
        pixel_height: 0,
    }
}
