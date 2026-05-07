use std::path::PathBuf;
use std::time::Instant;

use anyhow::Result;

use super::{App, FilterMode};
use crate::history::HistoryMetadata;
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

        self.archive_current_snapshot()?;
        self.set_current_snapshot(snapshot, label, current_changed);

        if !self.follow_latest && self.history.is_empty() {
            self.follow_latest = true;
        }
        self.rebuild_filter()?;
        self.append_log_record()?;
        self.last_tick = Instant::now();
        Ok(())
    }

    pub(super) fn archive_current_snapshot(&mut self) -> Result<()> {
        let (Some(previous_snapshot), Some(previous_label)) =
            (self.current_snapshot.take(), self.current_label.take())
        else {
            return Ok(());
        };

        self.history.push(
            previous_snapshot,
            HistoryMetadata {
                label: previous_label.clone(),
            },
        )?;
        self.metadata.push(super::AppHistoryMetadata {
            label: previous_label,
            changed: self.current_changed,
        });
        if self.metadata.len() > self.limit {
            self.trim_history()?;
        }
        Ok(())
    }

    pub(super) fn set_current_snapshot(
        &mut self,
        snapshot: ScreenSnapshot,
        label: String,
        changed: bool,
    ) {
        self.current_snapshot = Some(snapshot);
        self.current_label = Some(label);
        self.current_changed = changed;
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
