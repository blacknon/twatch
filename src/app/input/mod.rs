// Copyright (c) 2026 Blacknon. All rights reserved.
// Use of this source code is governed by an MIT license
// that can be found in the LICENSE file.

use std::time::Duration;

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

use super::{App, DiffMode, FilterMode, FocusPane, InputMode};
use crate::child_bindings::{ChildBindingAction, ChildBindingKey};
use crate::input_key::KeyPress;
use crate::keymap::KeyAction;

mod history_nav;
mod key;
mod mouse;
mod trace;

impl App {
    fn toggle_child_pause(&mut self) -> Result<()> {
        match self.source.toggle_child_pause()? {
            Some(paused) => {
                self.child_paused = paused;
                self.ui.status_message = Some(if paused {
                    "child process paused".to_string()
                } else {
                    "child process resumed".to_string()
                });
            }
            None => {
                self.child_paused = false;
                self.ui.status_message =
                    Some("child pause is not available for this source".to_string());
            }
        }
        Ok(())
    }

    fn handle_exit_confirm_key(&mut self, key: KeyEvent) -> Result<bool> {
        match (key.code, key.modifiers) {
            (KeyCode::Char(ch), _) if matches!(ch, 'y' | 'Y' | 'q' | 'Q') => Ok(true),
            (KeyCode::Enter, _) => Ok(true),
            (KeyCode::Esc, _) => {
                self.ui.show_exit_confirm = false;
                Ok(false)
            }
            (KeyCode::Char(ch), _) if matches!(ch, 'n' | 'N') => {
                self.ui.show_exit_confirm = false;
                Ok(false)
            }
            (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                self.ui.show_exit_confirm = false;
                Ok(false)
            }
            _ => Ok(false),
        }
    }

