use std::time::Duration;

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::{App, DiffMode, FocusPane, InputMode};

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
}
