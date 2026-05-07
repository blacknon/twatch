use anyhow::Result;
use regex::Regex;

use super::{App, FilterMode, FocusPane};
use crate::history::{HistoryMetadata, HistoryStore};
use crate::logging::{LogRecord, append_record, load_records};

impl App {
    pub(super) fn rebuild_filter(&mut self) -> Result<()> {
        self.filtered = match self.filter_mode {
            FilterMode::Plain => self.history.find_by_query(&self.filter_query)?,
            FilterMode::Regex => {
                if self.filter_query.is_empty() {
                    (0..self.history.len()).collect()
                } else {
                    match Regex::new(&self.filter_query) {
                        Ok(regex) => self.history.find_by_regex(&regex)?,
                        Err(err) => {
                            self.status_message = Some(format!("regex error: {err}"));
                            Vec::new()
                        }
                    }
                }
            }
        };
        self.filtered.reverse();
        if self.filter_query.is_empty() {
            if self.filtered.is_empty() {
                self.follow_latest = true;
            } else if !self.follow_latest && !self.filtered.contains(&self.selected_index) {
                self.selected_index = *self.filtered.first().unwrap_or(&0);
            }
            return Ok(());
        }

        if self.filtered.is_empty() {
            self.follow_latest = true;
        } else if !self.follow_latest && !self.filtered.contains(&self.selected_index) {
            self.selected_index = *self.filtered.first().unwrap_or(&0);
        }
        Ok(())
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
                rebuilt.push(
                    snapshot,
                    HistoryMetadata {
                        label: meta.label.clone(),
                    },
                )?;
                rebuilt_meta.push(meta);
            }
        }

        self.history = rebuilt;
        self.metadata = rebuilt_meta;
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
                rebuilt.push(
                    snapshot,
                    HistoryMetadata {
                        label: meta.label.clone(),
                    },
                )?;
                rebuilt_meta.push(meta);
            }
        }

        self.history = rebuilt;
        self.metadata = rebuilt_meta;
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
            self.set_current_snapshot(snapshot, metadata.label, changed);
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
        let Some(path) = &self.logfile else {
            return Ok(());
        };
        let Some(snapshot) = &self.current_snapshot else {
            return Ok(());
        };
        let Some(label) = &self.current_label else {
            return Ok(());
        };

        append_record(
            path,
            &LogRecord {
                label: label.clone(),
                changed: self.current_changed,
                snapshot: snapshot.clone(),
            },
        )
    }
}
