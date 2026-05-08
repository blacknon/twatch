use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::{App, DiffMode, FocusPane, InputMode};

impl App {
    pub(crate) fn handle_key_event(&mut self, key: KeyEvent) -> Result<bool> {
        if self.should_ignore_mouse_ghost_key(key) {
            return Ok(false);
        }

        self.ui.status_message = None;

        if self.ui.show_exit_confirm {
            return self.handle_exit_confirm_key(key);
        }

        if self.ui.show_help {
            match key.code {
                KeyCode::Char('h') | KeyCode::Esc => {
                    self.ui.show_help = false;
                }
                KeyCode::Char('q') => self.ui.show_exit_confirm = true,
                _ => {}
            }
            return Ok(false);
        }

        if self.input_mode == InputMode::Search {
            return self.handle_search_key(key);
        }

        if self.ui.app_input_mode {
            if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('g') {
                self.ui.app_input_mode = false;
                return Ok(false);
            }
            self.record_child_key_event(key);
            self.source.send_key(key)?;
            return Ok(false);
        }

        if self.ui.focus == FocusPane::Watch && self.should_passthrough_to_app(key) {
            self.record_child_key_event(key);
            self.source.send_key(key)?;
            return Ok(false);
        }

        if self.ui.show_inspector && self.ui.focus == FocusPane::Watch {
            match (key.code, key.modifiers) {
                (KeyCode::Up, KeyModifiers::SHIFT) => {
                    self.ui.inspect_y = self.ui.inspect_y.saturating_sub(1);
                    return Ok(false);
                }
                (KeyCode::Down, KeyModifiers::SHIFT) => {
                    self.ui.inspect_y = self.ui.inspect_y.saturating_add(1);
                    self.clamp_inspector_to_snapshot();
                    return Ok(false);
                }
                (KeyCode::Left, KeyModifiers::SHIFT) => {
                    self.ui.inspect_x = self.ui.inspect_x.saturating_sub(1);
                    return Ok(false);
                }
                (KeyCode::Right, KeyModifiers::SHIFT) => {
                    self.ui.inspect_x = self.ui.inspect_x.saturating_add(1);
                    self.clamp_inspector_to_snapshot();
                    return Ok(false);
                }
                _ => {}
            }
        }

        match (key.code, key.modifiers) {
            (KeyCode::Char('q'), _) => self.ui.show_exit_confirm = true,
            (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                if self.ui.filter_query.is_empty() {
                    self.ui.show_exit_confirm = true;
                } else {
                    self.clear_filter()?;
                }
            }
            (KeyCode::Char('h'), _) => self.ui.show_help = true,
            (KeyCode::Char('I'), _) => {
                self.ui.show_inspector = !self.ui.show_inspector;
                self.clamp_inspector_to_snapshot();
            }
            (KeyCode::Char('i'), _) => {
                if self.ui.focus == FocusPane::Watch {
                    self.ui.app_input_mode = true;
                }
            }
            (KeyCode::Tab, _) => self.toggle_focus(),
            (KeyCode::Left, KeyModifiers::ALT) => {
                self.ui.horizontal_scroll = self.ui.horizontal_scroll.saturating_sub(4)
            }
            (KeyCode::Right, KeyModifiers::ALT) => self.ui.horizontal_scroll += 4,
            (KeyCode::Left, _) => self.ui.focus = FocusPane::Watch,
            (KeyCode::Right, _) => self.ui.focus = FocusPane::History,
            (KeyCode::Backspace, _) => {
                self.ui.show_history = !self.ui.show_history;
                if self.ui.show_history {
                    self.ui.focus = FocusPane::History;
                } else if self.ui.focus == FocusPane::History {
                    self.ui.focus = FocusPane::Watch;
                }
                if !self.ui.show_history {
                    self.ui.show_history_details = false;
                }
            }
            (KeyCode::Char('/'), _) => self.start_search(super::super::FilterMode::Plain),
            (KeyCode::Char('*'), _) => self.start_search(super::super::FilterMode::Regex),
            (KeyCode::Char('S'), KeyModifiers::SHIFT) => {
                self.ui.show_history_details = !self.ui.show_history_details;
                if !self.ui.show_history {
                    self.ui.show_history = true;
                    self.ui.focus = FocusPane::History;
                }
            }
            (KeyCode::Char('s') | KeyCode::Char('S'), modifiers)
                if modifiers.contains(KeyModifiers::CONTROL) =>
            {
                self.save_snapshot()?
            }
            (KeyCode::Char('D'), _) => self.delete_selected_history()?,
            (KeyCode::Char('X'), _) => self.clear_history_except_selected()?,
            (KeyCode::Char('s'), _) => self.cycle_screenshot_format(),
            (KeyCode::Char('d'), _) => self.cycle_diff_mode(),
            (KeyCode::Char('0'), _) => self.diff_mode = DiffMode::None,
            (KeyCode::Char('1'), _) => self.diff_mode = DiffMode::Watch,
            (KeyCode::Char('P'), _) => self.toggle_child_pause()?,
            (KeyCode::Char('p'), _) => self.paused = !self.paused,
            (KeyCode::Up, _) => self.move_up(),
            (KeyCode::Down, _) => self.move_down(),
            (KeyCode::PageUp, _) => self.page_up(),
            (KeyCode::PageDown, _) => self.page_down(),
            (KeyCode::Home, _) => self.move_top(),
            (KeyCode::End, _) => self.move_end(),
            _ => {}
        }
        Ok(false)
    }
}
