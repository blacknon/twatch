use anyhow::Result;
use regex::Regex;

use super::{App, FilterMode, FocusPane};
use crate::history::{HistoryMetadata, HistoryStore};
use crate::logging::{LogRecord, append_record, load_records};

impl App {
    pub(super) fn rebuild_filter(&mut self) -> Result<()> {
        self.filtered = self.find_filtered_history()?;
        let visible_start = self.visible_history_start();
        self.filtered.retain(|index| *index >= visible_start);
        self.filtered.reverse();
        if self.filter_query.is_empty() {
            if self.filtered.is_empty() {
                self.follow_latest = true;
            } else if !self.follow_latest && !self.filtered.contains(&self.selected_index) {
                self.selected_index = if self.selected_index < visible_start {
                    *self.filtered.last().unwrap_or(&0)
                } else {
                    *self.filtered.first().unwrap_or(&0)
                };
            }
            return Ok(());
        }

        if self.follow_latest && !self.current_snapshot_matches_filter() {
            if let Some(first) = self.filtered.first().copied() {
                self.selected_index = first;
                self.follow_latest = false;
            }
        } else if !self.follow_latest && !self.filtered.contains(&self.selected_index) {
            self.selected_index = if self.selected_index < visible_start {
                *self.filtered.last().unwrap_or(&0)
            } else {
                *self.filtered.first().unwrap_or(&0)
            };
        }
        Ok(())
    }

    fn find_filtered_history(&mut self) -> Result<Vec<usize>> {
        match self.filter_mode {
            FilterMode::Plain => self.history.find_by_query(&self.filter_query),
            FilterMode::Regex => {
                if self.filter_query.is_empty() {
                    Ok((0..self.history.len()).collect())
                } else {
                    match Regex::new(&self.filter_query) {
                        Ok(regex) => self.history.find_by_regex(&regex),
                        Err(err) => {
                            self.status_message = Some(format!("regex error: {err}"));
                            Ok(Vec::new())
                        }
                    }
                }
            }
        }
    }

    fn current_snapshot_matches_filter(&self) -> bool {
        let Some(snapshot) = &self.current_snapshot else {
            return false;
        };
        let lines = snapshot.lines();

        match self.filter_mode {
            FilterMode::Plain => {
                let needle = self.filter_query.to_lowercase();
                lines
                    .iter()
                    .any(|line| line.to_lowercase().contains(&needle))
            }
            FilterMode::Regex => Regex::new(&self.filter_query)
                .ok()
                .is_some_and(|regex| lines.iter().any(|line| regex.is_match(line))),
        }
    }

    pub(super) fn delete_selected_history(&mut self) -> Result<()> {
        if self.focus != FocusPane::History || self.follow_latest {
            self.status_message = Some("delete works on selected history".to_string());
            return Ok(());
        }
        let selected = self.selected_index;
        self.rebuild_history_retaining(|index| index != selected)?;
        self.follow_latest = true;
        self.status_message = Some("history deleted".to_string());
        Ok(())
    }

    pub(super) fn clear_history_except_selected(&mut self) -> Result<()> {
        if self.follow_latest {
            self.rebuild_history_retaining(|_| false)?;
            self.follow_latest = true;
            self.status_message = Some("history cleared; latest kept".to_string());
            return Ok(());
        }

        let selected = self.selected_index;
        self.rebuild_history_retaining(|index| index == selected)?;
        if !self.history.is_empty() {
            self.follow_latest = false;
            self.selected_index = 0;
        } else {
            self.follow_latest = true;
        }
        self.status_message = Some("history cleared except selected".to_string());
        Ok(())
    }

    fn rebuild_history_retaining<F>(&mut self, mut keep: F) -> Result<()>
    where
        F: FnMut(usize) -> bool,
    {
        let mut rebuilt = HistoryStore::new(self.checkpoint_interval, self.compress);
        let mut rebuilt_meta = Vec::new();

        for index in 0..self.history.len() {
            if !keep(index) {
                continue;
            }
            if let Some(snapshot) = self.history.snapshot(index)? {
                let meta = self.metadata[index].clone();
                rebuilt.push(snapshot, meta.to_history_metadata())?;
                rebuilt_meta.push(meta);
            }
        }

        self.history = rebuilt;
        self.metadata = rebuilt_meta;
        self.invalidate_view_cache();
        self.selected_index = self.history.len().saturating_sub(1);
        self.rebuild_filter()?;
        Ok(())
    }

    pub(super) fn trim_history(&mut self) -> Result<()> {
        let total = self.history.len();
        if total <= self.limit {
            return Ok(());
        }

        let start = total - self.limit;
        let mut rebuilt = HistoryStore::new(self.checkpoint_interval, self.compress);
        let mut rebuilt_meta = Vec::with_capacity(self.limit);

        for index in start..total {
            if let Some(snapshot) = self.history.snapshot(index)? {
                let meta = self.metadata[index].clone();
                rebuilt.push(snapshot, meta.to_history_metadata())?;
                rebuilt_meta.push(meta);
            }
        }

        self.history = rebuilt;
        self.metadata = rebuilt_meta;
        self.invalidate_view_cache();
        if self.history.is_empty() {
            self.follow_latest = true;
            self.selected_index = 0;
        } else if self.selected_index < start {
            self.selected_index = 0;
            self.sync_follow_latest_with_selection();
        } else {
            self.selected_index -= start;
        }
        Ok(())
    }

    pub(super) fn load_history_from_log(&mut self) -> Result<()> {
        let Some(path) = &self.logfile else {
            return Ok(());
        };

        for record in load_records(path)? {
            let (snapshot, metadata, changed) = record.into_parts();
            self.archive_current_snapshot()?;
            let metadata = self.complete_metadata(
                &snapshot,
                super::AppHistoryMetadata::from_history_metadata(HistoryMetadata {
                    changed,
                    ..metadata
                }),
            );
            self.set_current_snapshot(snapshot, metadata);
        }

        if self.metadata.len() > self.limit {
            self.trim_history()?;
        }

        if self.current_snapshot.is_some() {
            self.selected_index = self.history.len().saturating_sub(1);
            self.follow_latest = true;
            self.rebuild_filter()?;
        }

        Ok(())
    }

    pub(super) fn append_log_record(&self) -> Result<()> {
        if self.replay_mode {
            return Ok(());
        }
        let Some(path) = &self.logfile else {
            return Ok(());
        };
        let Some(snapshot) = &self.current_snapshot else {
            return Ok(());
        };
        let Some(metadata) = &self.current_metadata else {
            return Ok(());
        };

        append_record(
            path,
            &LogRecord {
                label: metadata.label.clone(),
                changed: metadata.changed,
                timestamp_unix_ms: metadata.timestamp_unix_ms,
                frame_seq: metadata.frame_seq,
                width: metadata.width,
                height: metadata.height,
                changed_cell_count: metadata.changed_cell_count,
                input_event_count_since_prev: metadata.input_event_count_since_prev,
                resized: metadata.resized,
                resize_from_width: metadata.resize_from_width,
                resize_from_height: metadata.resize_from_height,
                resize_to_width: metadata.resize_to_width,
                resize_to_height: metadata.resize_to_height,
                resize_source: metadata.resize_source.clone(),
                snapshot: snapshot.clone(),
            },
        )
    }
}
