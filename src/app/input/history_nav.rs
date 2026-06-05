// Copyright (c) 2026 Blacknon. All rights reserved.
// Use of this source code is governed by an MIT license
// that can be found in the LICENSE file.

use crate::app::{App, FocusPane};

impl App {
    pub(crate) fn move_up(&mut self) {
        if !self.follow_latest {
            self.move_history_by(-1);
            return;
        }
        match self.ui.focus {
            FocusPane::Watch => self.ui.watch_scroll = self.ui.watch_scroll.saturating_sub(1),
            FocusPane::History => self.move_history_by(-1),
        }
    }

    pub(crate) fn move_down(&mut self) {
        if !self.follow_latest {
            self.move_history_by(1);
            return;
        }
        match self.ui.focus {
            FocusPane::Watch => self.ui.watch_scroll += 1,
            FocusPane::History => self.move_history_by(1),
        }
    }

    pub(crate) fn page_up(&mut self) {
        if !self.follow_latest {
            self.move_history_by(-10);
            return;
        }
        match self.ui.focus {
            FocusPane::Watch => self.ui.watch_scroll = self.ui.watch_scroll.saturating_sub(10),
            FocusPane::History => self.move_history_by(-10),
        }
    }

    pub(crate) fn page_down(&mut self) {
        if !self.follow_latest {
            self.move_history_by(10);
            return;
        }
        match self.ui.focus {
            FocusPane::Watch => self.ui.watch_scroll += 10,
            FocusPane::History => self.move_history_by(10),
        }
    }

    pub(crate) fn move_top(&mut self) {
        if !self.follow_latest {
            self.follow_latest = true;
            if let Some(first) = self.filtered.first().copied() {
                self.selected_index = first;
            }
            self.invalidate_view_cache();
            return;
        }
        match self.ui.focus {
            FocusPane::Watch => self.ui.watch_scroll = 0,
            FocusPane::History => {
                self.follow_latest = true;
                if let Some(first) = self.filtered.first().copied() {
                    self.selected_index = first;
                }
            }
        }
    }

    pub(crate) fn move_end(&mut self) {
        if !self.follow_latest {
            if let Some(last) = self.filtered.last().copied() {
                self.selected_index = last;
                self.follow_latest = false;
                self.sync_follow_latest_with_selection();
                self.invalidate_view_cache();
            }
            return;
        }
        match self.ui.focus {
            FocusPane::Watch => self.ui.watch_scroll = usize::MAX / 2,
            FocusPane::History => {
                if let Some(last) = self.filtered.last().copied() {
                    self.selected_index = last;
                    self.follow_latest = false;
                    self.sync_follow_latest_with_selection();
                }
            }
        }
    }

    pub(crate) fn sync_follow_latest_with_selection(&mut self) {
        self.follow_latest = false;
    }

    pub(super) fn move_history_by(&mut self, offset: isize) {
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

        if offset > 0
            && next_position as usize >= self.filtered.len().saturating_sub(1)
            && !self.replay_loading
            && self.replay_deferred_path.is_some()
        {
            if self.start_deferred_replay_loader() {
                self.ui.status_message =
                    Some("loading older replay history in background".to_string());
            }
        }

        let next = usize::min(
            next_position as usize,
            self.filtered.len().saturating_sub(1),
        );
        self.selected_index = self.filtered[next];
        self.sync_follow_latest_with_selection();
        self.invalidate_view_cache();
    }

    pub(crate) fn select_history_row(&mut self, row: usize) {
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
}
