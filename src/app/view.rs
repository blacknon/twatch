// Copyright (c) 2026 Blacknon. All rights reserved.
// Use of this source code is governed by an MIT license
// that can be found in the LICENSE file.

use super::{App, AppHistoryMetadata, ViewCache, ViewCacheKey};
use crate::screen::ScreenSnapshot;

impl App {
    pub(crate) fn history_overlay_width(&self) -> u16 {
        if self.ui.show_history { 36 } else { 2 }
    }

    pub fn visible_history_len(&self) -> usize {
        if self.limit == 0 {
            self.history_len()
        } else {
            self.history_len().min(self.limit)
        }
    }

    pub(super) fn visible_history_start(&self) -> usize {
        if self.limit == 0 {
            0
        } else {
            self.history_len().saturating_sub(self.limit)
        }
    }

    pub fn selected_snapshot(&self) -> Option<ScreenSnapshot> {
        if self.follow_latest && self.live_scrollback_offset > 0 {
            return self.live_scrollback_snapshot.clone();
        }
        self.ensure_view_cache();
        self.view
            .cache
            .borrow()
            .as_ref()
            .and_then(|cache| cache.selected.clone())
    }

    pub fn previous_snapshot(&self) -> Option<ScreenSnapshot> {
        if self.follow_latest && self.live_scrollback_offset > 0 {
            return None;
        }
        self.ensure_view_cache();
        self.view
            .cache
            .borrow()
            .as_ref()
            .and_then(|cache| cache.previous.clone())
    }

    pub fn selected_lines(&self) -> Vec<String> {
        self.selected_snapshot()
            .map(|snapshot| snapshot.lines())
            .unwrap_or_default()
    }

    pub fn previous_lines(&self) -> Option<Vec<String>> {
        self.previous_snapshot().map(|snapshot| snapshot.lines())
    }

    pub fn selected_history_metadata(&self) -> Option<&AppHistoryMetadata> {
        if self.follow_latest {
            self.current_metadata.as_ref()
        } else {
            self.metadata.get(self.selected_index)
        }
    }

    pub fn selected_input_summary(&self) -> Option<&str> {
        if self.follow_latest {
            self.current_metadata
                .as_ref()
                .map(|metadata| metadata.input_summary.as_str())
        } else {
            self.metadata
                .get(self.selected_index)
                .map(|metadata| metadata.input_summary.as_str())
        }
        .filter(|summary| !summary.is_empty())
    }

    pub fn inspect_cursor(&self) -> (u16, u16) {
        (self.ui.inspect_x, self.ui.inspect_y)
    }

    pub fn selected_resize_summary(&self) -> Option<String> {
        let metadata = if self.follow_latest {
            self.current_metadata.as_ref()
        } else {
            self.metadata.get(self.selected_index)
        }?;

        if !metadata.resized {
            return None;
        }

        Some(format!(
            "resize: {}x{} -> {}x{} ({})",
            metadata.resize_from_width,
            metadata.resize_from_height,
            metadata.resize_to_width,
            metadata.resize_to_height,
            metadata.resize_source
        ))
    }

    pub fn selected_filtered_position(&self) -> Option<usize> {
        self.filtered
            .iter()
            .position(|idx| *idx == self.selected_index)
    }

    pub fn selected_history_row(&self) -> usize {
        if self.follow_latest {
            0
        } else {
            self.selected_filtered_position()
                .map(|position| position + 1)
                .unwrap_or(0)
        }
    }

    pub fn history_overlay_window(&self, visible_rows: usize) -> (usize, usize) {
        let total_rows = self.filtered.len() + 1;
        if visible_rows == 0 || total_rows == 0 {
            return (0, 0);
        }
        if total_rows <= visible_rows {
            return (0, total_rows);
        }

        let selected_row = self.selected_history_row();
        let half = visible_rows / 2;
        let max_start = total_rows.saturating_sub(visible_rows);
        let start = selected_row.saturating_sub(half).min(max_start);
        let end = usize::min(start + visible_rows, total_rows);
        (start, end)
    }

