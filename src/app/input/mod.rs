// Copyright (c) 2026 Blacknon. All rights reserved.
// Use of this source code is governed by an MIT license
// that can be found in the LICENSE file.

use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

use super::{
    App, AutoReplayDirection, AutoReplayState, DiffMode, FilterMode, FocusPane, InputMode,
};
use crate::child_bindings::{ChildBindingAction, ChildBindingKey};
use crate::input_key::KeyPress;
use crate::keymap::KeyAction;

mod history_nav;
mod key;
mod mouse;
mod trace;

impl App {
    const AUTO_REPLAY_MAX_DELAY: Duration = Duration::from_secs(2);
    const AUTO_REPLAY_MIN_DELAY: Duration = Duration::from_millis(16);
    const AUTO_REPLAY_SPEED_STEPS: [f32; 6] = [0.25, 0.5, 1.0, 2.0, 4.0, 8.0];

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

    fn toggle_header_visibility(&mut self) {
        self.hide_header = !self.hide_header;
        self.ui.status_message = Some(if self.hide_header {
            "header hidden".to_string()
        } else {
            "header shown".to_string()
        });
    }

    pub(crate) fn auto_replay_available(&self) -> bool {
        self.replay_mode
    }

    pub(crate) fn auto_replay_status_label(&self) -> &'static str {
        match self.auto_replay_state {
            AutoReplayState::Stopped => "Off",
            AutoReplayState::Playing(direction) => match direction {
                AutoReplayDirection::Forward => "Play Fwd",
                AutoReplayDirection::Reverse => "Play Rev",
            },
            AutoReplayState::Paused(direction) => match direction {
                AutoReplayDirection::Forward => "Pause Fwd",
                AutoReplayDirection::Reverse => "Pause Rev",
            },
        }
    }

    pub(crate) fn auto_replay_speed_label(&self) -> String {
        format!("{:.2}x", self.auto_replay_speed)
    }

    pub(crate) fn replay_indicator_label(&self) -> Option<&'static str> {
        if !self.replay_indicator {
            return None;
        }
        match self.auto_replay_state {
            AutoReplayState::Playing(AutoReplayDirection::Forward) => Some("REPLAY"),
            AutoReplayState::Playing(AutoReplayDirection::Reverse) => Some("REWIND"),
            AutoReplayState::Stopped | AutoReplayState::Paused(_) => None,
        }
    }

    fn stop_auto_replay(&mut self) {
        self.auto_replay_state = AutoReplayState::Stopped;
        self.auto_replay_next_tick = None;
    }

    fn pause_auto_replay(&mut self, direction: AutoReplayDirection) {
        self.auto_replay_state = AutoReplayState::Paused(direction);
        self.auto_replay_next_tick = None;
    }

    fn start_auto_replay(&mut self, direction: AutoReplayDirection) {
        self.auto_replay_state = AutoReplayState::Playing(direction);
        self.auto_replay_next_tick = Some(Instant::now());
    }

    fn toggle_auto_replay(&mut self, direction: AutoReplayDirection) {
        if !self.auto_replay_available() {
            self.ui.status_message =
                Some("auto replay is available only in replay mode".to_string());
            return;
        }
        if !self.ui.filter_query.is_empty() || self.is_search_mode() {
            self.ui.status_message =
                Some("auto replay is unavailable while a filter is active".to_string());
            return;
        }

        match self.auto_replay_state {
            AutoReplayState::Playing(current) if current == direction => {
                self.pause_auto_replay(direction);
                self.ui.status_message = Some(format!(
                    "auto replay paused ({})",
                    direction.label().to_lowercase()
                ));
            }
            AutoReplayState::Paused(current) if current == direction => {
                self.start_auto_replay(direction);
                self.ui.status_message = Some(format!(
                    "auto replay resumed ({}, timestamp, {})",
                    direction.label().to_lowercase(),
                    self.auto_replay_speed_label()
                ));
            }
            _ => {
                self.start_auto_replay(direction);
                self.ui.status_message = Some(format!(
                    "auto replay started ({}, timestamp, {})",
                    direction.label().to_lowercase(),
                    self.auto_replay_speed_label()
                ));
            }
        }
    }

    fn adjust_auto_replay_speed(&mut self, faster: bool) {
        if !self.auto_replay_available() {
            self.ui.status_message =
                Some("auto replay speed is available only in replay mode".to_string());
            return;
        }

        let current_index = Self::AUTO_REPLAY_SPEED_STEPS
            .iter()
            .position(|speed| (*speed - self.auto_replay_speed).abs() < f32::EPSILON)
            .unwrap_or(2);
        let next_index = if faster {
            (current_index + 1).min(Self::AUTO_REPLAY_SPEED_STEPS.len() - 1)
        } else {
            current_index.saturating_sub(1)
        };
        self.auto_replay_speed = Self::AUTO_REPLAY_SPEED_STEPS[next_index];
        self.ui.status_message = Some(format!(
            "auto replay speed: {}",
            self.auto_replay_speed_label()
        ));
        if self.auto_replay_state.is_playing() {
            self.auto_replay_next_tick = Some(Instant::now());
        }
    }

    fn replay_neighbor_index(
        &self,
        direction: AutoReplayDirection,
    ) -> Option<(Option<usize>, u64, u64)> {
        let current = self.selected_history_metadata()?;
        match direction {
            AutoReplayDirection::Forward => {
                if let Some(index) = self.current_selected_index() {
                    if index + 1 < self.metadata.len() {
                        let next = &self.metadata[index + 1];
                        Some((
                            Some(index + 1),
                            current.timestamp_unix_ms,
                            next.timestamp_unix_ms,
                        ))
                    } else {
                        let next = self.current_metadata.as_ref()?;
                        Some((None, current.timestamp_unix_ms, next.timestamp_unix_ms))
                    }
                } else {
                    None
                }
            }
            AutoReplayDirection::Reverse => {
                if self.follow_latest {
                    let next_index = self.metadata.len().checked_sub(1)?;
                    let next = &self.metadata[next_index];
                    Some((
                        Some(next_index),
                        current.timestamp_unix_ms,
                        next.timestamp_unix_ms,
                    ))
                } else if let Some(index) = self.current_selected_index() {
                    if index == 0 {
                        None
                    } else {
                        let next = &self.metadata[index - 1];
                        Some((
                            Some(index - 1),
                            current.timestamp_unix_ms,
                            next.timestamp_unix_ms,
                        ))
                    }
                } else {
                    None
                }
            }
        }
    }

    fn apply_auto_replay_step(&mut self, direction: AutoReplayDirection) -> bool {
        match direction {
            AutoReplayDirection::Forward => {
                if let Some(index) = self.current_selected_index() {
                    if index + 1 < self.metadata.len() {
                        self.selected_index = index + 1;
                        self.follow_latest = false;
                    } else {
                        self.follow_latest = true;
                    }
                    self.reset_watch_viewport();
                    self.clear_live_scrollback_view();
                    self.invalidate_view_cache();
                    true
                } else {
                    false
                }
            }
            AutoReplayDirection::Reverse => {
                if self.follow_latest {
                    if let Some(index) = self.metadata.len().checked_sub(1) {
                        self.selected_index = index;
                        self.follow_latest = false;
                        self.reset_watch_viewport();
                        self.clear_live_scrollback_view();
                        self.invalidate_view_cache();
                        return true;
                    }
                    return false;
                }
                if self.selected_index > 0 {
                    self.selected_index -= 1;
                    self.follow_latest = false;
                    self.reset_watch_viewport();
                    self.clear_live_scrollback_view();
                    self.invalidate_view_cache();
                    return true;
                }
                false
            }
        }
    }

    fn auto_replay_step_delay(&self, current_ts: u64, next_ts: u64) -> Duration {
        let delta_ms = current_ts.abs_diff(next_ts).max(1);
        let scaled_ms = ((delta_ms as f64) / f64::from(self.auto_replay_speed))
            .round()
            .clamp(
                Self::AUTO_REPLAY_MIN_DELAY.as_millis() as f64,
                Self::AUTO_REPLAY_MAX_DELAY.as_millis() as f64,
            ) as u64;
        Duration::from_millis(scaled_ms)
    }

    pub(crate) fn advance_auto_replay(&mut self, now: Instant) -> Result<bool> {
        let AutoReplayState::Playing(direction) = self.auto_replay_state else {
            return Ok(false);
        };
        let Some(next_tick) = self.auto_replay_next_tick else {
            self.auto_replay_next_tick = Some(now);
            return Ok(false);
        };
        if now < next_tick {
            return Ok(false);
        }
        if self.replay_loading {
            self.auto_replay_next_tick = Some(now + Duration::from_millis(100));
            return Ok(false);
        }

        let Some((_, current_ts, next_ts)) = self.replay_neighbor_index(direction) else {
            if direction == AutoReplayDirection::Reverse
                && self.replay_deferred_path.is_some()
                && !self.replay_loading
                && self.start_deferred_replay_loader()
            {
                self.ui.status_message =
                    Some("loading older replay history in background".to_string());
                self.auto_replay_next_tick = Some(now + Duration::from_millis(100));
                return Ok(true);
            }

            self.pause_auto_replay(direction);
            self.ui.status_message = Some(format!(
                "auto replay reached {} frame",
                match direction {
                    AutoReplayDirection::Forward => "latest",
                    AutoReplayDirection::Reverse => "oldest",
                }
            ));
            return Ok(true);
        };

        if !self.apply_auto_replay_step(direction) {
            self.pause_auto_replay(direction);
            return Ok(true);
        }

        self.auto_replay_next_tick = Some(now + self.auto_replay_step_delay(current_ts, next_ts));
        Ok(true)
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
            KeyAction::Up => {
                self.stop_auto_replay();
                self.move_up()
            }
            KeyAction::WatchPaneUp => {
                self.stop_auto_replay();
                self.ui.focus = FocusPane::Watch;
                self.scroll_selected_watch_view(-1);
            }
            KeyAction::HistoryPaneUp => {
                self.stop_auto_replay();
                self.ui.focus = FocusPane::History;
                self.move_history_by(-1);
            }
            KeyAction::Down => {
                self.stop_auto_replay();
                self.move_down()
            }
            KeyAction::WatchPaneDown => {
                self.stop_auto_replay();
                self.ui.focus = FocusPane::Watch;
                self.scroll_selected_watch_view(1);
            }
            KeyAction::HistoryPaneDown => {
                self.stop_auto_replay();
                self.ui.focus = FocusPane::History;
                self.move_history_by(1);
            }
            KeyAction::PageUp => {
                self.stop_auto_replay();
                self.page_up()
            }
            KeyAction::WatchPanePageUp => {
                self.stop_auto_replay();
                self.ui.focus = FocusPane::Watch;
                self.scroll_selected_watch_view(-10);
            }
            KeyAction::HistoryPanePageUp => {
                self.stop_auto_replay();
                self.ui.focus = FocusPane::History;
                self.move_history_by(-10);
            }
            KeyAction::PageDown => {
                self.stop_auto_replay();
                self.page_down()
            }
            KeyAction::WatchPanePageDown => {
                self.stop_auto_replay();
                self.ui.focus = FocusPane::Watch;
                self.scroll_selected_watch_view(10);
            }
            KeyAction::HistoryPanePageDown => {
                self.stop_auto_replay();
                self.ui.focus = FocusPane::History;
                self.move_history_by(10);
            }
            KeyAction::MoveTop => {
                self.stop_auto_replay();
                self.move_top()
            }
            KeyAction::WatchPaneMoveTop => {
                self.stop_auto_replay();
                self.ui.focus = FocusPane::Watch;
                self.ui.watch_scroll = 0;
            }
            KeyAction::HistoryPaneMoveTop => {
                self.stop_auto_replay();
                self.ui.focus = FocusPane::History;
                self.follow_latest = true;
                if let Some(first) = self.filtered.first().copied() {
                    self.selected_index = first;
                }
                self.invalidate_view_cache();
            }
            KeyAction::MoveEnd => {
                self.stop_auto_replay();
                self.move_end()
            }
            KeyAction::WatchPaneMoveEnd => {
                self.stop_auto_replay();
                self.ui.focus = FocusPane::Watch;
                self.ui.watch_scroll = usize::MAX / 2;
            }
            KeyAction::HistoryPaneMoveEnd => {
                self.stop_auto_replay();
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
            KeyAction::ToggleHeader => self.toggle_header_visibility(),
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
