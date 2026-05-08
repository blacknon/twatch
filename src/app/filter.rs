use anyhow::Result;
use regex::Regex;

use super::{App, FilterMode};

impl App {
    pub(super) fn rebuild_filter(&mut self) -> Result<()> {
        self.filtered = self.find_filtered_history()?;
        let visible_start = self.visible_history_start();
        self.filtered.retain(|index| *index >= visible_start);
        self.filtered.reverse();

        if self.ui.filter_query.is_empty() {
            if self.filtered.is_empty() {
                self.follow_latest = true;
            } else if !self.follow_latest && !self.filtered.contains(&self.selected_index) {
                self.selected_index = self.fallback_filtered_selection(visible_start);
            }
            return Ok(());
        }

        if self.follow_latest && !self.current_snapshot_matches_filter() {
            if let Some(first) = self.filtered.first().copied() {
                self.selected_index = first;
                self.follow_latest = false;
            }
        } else if !self.follow_latest && !self.filtered.contains(&self.selected_index) {
            self.selected_index = self.fallback_filtered_selection(visible_start);
        }
        Ok(())
    }

    fn find_filtered_history(&mut self) -> Result<Vec<usize>> {
        match self.filter_mode {
            FilterMode::Plain => self.history.find_by_query(&self.ui.filter_query),
            FilterMode::Regex => {
                if self.ui.filter_query.is_empty() {
                    Ok((0..self.history.len()).collect())
                } else {
                    match Regex::new(&self.ui.filter_query) {
                        Ok(regex) => self.history.find_by_regex(&regex),
                        Err(err) => {
                            self.ui.status_message = Some(format!("regex error: {err}"));
                            Ok(Vec::new())
                        }
                    }
                }
            }
        }
    }

    fn current_snapshot_matches_filter(&self) -> bool {
        let Some(snapshot) = &self.current_snapshot else {
            return false;
        };
        let lines = snapshot.lines();

        match self.filter_mode {
            FilterMode::Plain => {
                let needle = self.ui.filter_query.to_lowercase();
                lines
                    .iter()
                    .any(|line| line.to_lowercase().contains(&needle))
            }
            FilterMode::Regex => Regex::new(&self.ui.filter_query)
                .ok()
                .is_some_and(|regex| lines.iter().any(|line| regex.is_match(line))),
        }
    }

    fn fallback_filtered_selection(&self, visible_start: usize) -> usize {
        if self.selected_index < visible_start {
            *self.filtered.last().unwrap_or(&0)
        } else {
            *self.filtered.first().unwrap_or(&0)
        }
    }
}
