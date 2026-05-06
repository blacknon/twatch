use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::history::HistoryMetadata;
use crate::screen::ScreenSnapshot;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LogRecord {
    pub label: String,
    pub changed: bool,
    pub snapshot: ScreenSnapshot,
}

impl LogRecord {
    pub fn into_parts(self) -> (ScreenSnapshot, HistoryMetadata, bool) {
        (
            self.snapshot,
            HistoryMetadata { label: self.label },
            self.changed,
        )
    }
}

pub fn append_record(path: &str, record: &LogRecord) -> Result<()> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("failed to open logfile for append: {path}"))?;
    serde_json::to_writer(&mut file, record).context("failed to write log record")?;
    file.write_all(b"\n").context("failed to terminate log line")?;
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
