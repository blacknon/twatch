use std::path::PathBuf;
use std::time::Instant;

use anyhow::Result;

use super::{App, FilterMode};
use crate::runner::CaptureFrame;
use crate::screen::ScreenSnapshot;
use crate::screenshot::save_snapshot;

impl App {
    pub fn selected_snapshot(&self) -> Option<ScreenSnapshot> {
        if self.follow_latest {
            self.current_snapshot.clone()
        } else {
            self.history.snapshot(self.selected_index).ok().flatten()
        }
    }

    pub fn previous_snapshot(&self) -> Option<ScreenSnapshot> {
        if self.follow_latest {
            if self.history.is_empty() {
                None
            } else {
                self.history.snapshot(self.history.len() - 1).ok().flatten()
            }
        } else if self.selected_index == 0 {
            None
        } else {
            self.history
                .snapshot(self.selected_index - 1)
                .ok()
                .flatten()
        }
    }

    pub fn selected_lines(&self) -> Vec<String> {
        self.selected_snapshot()
            .map(|snapshot| snapshot.lines())
            .unwrap_or_default()
    }

    pub fn previous_lines(&self) -> Option<Vec<String>> {
        self.previous_snapshot().map(|snapshot| snapshot.lines())
    }

    pub fn capture(&mut self, width: u16, height: u16) -> Result<()> {
        let CaptureFrame {
            label,
            timestamp_unix_ms,
            snapshot,
            raw_output,
            changed,
        } = self.source.capture(width, height)?;

        if !changed && self.current_snapshot.is_some() {
            self.last_tick = Instant::now();
            return Ok(());
        }

        let current_changed = if raw_output.is_empty() && self.current_snapshot.is_none() {
            false
        } else {
            changed
        };
        let metadata = self.complete_metadata(
            &snapshot,
            super::AppHistoryMetadata {
                label,
                timestamp_unix_ms,
                frame_seq: 0,
                changed: current_changed,
                width: snapshot.width(),
                height: snapshot.height(),
                changed_cell_count: 0,
                input_event_count_since_prev: 0,
                resized: false,
                input_summary: String::new(),
            },
        );

        self.archive_current_snapshot()?;
        self.set_current_snapshot(snapshot, metadata);
        self.pending_input_events.clear();

        if !self.follow_latest && self.history.is_empty() {
            self.follow_latest = true;
        }
        self.rebuild_filter()?;
        self.append_log_record()?;
        self.last_tick = Instant::now();
        Ok(())
    }

    pub(super) fn archive_current_snapshot(&mut self) -> Result<()> {
        let (Some(previous_snapshot), Some(previous_metadata)) =
            (self.current_snapshot.take(), self.current_metadata.take())
        else {
            return Ok(());
        };

        self.history
            .push(previous_snapshot, previous_metadata.to_history_metadata())?;
        self.metadata.push(previous_metadata);
        if self.metadata.len() > self.limit {
            self.trim_history()?;
        }
        Ok(())
    }

    pub(super) fn set_current_snapshot(
        &mut self,
        snapshot: ScreenSnapshot,
        metadata: super::AppHistoryMetadata,
    ) {
        self.current_snapshot = Some(snapshot);
        self.current_metadata = Some(metadata);
    }

    pub(super) fn complete_metadata(
        &mut self,
        snapshot: &ScreenSnapshot,
        mut metadata: super::AppHistoryMetadata,
    ) -> super::AppHistoryMetadata {
        if metadata.frame_seq == 0 {
            metadata.frame_seq = self.next_frame_seq;
        }
        self.next_frame_seq = self
            .next_frame_seq
            .max(metadata.frame_seq.saturating_add(1));

        if metadata.width == 0 {
            metadata.width = snapshot.width();
        }
        if metadata.height == 0 {
            metadata.height = snapshot.height();
        }

        let previous = self.current_snapshot.as_ref();
        if metadata.changed && metadata.changed_cell_count == 0 {
            metadata.changed_cell_count = snapshot.changed_cell_count_since(previous);
        }
        if metadata.input_event_count_since_prev == 0 {
            metadata.input_event_count_since_prev = self.pending_input_events.len();
        }
        if metadata.input_summary.is_empty() {
            metadata.input_summary = self.summarize_pending_input_events();
        }
        metadata.resized = metadata.resized
            || previous.is_some_and(|previous_snapshot| {
                previous_snapshot.width() != snapshot.width()
                    || previous_snapshot.height() != snapshot.height()
            });

        metadata
    }

    fn summarize_pending_input_events(&self) -> String {
        if self.pending_input_events.is_empty() {
            return String::new();
        }

        let mut parts = Vec::new();
        for event in self.pending_input_events.iter().rev().take(3).rev() {
            parts.push(event.summary());
        }

        let suffix = if self.pending_input_events.len() > 3 {
            format!(" (+{} more)", self.pending_input_events.len() - 3)
        } else {
            String::new()
        };
        format!("input: {}{}", parts.join(", "), suffix)
    }

    pub(super) fn save_snapshot(&mut self) -> Result<()> {
        let Some(snapshot) = self.selected_snapshot() else {
            self.status_message = Some("no snapshot to save".to_string());
            return Ok(());
        };

        let label = self
            .current_label()
            .unwrap_or("snapshot")
            .chars()
            .map(|ch| match ch {
                '0'..='9' | 'A'..='Z' | 'a'..='z' | '-' | '_' => ch,
                _ => '_',
            })
            .collect::<String>();
        let path = self.screenshot_dir.join(format!(
            "twatch-{label}.{}",
            self.screenshot_format.extension()
        ));
        let header = [
            format!("twatch snapshot | {}", self.command_display()),
            format!(
                "filter: {}{}",
                match self.filter_mode {
                    FilterMode::Plain => "/",
                    FilterMode::Regex => "*",
                },
                self.filter_query
            ),
        ];
        save_snapshot(&snapshot, &path, self.screenshot_format, &header)?;
        self.status_message = Some(format!(
            "snapshot saved: {} ({})",
            display_tmp_path(&path),
            self.screenshot_format.label()
        ));
        Ok(())
    }

    pub(super) fn cycle_screenshot_format(&mut self) {
        self.screenshot_format = self.screenshot_format.cycle();
        self.status_message = Some(format!(
            "snapshot format: {} -> {}",
            self.screenshot_format.label(),
            display_tmp_path(&self.screenshot_dir)
        ));
    }
}

fn display_tmp_path(path: &PathBuf) -> String {
    path.to_string_lossy().into_owned()
}
