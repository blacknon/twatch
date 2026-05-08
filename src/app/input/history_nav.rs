use crate::app::{App, FocusPane};

impl App {
    pub(crate) fn move_up(&mut self) {
        match self.focus {
            FocusPane::Watch => self.watch_scroll = self.watch_scroll.saturating_sub(1),
            FocusPane::History => self.move_history_by(-1),
        }
    }

    pub(crate) fn move_down(&mut self) {
        match self.focus {
            FocusPane::Watch => self.watch_scroll += 1,
            FocusPane::History => self.move_history_by(1),
        }
    }

    pub(crate) fn page_up(&mut self) {
        match self.focus {
            FocusPane::Watch => self.watch_scroll = self.watch_scroll.saturating_sub(10),
            FocusPane::History => self.move_history_by(-10),
        }
    }

    pub(crate) fn page_down(&mut self) {
        match self.focus {
            FocusPane::Watch => self.watch_scroll += 10,
            FocusPane::History => self.move_history_by(10),
        }
    }

    pub(crate) fn move_top(&mut self) {
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

    pub(crate) fn move_end(&mut self) {
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

    pub(crate) fn sync_follow_latest_with_selection(&mut self) {
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
