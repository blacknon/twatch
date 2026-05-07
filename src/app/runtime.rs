use std::sync::mpsc::{self, RecvTimeoutError};

use anyhow::Result;
use crossterm::event::{self, Event};
use ratatui::DefaultTerminal;

use super::{App, AppEvent, LoopControl};
use crate::runner::SourceEvent;
use crate::ui;

impl App {
    pub fn run(mut self, mut terminal: DefaultTerminal) -> Result<()> {
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
            std::thread::spawn(move || {
                while let Ok(event) = update_rx.recv() {
                    let app_event = match event {
                        SourceEvent::Updated => AppEvent::SourceUpdated,
                        SourceEvent::Closed => AppEvent::SourceClosed,
                    };
                    if update_tx.send(app_event).is_err() {
                        break;
                    }
                }
            });
        }
        drop(tx);

        let size = terminal.size()?;
        if let Err(err) = self.capture(size.width, size.height.saturating_sub(2)) {
            if self.is_source_closed_error(&err) {
                self.source.terminate().ok();
                return Ok(());
            }
            return Err(err);
        }
        let mut needs_redraw = true;

        loop {
            if needs_redraw {
                terminal.draw(|frame| ui::draw(frame, &self))?;
                needs_redraw = false;
            }

            if self.source.is_event_driven() {
                match rx.recv() {
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
                        if !self.paused {
                            self.capture_terminal_size(&terminal)?;
                            needs_redraw = true;
                        }
                    }
                    Ok(AppEvent::SourceClosed) => {
                        if !self.paused && self.source.has_pending_update() {
                            self.capture_terminal_size(&terminal)?;
                            terminal.draw(|frame| ui::draw(frame, &self))?;
                        }
                        break;
                    }
                    Err(_) => break,
                }
            } else {
                match rx.recv_timeout(self.tick_timeout()) {
                    Ok(AppEvent::Terminal(event)) => match self.process_terminal_event(event)? {
                        LoopControl::Continue(redraw) => {
                            needs_redraw = redraw || needs_redraw;
                        }
                        LoopControl::Break => break,
                    },
                    Ok(AppEvent::SourceUpdated) | Ok(AppEvent::SourceClosed) => {}
                    Err(RecvTimeoutError::Timeout) => {
                        if !self.paused && self.should_capture_now() {
                            self.capture_terminal_size(&terminal)?;
                            needs_redraw = true;
                        }
                    }
                    Err(RecvTimeoutError::Disconnected) => break,
                }
            }
        }

        self.source.terminate().ok();
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
}
