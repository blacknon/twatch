use std::path::PathBuf;

use anyhow::Result;

use super::App;
use crate::aftercommand::AfterCommandEvent;
use crate::screenshot::save_snapshot;

impl App {
    pub(super) fn maybe_save_triggered_snapshot(&mut self) -> Result<()> {
        if self.snapshot_once && self.snapshot_trigger_fired {
            return Ok(());
        }

        let Some(snapshot) = &self.current_snapshot else {
            return Ok(());
        };
        let Some(metadata) = &self.current_metadata else {
            return Ok(());
        };

        let lines = snapshot.lines();
        let mut reasons = Vec::new();

        if let Some(needle) = &self.snapshot_on
            && lines.iter().any(|line| line.contains(needle))
        {
            reasons.push(format!("string:{needle}"));
        }

        if let Some(regex) = &self.snapshot_on_regex
            && lines.iter().any(|line| regex.is_match(line))
        {
            reasons.push(format!("regex:{}", regex.as_str()));
        }

        if let Some(threshold) = self.snapshot_on_change_cells
            && metadata.changed_cell_count >= threshold
        {
            reasons.push(format!("changed-cells:{}", metadata.changed_cell_count));
        }

        if reasons.is_empty() {
            return Ok(());
        }

        let path = self.snapshot_trigger_path(&metadata.label, metadata.frame_seq);
        let header = [
            format!("twatch snapshot(trigger) | {}", self.command_display()),
            format!("reason: {}", reasons.join(" | ")),
        ];
        save_snapshot(snapshot, &path, self.screenshot_format, &header)?;
        self.snapshot_trigger_fired = true;
        self.ui.status_message = Some(format!(
            "snapshot trigger saved: {} ({})",
            display_tmp_path(&path),
            self.screenshot_format.label()
        ));
        Ok(())
    }

    pub(super) fn maybe_run_aftercommand(&mut self, raw_output: &str) -> Result<()> {
        let Some(runtime) = &mut self.aftercommand_runtime else {
            return Ok(());
        };
        let Some(metadata) = &self.current_metadata else {
            return Ok(());
        };

        let result = runtime.evaluate_and_enqueue(AfterCommandEvent {
            changed: metadata.changed,
            output: raw_output.to_string(),
            timestamp_unix_ms: metadata.timestamp_unix_ms,
            frame_seq: metadata.frame_seq,
            width: metadata.width,
            height: metadata.height,
            changed_cell_count: metadata.changed_cell_count,
            last_input_summary: metadata.input_summary.clone(),
        })?;

        if let Some(reason) = result {
            self.ui.status_message = Some(format!("aftercommand: {reason}"));
        }
        Ok(())
    }

    fn snapshot_trigger_path(&self, label: &str, frame_seq: u64) -> PathBuf {
        let label = sanitize_snapshot_label(label);
        self.screenshot_dir.join(format!(
            "twatch-auto-{frame_seq:04}-{label}.{}",
            self.screenshot_format.extension()
        ))
    }
}

fn sanitize_snapshot_label(label: &str) -> String {
    label
        .chars()
        .map(|ch| match ch {
            '0'..='9' | 'A'..='Z' | 'a'..='z' | '-' | '_' => ch,
            _ => '_',
        })
        .collect::<String>()
}

fn display_tmp_path(path: &PathBuf) -> String {
    path.to_string_lossy().into_owned()
}
