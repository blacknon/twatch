// Copyright (c) 2026 Blacknon. All rights reserved.
// Use of this source code is governed by an MIT license
// that can be found in the LICENSE file.

use std::time::Instant;

use anyhow::Result;
use crossterm::event::{MouseEvent, MouseEventKind};

use crate::app::{App, FocusPane};

impl App {
    pub(crate) fn handle_mouse(&mut self, mouse: MouseEvent) -> Result<bool> {
        if self.ui.show_exit_confirm {
            return Ok(false);
        }

        self.last_mouse_input = Some(Instant::now());

        if self.ui.app_input_mode {
            if self.follow_latest {
                if matches!(
                    mouse.kind,
                    MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
                ) && self.selected_snapshot_has_mouse_reporting()
                {
                    if self.source.send_mouse(mouse, self.header_rows())? {
                        self.record_child_mouse_event(mouse);
                    }
                    return Ok(false);
                }
                if matches!(
                    mouse.kind,
                    MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
                ) && !self.selected_snapshot_is_alternate_screen()
                {
                    let delta = match mouse.kind {
                        MouseEventKind::ScrollUp => 3,
                        MouseEventKind::ScrollDown => -3,
                        _ => 0,
                    };
                    return self.scroll_main_screen_view(delta);
                }
                if self.source.send_mouse(mouse, self.header_rows())? {
                    self.record_child_mouse_event(mouse);
                }
            }
            return Ok(false);
        }

        if mouse.row < self.header_rows() {
            return Ok(false);
        }

        let total_width = crossterm::terminal::size()
            .map(|(width, _)| width)
            .unwrap_or(0);
        let over_history =
            self.ui.show_history && mouse.column >= self.history_overlay_start(total_width);
        let history_content_top = self.history_content_top();
        let previous_focus = self.ui.focus;

        match mouse.kind {
            MouseEventKind::ScrollDown => {
                self.last_mouse_scroll_input = Some(Instant::now());
                if over_history {
                    self.ui.focus = FocusPane::History;
                    self.move_down();
                    return Ok(true);
                } else if self.follow_latest && self.selected_snapshot_has_mouse_reporting() {
                    self.ui.focus = FocusPane::Watch;
                    if self.source.send_mouse(mouse, self.header_rows())? {
                        self.record_child_mouse_event(mouse);
                    }
                    return Ok(previous_focus != self.ui.focus);
                } else if self.follow_latest && !self.selected_snapshot_is_alternate_screen() {
                    self.ui.focus = FocusPane::Watch;
                    let changed = self.scroll_main_screen_view(-3)?;
                    return Ok(changed || previous_focus != self.ui.focus);
                } else if self.follow_latest {
                    self.ui.focus = FocusPane::Watch;
                    if self.source.send_mouse(mouse, self.header_rows())? {
                        self.record_child_mouse_event(mouse);
                    }
                    return Ok(previous_focus != self.ui.focus);
                } else {
                    self.ui.focus = FocusPane::Watch;
                    let changed = self.scroll_selected_watch_view(3);
                    return Ok(changed || previous_focus != self.ui.focus);
                }
            }
            MouseEventKind::ScrollUp => {
                self.last_mouse_scroll_input = Some(Instant::now());
                if over_history {
                    self.ui.focus = FocusPane::History;
                    self.move_up();
                    return Ok(true);
                } else if self.follow_latest && self.selected_snapshot_has_mouse_reporting() {
                    self.ui.focus = FocusPane::Watch;
                    if self.source.send_mouse(mouse, self.header_rows())? {
                        self.record_child_mouse_event(mouse);
                    }
                    return Ok(previous_focus != self.ui.focus);
                } else if self.follow_latest && !self.selected_snapshot_is_alternate_screen() {
                    self.ui.focus = FocusPane::Watch;
                    let changed = self.scroll_main_screen_view(3)?;
                    return Ok(changed || previous_focus != self.ui.focus);
                } else if self.follow_latest {
                    self.ui.focus = FocusPane::Watch;
                    if self.source.send_mouse(mouse, self.header_rows())? {
                        self.record_child_mouse_event(mouse);
                    }
                    return Ok(previous_focus != self.ui.focus);
                } else {
                    self.ui.focus = FocusPane::Watch;
                    let changed = self.scroll_selected_watch_view(-3);
                    return Ok(changed || previous_focus != self.ui.focus);
                }
            }
            MouseEventKind::Down(_) => {
                if over_history {
                    self.ui.focus = FocusPane::History;
                    if mouse.row < history_content_top {
                        return Ok(true);
                    }
                    self.select_history_overlay_row(
                        usize::from(mouse.row.saturating_sub(history_content_top)),
                        self.history_visible_rows(),
                    );
                    return Ok(true);
                } else if self.follow_latest {
                    self.ui.focus = FocusPane::Watch;
                    if self.source.send_mouse(mouse, self.header_rows())? {
                        self.record_child_mouse_event(mouse);
                    }
                    return Ok(previous_focus != self.ui.focus);
                } else {
                    self.ui.focus = FocusPane::Watch;
                    return Ok(previous_focus != self.ui.focus);
                }
            }
            MouseEventKind::Moved => return Ok(false),
            MouseEventKind::Up(_) | MouseEventKind::Drag(_) => {
                if !over_history && self.follow_latest {
                    self.ui.focus = FocusPane::Watch;
                    if self.source.send_mouse(mouse, self.header_rows())? {
                        self.record_child_mouse_event(mouse);
                    }
                    return Ok(previous_focus != self.ui.focus);
                } else if !over_history {
                    self.ui.focus = FocusPane::Watch;
                    return Ok(previous_focus != self.ui.focus);
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
            .map(|(_, height)| usize::from(height.saturating_sub(self.header_rows() + 2)))
            .unwrap_or(0)
    }

    fn history_content_top(&self) -> u16 {
        self.header_rows() + 1
    }
}
