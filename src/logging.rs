// Copyright (c) 2026 Blacknon. All rights reserved.
// Use of this source code is governed by an MIT license
// that can be found in the LICENSE file.

use std::collections::VecDeque;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::Path;

use anyhow::{Context, Result};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::read::MultiGzDecoder;
use flate2::write::GzEncoder;
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::history::HistoryMetadata;
use crate::screen::{Cell, ScreenSnapshot, Style, Symbol};
const LOG_INLINE_PAYLOAD_MIN_BYTES: usize = 96;
const COMPACT_ARCHIVE_EXTENSION: &str = ".twar";
const ACTIVE_SPILL_EXTENSION: &str = ".spill.gz";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct LogRecord {
    pub label: String,
    pub changed: bool,
    pub timestamp_unix_ms: u64,
    pub frame_seq: u64,
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
    pub snapshot: ScreenSnapshot,
}

#[derive(Clone, Debug)]
struct StoredLogRecord {
    label: String,
    changed: bool,
    timestamp_unix_ms: u64,
    frame_seq: u64,
    width: u16,
    height: u16,
    changed_cell_count: usize,
    input_event_count_since_prev: usize,
    resized: bool,
    resize_from_width: u16,
    resize_from_height: u16,
    resize_to_width: u16,
    resize_to_height: u16,
    resize_source: String,
    snapshot: Option<InlinePayload<ScreenSnapshot>>,
    delta: Option<InlinePayload<LogFrameDelta>>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct LegacyStoredLogRecordSerde {
    label: String,
    changed: bool,
    timestamp_unix_ms: u64,
    frame_seq: u64,
    width: u16,
    height: u16,
    changed_cell_count: usize,
    input_event_count_since_prev: usize,
    resized: bool,
    resize_from_width: u16,
    resize_from_height: u16,
    resize_to_width: u16,
    resize_to_height: u16,
    resize_source: String,
    snapshot: Option<InlinePayload<ScreenSnapshot>>,
    delta: Option<InlinePayload<LogFrameDelta>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct CompactStoredLogRecordSerde(
    String,
    bool,
    u64,
    u64,
    u16,
    u16,
    usize,
    usize,
    bool,
    u16,
    u16,
    u16,
    u16,
    String,
    Option<InlinePayload<ScreenSnapshot>>,
    Option<InlinePayload<LogFrameDelta>>,
);

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
enum StoredLogRecordSerde {
    Compact(CompactStoredLogRecordSerde),
    Legacy(LegacyStoredLogRecordSerde),
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct LogFrameDelta {
    width: u16,
    height: u16,
    changes: Vec<(usize, Cell)>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
struct DeltaSerializableCell {
    symbol: Symbol,
    style_id: u32,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
struct DeltaRunSerde {
    start: usize,
    cells: Vec<DeltaSerializableCell>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct CompactLogFrameDeltaSerde {
    width: u16,
    height: u16,
    runs: Vec<DeltaRunSerde>,
    styles: Vec<Style>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct LegacyLogFrameDeltaSerde {
    width: u16,
    height: u16,
    changes: Vec<(usize, Cell)>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
#[serde(untagged)]
enum LogFrameDeltaSerde {
    Compact(CompactLogFrameDeltaSerde),
    Legacy(LegacyLogFrameDeltaSerde),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
enum InlinePayload<T> {
    Raw(T),
    GzipBase64(String),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct CompactArchiveFile {
    version: u8,
    records: Vec<CompactArchiveRecord>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct CompactArchiveRecord(
    String,
    bool,
    u64,
    u64,
    u16,
    u16,
    usize,
    usize,
    bool,
    u16,
    u16,
    u16,
    u16,
    String,
    Option<InlinePayload<ScreenSnapshot>>,
    Option<InlinePayload<LogFrameDelta>>,
);

impl LogRecord {
    pub fn into_parts(self) -> (ScreenSnapshot, HistoryMetadata, bool) {
        let width = if self.width == 0 {
            self.snapshot.width()
        } else {
            self.width
        };
        let height = if self.height == 0 {
            self.snapshot.height()
        } else {
            self.height
        };
        (
            self.snapshot,
            HistoryMetadata {
                label: self.label,
                timestamp_unix_ms: self.timestamp_unix_ms,
                frame_seq: self.frame_seq,
                changed: self.changed,
                width,
                height,
                changed_cell_count: self.changed_cell_count,
                input_event_count_since_prev: self.input_event_count_since_prev,
                resized: self.resized,
                resize_from_width: self.resize_from_width,
                resize_from_height: self.resize_from_height,
                resize_to_width: self.resize_to_width,
                resize_to_height: self.resize_to_height,
                resize_source: self.resize_source,
            },
            self.changed,
        )
    }
}

impl Default for LogRecord {
    fn default() -> Self {
        Self {
            label: String::new(),
            changed: false,
            timestamp_unix_ms: 0,
            frame_seq: 0,
            width: 0,
            height: 0,
            changed_cell_count: 0,
            input_event_count_since_prev: 0,
            resized: false,
            resize_from_width: 0,
            resize_from_height: 0,
            resize_to_width: 0,
            resize_to_height: 0,
            resize_source: String::new(),
            snapshot: ScreenSnapshot::new(0, 0),
        }
    }
}

impl Default for StoredLogRecord {
    fn default() -> Self {
        Self {
            label: String::new(),
            changed: false,
            timestamp_unix_ms: 0,
            frame_seq: 0,
            width: 0,
            height: 0,
            changed_cell_count: 0,
            input_event_count_since_prev: 0,
            resized: false,
            resize_from_width: 0,
            resize_from_height: 0,
            resize_to_width: 0,
            resize_to_height: 0,
            resize_source: String::new(),
            snapshot: None,
            delta: None,
        }
    }
}

impl Serialize for StoredLogRecord {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        CompactStoredLogRecordSerde(
            self.label.clone(),
            self.changed,
            self.timestamp_unix_ms,
            self.frame_seq,
            self.width,
            self.height,
            self.changed_cell_count,
            self.input_event_count_since_prev,
            self.resized,
            self.resize_from_width,
            self.resize_from_height,
            self.resize_to_width,
            self.resize_to_height,
            self.resize_source.clone(),
            self.snapshot.clone(),
            self.delta.clone(),
        )
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for StoredLogRecord {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        match StoredLogRecordSerde::deserialize(deserializer)? {
            StoredLogRecordSerde::Compact(value) => Ok(Self {
                label: value.0,
                changed: value.1,
                timestamp_unix_ms: value.2,
                frame_seq: value.3,
                width: value.4,
                height: value.5,
                changed_cell_count: value.6,
                input_event_count_since_prev: value.7,
                resized: value.8,
                resize_from_width: value.9,
                resize_from_height: value.10,
                resize_to_width: value.11,
                resize_to_height: value.12,
                resize_source: value.13,
                snapshot: value.14,
                delta: value.15,
            }),
            StoredLogRecordSerde::Legacy(value) => Ok(Self {
                label: value.label,
                changed: value.changed,
                timestamp_unix_ms: value.timestamp_unix_ms,
                frame_seq: value.frame_seq,
                width: value.width,
                height: value.height,
                changed_cell_count: value.changed_cell_count,
                input_event_count_since_prev: value.input_event_count_since_prev,
                resized: value.resized,
                resize_from_width: value.resize_from_width,
                resize_from_height: value.resize_from_height,
                resize_to_width: value.resize_to_width,
                resize_to_height: value.resize_to_height,
                resize_source: value.resize_source,
                snapshot: value.snapshot,
                delta: value.delta,
            }),
        }
    }
}

impl StoredLogRecord {
    fn from_full(record: &LogRecord) -> Result<Self> {
        Ok(Self {
            label: record.label.clone(),
            changed: record.changed,
            timestamp_unix_ms: record.timestamp_unix_ms,
            frame_seq: record.frame_seq,
            width: record.width,
            height: record.height,
            changed_cell_count: record.changed_cell_count,
            input_event_count_since_prev: record.input_event_count_since_prev,
            resized: record.resized,
            resize_from_width: record.resize_from_width,
            resize_from_height: record.resize_from_height,
            resize_to_width: record.resize_to_width,
            resize_to_height: record.resize_to_height,
            resize_source: record.resize_source.clone(),
            snapshot: Some(store_inline_payload(&record.snapshot, true)?),
            delta: None,
        })
    }

    fn into_log_record(self, previous: Option<&ScreenSnapshot>) -> Result<LogRecord> {
        let snapshot = match (self.snapshot, self.delta) {
            (Some(snapshot), _) => load_inline_payload(snapshot)?,
            (None, Some(delta)) => {
                let mut snapshot = previous
                    .cloned()
                    .context("delta log record is missing a previous snapshot")?;
                load_inline_payload(delta)?.apply(&mut snapshot);
                snapshot
            }
            (None, None) => ScreenSnapshot::new(self.width, self.height),
        };

        Ok(LogRecord {
            label: self.label,
            changed: self.changed,
            timestamp_unix_ms: self.timestamp_unix_ms,
            frame_seq: self.frame_seq,
            width: if self.width == 0 {
                snapshot.width()
            } else {
                self.width
            },
            height: if self.height == 0 {
                snapshot.height()
            } else {
                self.height
            },
            changed_cell_count: self.changed_cell_count,
            input_event_count_since_prev: self.input_event_count_since_prev,
            resized: self.resized,
            resize_from_width: self.resize_from_width,
            resize_from_height: self.resize_from_height,
            resize_to_width: self.resize_to_width,
            resize_to_height: self.resize_to_height,
            resize_source: self.resize_source,
            snapshot,
        })
    }
}

impl From<StoredLogRecord> for CompactArchiveRecord {
    fn from(value: StoredLogRecord) -> Self {
        Self(
            value.label,
            value.changed,
            value.timestamp_unix_ms,
            value.frame_seq,
            value.width,
            value.height,
            value.changed_cell_count,
            value.input_event_count_since_prev,
            value.resized,
            value.resize_from_width,
            value.resize_from_height,
            value.resize_to_width,
            value.resize_to_height,
            value.resize_source,
            value.snapshot,
            value.delta,
        )
    }
}

impl From<CompactArchiveRecord> for StoredLogRecord {
    fn from(value: CompactArchiveRecord) -> Self {
        Self {
            label: value.0,
            changed: value.1,
            timestamp_unix_ms: value.2,
            frame_seq: value.3,
            width: value.4,
            height: value.5,
            changed_cell_count: value.6,
            input_event_count_since_prev: value.7,
            resized: value.8,
            resize_from_width: value.9,
            resize_from_height: value.10,
            resize_to_width: value.11,
            resize_to_height: value.12,
            resize_source: value.13,
            snapshot: value.14,
            delta: value.15,
        }
    }
}

impl LogFrameDelta {
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

impl Serialize for LogFrameDelta {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut styles = vec![Style::default()];
        let mut runs = Vec::new();
        let mut current_run: Option<DeltaRunSerde> = None;

        for (idx, cell) in &self.changes {
            let style_id = styles
                .iter()
                .position(|style| style == &cell.style)
                .map(|index| index as u32)
                .unwrap_or_else(|| {
                    styles.push(cell.style);
                    (styles.len() - 1) as u32
                });
            let serializable_cell = DeltaSerializableCell {
                symbol: cell.symbol.clone(),
                style_id,
            };

            match &mut current_run {
                Some(run) if run.start + run.cells.len() == *idx => {
                    run.cells.push(serializable_cell);
                }
                Some(run) => {
                    runs.push(std::mem::take(run));
                    *run = DeltaRunSerde {
                        start: *idx,
                        cells: vec![serializable_cell],
                    };
                }
                None => {
                    current_run = Some(DeltaRunSerde {
                        start: *idx,
                        cells: vec![serializable_cell],
                    });
                }
            }
        }

        if let Some(run) = current_run.take() {
            runs.push(run);
        }

        CompactLogFrameDeltaSerde {
            width: self.width,
            height: self.height,
            runs,
            styles,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for LogFrameDelta {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        match LogFrameDeltaSerde::deserialize(deserializer)? {
            LogFrameDeltaSerde::Legacy(value) => Ok(Self {
                width: value.width,
                height: value.height,
                changes: value.changes,
            }),
            LogFrameDeltaSerde::Compact(value) => {
                let mut changes = Vec::new();
                let mut styles = value.styles;
                if styles.is_empty() {
                    styles.push(Style::default());
                }

                for run in value.runs {
                    for (offset, cell) in run.cells.into_iter().enumerate() {
                        let style = styles
                            .get(cell.style_id as usize)
                            .copied()
                            .unwrap_or_default();
                        changes.push((
                            run.start + offset,
                            Cell {
                                symbol: cell.symbol,
                                style,
                            },
                        ));
                    }
                }

                Ok(Self {
                    width: value.width,
                    height: value.height,
                    changes,
                })
            }
        }
    }
}

pub fn append_record(path: &str, record: &LogRecord) -> Result<()> {
    write_stored_record(path, &StoredLogRecord::from_full(record)?)
}

pub fn append_delta_record(
    path: &str,
    record: &LogRecord,
    previous_snapshot: Option<&ScreenSnapshot>,
    checkpoint_interval: u64,
) -> Result<()> {
    let stored = if record.frame_seq <= 1
        || previous_snapshot.is_none()
        || record.frame_seq % checkpoint_interval.max(1) == 0
        || previous_snapshot.is_some_and(|previous| {
            previous.width() != record.snapshot.width()
                || previous.height() != record.snapshot.height()
        }) {
        StoredLogRecord::from_full(record)?
    } else {
        StoredLogRecord {
            label: record.label.clone(),
            changed: record.changed,
            timestamp_unix_ms: record.timestamp_unix_ms,
            frame_seq: record.frame_seq,
            width: record.width,
            height: record.height,
            changed_cell_count: record.changed_cell_count,
            input_event_count_since_prev: record.input_event_count_since_prev,
            resized: record.resized,
            resize_from_width: record.resize_from_width,
            resize_from_height: record.resize_from_height,
            resize_to_width: record.resize_to_width,
            resize_to_height: record.resize_to_height,
            resize_source: record.resize_source.clone(),
            snapshot: None,
            delta: Some(store_inline_payload(
                &LogFrameDelta::between(
                    previous_snapshot.expect("checked previous snapshot"),
                    &record.snapshot,
                ),
                true,
            )?),
        }
    };

    write_stored_record(path, &stored)
}

fn write_stored_record(path: &str, record: &StoredLogRecord) -> Result<()> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("failed to open logfile for append: {path}"))?;
    if is_gzip_log_path(path) {
        let mut encoder = GzEncoder::new(&mut file, Compression::fast());
        serde_json::to_writer(&mut encoder, record).context("failed to write log record")?;
        encoder
            .write_all(b"\n")
            .context("failed to terminate log line")?;
        encoder
            .finish()
            .context("failed to finalize gzip log record")?;
    } else {
        serde_json::to_writer(&mut file, record).context("failed to write log record")?;
        file.write_all(b"\n")
            .context("failed to terminate log line")?;
    }
    Ok(())
}

fn write_stored_records(path: &str, records: &[StoredLogRecord]) -> Result<()> {
    let mut file = File::create(path)
        .with_context(|| format!("failed to create logfile for rewrite: {path}"))?;
    for record in records {
        serde_json::to_writer(&mut file, record).context("failed to write log record")?;
        file.write_all(b"\n")
            .context("failed to terminate log line")?;
    }
    Ok(())
}

fn load_stored_records_from_path(path: &str) -> Result<Vec<StoredLogRecord>> {
    if !Path::new(path).exists() {
        return Ok(Vec::new());
    }

    if is_compact_archive_path(path) {
        let file = File::open(path).with_context(|| format!("failed to open logfile: {path}"))?;
        let decoder = GzDecoder::new(file);
        let archive: CompactArchiveFile =
            serde_json::from_reader(decoder).context("failed to parse compact twatch archive")?;
        return Ok(archive
            .records
            .into_iter()
            .map(StoredLogRecord::from)
            .collect());
    }

    let file = File::open(path).with_context(|| format!("failed to open logfile: {path}"))?;
    let reader: Box<dyn BufRead> = if is_gzip_log_path(path) {
        Box::new(BufReader::new(MultiGzDecoder::new(file)))
    } else {
        Box::new(BufReader::new(file))
    };
    let mut records = Vec::new();
    for line in reader.lines() {
        let line = line.context("failed to read log line")?;
        if line.trim().is_empty() {
            continue;
        }
        let stored: StoredLogRecord =
            serde_json::from_str(&line).context("failed to parse jsonl log record")?;
        records.push(stored);
    }
    Ok(records)
}

fn load_stored_records(path: &str) -> Result<Vec<StoredLogRecord>> {
    let mut records = Vec::new();
    if let Some(spill_path) = active_spill_path(path)
        && Path::new(&spill_path).exists()
    {
        records.extend(load_stored_records_from_path(&spill_path)?);
    }
    records.extend(load_stored_records_from_path(path)?);
    Ok(records)
}

pub fn load_records_from_single_path(path: &str) -> Result<Vec<LogRecord>> {
    if !Path::new(path).exists() {
        return Ok(Vec::new());
    }

    let mut previous_snapshot = None;
    let mut records = Vec::new();
    for stored in load_stored_records_from_path(path)? {
        let record = stored.into_log_record(previous_snapshot.as_ref())?;
        previous_snapshot = Some(record.snapshot.clone());
        records.push(record);
    }
    Ok(records)
}

pub fn active_spill_exists(path: &str) -> bool {
    active_spill_path(path)
        .as_ref()
        .is_some_and(|spill_path| Path::new(spill_path).exists())
}

pub fn load_recent_records_from_plain_path(
    path: &str,
    max_records: usize,
) -> Result<Vec<LogRecord>> {
    if max_records == 0
        || !Path::new(path).exists()
        || is_gzip_log_path(path)
        || is_compact_archive_path(path)
    {
        return Ok(Vec::new());
    }

    let stored = load_recent_stored_records_from_plain_path(path, max_records)?;
    let mut previous_snapshot = None;
    let mut records = Vec::with_capacity(stored.len());
    for record in stored {
        let record = record.into_log_record(previous_snapshot.as_ref())?;
        previous_snapshot = Some(record.snapshot.clone());
        records.push(record);
    }
    Ok(records)
}

fn load_recent_stored_records_from_plain_path(
    path: &str,
    max_records: usize,
) -> Result<Vec<StoredLogRecord>> {
    let mut file = File::open(path).with_context(|| format!("failed to open logfile: {path}"))?;
    let file_len = file
        .seek(SeekFrom::End(0))
        .context("failed to seek logfile end")?;
    if file_len == 0 {
        return Ok(Vec::new());
    }

    const INITIAL_WINDOW_BYTES: u64 = 256 * 1024;
    const MAX_WINDOW_BYTES: u64 = 4 * 1024 * 1024;

    let mut window_bytes = INITIAL_WINDOW_BYTES.min(file_len);

    loop {
        let start = file_len.saturating_sub(window_bytes);
        file.seek(SeekFrom::Start(start))
            .context("failed to seek logfile chunk")?;
        let mut buffer = vec![0u8; usize::try_from(file_len - start).unwrap_or(0)];
        file.read_exact(&mut buffer)
            .context("failed to read logfile chunk")?;

        let text = String::from_utf8(buffer).context("logfile is not valid utf-8")?;
        let lines = collect_complete_lines_from_window(&text, start > 0);
        if lines.is_empty() {
            return Ok(Vec::new());
        }

        if let Some(trimmed) = trim_recent_lines_with_snapshot_boundary(&lines, max_records)? {
            return trimmed
                .into_iter()
                .map(|line| serde_json::from_str(&line).context("failed to parse jsonl log record"))
                .collect();
        }

        if start == 0 || window_bytes >= MAX_WINDOW_BYTES {
            return lines
                .into_iter()
                .map(|line| serde_json::from_str(&line).context("failed to parse jsonl log record"))
                .collect();
        }

        window_bytes = (window_bytes.saturating_mul(2))
            .min(MAX_WINDOW_BYTES)
            .min(file_len);
    }
}

fn starts_with_full_snapshot(lines: &[String]) -> Result<bool> {
    let Some(first) = lines.first() else {
        return Ok(false);
    };
    let stored: StoredLogRecord =
        serde_json::from_str(first).context("failed to parse jsonl log record")?;
    Ok(stored.snapshot.is_some())
}

fn collect_complete_lines_from_window(text: &str, drop_partial_first_line: bool) -> Vec<String> {
    let mut lines: Vec<String> = text.lines().map(|line| line.to_string()).collect();
    if drop_partial_first_line && !text.starts_with('\n') && !lines.is_empty() {
        lines.remove(0);
    }
    lines.retain(|line| !line.trim().is_empty());
    lines
}

fn trim_recent_lines_with_snapshot_boundary(
    lines: &[String],
    max_records: usize,
) -> Result<Option<Vec<String>>> {
    if lines.is_empty() {
        return Ok(Some(Vec::new()));
    }

    let keep = max_records.max(1);
    let tail_start = lines.len().saturating_sub(keep);
    let first_full_in_tail = (tail_start..lines.len()).find(|&idx| {
        serde_json::from_str::<StoredLogRecord>(&lines[idx])
            .map(|record| record.snapshot.is_some())
            .unwrap_or(false)
    });

    if let Some(start) = first_full_in_tail {
        return Ok(Some(lines[start..].to_vec()));
    }

    if tail_start == 0 {
        return Ok(Some(lines.to_vec()));
    }

    if starts_with_full_snapshot(&lines[tail_start - 1..])? {
        return Ok(Some(lines[(tail_start - 1)..].to_vec()));
    }

    Ok(None)
}

pub fn pack_log_as_compact_archive(input_path: &str, output_path: &str) -> Result<()> {
    let archive = CompactArchiveFile {
        version: 1,
        records: load_stored_records(input_path)?
            .into_iter()
            .map(CompactArchiveRecord::from)
            .collect(),
    };
    let file = File::create(output_path)
        .with_context(|| format!("failed to create compact archive: {output_path}"))?;
    let encoder = GzEncoder::new(file, Compression::best());
    serde_json::to_writer(encoder, &archive).context("failed to write compact archive")
}

pub fn compact_active_log(path: &str, retain_recent_records: usize) -> Result<()> {
    if is_gzip_log_path(path) || is_compact_archive_path(path) {
        return Ok(());
    }

    let current_records = load_stored_records_from_path(path)?;
    if current_records.len() <= retain_recent_records {
        return Ok(());
    }
    let current_full_records = load_records_from_single_path(path)?;

    let split_at = current_records.len() - retain_recent_records;
    let Some(spill_path) = active_spill_path(path) else {
        return Ok(());
    };

    for record in &current_records[..split_at] {
        write_stored_record(&spill_path, record)?;
    }
    let mut tail_records = Vec::with_capacity(current_records.len() - split_at);
    tail_records.push(StoredLogRecord::from_full(&current_full_records[split_at])?);
    tail_records.extend(current_records[(split_at + 1)..].iter().cloned());
    write_stored_records(path, &tail_records)?;
    Ok(())
}

enum LogRecordStreamSource {
    Reader(Box<dyn BufRead + Send>),
    Readers(VecDeque<Box<dyn BufRead + Send>>),
    Queue(VecDeque<StoredLogRecord>),
}

pub struct LogRecordStream {
    source: LogRecordStreamSource,
    previous_snapshot: Option<ScreenSnapshot>,
    prefetched: Option<LogRecord>,
}

impl LogRecordStream {
    pub fn open(path: &str) -> Result<Self> {
        let source = if is_compact_archive_path(path) {
            LogRecordStreamSource::Queue(VecDeque::from(load_stored_records(path)?))
        } else if let Some(spill_path) = active_spill_path(path)
            && Path::new(&spill_path).exists()
        {
            let mut readers = VecDeque::new();
            readers.push_back(open_log_reader(&spill_path)?);
            readers.push_back(open_log_reader(path)?);
            LogRecordStreamSource::Readers(readers)
        } else {
            LogRecordStreamSource::Reader(open_log_reader(path)?)
        };
        Ok(Self {
            source,
            previous_snapshot: None,
            prefetched: None,
        })
    }

    pub fn next_record(&mut self) -> Result<Option<LogRecord>> {
        if let Some(record) = self.prefetched.take() {
            self.previous_snapshot = Some(record.snapshot.clone());
            return Ok(Some(record));
        }
        self.read_next_record()
    }

    pub fn has_more(&mut self) -> Result<bool> {
        if self.prefetched.is_some() {
            return Ok(true);
        }
        self.prefetched = self.read_next_record()?;
        Ok(self.prefetched.is_some())
    }

    fn read_next_record(&mut self) -> Result<Option<LogRecord>> {
        match &mut self.source {
            LogRecordStreamSource::Queue(records) => {
                if let Some(stored) = records.pop_front() {
                    let record = stored.into_log_record(self.previous_snapshot.as_ref())?;
                    self.previous_snapshot = Some(record.snapshot.clone());
                    Ok(Some(record))
                } else {
                    Ok(None)
                }
            }
            LogRecordStreamSource::Readers(readers) => loop {
                let Some(reader) = readers.front_mut() else {
                    return Ok(None);
                };
                let mut line = String::new();
                let bytes = reader
                    .read_line(&mut line)
                    .context("failed to read log line")?;
                if bytes == 0 {
                    readers.pop_front();
                    continue;
                }
                if line.trim().is_empty() {
                    continue;
                }
                let stored: StoredLogRecord =
                    serde_json::from_str(&line).context("failed to parse jsonl log record")?;
                let record = stored.into_log_record(self.previous_snapshot.as_ref())?;
                self.previous_snapshot = Some(record.snapshot.clone());
                return Ok(Some(record));
            },
            LogRecordStreamSource::Reader(reader) => loop {
                let mut line = String::new();
                let bytes = reader
                    .read_line(&mut line)
                    .context("failed to read log line")?;
                if bytes == 0 {
                    return Ok(None);
                }
                if line.trim().is_empty() {
                    continue;
                }
                let stored: StoredLogRecord =
                    serde_json::from_str(&line).context("failed to parse jsonl log record")?;
                let record = stored.into_log_record(self.previous_snapshot.as_ref())?;
                self.previous_snapshot = Some(record.snapshot.clone());
                return Ok(Some(record));
            },
        }
    }
}

pub fn load_records(path: &str) -> Result<Vec<LogRecord>> {
    if !Path::new(path).exists() {
        return Ok(Vec::new());
    }

    let mut stream = LogRecordStream::open(path)?;
    let mut records = Vec::new();

    while let Some(record) = stream.next_record()? {
        records.push(record);
    }

    Ok(records)
}

fn open_log_reader(path: &str) -> Result<Box<dyn BufRead + Send>> {
    let file = File::open(path).with_context(|| format!("failed to open logfile: {path}"))?;
    if is_gzip_log_path(path) {
        Ok(Box::new(BufReader::new(MultiGzDecoder::new(file))))
    } else {
        Ok(Box::new(BufReader::new(file)))
    }
}

fn is_gzip_log_path(path: &str) -> bool {
    path.ends_with(".gz")
}

fn is_compact_archive_path(path: &str) -> bool {
    path.ends_with(COMPACT_ARCHIVE_EXTENSION)
}

pub fn active_spill_path(path: &str) -> Option<String> {
    if path.ends_with(".jsonl") {
        Some(format!("{path}{ACTIVE_SPILL_EXTENSION}"))
    } else {
        None
    }
}

fn store_inline_payload<T>(value: &T, compress: bool) -> Result<InlinePayload<T>>
where
    T: Clone + Serialize,
{
    if !compress {
        return Ok(InlinePayload::Raw(value.clone()));
    }

    let bytes = serde_json::to_vec(value).context("failed to serialize inline payload")?;
    if bytes.len() < LOG_INLINE_PAYLOAD_MIN_BYTES {
        return Ok(InlinePayload::Raw(value.clone()));
    }

    let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
    encoder
        .write_all(&bytes)
        .context("failed to compress inline payload")?;
    let compressed = encoder
        .finish()
        .context("failed to finalize inline payload compression")?;
    Ok(InlinePayload::GzipBase64(
        BASE64_STANDARD.encode(compressed),
    ))
}

fn load_inline_payload<T>(payload: InlinePayload<T>) -> Result<T>
where
    T: Clone + DeserializeOwned,
{
    match payload {
        InlinePayload::Raw(value) => Ok(value),
        InlinePayload::GzipBase64(encoded) => {
            let compressed = BASE64_STANDARD
                .decode(encoded)
                .context("failed to decode inline payload base64")?;
            let mut decoder = GzDecoder::new(compressed.as_slice());
            let mut out = Vec::new();
            std::io::Read::read_to_end(&mut decoder, &mut out)
                .context("failed to decompress inline payload")?;
            serde_json::from_slice(&out).context("failed to deserialize inline payload")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        LogFrameDelta, LogRecord, LogRecordStream, active_spill_path, append_delta_record,
        append_record, compact_active_log, load_recent_records_from_plain_path, load_records,
        pack_log_as_compact_archive,
    };
    use crate::screen::ScreenSnapshot;

    #[test]
    fn loads_legacy_log_record_with_defaults() {
        let record: LogRecord =
            serde_json::from_str(r#"{"label":"frame-a","changed":true,"snapshot":{"width":4,"height":1,"cells":[{"symbol":"a","style":{"fg":"Default","bg":"Default","bold":false,"italic":false,"underline":false,"inverted":false}},{"symbol":" ","style":{"fg":"Default","bg":"Default","bold":false,"italic":false,"underline":false,"inverted":false}},{"symbol":" ","style":{"fg":"Default","bg":"Default","bold":false,"italic":false,"underline":false,"inverted":false}},{"symbol":" ","style":{"fg":"Default","bg":"Default","bold":false,"italic":false,"underline":false,"inverted":false}}]}}"#)
                .unwrap();

        let (_snapshot, metadata, changed) = record.into_parts();

        assert!(changed);
        assert_eq!(metadata.label, "frame-a");
        assert_eq!(metadata.width, 4);
        assert_eq!(metadata.height, 1);
        assert_eq!(metadata.frame_seq, 0);
    }

    #[test]
    fn loads_delta_records_as_full_snapshots() {
        let path =
            std::env::temp_dir().join(format!("twatch-log-delta-{}.jsonl", std::process::id()));
        let _ = std::fs::remove_file(&path);

        let first = LogRecord {
            label: "a".to_string(),
            changed: true,
            timestamp_unix_ms: 1,
            frame_seq: 1,
            width: 6,
            height: 1,
            changed_cell_count: 1,
            input_event_count_since_prev: 0,
            resized: false,
            resize_from_width: 0,
            resize_from_height: 0,
            resize_to_width: 0,
            resize_to_height: 0,
            resize_source: "stdin".to_string(),
            snapshot: ScreenSnapshot::from_text_lines(6, 1, &["alpha"]),
        };
        let second = LogRecord {
            label: "b".to_string(),
            changed: true,
            timestamp_unix_ms: 2,
            frame_seq: 2,
            width: 6,
            height: 1,
            changed_cell_count: 1,
            input_event_count_since_prev: 0,
            resized: false,
            resize_from_width: 0,
            resize_from_height: 0,
            resize_to_width: 0,
            resize_to_height: 0,
            resize_source: "stdin".to_string(),
            snapshot: ScreenSnapshot::from_text_lines(6, 1, &["alpHb"]),
        };

        append_delta_record(path.to_str().unwrap(), &first, None, 120).unwrap();
        append_delta_record(path.to_str().unwrap(), &second, Some(&first.snapshot), 120).unwrap();

        let loaded = load_records(path.to_str().unwrap()).unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].snapshot.lines(), vec!["alpha".to_string()]);
        assert_eq!(loaded[1].snapshot.lines(), vec!["alpHb".to_string()]);

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn loads_gzip_delta_records_as_full_snapshots() {
        let path =
            std::env::temp_dir().join(format!("twatch-log-delta-{}.jsonl.gz", std::process::id()));
        let _ = std::fs::remove_file(&path);

        let first = LogRecord {
            label: "a".to_string(),
            changed: true,
            timestamp_unix_ms: 1,
            frame_seq: 1,
            width: 6,
            height: 1,
            changed_cell_count: 1,
            input_event_count_since_prev: 0,
            resized: false,
            resize_from_width: 0,
            resize_from_height: 0,
            resize_to_width: 0,
            resize_to_height: 0,
            resize_source: "stdin".to_string(),
            snapshot: ScreenSnapshot::from_text_lines(6, 1, &["alpha"]),
        };
        let second = LogRecord {
            label: "b".to_string(),
            changed: true,
            timestamp_unix_ms: 2,
            frame_seq: 2,
            width: 6,
            height: 1,
            changed_cell_count: 1,
            input_event_count_since_prev: 0,
            resized: false,
            resize_from_width: 0,
            resize_from_height: 0,
            resize_to_width: 0,
            resize_to_height: 0,
            resize_source: "stdin".to_string(),
            snapshot: ScreenSnapshot::from_text_lines(6, 1, &["alpHb"]),
        };

        append_delta_record(path.to_str().unwrap(), &first, None, 120).unwrap();
        append_delta_record(path.to_str().unwrap(), &second, Some(&first.snapshot), 120).unwrap();

        let loaded = load_records(path.to_str().unwrap()).unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].snapshot.lines(), vec!["alpha".to_string()]);
        assert_eq!(loaded[1].snapshot.lines(), vec!["alpHb".to_string()]);

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn writes_inline_compressed_snapshot_payload_for_large_records() {
        let path =
            std::env::temp_dir().join(format!("twatch-log-inline-{}.jsonl", std::process::id()));
        let _ = std::fs::remove_file(&path);

        let record = LogRecord {
            label: "a".to_string(),
            changed: true,
            timestamp_unix_ms: 1,
            frame_seq: 1,
            width: 40,
            height: 4,
            changed_cell_count: 1,
            input_event_count_since_prev: 0,
            resized: false,
            resize_from_width: 0,
            resize_from_height: 0,
            resize_to_width: 0,
            resize_to_height: 0,
            resize_source: "stdin".to_string(),
            snapshot: ScreenSnapshot::from_text_lines(
                40,
                4,
                &[
                    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                    "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                    "cccccccccccccccccccccccccccccccccccccccc",
                    "dddddddddddddddddddddddddddddddddddddddd",
                ],
            ),
        };

        append_record(path.to_str().unwrap(), &record).unwrap();
        let line = std::fs::read_to_string(&path).unwrap();
        assert!(line.starts_with("[\"a\",true,1,1,40,4,1,0,false,0,0,0,0,\"stdin\","));
        assert!(!line.contains("\"snapshot\""));

        let loaded = load_records(path.to_str().unwrap()).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].snapshot.lines(), record.snapshot.lines());

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn stores_deltas_with_compact_runs_and_style_table() {
        let first =
            ScreenSnapshot::from_text_lines(16, 2, &["aaaaaaaaaaaaaaaa", "bbbbbbbbbbbbbbbb"]);
        let second =
            ScreenSnapshot::from_text_lines(16, 2, &["AAAAAAAAbbbbbbbb", "bbbbbbbbbbbbbbbb"]);
        let delta = LogFrameDelta::between(&first, &second);

        let serialized = serde_json::to_string(&delta).unwrap();
        assert!(serialized.contains("\"runs\""));
        assert!(serialized.contains("\"styles\""));
        assert!(!serialized.contains("\"changes\""));

        let restored: LogFrameDelta = serde_json::from_str(&serialized).unwrap();
        let mut restored_snapshot = first.clone();
        restored.apply(&mut restored_snapshot);
        assert_eq!(restored_snapshot.lines(), second.lines());
    }

    #[test]
    fn stream_reads_records_incrementally() {
        let path = std::env::temp_dir().join(format!("twatch-stream-{}.jsonl", std::process::id()));
        let _ = std::fs::remove_file(&path);

        append_record(
            path.to_str().unwrap(),
            &LogRecord {
                label: "one".to_string(),
                changed: true,
                timestamp_unix_ms: 1,
                frame_seq: 1,
                width: 4,
                height: 1,
                changed_cell_count: 3,
                input_event_count_since_prev: 0,
                resized: false,
                resize_from_width: 0,
                resize_from_height: 0,
                resize_to_width: 0,
                resize_to_height: 0,
                resize_source: String::new(),
                snapshot: ScreenSnapshot::from_text_lines(4, 1, &["one"]),
            },
        )
        .unwrap();
        append_record(
            path.to_str().unwrap(),
            &LogRecord {
                label: "two".to_string(),
                changed: true,
                timestamp_unix_ms: 2,
                frame_seq: 2,
                width: 4,
                height: 1,
                changed_cell_count: 3,
                input_event_count_since_prev: 0,
                resized: false,
                resize_from_width: 0,
                resize_from_height: 0,
                resize_to_width: 0,
                resize_to_height: 0,
                resize_source: String::new(),
                snapshot: ScreenSnapshot::from_text_lines(4, 1, &["two"]),
            },
        )
        .unwrap();

        let mut stream = LogRecordStream::open(path.to_str().unwrap()).unwrap();
        assert!(stream.has_more().unwrap());
        assert_eq!(stream.next_record().unwrap().unwrap().label, "one");
        assert!(stream.has_more().unwrap());
        assert_eq!(stream.next_record().unwrap().unwrap().label, "two");
        assert!(!stream.has_more().unwrap());

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn packs_jsonl_into_compact_archive_and_loads_it() {
        let src =
            std::env::temp_dir().join(format!("twatch-pack-src-{}.jsonl", std::process::id()));
        let dst = std::env::temp_dir().join(format!("twatch-pack-dst-{}.twar", std::process::id()));
        let _ = std::fs::remove_file(&src);
        let _ = std::fs::remove_file(&dst);

        let first = LogRecord {
            label: "a".to_string(),
            changed: true,
            timestamp_unix_ms: 1,
            frame_seq: 1,
            width: 6,
            height: 1,
            changed_cell_count: 1,
            input_event_count_since_prev: 0,
            resized: false,
            resize_from_width: 0,
            resize_from_height: 0,
            resize_to_width: 0,
            resize_to_height: 0,
            resize_source: "stdin".to_string(),
            snapshot: ScreenSnapshot::from_text_lines(6, 1, &["alpha"]),
        };
        let second = LogRecord {
            label: "b".to_string(),
            changed: true,
            timestamp_unix_ms: 2,
            frame_seq: 2,
            width: 6,
            height: 1,
            changed_cell_count: 1,
            input_event_count_since_prev: 0,
            resized: false,
            resize_from_width: 0,
            resize_from_height: 0,
            resize_to_width: 0,
            resize_to_height: 0,
            resize_source: "stdin".to_string(),
            snapshot: ScreenSnapshot::from_text_lines(6, 1, &["alpHb"]),
        };

        append_delta_record(src.to_str().unwrap(), &first, None, 120).unwrap();
        append_delta_record(src.to_str().unwrap(), &second, Some(&first.snapshot), 120).unwrap();
        pack_log_as_compact_archive(src.to_str().unwrap(), dst.to_str().unwrap()).unwrap();

        let loaded = load_records(dst.to_str().unwrap()).unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].snapshot.lines(), vec!["alpha".to_string()]);
        assert_eq!(loaded[1].snapshot.lines(), vec!["alpHb".to_string()]);

        let _ = std::fs::remove_file(src);
        let _ = std::fs::remove_file(dst);
    }

    #[test]
    fn compacts_active_jsonl_into_spill_and_keeps_recent_tail() {
        let path = std::env::temp_dir().join(format!(
            "twatch-active-compact-{}.jsonl",
            std::process::id()
        ));
        let spill = active_spill_path(path.to_str().unwrap()).unwrap();
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&spill);

        let mut previous: Option<ScreenSnapshot> = None;
        for index in 0..10u64 {
            let record = LogRecord {
                label: format!("f{index}"),
                changed: true,
                timestamp_unix_ms: index + 1,
                frame_seq: index + 1,
                width: 8,
                height: 1,
                changed_cell_count: 1,
                input_event_count_since_prev: 0,
                resized: false,
                resize_from_width: 0,
                resize_from_height: 0,
                resize_to_width: 0,
                resize_to_height: 0,
                resize_source: "stdin".to_string(),
                snapshot: ScreenSnapshot::from_text_lines(8, 1, &[&format!("line-{index}")]),
            };
            append_delta_record(path.to_str().unwrap(), &record, previous.as_ref(), 120).unwrap();
            previous = Some(record.snapshot);
        }

        compact_active_log(path.to_str().unwrap(), 3).unwrap();

        assert!(std::path::Path::new(&spill).exists());
        let tail_records = super::load_stored_records_from_path(path.to_str().unwrap()).unwrap();
        assert_eq!(tail_records.len(), 3);

        let loaded = load_records(path.to_str().unwrap()).unwrap();
        assert_eq!(loaded.len(), 10);
        assert_eq!(loaded.first().unwrap().label, "f0");
        assert_eq!(loaded.last().unwrap().label, "f9");

        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(spill);
    }

    #[test]
    fn stream_reads_spill_and_tail_sequentially() {
        let path =
            std::env::temp_dir().join(format!("twatch-stream-spill-{}.jsonl", std::process::id()));
        let spill = active_spill_path(path.to_str().unwrap()).unwrap();
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&spill);

        let mut previous: Option<ScreenSnapshot> = None;
        for index in 0..6u64 {
            let record = LogRecord {
                label: format!("f{index}"),
                changed: true,
                timestamp_unix_ms: index + 1,
                frame_seq: index + 1,
                width: 8,
                height: 1,
                changed_cell_count: 1,
                input_event_count_since_prev: 0,
                resized: false,
                resize_from_width: 0,
                resize_from_height: 0,
                resize_to_width: 0,
                resize_to_height: 0,
                resize_source: "stdin".to_string(),
                snapshot: ScreenSnapshot::from_text_lines(8, 1, &[&format!("line-{index}")]),
            };
            append_delta_record(path.to_str().unwrap(), &record, previous.as_ref(), 120).unwrap();
            previous = Some(record.snapshot);
        }

        compact_active_log(path.to_str().unwrap(), 2).unwrap();

        let mut stream = LogRecordStream::open(path.to_str().unwrap()).unwrap();
        let mut labels = Vec::new();
        while let Some(record) = stream.next_record().unwrap() {
            labels.push(record.label);
        }

        assert_eq!(labels, vec!["f0", "f1", "f2", "f3", "f4", "f5"]);

        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(spill);
    }

    #[test]
    fn loads_recent_records_from_plain_jsonl_tail() {
        let path =
            std::env::temp_dir().join(format!("twatch-tail-plain-{}.jsonl", std::process::id()));
        let _ = std::fs::remove_file(&path);

        let mut previous = None;
        for index in 0..10u64 {
            let record = LogRecord {
                label: format!("f{index}"),
                changed: true,
                timestamp_unix_ms: index + 1,
                frame_seq: index + 1,
                width: 8,
                height: 1,
                changed_cell_count: 1,
                input_event_count_since_prev: 0,
                resized: false,
                resize_from_width: 0,
                resize_from_height: 0,
                resize_to_width: 0,
                resize_to_height: 0,
                resize_source: "stdin".to_string(),
                snapshot: ScreenSnapshot::from_text_lines(8, 1, &[&format!("line-{index}")]),
            };
            append_delta_record(path.to_str().unwrap(), &record, previous.as_ref(), 4).unwrap();
            previous = Some(record.snapshot);
        }

        let records = load_recent_records_from_plain_path(path.to_str().unwrap(), 3).unwrap();
        assert_eq!(
            records
                .iter()
                .map(|record| record.label.as_str())
                .collect::<Vec<_>>(),
            vec!["f7", "f8", "f9"]
        );

        let _ = std::fs::remove_file(path);
    }
}
