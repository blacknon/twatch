// Copyright (c) 2026 Blacknon. All rights reserved.
// Use of this source code is governed by an MIT license
// that can be found in the LICENSE file.

use super::{prepare_snapshot, render_output};
use crate::cli::{Cli, CropSpec, DiffModeArg, ScreenshotFormatArg};
use crate::screen::ScreenSnapshot;

#[test]
fn crops_from_top_left() {
    let snapshot = ScreenSnapshot::from_text_lines(6, 3, &["abcdef", "ghijkl", "mnopqr"]);
    let cropped = prepare_snapshot(
        &snapshot,
        Some(CropSpec {
            x: 1,
            y: 1,
            width: 3,
            height: 2,
        }),
    );
    assert_eq!(cropped.lines(), vec!["hij".to_string(), "nop".to_string()]);
}

#[test]
fn renders_list_diff_only() {
    let mut cli = test_cli();
    cli.differences = DiffModeArg::List;
    cli.batch_diff_only = true;
    cli.batch_no_color = true;
    let before = ScreenSnapshot::from_text_lines(8, 1, &["alpha"]);
    let after = ScreenSnapshot::from_text_lines(8, 1, &["bravo"]);
    let rendered = render_output(&cli, "tick", &after, Some(&before));
    assert!(rendered.contains("+  bravo"));
    assert!(rendered.contains("-  alpha"));
    assert!(!rendered.contains("   alpha"));
}

#[test]
fn renders_list_diff_like_hwatch_prefixes() {
    let mut cli = test_cli();
    cli.differences = DiffModeArg::List;
    cli.batch_no_color = true;
    let before = ScreenSnapshot::from_text_lines(8, 2, &["same", "alpha"]);
    let after = ScreenSnapshot::from_text_lines(8, 2, &["same", "bravo"]);
    let rendered = render_output(&cli, "tick", &after, Some(&before));
    assert!(rendered.contains("   same"));
    assert!(rendered.contains("-  alpha"));
    assert!(rendered.contains("+  bravo"));
}

#[test]
fn renders_word_diff_with_same_prefix_shape() {
    let mut cli = test_cli();
    cli.differences = DiffModeArg::Word;
    cli.batch_no_color = true;
    let before = ScreenSnapshot::from_text_lines(16, 1, &["alpha beta"]);
    let after = ScreenSnapshot::from_text_lines(16, 1, &["alpha gamma"]);
    let rendered = render_output(&cli, "tick", &after, Some(&before));
    assert!(rendered.contains("-  "));
    assert!(rendered.contains("+  "));
    assert!(rendered.contains("alpha"));
    assert!(rendered.contains("beta"));
    assert!(rendered.contains("gamma"));
}

fn test_cli() -> Cli {
    Cli {
        interval: 2.0,
        batch: true,
        batch_count: None,
        batch_size: None,
        batch_crop: None,
        batch_diff_only: false,
        batch_no_color: false,
        aftercommand: None,
        aftercommand_regex: None,
        aftercommand_change_cells: None,
        aftercommand_every: None,
        aftercommand_debounce_ms: None,
        aftercommand_timeout_ms: 3000,
        compress: false,
        logfile: None,
        replay: None,
        screenshot_dir: "/tmp".to_string(),
        screenshot_format: ScreenshotFormatArg::Text,
        snapshot_on: None,
        snapshot_on_regex: None,
        snapshot_on_change_cells: None,
        snapshot_once: false,
        shell: "sh -c".to_string(),
        differences: DiffModeArg::None,
        limit: 500,
        checkpoint_interval: 12,
        debug: false,
        command: vec!["mock".to_string()],
    }
}
