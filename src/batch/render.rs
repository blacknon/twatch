// Copyright (c) 2026 Blacknon. All rights reserved.
// Use of this source code is governed by an MIT license
// that can be found in the LICENSE file.

use crate::cli::{Cli, DiffModeArg};
use crate::screen::ScreenSnapshot;
use crate::screenshot::render_ansi_text;
use similar::{ChangeTag, DiffTag, TextDiff};

pub(super) fn render_output(
    cli: &Cli,
    label: &str,
    current: &ScreenSnapshot,
    previous: Option<&ScreenSnapshot>,
) -> String {
    let header = batch_header(cli, label, current);
    match (cli.differences, previous) {
        (DiffModeArg::None, _) | (DiffModeArg::Watch, _) | (_, None) => {
            render_snapshot(current, &header, !cli.batch_no_color)
        }
        (DiffModeArg::List, Some(before)) => render_list_diff(
            before,
            current,
            &header,
            cli.batch_diff_only,
            !cli.batch_no_color,
        ),
        (DiffModeArg::Word, Some(before)) => render_word_diff(
            before,
            current,
            &header,
            cli.batch_diff_only,
            !cli.batch_no_color,
        ),
    }
}

fn batch_header(cli: &Cli, label: &str, snapshot: &ScreenSnapshot) -> [String; 2] {
    let size = format!("{}x{}", snapshot.width(), snapshot.height());
    [
        format!("twatch | batch mode | {label}"),
        format!(
            "diff: {}{} | size: {}",
            diff_mode_label(cli.differences),
            if cli.batch_diff_only {
                " | diff-only"
            } else {
                ""
            },
            size
        ),
    ]
}

fn diff_mode_label(mode: DiffModeArg) -> &'static str {
    match mode {
        DiffModeArg::None => "none",
        DiffModeArg::Watch => "watch",
        DiffModeArg::List => "list",
        DiffModeArg::Word => "word",
    }
}

fn render_snapshot(snapshot: &ScreenSnapshot, header: &[String; 2], color: bool) -> String {
    if color {
        render_ansi_text(snapshot, header)
    } else {
        snapshot.batch_render(header)
    }
}

fn render_list_diff(
    before: &ScreenSnapshot,
    after: &ScreenSnapshot,
    header: &[String; 2],
    diff_only: bool,
    color: bool,
) -> String {
    let mut out = render_header_lines(header);
    let before_text = join_lines(before);
    let after_text = join_lines(after);
    let diff = TextDiff::from_lines(&before_text, &after_text);

    for op in diff.ops() {
        let old_lines = &diff.old_slices()[op.old_range()];
        let new_lines = &diff.new_slices()[op.new_range()];
        match op.tag() {
            DiffTag::Equal => {
                if diff_only {
                    continue;
                }
                for line in new_lines {
                    push_prefixed_line(&mut out, "   ", line.trim_end_matches('\n'), color, None);
                }
            }
            DiffTag::Delete => {
                render_plain_lines(&mut out, old_lines, "-  ", color, LineColor::Removed);
            }
            DiffTag::Insert => {
                render_plain_lines(&mut out, new_lines, "+  ", color, LineColor::Added);
            }
            DiffTag::Replace => {
                render_plain_lines(&mut out, old_lines, "-  ", color, LineColor::Removed);
                render_plain_lines(&mut out, new_lines, "+  ", color, LineColor::Added);
            }
        }
    }

    out
}

fn render_word_diff(
    before: &ScreenSnapshot,
    after: &ScreenSnapshot,
    header: &[String; 2],
    diff_only: bool,
    color: bool,
) -> String {
    let mut out = render_header_lines(header);
    let before_text = join_lines(before);
    let after_text = join_lines(after);
    let diff = TextDiff::from_lines(&before_text, &after_text);

    for op in diff.ops() {
        let old_lines = &diff.old_slices()[op.old_range()];
        let new_lines = &diff.new_slices()[op.new_range()];
        match op.tag() {
            DiffTag::Equal => {
                if diff_only {
                    continue;
                }
                for line in new_lines {
                    push_prefixed_line(&mut out, "   ", line.trim_end_matches('\n'), color, None);
                }
            }
            _ if !old_lines.is_empty() && old_lines.len() == new_lines.len() => {
                for (old_line, new_line) in old_lines.iter().zip(new_lines.iter()) {
                    push_word_diff_line(
                        &mut out,
                        "-  ",
                        old_line.trim_end_matches('\n'),
                        new_line.trim_end_matches('\n'),
                        color,
                        LineColor::Removed,
                    );
                    push_word_diff_line(
                        &mut out,
                        "+  ",
                        new_line.trim_end_matches('\n'),
                        old_line.trim_end_matches('\n'),
                        color,
                        LineColor::Added,
                    );
                }
            }
            _ => {
                render_plain_lines(&mut out, old_lines, "-  ", color, LineColor::Removed);
                render_plain_lines(&mut out, new_lines, "+  ", color, LineColor::Added);
            }
        }
    }

    out
}

fn render_header_lines(header: &[String; 2]) -> String {
    let mut out = String::new();
    for line in header {
        out.push_str(line);
        out.push('\n');
    }
    out
}

fn render_plain_lines(
    out: &mut String,
    lines: &[&str],
    prefix: &str,
    color: bool,
    line_color: LineColor,
) {
    for line in lines {
        push_prefixed_line(
            out,
            prefix,
            line.trim_end_matches('\n'),
            color,
            Some(line_color),
        );
    }
}

fn push_prefixed_line(
    out: &mut String,
    prefix: &str,
    text: &str,
    color: bool,
    line_color: Option<LineColor>,
) {
    if color && let Some(line_color) = line_color {
        out.push_str(line_color.sgr());
    }
    out.push_str(prefix);
    out.push_str(text);
    if color && line_color.is_some() {
        out.push_str("\x1b[0m");
    }
    out.push('\n');
}

fn push_word_diff_line(
    out: &mut String,
    prefix: &str,
    line: &str,
    other: &str,
    color: bool,
    line_color: LineColor,
) {
    let word_diff = TextDiff::from_words(other, line);
    if color {
        out.push_str(line_color.sgr());
    }
    out.push_str(prefix);
    for change in word_diff.iter_all_changes() {
        if change.tag() == ChangeTag::Delete {
            continue;
        }
        let text = change.to_string();
        if text.is_empty() {
            continue;
        }
        if color && change.tag() == ChangeTag::Insert {
            out.push_str("\x1b[7m");
            out.push_str(&text);
            out.push_str("\x1b[27m");
            out.push_str(line_color.sgr());
        } else {
            out.push_str(&text);
        }
    }
    if color {
        out.push_str("\x1b[0m");
    }
    out.push('\n');
}

fn join_lines(snapshot: &ScreenSnapshot) -> String {
    snapshot
        .lines()
        .into_iter()
        .map(|line| format!("{line}\n"))
        .collect()
}

#[derive(Copy, Clone)]
enum LineColor {
    Removed,
    Added,
}

impl LineColor {
    fn sgr(self) -> &'static str {
        match self {
            Self::Removed => "\x1b[31m",
            Self::Added => "\x1b[32m",
        }
    }
}