    pub fn selected_history_row_in_window(&self, visible_rows: usize) -> usize {
        let (start, end) = self.history_overlay_window(visible_rows);
        if start >= end {
            return 0;
        }
        self.selected_history_row().saturating_sub(start)
    }

    pub fn select_history_overlay_row(&mut self, row: usize, visible_rows: usize) {
        let (start, end) = self.history_overlay_window(visible_rows);
        if start >= end {
            return;
        }
        self.select_history_row(start + row);
    }

    fn ensure_view_cache(&self) {
        let key = self.current_view_cache_key();
        if self
            .view
            .cache
            .borrow()
            .as_ref()
            .is_some_and(|cache| cache.key == key)
        {
            return;
        }

        let (selected, previous) = if self.follow_latest {
            let selected = self.current_snapshot.clone();
            let previous = if self.history.is_empty() {
                None
            } else {
                self.history.snapshot(self.history.len() - 1).ok().flatten()
            };
            (selected, previous)
        } else {
            let selected = self.history.snapshot(self.selected_index).ok().flatten();
            let previous = if self.selected_index == 0 {
                None
            } else {
                self.history
                    .snapshot(self.selected_index - 1)
                    .ok()
                    .flatten()
            };
            (selected, previous)
        };

        *self.view.cache.borrow_mut() = Some(ViewCache {
            key,
            selected,
            previous,
        });
    }

    fn current_view_cache_key(&self) -> ViewCacheKey {
        ViewCacheKey {
            follow_latest: self.follow_latest,
            selected_index: self.selected_index,
            history_len: self.history.len(),
            current_frame_seq: self
                .current_metadata
                .as_ref()
                .map(|metadata| metadata.frame_seq)
                .unwrap_or(0),
        }
    }

    pub(super) fn invalidate_view_cache(&self) {
        *self.view.cache.borrow_mut() = None;
    }

    pub(super) fn clear_live_scrollback_view(&mut self) {
        self.live_scrollback_offset = 0;
        self.live_scrollback_snapshot = None;
    }

    pub(super) fn reset_watch_viewport(&mut self) {
        self.ui.watch_scroll = 0;
        self.ui.horizontal_scroll = 0;
    }

    pub(super) fn scroll_selected_watch_view(&mut self, delta: isize) -> bool {
        let max_offset = self
            .selected_snapshot()
            .map(|snapshot| usize::from(snapshot.height().saturating_sub(1)))
            .unwrap_or(0);
        let next = if delta >= 0 {
            self.ui.watch_scroll.saturating_add(delta as usize)
        } else {
            self.ui.watch_scroll.saturating_sub(delta.unsigned_abs())
        }
        .min(max_offset);
        let changed = next != self.ui.watch_scroll;
        self.ui.watch_scroll = next;
        changed
    }

    pub(super) fn refresh_live_scrollback_view(&mut self) -> anyhow::Result<()> {
        self.refresh_live_scrollback_view_at(self.live_scrollback_offset)
    }

    fn refresh_live_scrollback_view_at(&mut self, offset: usize) -> anyhow::Result<()> {
        if offset == 0 {
            self.clear_live_scrollback_view();
            return Ok(());
        }
        let (width, height) = crossterm::terminal::size().unwrap_or((120, 40));
        let Some(snapshot) = self
            .source
            .view_snapshot(width, height.saturating_sub(2), offset)?
        else {
            self.clear_live_scrollback_view();
            return Ok(());
        };
        self.live_scrollback_offset = snapshot.scrollback_offset();
        self.live_scrollback_snapshot = Some(snapshot);
        Ok(())
    }

    pub(super) fn scroll_main_screen_view(&mut self, delta: isize) -> anyhow::Result<bool> {
        let target = if delta >= 0 {
            self.live_scrollback_offset.saturating_add(delta as usize)
        } else {
            self.live_scrollback_offset
                .saturating_sub(delta.unsigned_abs())
        };
        if target == self.live_scrollback_offset && self.live_scrollback_offset == 0 {
            return Ok(false);
        }
        let previous_offset = self.live_scrollback_offset;
        self.refresh_live_scrollback_view_at(target)?;
        Ok(self.live_scrollback_offset != previous_offset)
    }
}
