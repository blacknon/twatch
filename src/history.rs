use crate::diff::{LineDiff, WordDiff, diff_lines, diff_words};
use crate::screen::{Cell, ScreenSnapshot};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistoryMetadata {
    pub label: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum HistoryEntryKind {
    Checkpoint(ScreenSnapshot),
    Delta(FrameDelta),
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct HistoryEntry {
    kind: HistoryEntryKind,
    metadata: HistoryMetadata,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct FrameDelta {
    width: u16,
    height: u16,
    changes: Vec<(usize, Cell)>,
}

#[derive(Clone, Debug)]
pub struct HistoryStore {
    checkpoint_interval: usize,
    entries: Vec<HistoryEntry>,
}

impl HistoryStore {
    pub fn new(checkpoint_interval: usize) -> Self {
        Self {
            checkpoint_interval: checkpoint_interval.max(1),
            entries: Vec::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn push(&mut self, snapshot: ScreenSnapshot, metadata: HistoryMetadata) {
        let entry = match self.entries.last() {
            None => HistoryEntry {
                kind: HistoryEntryKind::Checkpoint(snapshot),
                metadata,
            },
            Some(_) if self.entries.len() % self.checkpoint_interval == 0 => HistoryEntry {
                kind: HistoryEntryKind::Checkpoint(snapshot),
                metadata,
            },
            Some(_) => {
                let previous = self
                    .snapshot(self.entries.len() - 1)
                    .expect("previous snapshot must exist");
                HistoryEntry {
                    kind: HistoryEntryKind::Delta(FrameDelta::between(&previous, &snapshot)),
                    metadata,
                }
            }
        };

        self.entries.push(entry);
    }

    pub fn snapshot(&self, index: usize) -> Option<ScreenSnapshot> {
        let entry = self.entries.get(index)?;
        match &entry.kind {
            HistoryEntryKind::Checkpoint(snapshot) => Some(snapshot.clone()),
            HistoryEntryKind::Delta(_) => {
                let checkpoint_index = self.find_checkpoint(index)?;
                let mut snapshot = match &self.entries[checkpoint_index].kind {
                    HistoryEntryKind::Checkpoint(snapshot) => snapshot.clone(),
                    HistoryEntryKind::Delta(_) => return None,
                };

                for entry in &self.entries[(checkpoint_index + 1)..=index] {
                    if let HistoryEntryKind::Delta(delta) = &entry.kind {
                        delta.apply(&mut snapshot);
                    }
                }

                Some(snapshot)
            }
        }
    }

    pub fn metadata(&self, index: usize) -> Option<&HistoryMetadata> {
        self.entries.get(index).map(|entry| &entry.metadata)
    }

    pub fn find_by_query(&self, query: &str) -> Vec<usize> {
        if query.is_empty() {
            return (0..self.entries.len()).collect();
        }

        let needle = query.to_lowercase();
        self.entries
            .iter()
            .enumerate()
            .filter_map(|(index, _)| {
                let snapshot = self.snapshot(index)?;
                let matches_snapshot = snapshot
                    .lines()
                    .iter()
                    .any(|line| line.to_lowercase().contains(&needle));
                if matches_snapshot { Some(index) } else { None }
            })
            .collect()
    }

    pub fn line_diffs(&self, before: usize, after: usize) -> Option<Vec<LineDiff>> {
        let before = self.snapshot(before)?;
        let after = self.snapshot(after)?;
        Some(diff_lines(&before.lines(), &after.lines()))
    }

    pub fn word_diffs(&self, before: usize, after: usize) -> Option<Vec<WordDiff>> {
        let line_diffs = self.line_diffs(before, after)?;
        Some(
            line_diffs
                .iter()
                .map(|diff| diff_words(&diff.before, &diff.after, diff.line_index))
                .collect(),
        )
    }

    pub fn debug_storage_stats(&self) -> (usize, usize) {
        self.entries
            .iter()
            .fold((0, 0), |(checkpoints, deltas), entry| match entry.kind {
                HistoryEntryKind::Checkpoint(_) => (checkpoints + 1, deltas),
                HistoryEntryKind::Delta(_) => (checkpoints, deltas + 1),
            })
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
                let before_cell = before.cell(x, y).copied().unwrap_or_default();
                let after_cell = after.cell(x, y).copied().unwrap_or_default();

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

#[cfg(test)]
mod tests {
    use super::{HistoryMetadata, HistoryStore};
    use crate::screen::ScreenSnapshot;

    #[test]
    fn reconstructs_snapshots_from_checkpoints_and_deltas() {
        let mut history = HistoryStore::new(3);

        history.push(
            ScreenSnapshot::from_text_lines(6, 2, &["alpha", ""]),
            meta("t0"),
        );
        history.push(
            ScreenSnapshot::from_text_lines(6, 2, &["alpha", "beta"]),
            meta("t1"),
        );
        history.push(
            ScreenSnapshot::from_text_lines(6, 2, &["gamma", "beta"]),
            meta("t2"),
        );
        history.push(
            ScreenSnapshot::from_text_lines(6, 2, &["gamma", "delta"]),
            meta("t3"),
        );

        assert_eq!(
            history.snapshot(3).expect("snapshot").lines(),
            vec!["gamma".to_string(), "delta".to_string()]
        );

        assert_eq!(history.debug_storage_stats(), (2, 2));
    }

    #[test]
    fn finds_matching_snapshots_by_query() {
        let mut history = HistoryStore::new(2);
        history.push(
            ScreenSnapshot::from_text_lines(8, 1, &["jobs 10"]),
            meta("a"),
        );
        history.push(
            ScreenSnapshot::from_text_lines(8, 1, &["jobs 11"]),
            meta("b"),
        );
        history.push(
            ScreenSnapshot::from_text_lines(8, 1, &["queue 12"]),
            meta("c"),
        );

        assert_eq!(history.find_by_query("jobs"), vec![0, 1]);
    }

    #[test]
    fn computes_line_and_word_diffs() {
        let mut history = HistoryStore::new(10);
        history.push(
            ScreenSnapshot::from_text_lines(16, 1, &["task pending"]),
            meta("a"),
        );
        history.push(
            ScreenSnapshot::from_text_lines(16, 1, &["task running"]),
            meta("b"),
        );

        let line_diffs = history.line_diffs(0, 1).expect("line diffs");
        assert_eq!(line_diffs.len(), 1);
        assert_eq!(line_diffs[0].before, "task pending");
        assert_eq!(line_diffs[0].after, "task running");

        let word_diffs = history.word_diffs(0, 1).expect("word diffs");
        assert_eq!(word_diffs[0].removed, vec!["pending".to_string()]);
        assert_eq!(word_diffs[0].added, vec!["running".to_string()]);
    }

    fn meta(label: &str) -> HistoryMetadata {
        HistoryMetadata {
            label: label.to_string(),
        }
    }
}
