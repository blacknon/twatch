use twatch::history::{HistoryMetadata, HistoryStore};
use twatch::screen::ScreenSnapshot;

fn main() {
    let mut history = HistoryStore::new(8);
    history.push(
        ScreenSnapshot::from_text_lines(24, 4, &["twatch bootstrap", "history core ready"]),
        HistoryMetadata {
            label: "bootstrap".to_string(),
        },
    );

    let latest = history
        .snapshot(history.len() - 1)
        .expect("bootstrap snapshot must exist");

    print!(
        "{}",
        latest.batch_render(&[
            "twatch | core history engine initialized",
            "search: <not yet wired> | focus: app",
        ])
    );
}