    fn handle_search_key(&mut self, key: KeyEvent) -> Result<bool> {
        match key.code {
            KeyCode::Esc => self.clear_filter()?,
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.clear_filter()?;
            }
            KeyCode::Enter => {
                self.input_mode = InputMode::Normal;
                self.rebuild_filter()?;
            }
            KeyCode::Backspace => {
                self.ui.filter_query.pop();
                self.rebuild_filter()?;
            }
            KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.ui.filter_query.push(ch);
                self.rebuild_filter()?;
            }
            _ => {}
        }
        Ok(false)
    }

    fn start_search(&mut self, mode: super::FilterMode) {
        self.input_mode = InputMode::Search;
        self.filter_mode = mode;
    }

    fn clear_filter(&mut self) -> Result<()> {
        self.input_mode = InputMode::Normal;
        self.ui.filter_query.clear();
        self.rebuild_filter()
    }

    fn toggle_focus(&mut self) {
        self.ui.focus = match self.ui.focus {
            FocusPane::Watch => FocusPane::History,
            FocusPane::History => FocusPane::Watch,
        };
    }

    fn cycle_diff_mode(&mut self) {
        self.diff_mode = match self.diff_mode {
            DiffMode::None => DiffMode::Watch,
            DiffMode::Watch => DiffMode::None,
        };
    }

    fn should_ignore_mouse_ghost_key(&self, key: KeyEvent) -> bool {
        let Some(last_mouse_scroll_input) = self.last_mouse_scroll_input else {
            return false;
        };
        if last_mouse_scroll_input.elapsed() > Duration::from_millis(250) {
            return false;
        }

        matches!(
            (key.code, key.modifiers),
            (KeyCode::Char('d'), KeyModifiers::NONE)
                | (KeyCode::Char('D'), KeyModifiers::SHIFT)
                | (KeyCode::Char('0'), KeyModifiers::NONE)
                | (KeyCode::Char('1'), KeyModifiers::NONE)
        )
    }

    fn should_passthrough_to_app(&self, key: KeyEvent) -> bool {
        if key.modifiers.contains(KeyModifiers::CONTROL)
            || key.modifiers.contains(KeyModifiers::ALT)
        {
            return false;
        }

        matches!(
            key.code,
            KeyCode::Up
                | KeyCode::Down
                | KeyCode::PageUp
                | KeyCode::PageDown
                | KeyCode::Home
                | KeyCode::End
                | KeyCode::Enter
                | KeyCode::F(_)
        )
    }

    fn clamp_inspector_to_snapshot(&mut self) {
        let Some(snapshot) = self.selected_snapshot() else {
            self.ui.inspect_x = 0;
            self.ui.inspect_y = 0;
            return;
        };
        self.ui.inspect_x = self.ui.inspect_x.min(snapshot.width().saturating_sub(1));
        self.ui.inspect_y = self.ui.inspect_y.min(snapshot.height().saturating_sub(1));
    }

    fn find_key_action(&self, key: KeyEvent) -> Option<KeyAction> {
        let press = KeyPress::from(key);
        self.keymap
            .iter()
            .rev()
            .find(|binding| binding.trigger == press)
            .map(|binding| binding.action)
    }

    fn find_child_binding_action(&self, key: KeyEvent) -> Option<ChildBindingAction> {
        let press = KeyPress::from(key);
        self.child_bindings
            .iter()
            .rev()
            .find(|binding| binding.trigger == press)
            .map(|binding| binding.action.clone())
    }

    fn apply_child_binding(&mut self, key: KeyEvent, action: ChildBindingAction) -> Result<()> {
        match action {
            ChildBindingAction::SendKeys(keys) => {
                self.record_child_key_event(key);
                for item in keys {
                    match item {
                        ChildBindingKey::Press(press) => {
                            self.source.send_key(KeyEvent {
                                code: press.code,
                                modifiers: press.modifiers,
                                kind: KeyEventKind::Press,
                                state: KeyEventState::NONE,
                            })?;
                        }
                        ChildBindingKey::Bytes(bytes) => self.source.send_bytes(&bytes)?,
                    }
                }
            }
            ChildBindingAction::SaveSnapshot => self.save_snapshot()?,
        }
        Ok(())
    }

    fn execute_key_action(&mut self, action: KeyAction) -> Result<bool> {
        match action {
            KeyAction::Up => self.move_up(),
            KeyAction::WatchPaneUp => {
                self.ui.focus = FocusPane::Watch;
                self.scroll_selected_watch_view(-1);
            }
            KeyAction::HistoryPaneUp => {
                self.ui.focus = FocusPane::History;
                self.move_history_by(-1);
            }
            KeyAction::Down => self.move_down(),
            KeyAction::WatchPaneDown => {
                self.ui.focus = FocusPane::Watch;
                self.scroll_selected_watch_view(1);
            }
            KeyAction::HistoryPaneDown => {
                self.ui.focus = FocusPane::History;
                self.move_history_by(1);
            }
            KeyAction::PageUp => self.page_up(),
            KeyAction::WatchPanePageUp => {
                self.ui.focus = FocusPane::Watch;
                self.scroll_selected_watch_view(-10);
            }
            KeyAction::HistoryPanePageUp => {
                self.ui.focus = FocusPane::History;
                self.move_history_by(-10);
            }
            KeyAction::PageDown => self.page_down(),
            KeyAction::WatchPanePageDown => {
                self.ui.focus = FocusPane::Watch;
                self.scroll_selected_watch_view(10);
            }
            KeyAction::HistoryPanePageDown => {
                self.ui.focus = FocusPane::History;
                self.move_history_by(10);
            }
            KeyAction::MoveTop => self.move_top(),
            KeyAction::WatchPaneMoveTop => {
                self.ui.focus = FocusPane::Watch;
                self.ui.watch_scroll = 0;
            }
            KeyAction::HistoryPaneMoveTop => {
                self.ui.focus = FocusPane::History;
                self.follow_latest = true;
                if let Some(first) = self.filtered.first().copied() {
                    self.selected_index = first;
                }
                self.invalidate_view_cache();
            }
            KeyAction::MoveEnd => self.move_end(),
            KeyAction::WatchPaneMoveEnd => {
                self.ui.focus = FocusPane::Watch;
                self.ui.watch_scroll = usize::MAX / 2;
            }
            KeyAction::HistoryPaneMoveEnd => {
                self.ui.focus = FocusPane::History;
                if let Some(last) = self.filtered.last().copied() {
                    self.selected_index = last;
                    self.follow_latest = false;
                    self.sync_follow_latest_with_selection();
                }
            }
            KeyAction::ToggleFocus => self.toggle_focus(),
            KeyAction::FocusWatchPane => self.ui.focus = FocusPane::Watch,
            KeyAction::FocusHistoryPane => self.ui.focus = FocusPane::History,
            KeyAction::Quit => self.ui.show_exit_confirm = true,
            KeyAction::Reset => {
                if self.ui.show_help {
                    self.ui.show_help = false;
                } else if self.ui.show_exit_confirm {
                    self.ui.show_exit_confirm = false;
                } else if self.input_mode == InputMode::Search || !self.ui.filter_query.is_empty() {
                    self.clear_filter()?;
                }
            }
            KeyAction::Delete => self.delete_selected_history()?,
            KeyAction::ClearExceptSelected => self.clear_history_except_selected()?,
            KeyAction::Cancel => {
                if self.ui.filter_query.is_empty() {
                    self.ui.show_exit_confirm = true;
                } else {
                    self.clear_filter()?;
                }
            }
            KeyAction::ForceCancel => return Ok(true),
            KeyAction::Help => self.ui.show_help = !self.ui.show_help,
            KeyAction::ToggleViewHistoryPane => {
                self.ui.show_history = !self.ui.show_history;
                if self.ui.show_history {
                    self.ui.focus = FocusPane::History;
                } else if self.ui.focus == FocusPane::History {
                    self.ui.focus = FocusPane::Watch;
                    self.ui.show_history_details = false;
                }
            }
            KeyAction::ToggleHistorySummary => {
                self.ui.show_history_details = !self.ui.show_history_details;
                if !self.ui.show_history {
                    self.ui.show_history = true;
                    self.ui.focus = FocusPane::History;
                }
            }
            KeyAction::ToggleDiffMode => self.cycle_diff_mode(),
            KeyAction::SetDiffModeNone => self.diff_mode = DiffMode::None,
            KeyAction::SetDiffModeWatch => self.diff_mode = DiffMode::Watch,
            KeyAction::TogglePause => self.paused = !self.paused,
            KeyAction::ToggleChildPause => self.toggle_child_pause()?,
            KeyAction::ChangeFilterMode => self.start_search(FilterMode::Plain),
            KeyAction::ChangeRegexFilterMode => self.start_search(FilterMode::Regex),
            KeyAction::EnterAppInputMode => {
                if self.ui.focus == FocusPane::Watch && self.follow_latest {
                    self.reset_watch_viewport();
                    self.clear_live_scrollback_view();
                    self.ui.app_input_mode = true;
                } else if self.ui.focus == FocusPane::Watch {
                    self.ui.status_message =
                        Some("app input mode is available only on latest".to_string());
                }
            }
            KeyAction::LeaveAppInputMode => self.ui.app_input_mode = false,
            KeyAction::ToggleInspector => {
                self.ui.show_inspector = !self.ui.show_inspector;
                self.clamp_inspector_to_snapshot();
            }
            KeyAction::SaveSnapshot => self.save_snapshot()?,
            KeyAction::CycleSnapshotFormat => self.cycle_screenshot_format(),
            KeyAction::ScrollLeft => {
                self.ui.horizontal_scroll = self.ui.horizontal_scroll.saturating_sub(4)
            }
            KeyAction::ScrollRight => self.ui.horizontal_scroll += 4,
        }
        Ok(false)
    }
}
