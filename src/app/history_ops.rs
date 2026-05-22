// Copyright (c) 2026 Blacknon. All rights reserved.
// Use of this source code is governed by an MIT license
// that can be found in the LICENSE file.

use anyhow::Result;
use std::path::Path;

use super::{App, AppHistoryMetadata, FocusPane, ReplayReplaceState};
use crate::history::{HistoryMetadata, HistoryStore};
use crate::logging::{
    LogRecord, LogRecordStream, append_delta_record, load_recent_records_from_cache,
    load_recent_records_from_plain_path, load_recent_records_from_single_path, load_records,
    load_records_from_single_path, replay_path_info,
};

impl App {
    pub(super) fn delete_selected_history(&mut self) -> Result<()> {
        if self.ui.focus != FocusPane::History || self.follow_latest {
            self.ui.status_message = Some("delete works on selected history".to_string());
            return Ok(());
        }
        let selected = self.selected_index;
        self.rebuild_history_retaining(|index| index != selected)?;
        self.follow_latest = true;
        self.ui.status_message = Some("history deleted".to_string());
        Ok(())
    }

    pub(super) fn clear_history_except_selected(&mut self) -> Result<()> {
        if self.follow_latest {
            self.rebuild_history_retaining(|_| false)?;
            self.follow_latest = true;
            self.ui.status_message = Some("history cleared; latest kept".to_string());
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
        self.ui.status_message = Some("history cleared except selected".to_string());
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
        if self.limit == 0 {
            return Ok(());
        }
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
        } else if self.follow_latest {
            self.selected_index = self.history.len().saturating_sub(1);
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

        self.apply_loaded_records(load_records(path)?)?;

        Ok(())
    }

    pub(super) fn load_history_from_replay_log_prefetch(&mut self) -> Result<()> {
        let Some(path) = self.logfile.clone() else {
            return Ok(());
        };
        if !Path::new(&path).exists() {
            return Ok(());
        }

        let replay_info = replay_path_info(&path)?;
        let replay_uses_manifest = path.ends_with(".replay.json");

        if !replay_info.spill_paths.is_empty() {
            let latest_spill_records = replay_info
                .spill_paths
                .last()
                .filter(|spill_path| {
                    std::fs::metadata(spill_path)
                        .ok()
                        .map(|meta| meta.len() <= super::state::REPLAY_SYNC_LATEST_SPILL_MAX_BYTES)
                        .unwrap_or(false)
                })
                .map(|spill_path| load_records_from_single_path(spill_path))
                .transpose()?
                .unwrap_or_default();
            let cached = load_recent_records_from_cache(
                &path,
                super::state::replay_prefetch_record_count(),
            )?;
            if !cached.is_empty() {
                let mut loaded = latest_spill_records;
                loaded.extend(cached);
                self.apply_loaded_records(loaded)?;
                if replay_uses_manifest {
                    self.replay_reload_path = Some(path);
                    self.replay_loading = true;
                    self.ui.status_message = Some(format!(
                        "replay loading recent cache: {} history frames ready, older history loading in background",
                        self.metadata.len()
                    ));
                } else {
                    self.replay_deferred_path = Some(path);
                    self.replay_loading = false;
                    self.ui.status_message = Some(format!(
                        "replay loaded recent cache: {} history frames ready, load older history on demand",
                        self.metadata.len()
                    ));
                }
                return Ok(());
            }

            let total_spill_size_bytes: u64 = replay_info
                .spill_paths
                .iter()
                .filter_map(|spill_path| std::fs::metadata(spill_path).ok().map(|meta| meta.len()))
                .sum();
            if total_spill_size_bytes <= super::state::REPLAY_SYNC_SMALL_SPILL_MAX_BYTES {
                self.apply_loaded_records(load_records(&path)?)?;
                self.ui.status_message = Some(format!(
                    "replay loaded: {} frames",
                    self.loaded_frame_count()
                ));
                return Ok(());
            }
            let loaded = load_recent_records_from_single_path(
                &path,
                super::state::replay_prefetch_record_count(),
            )?;
            let mut prefetched = latest_spill_records;
            prefetched.extend(loaded);
            self.apply_loaded_records(prefetched)?;
            if replay_uses_manifest {
                self.replay_reload_path = Some(path);
                self.replay_loading = true;
                self.ui.status_message = Some(format!(
                    "replay loading recent tail: {} history frames ready, older history loading in background",
                    self.metadata.len()
                ));
            } else {
                self.replay_deferred_path = Some(path);
                self.replay_loading = false;
                self.ui.status_message = Some(format!(
                    "replay loaded recent tail: {} history frames ready, load older history on demand",
                    self.metadata.len()
                ));
            }
            return Ok(());
        }

        if path.ends_with(".jsonl") {
            let loaded = load_recent_records_from_plain_path(
                &path,
                super::state::replay_prefetch_record_count(),
            )?;
            if !loaded.is_empty() {
                self.apply_loaded_records(loaded)?;
                self.replay_reload_path = Some(path);
                self.replay_loading = true;
                self.ui.status_message = Some(format!(
                    "replay loading recent tail: {} history frames ready, older history loading in background",
                    self.metadata.len()
                ));
                return Ok(());
            }
        }

        let mut stream = LogRecordStream::open(&path)?;
        let mut loaded = Vec::new();

        while loaded.len() < super::state::replay_prefetch_record_count() {
            let Some(record) = stream.next_record()? else {
                break;
            };
            loaded.push(record);
        }

        self.apply_loaded_records(loaded)?;

        if stream.has_more()? {
            self.replay_loader = Some(stream);
            self.replay_loading = true;
            self.ui.status_message = Some(format!(
                "replay loading: {} history frames ready, more loading in background",
                self.metadata.len()
            ));
        }

        Ok(())
    }

    pub(super) fn replace_loaded_replay_state(&mut self, state: ReplayReplaceState) -> Result<()> {
        let preserve_follow_latest = self.follow_latest;
        let preserve_frame_seq = if self.follow_latest {
            self.current_metadata
                .as_ref()
                .map(|metadata| metadata.frame_seq)
        } else {
            self.metadata
                .get(self.selected_index)
                .map(|metadata| metadata.frame_seq)
        };

        self.history = state.history;
        self.metadata = state.metadata;
        self.current_snapshot = state.current_snapshot;
        self.current_metadata = state.current_metadata;
        self.filtered.clear();
        self.selected_index = 0;
        self.follow_latest = preserve_follow_latest;
        self.invalidate_view_cache();

        if self.limit > 0 && self.metadata.len() > self.limit {
            self.trim_history()?;
        }

        if self.current_snapshot.is_some() {
            if preserve_follow_latest {
                self.selected_index = self.history.len().saturating_sub(1);
                self.follow_latest = true;
            }
            self.rebuild_filter()?;
        }

        if !preserve_follow_latest {
            if let Some(frame_seq) = preserve_frame_seq
                && let Some(index) = self
                    .metadata
                    .iter()
                    .position(|metadata| metadata.frame_seq == frame_seq)
            {
                self.follow_latest = false;
                self.selected_index = index;
                self.rebuild_filter()?;
            }
        }

        Ok(())
    }

    pub(super) fn apply_loaded_records(&mut self, records: Vec<LogRecord>) -> Result<()> {
        let was_follow_latest = self.follow_latest;

        for record in records {
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

        if self.limit > 0 && self.metadata.len() > self.limit {
            self.trim_history()?;
        }

        if self.current_snapshot.is_some() {
            if was_follow_latest {
                self.selected_index = self.history.len().saturating_sub(1);
                self.follow_latest = true;
            }
            self.rebuild_filter()?;
        }

        Ok(())
    }

    pub(super) fn loaded_frame_count(&self) -> usize {
        self.metadata.len() + usize::from(self.current_snapshot.is_some())
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

        let previous_snapshot = if self.history.is_empty() {
            None
        } else {
            self.history.snapshot(self.history.len() - 1)?
        };

        append_delta_record(
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
            previous_snapshot.as_ref(),
            self.checkpoint_interval as u64,
        )
    }
}

pub(super) fn build_replay_replace_state(
    path: &str,
    checkpoint_interval: usize,
    compress: bool,
) -> Result<ReplayReplaceState> {
    let mut loader = LogRecordStream::open(path)?;
    let mut history = HistoryStore::new(checkpoint_interval, compress);
    let mut metadata = Vec::new();
    let mut current_snapshot = None;
    let mut current_metadata: Option<AppHistoryMetadata> = None;

    while let Some(record) = loader.next_record()? {
        let (snapshot, record_metadata, changed) = record.into_parts();

        if let (Some(previous_snapshot), Some(previous_metadata)) =
            (current_snapshot.take(), current_metadata.take())
        {
            history.push(previous_snapshot, previous_metadata.to_history_metadata())?;
            metadata.push(previous_metadata);
        }

        current_snapshot = Some(snapshot);
        current_metadata = Some(AppHistoryMetadata::from_history_metadata(HistoryMetadata {
            changed,
            ..record_metadata
        }));
    }

    Ok(ReplayReplaceState {
        history,
        metadata,
        current_snapshot,
        current_metadata,
    })
}
