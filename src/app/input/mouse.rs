use std::time::Instant;

use anyhow::Result;
use crossterm::event::{MouseEvent, MouseEventKind};

use crate::app::{App, FocusPane};

impl App {
    pub(crate) fn handle_mouse(&mut self, mouse: MouseEvent) -> Result<bool> {
        if self.show_exit_confirm {
            return Ok(false);
        }

        self.last_mouse_input = Some(Instant::now());

        if self.app_input_mode {
            if self.follow_latest {
                self.record_child_mouse_event(mouse);
                self.source.send_mouse(mouse, 2)?;
            }
            return Ok(false);
        }

        if mouse.row < 2 {
            return Ok(false);
        }

        let total_width = crossterm::terminal::size()
            .map(|(width, _)| width)
            .unwrap_or(0);
        let over_history =
            self.show_history && mouse.column >= self.history_overlay_start(total_width);
        let history_content_top = self.history_content_top();
        let previous_focus = self.focus;

        match mouse.kind {
            MouseEventKind::ScrollDown => {
                self.last_mouse_scroll_input = Some(Instant::now());
                if over_history {
                    self.focus = FocusPane::History;
                    self.move_down();
                    return Ok(true);
                } else if self.follow_latest {
                    self.focus = FocusPane::Watch;
                    self.record_child_mouse_event(mouse);
                    self.source.send_mouse(mouse, 2)?;
                    return Ok(previous_focus != self.focus);
                } else {
                    self.focus = FocusPane::Watch;
                    return Ok(previous_focus != self.focus);
                }
            }
            MouseEventKind::ScrollUp => {
                self.last_mouse_scroll_input = Some(Instant::now());
                if over_history {
                    self.focus = FocusPane::History;
                    self.move_up();
                    return Ok(true);
                } else if self.follow_latest {
                    self.focus = FocusPane::Watch;
                    self.record_child_mouse_event(mouse);
                    self.source.send_mouse(mouse, 2)?;
                    return Ok(previous_focus != self.focus);
                } else {
                    self.focus = FocusPane::Watch;
                    return Ok(previous_focus != self.focus);
                }
            }
            MouseEventKind::Down(_) => {
                if over_history {
                    self.focus = FocusPane::History;
                    if mouse.row < history_content_top {
                        return Ok(true);
                    }
                    self.select_history_overlay_row(
                        usize::from(mouse.row.saturating_sub(history_content_top)),
                        self.history_visible_rows(),
                    );
                    return Ok(true);
                } else if self.follow_latest {
                    self.focus = FocusPane::Watch;
                    self.record_child_mouse_event(mouse);
                    self.source.send_mouse(mouse, 2)?;
                    return Ok(previous_focus != self.focus);
                } else {
                    self.focus = FocusPane::Watch;
                    return Ok(previous_focus != self.focus);
                }
            }
            MouseEventKind::Moved => return Ok(false),
            MouseEventKind::Up(_) | MouseEventKind::Drag(_) => {
                if !over_history && self.follow_latest {
                    self.focus = FocusPane::Watch;
                    self.record_child_mouse_event(mouse);
                    self.source.send_mouse(mouse, 2)?;
                    return Ok(previous_focus != self.focus);
                } else if !over_history {
                    self.focus = FocusPane::Watch;
                    return Ok(previous_focus != self.focus);
                }
            }
            _ => {}
        }
        Ok(false)
    }

    fn history_overlay_start(&self, total_width: u16) -> u16 {
        total_width.saturating_sub(self.history_overlay_width())
    }

    fn history_visible_rows(&self) -> usize {
        crossterm::terminal::size()
            .map(|(_, height)| usize::from(height.saturating_sub(4)))
            .unwrap_or(0)
    }

    fn history_content_top(&self) -> u16 {
        3
    }
}
