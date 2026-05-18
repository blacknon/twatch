// Copyright (c) 2026 Blacknon. All rights reserved.
// Use of this source code is governed by an MIT license
// that can be found in the LICENSE file.

use crate::aftercommand::{AfterCommandConfig, AfterCommandEvent};

pub(super) fn evaluate_rules(
    config: &AfterCommandConfig,
    event: &AfterCommandEvent,
    changed_frame_count: &mut usize,
    last_trigger_unix_ms: Option<u64>,
) -> Option<Vec<String>> {
    if !event.changed {
        return None;
    }

    *changed_frame_count += 1;

    if config
        .debounce_ms
        .zip(last_trigger_unix_ms)
        .is_some_and(|(debounce_ms, last_ms)| {
            event.timestamp_unix_ms.saturating_sub(last_ms) < debounce_ms
        })
    {
        return None;
    }

    let mut matched_rules = Vec::new();

    if let Some(regex) = &config.regex
        && regex.is_match(&event.output)
    {
        matched_rules.push(format!("regex:{}", regex.as_str()));
    }

    if let Some(threshold) = config.changed_cells
        && event.changed_cell_count >= threshold
    {
        matched_rules.push(format!("changed-cells:{}", event.changed_cell_count));
    }

    if let Some(every) = config.every
        && every > 0
        && *changed_frame_count % every == 0
    {
        matched_rules.push(format!("every:{every}"));
    }

    if config.regex.is_none() && config.changed_cells.is_none() && config.every.is_none() {
        matched_rules.push("changed".to_string());
    }

    if matched_rules.is_empty() {
        None
    } else {
        Some(matched_rules)
    }
}
