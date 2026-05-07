use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::history::HistoryMetadata;
use crate::screen::ScreenSnapshot;

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

pub fn append_record(path: &str, record: &LogRecord) -> Result<()> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("failed to open logfile for append: {path}"))?;
    serde_json::to_writer(&mut file, record).context("failed to write log record")?;
    file.write_all(b"\n")
        .context("failed to terminate log line")?;
    Ok(())
}

pub fn load_records(path: &str) -> Result<Vec<LogRecord>> {
    if !Path::new(path).exists() {
        return Ok(Vec::new());
    }

    let file = File::open(path).with_context(|| format!("failed to open logfile: {path}"))?;
    let reader = BufReader::new(file);
    let mut records = Vec::new();

    for line in reader.lines() {
        let line = line.context("failed to read log line")?;
        if line.trim().is_empty() {
            continue;
        }
        let record: LogRecord =
            serde_json::from_str(&line).context("failed to parse jsonl log record")?;
        records.push(record);
    }

    Ok(records)
}

#[cfg(test)]
mod tests {
    use super::LogRecord;

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
}
