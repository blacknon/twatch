use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};

use super::{
    App, DiffMode, FocusPane, InputMode, InputTargetFocus, InputTraceEvent, InputTraceKind,
    ResizeTraceEvent,
};

impl App {
    pub(super) fn handle_key_event(&mut self, key: KeyEvent) -> Result<bool> {
        if self.should_ignore_mouse_ghost_key(key) {
            return Ok(false);
        }

        self.status_message = None;

        if self.show_exit_confirm {
            return self.handle_exit_confirm_key(key);
        }

        if self.show_help {
            match key.code {
                KeyCode::Char('h') | KeyCode::Esc => {
                    self.show_help = false;
                }
                KeyCode::Char('q') => self.show_exit_confirm = true,
                _ => {}
            }
            return Ok(false);
        }

        if self.input_mode == InputMode::Search {
            return self.handle_search_key(key);
        }

        if self.app_input_mode {
            if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('g') {
                self.app_input_mode = false;
                return Ok(false);
            }
            self.record_child_key_event(key);
            self.source.send_key(key)?;
            return Ok(false);
        }

        if self.focus == FocusPane::Watch && self.should_passthrough_to_app(key) {
            self.record_child_key_event(key);
            self.source.send_key(key)?;
            return Ok(false);
        }

        if self.show_inspector && self.focus == FocusPane::Watch {
            match (key.code, key.modifiers) {
                (KeyCode::Up, KeyModifiers::SHIFT) => {
                    self.inspect_y = self.inspect_y.saturating_sub(1);
                    return Ok(false);
                }
                (KeyCode::Down, KeyModifiers::SHIFT) => {
                    self.inspect_y = self.inspect_y.saturating_add(1);
                    self.clamp_inspector_to_snapshot();
                    return Ok(false);
                }
                (KeyCode::Left, KeyModifiers::SHIFT) => {
                    self.inspect_x = self.inspect_x.saturating_sub(1);
                    return Ok(false);
                }
                (KeyCode::Right, KeyModifiers::SHIFT) => {
                    self.inspect_x = self.inspect_x.saturating_add(1);
                    self.clamp_inspector_to_snapshot();
                    return Ok(false);
                }
                _ => {}
            }
        }

        match (key.code, key.modifiers) {
            (KeyCode::Char('q'), _) => self.show_exit_confirm = true,
            (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                if self.filter_query.is_empty() {
                    self.show_exit_confirm = true;
                } else {
                    self.clear_filter()?;
                }
            }
            (KeyCode::Char('h'), _) => self.show_help = true,
            (KeyCode::Char('I'), _) => {
                self.show_inspector = !self.show_inspector;
                self.clamp_inspector_to_snapshot();
            }
            (KeyCode::Char('i'), _) => {
                if self.focus == FocusPane::Watch {
                    self.app_input_mode = true;
                }
            }
            (KeyCode::Tab, _) => self.toggle_focus(),
            (KeyCode::Left, KeyModifiers::ALT) => {
                self.horizontal_scroll = self.horizontal_scroll.saturating_sub(4)
            }
            (KeyCode::Right, KeyModifiers::ALT) => self.horizontal_scroll += 4,
            (KeyCode::Left, _) => self.focus = FocusPane::Watch,
            (KeyCode::Right, _) => self.focus = FocusPane::History,
            (KeyCode::Backspace, _) => {
                self.show_history = !self.show_history;
                if self.show_history {
                    self.focus = FocusPane::History;
                } else if self.focus == FocusPane::History {
                    self.focus = FocusPane::Watch;
                }
                if !self.show_history {
                    self.show_history_details = false;
                }
            }
            (KeyCode::Char('/'), _) => {
                self.start_search(super::FilterMode::Plain);
            }
            (KeyCode::Char('*'), _) => {
                self.start_search(super::FilterMode::Regex);
            }
            (KeyCode::Char('S'), KeyModifiers::SHIFT) => {
                self.show_history_details = !self.show_history_details;
                if !self.show_history {
                    self.show_history = true;
                    self.focus = FocusPane::History;
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

    fn toggle_child_pause(&mut self) -> Result<()> {
        match self.source.toggle_child_pause()? {
            Some(paused) => {
                self.child_paused = paused;
                self.status_message = Some(if paused {
                    "child process paused".to_string()
                } else {
                    "child process resumed".to_string()
                });
            }
            None => {
                self.child_paused = false;
                self.status_message =
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
                self.show_exit_confirm = false;
                Ok(false)
            }
            (KeyCode::Char(ch), _) if matches!(ch, 'n' | 'N') => {
                self.show_exit_confirm = false;
                Ok(false)
            }
            (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                self.show_exit_confirm = false;
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
                self.filter_query.pop();
                self.rebuild_filter()?;
            }
            KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.filter_query.push(ch);
                self.rebuild_filter()?;
            }
            _ => {}
        }
        Ok(false)
    }

    pub(super) fn handle_mouse(&mut self, mouse: MouseEvent) -> Result<bool> {
        if self.show_exit_confirm {
            return Ok(false);
        }

        self.last_mouse_input = Some(Instant::now());

        if self.app_input_mode {
            self.record_child_mouse_event(mouse);
            self.source.send_mouse(mouse, 2)?;
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
                } else {
                    self.focus = FocusPane::Watch;
                    self.record_child_mouse_event(mouse);
                    self.source.send_mouse(mouse, 2)?;
                    return Ok(previous_focus != self.focus);
                }
            }
            MouseEventKind::ScrollUp => {
                self.last_mouse_scroll_input = Some(Instant::now());
                if over_history {
                    self.focus = FocusPane::History;
                    self.move_up();
                    return Ok(true);
                } else {
                    self.focus = FocusPane::Watch;
                    self.record_child_mouse_event(mouse);
                    self.source.send_mouse(mouse, 2)?;
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
                } else {
                    self.focus = FocusPane::Watch;
                    self.record_child_mouse_event(mouse);
                    self.source.send_mouse(mouse, 2)?;
                    return Ok(previous_focus != self.focus);
                }
            }
            MouseEventKind::Moved => return Ok(false),
            MouseEventKind::Up(_) | MouseEventKind::Drag(_) => {
                if !over_history {
                    self.focus = FocusPane::Watch;
                    self.record_child_mouse_event(mouse);
                    self.source.send_mouse(mouse, 2)?;
                    return Ok(previous_focus != self.focus);
                }
            }
            _ => {}
        }
        Ok(false)
    }

    fn history_overlay_start(&self, total_width: u16) -> u16 {
        let overlay_width = if self.show_history { 48 } else { 2 };
        total_width.saturating_sub(overlay_width)
    }

    fn history_visible_rows(&self) -> usize {
        crossterm::terminal::size()
            .map(|(_, height)| usize::from(height.saturating_sub(4)))
            .unwrap_or(0)
    }

    fn history_content_top(&self) -> u16 {
        3
    }

    fn start_search(&mut self, mode: super::FilterMode) {
        self.input_mode = InputMode::Search;
        self.filter_mode = mode;
    }

    fn clear_filter(&mut self) -> Result<()> {
        self.input_mode = InputMode::Normal;
        self.filter_query.clear();
        self.rebuild_filter()
    }

    fn toggle_focus(&mut self) {
        self.focus = match self.focus {
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

    fn move_up(&mut self) {
        match self.focus {
            FocusPane::Watch => self.watch_scroll = self.watch_scroll.saturating_sub(1),
            FocusPane::History => self.move_history_by(-1),
        }
    }

    fn move_down(&mut self) {
        match self.focus {
            FocusPane::Watch => self.watch_scroll += 1,
            FocusPane::History => self.move_history_by(1),
        }
    }

    fn page_up(&mut self) {
        match self.focus {
            FocusPane::Watch => self.watch_scroll = self.watch_scroll.saturating_sub(10),
            FocusPane::History => self.move_history_by(-10),
        }
    }

    fn page_down(&mut self) {
        match self.focus {
            FocusPane::Watch => self.watch_scroll += 10,
            FocusPane::History => self.move_history_by(10),
        }
    }

    fn move_top(&mut self) {
        match self.focus {
            FocusPane::Watch => self.watch_scroll = 0,
            FocusPane::History => {
                self.follow_latest = true;
                if let Some(first) = self.filtered.first().copied() {
                    self.selected_index = first;
                }
            }
        }
    }

    fn move_end(&mut self) {
        match self.focus {
            FocusPane::Watch => self.watch_scroll = usize::MAX / 2,
            FocusPane::History => {
                if let Some(last) = self.filtered.last().copied() {
                    self.selected_index = last;
                    self.follow_latest = false;
                    self.sync_follow_latest_with_selection();
                }
            }
        }
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

    pub(super) fn sync_follow_latest_with_selection(&mut self) {
        self.follow_latest = false;
    }

    fn move_history_by(&mut self, offset: isize) {
        if self.filtered.is_empty() {
            return;
        }

        if offset < 0 && self.follow_latest {
            return;
        }

        let latest_index = self.filtered[0];
        if self.follow_latest {
            let next = usize::min(offset as usize - 1, self.filtered.len().saturating_sub(1));
            self.follow_latest = false;
            self.selected_index = self.filtered[next];
            return;
        }

        let Some(position) = self.selected_filtered_position() else {
            self.follow_latest = true;
            self.selected_index = latest_index;
            return;
        };

        let next_position = position as isize + offset;
        if next_position < 0 {
            self.follow_latest = true;
            self.selected_index = latest_index;
            return;
        }

        let next = usize::min(
            next_position as usize,
            self.filtered.len().saturating_sub(1),
        );
        self.selected_index = self.filtered[next];
        self.sync_follow_latest_with_selection();
        self.invalidate_view_cache();
    }

    pub(super) fn select_history_row(&mut self, row: usize) {
        if row == 0 {
            self.follow_latest = true;
            if let Some(latest) = self.filtered.first().copied() {
                self.selected_index = latest;
            }
            self.invalidate_view_cache();
            return;
        }

        if let Some(index) = self.filtered.get(row - 1).copied() {
            self.follow_latest = false;
            self.selected_index = index;
            self.sync_follow_latest_with_selection();
            self.invalidate_view_cache();
        }
    }

    pub(super) fn record_child_key_event(&mut self, key: KeyEvent) {
        self.record_input_event(InputTraceKind::Key {
            code: key.code,
            modifiers: key.modifiers,
        });
    }

    pub(super) fn record_child_mouse_event(&mut self, mouse: MouseEvent) {
        self.record_input_event(InputTraceKind::Mouse {
            kind: mouse.kind,
            column: mouse.column,
            row: mouse.row,
        });
    }

    fn record_input_event(&mut self, kind: InputTraceKind) {
        let event = InputTraceEvent {
            seq: self.next_input_seq,
            timestamp_unix_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or(Duration::ZERO)
                .as_millis() as u64,
            kind,
            target_focus: InputTargetFocus::Child,
        };
        self.next_input_seq += 1;
        self.pending_input_events.push(event.clone());
        self.input_trace.push_back(event);
        while self.input_trace.len() > 256 {
            self.input_trace.pop_front();
        }
    }

    pub(super) fn note_resize_event(
        &mut self,
        new_width: u16,
        new_height: u16,
        source: &'static str,
    ) {
        let (old_width, old_height) = self
            .current_snapshot
            .as_ref()
            .map(|snapshot| (snapshot.width(), snapshot.height()))
            .unwrap_or((new_width, new_height));

        if old_width == new_width && old_height == new_height {
            return;
        }

        let event = ResizeTraceEvent {
            timestamp_unix_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or(Duration::ZERO)
                .as_millis() as u64,
            old_width,
            old_height,
            new_width,
            new_height,
            source,
        };
        self.pending_resize_event = Some(event.clone());
        self.resize_trace.push_back(event);
        while self.resize_trace.len() > 64 {
            self.resize_trace.pop_front();
        }
    }

    fn clamp_inspector_to_snapshot(&mut self) {
        let Some(snapshot) = self.selected_snapshot() else {
            self.inspect_x = 0;
            self.inspect_y = 0;
            return;
        };
        self.inspect_x = self.inspect_x.min(snapshot.width().saturating_sub(1));
        self.inspect_y = self.inspect_y.min(snapshot.height().saturating_sub(1));
    }
}
