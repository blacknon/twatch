// Copyright (c) 2026 Blacknon. All rights reserved.
// Use of this source code is governed by an MIT license
// that can be found in the LICENSE file.

use std::io::{Read, Write};

use anyhow::{Context, Result};
use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use regex::Regex;
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::diff::{LineDiff, WordDiff, diff_lines, diff_words};
use crate::screen::{Cell, ScreenSnapshot};

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HistoryMetadata {
    pub label: String,
    pub timestamp_unix_ms: u64,
    pub frame_seq: u64,
    pub changed: bool,
    pub width: u16,
    pub height: u16,
    pub changed_cell_count: usize,
    pub input_event_count_since_prev: usize,
    pub resized: bool,
    pub resize_from_width: u16,
    pub resize_from_height: u16,
    pub resize_to_width: u16,
    pub resize_to_height: u16,
    pub resize_source: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
enum HistoryEntryKind {
    Checkpoint(StoredPayload<ScreenSnapshot>),
    Delta(StoredPayload<FrameDelta>),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct HistoryEntry {
    kind: HistoryEntryKind,
    metadata: HistoryMetadata,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct FrameDelta {
    width: u16,
    height: u16,
    changes: Vec<(usize, Cell)>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
enum StoredPayload<T> {
    Raw(T),
    Compressed(Vec<u8>),
}

#[derive(Clone, Debug, Default)]
pub struct HistoryStats {
    pub checkpoints: usize,
    pub deltas: usize,
    pub compressed_entries: usize,
}

#[derive(Clone, Debug)]
pub struct HistoryStore {
    checkpoint_interval: usize,
    compress: bool,
    entries: Vec<HistoryEntry>,
    last_snapshot: Option<ScreenSnapshot>,
}

impl HistoryStore {
    pub fn new(checkpoint_interval: usize, compress: bool) -> Self {
        Self {
            checkpoint_interval: checkpoint_interval.max(1),
            compress,
            entries: Vec::new(),
            last_snapshot: None,
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn push(&mut self, snapshot: ScreenSnapshot, metadata: HistoryMetadata) -> Result<()> {
        let entry = match self.entries.last() {
            None => HistoryEntry {
                kind: HistoryEntryKind::Checkpoint(store_checkpoint(
                    snapshot.clone(),
                    self.compress,
                )?),
                metadata,
            },
            Some(_) if self.entries.len() % self.checkpoint_interval == 0 => HistoryEntry {
                kind: HistoryEntryKind::Checkpoint(store_checkpoint(
                    snapshot.clone(),
                    self.compress,
                )?),
                metadata,
            },
            Some(_) => {
                let previous = self
                    .last_snapshot
                    .as_ref()
                    .cloned()
                    .or_else(|| self.snapshot(self.entries.len() - 1).ok().flatten())
                    .expect("previous snapshot must exist");
                HistoryEntry {
                    kind: HistoryEntryKind::Delta(store_delta(
                        FrameDelta::between(&previous, &snapshot),
                        self.compress,
                    )?),
                    metadata,
                }
            }
        };

        self.entries.push(entry);
        self.last_snapshot = Some(snapshot);
        Ok(())
    }

    pub fn snapshot(&self, index: usize) -> Result<Option<ScreenSnapshot>> {
        if index + 1 == self.entries.len() {
            if let Some(snapshot) = &self.last_snapshot {
                return Ok(Some(snapshot.clone()));
            }
        }
        let entry = match self.entries.get(index) {
            Some(entry) => entry,
            None => return Ok(None),
        };
        match &entry.kind {
            HistoryEntryKind::Checkpoint(snapshot) => Ok(Some(load_payload(snapshot)?)),
            HistoryEntryKind::Delta(_) => {
                let checkpoint_index = match self.find_checkpoint(index) {
                    Some(index) => index,
                    None => return Ok(None),
                };
                let mut snapshot = match &self.entries[checkpoint_index].kind {
                    HistoryEntryKind::Checkpoint(snapshot) => load_payload(snapshot)?,
                    HistoryEntryKind::Delta(_) => return Ok(None),
                };

                for entry in &self.entries[(checkpoint_index + 1)..=index] {
                    if let HistoryEntryKind::Delta(delta) = &entry.kind {
                        load_payload(delta)?.apply(&mut snapshot);
                    }
                }

                Ok(Some(snapshot))
            }
        }
    }

    pub fn metadata(&self, index: usize) -> Option<&HistoryMetadata> {
        self.entries.get(index).map(|entry| &entry.metadata)
    }

    pub fn find_by_query(&self, query: &str) -> Result<Vec<usize>> {
        if query.is_empty() {
            return Ok((0..self.entries.len()).collect());
        }

        let needle = query.to_lowercase();
        let mut matches = Vec::new();
        for index in 0..self.entries.len() {
            let Some(snapshot) = self.snapshot(index)? else {
                continue;
            };
            if snapshot
                .lines()
                .iter()
                .any(|line| line.to_lowercase().contains(&needle))
            {
                matches.push(index);
            }
        }
        Ok(matches)
    }

    pub fn find_by_regex(&self, regex: &Regex) -> Result<Vec<usize>> {
        let mut matches = Vec::new();
        for index in 0..self.entries.len() {
            let Some(snapshot) = self.snapshot(index)? else {
                continue;
            };
            if snapshot.lines().iter().any(|line| regex.is_match(line)) {
                matches.push(index);
            }
        }
        Ok(matches)
    }

    pub fn line_diffs(&self, before: usize, after: usize) -> Result<Option<Vec<LineDiff>>> {
        let before = match self.snapshot(before)? {
            Some(snapshot) => snapshot,
            None => return Ok(None),
        };
        let after = match self.snapshot(after)? {
            Some(snapshot) => snapshot,
            None => return Ok(None),
        };
        Ok(Some(diff_lines(&before.lines(), &after.lines())))
    }

    pub fn word_diffs(&self, before: usize, after: usize) -> Result<Option<Vec<WordDiff>>> {
        let line_diffs = match self.line_diffs(before, after)? {
            Some(diffs) => diffs,
            None => return Ok(None),
        };
        Ok(Some(
            line_diffs
                .iter()
                .map(|diff| diff_words(&diff.before, &diff.after, diff.line_index))
                .collect(),
        ))
    }

    pub fn stats(&self) -> HistoryStats {
        let mut stats = HistoryStats::default();
        for entry in &self.entries {
            match &entry.kind {
                HistoryEntryKind::Checkpoint(payload) => {
                    stats.checkpoints += 1;
                    if matches!(payload, StoredPayload::Compressed(_)) {
                        stats.compressed_entries += 1;
                    }
                }
                HistoryEntryKind::Delta(payload) => {
                    stats.deltas += 1;
                    if matches!(payload, StoredPayload::Compressed(_)) {
                        stats.compressed_entries += 1;
                    }
                }
            }
        }
        stats
    }

    fn find_checkpoint(&self, index: usize) -> Option<usize> {
        (0..=index).rev().find(|idx| {
            self.entries
                .get(*idx)
                .is_some_and(|entry| matches!(entry.kind, HistoryEntryKind::Checkpoint(_)))
        })
    }
}

impl FrameDelta {
    fn between(before: &ScreenSnapshot, after: &ScreenSnapshot) -> Self {
        let mut changes = Vec::new();

        let after_width = after.width();
        let after_height = after.height();
        let max_width = before.width().max(after_width);
        let max_height = before.height().max(after_height);

        for y in 0..max_height {
            for x in 0..max_width {
                let idx = usize::from(y) * usize::from(after_width.max(1)) + usize::from(x);
                let before_cell = before.cell(x, y).unwrap_or_default();
                let after_cell = after.cell(x, y).unwrap_or_default();

                if before_cell != after_cell && x < after_width && y < after_height {
                    changes.push((idx, after_cell));
                }
            }
        }

        Self {
            width: after_width,
            height: after_height,
            changes,
        }
    }

    fn apply(&self, snapshot: &mut ScreenSnapshot) {
        snapshot.resize(self.width, self.height);
        snapshot.apply_changes(&self.changes);
    }
}

fn store_checkpoint(
    value: ScreenSnapshot,
    compress: bool,
) -> Result<StoredPayload<ScreenSnapshot>> {
    store_payload(value, compress)
}

fn store_delta(value: FrameDelta, compress: bool) -> Result<StoredPayload<FrameDelta>> {
    store_payload(value, compress)
}

fn store_payload<T>(value: T, compress: bool) -> Result<StoredPayload<T>>
where
    T: Serialize,
{
    if !compress {
        return Ok(StoredPayload::Raw(value));
    }

    let bytes = serde_json::to_vec(&value).context("failed to serialize history payload")?;
    let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
    encoder
        .write_all(&bytes)
        .context("failed to compress history payload")?;
    Ok(StoredPayload::Compressed(
        encoder
            .finish()
            .context("failed to finalize history compression")?,
    ))
}

fn load_payload<T>(payload: &StoredPayload<T>) -> Result<T>
where
    T: Clone + DeserializeOwned,
{
    match payload {
        StoredPayload::Raw(value) => Ok(value.clone()),
        StoredPayload::Compressed(bytes) => {
            let mut decoder = GzDecoder::new(bytes.as_slice());
            let mut out = Vec::new();
            decoder
                .read_to_end(&mut out)
                .context("failed to decompress history payload")?;
            serde_json::from_slice(&out).context("failed to deserialize history payload")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{HistoryMetadata, HistoryStore};
    use crate::screen::ScreenSnapshot;

    #[test]
    fn reconstructs_snapshots_from_checkpoints_and_deltas() {
        let mut history = HistoryStore::new(3, false);

        history
            .push(
                ScreenSnapshot::from_text_lines(6, 2, &["alpha", ""]),
                meta("t0"),
            )
            .unwrap();
        history
            .push(
                ScreenSnapshot::from_text_lines(6, 2, &["alpha", "beta"]),
                meta("t1"),
            )
            .unwrap();
        history
            .push(
                ScreenSnapshot::from_text_lines(6, 2, &["gamma", "beta"]),
                meta("t2"),
            )
            .unwrap();
        history
            .push(
                ScreenSnapshot::from_text_lines(6, 2, &["gamma", "delta"]),
                meta("t3"),
            )
            .unwrap();

        assert_eq!(
            history.snapshot(3).unwrap().expect("snapshot").lines(),
            vec!["gamma".to_string(), "delta".to_string()]
        );

        let stats = history.stats();
        assert_eq!((stats.checkpoints, stats.deltas), (2, 2));
    }

    #[test]
    fn supports_compressed_entries() {
        let mut history = HistoryStore::new(2, true);
        history
            .push(
                ScreenSnapshot::from_text_lines(8, 1, &["jobs 10"]),
                meta("a"),
            )
            .unwrap();
        history
            .push(
                ScreenSnapshot::from_text_lines(8, 1, &["jobs 11"]),
                meta("b"),
            )
            .unwrap();
        history
            .push(
                ScreenSnapshot::from_text_lines(8, 1, &["queue 12"]),
                meta("c"),
            )
            .unwrap();

        assert_eq!(history.find_by_query("jobs").unwrap(), vec![0, 1]);
        assert_eq!(history.stats().compressed_entries, 3);
    }

    #[test]
    fn supports_regex_search() {
        let mut history = HistoryStore::new(2, false);
        history
            .push(
                ScreenSnapshot::from_text_lines(20, 1, &["worker-01 running"]),
                meta("a"),
            )
            .unwrap();
        history
            .push(
                ScreenSnapshot::from_text_lines(20, 1, &["worker-02 failed"]),
                meta("b"),
            )
            .unwrap();

        let regex = regex::Regex::new(r"worker-\d{2} failed").unwrap();
        assert_eq!(history.find_by_regex(&regex).unwrap(), vec![1]);
    }

    #[test]
    fn computes_line_and_word_diffs() {
        let mut history = HistoryStore::new(10, false);
        history
            .push(
                ScreenSnapshot::from_text_lines(16, 1, &["task pending"]),
                meta("a"),
            )
            .unwrap();
        history
            .push(
                ScreenSnapshot::from_text_lines(16, 1, &["task running"]),
                meta("b"),
            )
            .unwrap();

        let line_diffs = history.line_diffs(0, 1).unwrap().expect("line diffs");
        assert_eq!(line_diffs.len(), 1);
        assert_eq!(line_diffs[0].before, "task pending");
        assert_eq!(line_diffs[0].after, "task running");

        let word_diffs = history.word_diffs(0, 1).unwrap().expect("word diffs");
        assert_eq!(word_diffs[0].removed, vec!["pending".to_string()]);
        assert_eq!(word_diffs[0].added, vec!["running".to_string()]);
    }

    fn meta(label: &str) -> HistoryMetadata {
        HistoryMetadata {
            label: label.to_string(),
            ..HistoryMetadata::default()
        }
    }
}
