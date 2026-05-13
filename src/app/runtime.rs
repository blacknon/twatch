// Copyright (c) 2026 Blacknon. All rights reserved.
// Use of this source code is governed by an MIT license
// that can be found in the LICENSE file.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event};
use ratatui::DefaultTerminal;

use super::{App, AppEvent, LoopControl};
use crate::runner::SourceEvent;
use crate::ui;

impl App {
    const EVENT_POLL_TIMEOUT: Duration = Duration::from_millis(50);

    pub fn run(mut self, terminal: DefaultTerminal) -> Result<()> {
        let (tx, rx) = mpsc::channel();
        let input_tx = tx.clone();
        std::thread::spawn(move || {
            while let Ok(event) = event::read() {
                if input_tx.send(AppEvent::Terminal(event)).is_err() {
                    break;
                }
            }
        });

        if let Some(update_rx) = self.source.take_update_receiver() {
            let update_tx = tx.clone();
            let pending_source_update = Arc::new(AtomicBool::new(false));
            let pending_source_update_worker = pending_source_update.clone();
            std::thread::spawn(move || {
                while let Ok(event) = update_rx.recv() {
                    if relay_source_event(event, &pending_source_update_worker, &update_tx).is_err()
                    {
                        break;
                    }
                }
            });
            self.run_event_loop(terminal, rx, Some(pending_source_update))
        } else {
            self.run_event_loop(terminal, rx, None)
        }?;
        drop(tx);
        Ok(())
    }

    fn process_terminal_event(&mut self, event: Event) -> Result<LoopControl> {
        match event {
            Event::Key(key) => {
                let should_quit = self.handle_key_event(key)?;
                if should_quit {
                    Ok(LoopControl::Break)
                } else {
                    Ok(LoopControl::Continue(true))
                }
            }
            Event::Mouse(mouse) => Ok(LoopControl::Continue(self.handle_mouse(mouse)?)),
            Event::Resize(width, height) => {
                self.resize_and_capture(width, height)?;
                Ok(LoopControl::Continue(true))
            }
            _ => Ok(LoopControl::Continue(false)),
        }
    }

    fn capture_terminal_size(&mut self, terminal: &DefaultTerminal) -> Result<()> {
        let size = terminal.size()?;
        self.capture(size.width, size.height.saturating_sub(2))
    }

    fn resize_and_capture(&mut self, width: u16, height: u16) -> Result<()> {
        self.note_resize_event(width, height.saturating_sub(2), "terminal");
        self.source.resize(width, height.saturating_sub(2))?;
        if !self.paused {
            self.capture(width, height.saturating_sub(2))?;
        }
        Ok(())
    }

    fn run_event_loop(
        mut self,
        mut terminal: DefaultTerminal,
        rx: mpsc::Receiver<AppEvent>,
        pending_source_update: Option<Arc<AtomicBool>>,
    ) -> Result<()> {
        if self.current_snapshot.is_none() {
            let size = terminal.size()?;
            if let Err(err) = self.capture(size.width, size.height.saturating_sub(2)) {
                if self.is_source_closed_error(&err) {
                    self.source.terminate().ok();
                    return Ok(());
                }
                return Err(err);
            }
        }
        let mut needs_redraw = true;

        loop {
            if needs_redraw {
                terminal.draw(|frame| ui::draw(frame, &self))?;
                needs_redraw = false;
            }

            if self.source.is_event_driven() {
                match rx.recv_timeout(Self::EVENT_POLL_TIMEOUT) {
                    Ok(AppEvent::Terminal(event)) => match self.process_terminal_event(event)? {
                        LoopControl::Continue(redraw) => {
                            if !self.paused && self.source.has_pending_update() {
                                self.capture_terminal_size(&terminal)?;
                            }
                            needs_redraw = redraw || needs_redraw;
                        }
                        LoopControl::Break => break,
                    },
                    Ok(AppEvent::SourceUpdated) => {
                        if let Some(flag) = &pending_source_update {
                            flag.store(false, Ordering::Release);
                        }
                        if !self.paused {
                            self.capture_terminal_size(&terminal)?;
                            needs_redraw = true;
                        }
                    }
                    Ok(AppEvent::SourceClosed(reason)) => {
                        if let Some(flag) = &pending_source_update {
                            flag.store(false, Ordering::Release);
                        }
                        if !self.paused && self.source.has_pending_update() {
                            self.capture_terminal_size(&terminal)?;
                            terminal.draw(|frame| ui::draw(frame, &self))?;
                        }
                        if let Some(reason) = reason {
                            self.ui.status_message = Some(format!("source closed: {reason}"));
                        }
                        break;
                    }
                    Err(RecvTimeoutError::Timeout) => {
                        if !self.paused && self.source.has_pending_update() {
                            if let Some(flag) = &pending_source_update {
                                flag.store(false, Ordering::Release);
                            }
                            self.capture_terminal_size(&terminal)?;
                            needs_redraw = true;
                        }
                    }
                    Err(RecvTimeoutError::Disconnected) => break,
                }
            } else {
                match rx.recv_timeout(Self::EVENT_POLL_TIMEOUT) {
                    Ok(AppEvent::Terminal(event)) => match self.process_terminal_event(event)? {
                        LoopControl::Continue(redraw) => {
                            needs_redraw = redraw || needs_redraw;
                        }
                        LoopControl::Break => break,
                    },
                    Ok(AppEvent::SourceUpdated) | Ok(AppEvent::SourceClosed(_)) => {}
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => break,
                }
            }
        }

        self.source.terminate().ok();
        Ok(())
    }
}

fn relay_source_event(
    event: SourceEvent,
    pending_source_update: &AtomicBool,
    update_tx: &Sender<AppEvent>,
) -> Result<()> {
    match event {
        SourceEvent::Updated => {
            if pending_source_update.swap(true, Ordering::AcqRel) {
                return Ok(());
            }
            update_tx
                .send(AppEvent::SourceUpdated)
                .map_err(|_| anyhow::anyhow!("source event channel closed"))?;
        }
        SourceEvent::Closed => {
            update_tx
                .send(AppEvent::SourceClosed(None))
                .map_err(|_| anyhow::anyhow!("source event channel closed"))?;
        }
        SourceEvent::ClosedWithError(reason) => {
            update_tx
                .send(AppEvent::SourceClosed(Some(reason)))
                .map_err(|_| anyhow::anyhow!("source event channel closed"))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicBool;
    use std::sync::mpsc;

    use super::{AppEvent, relay_source_event};
    use crate::runner::SourceEvent;

    #[test]
    fn coalesces_redundant_source_updated_events() {
        let (tx, rx) = mpsc::channel();
        let pending = AtomicBool::new(false);

        relay_source_event(SourceEvent::Updated, &pending, &tx).unwrap();
        relay_source_event(SourceEvent::Updated, &pending, &tx).unwrap();

        match rx.try_recv().unwrap() {
            AppEvent::SourceUpdated => {}
            _ => panic!("expected source updated event"),
        }
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn relays_source_closed_error_reason() {
        let (tx, rx) = mpsc::channel();
        let pending = AtomicBool::new(false);

        relay_source_event(
            SourceEvent::ClosedWithError("boom".to_string()),
            &pending,
            &tx,
        )
        .unwrap();

        match rx.try_recv().unwrap() {
            AppEvent::SourceClosed(Some(reason)) => assert_eq!(reason, "boom"),
            _ => panic!("expected source closed with reason"),
        }
    }
}
