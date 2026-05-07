use crate::cli::{Cli, CropSpec, DiffModeArg};
use crate::screen::ScreenSnapshot;
use crate::screenshot::render_ansi_text;
use similar::{ChangeTag, DiffTag, TextDiff};

pub fn terminal_size(cli: &Cli) -> (u16, u16) {
    cli.batch_size
        .map(|size| (size.width, size.height))
        .unwrap_or_else(|| crossterm::terminal::size().unwrap_or((120, 40)))
}

pub fn prepare_snapshot(snapshot: &ScreenSnapshot, crop: Option<CropSpec>) -> ScreenSnapshot {
    crop.map_or_else(
        || snapshot.clone(),
        |crop| snapshot.cropped(crop.x, crop.y, crop.width, crop.height),
    )
}

pub fn render_output(
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
                for line in old_lines {
                    push_prefixed_line(
                        &mut out,
                        "-  ",
                        line.trim_end_matches('\n'),
                        color,
                        Some(LineColor::Removed),
                    );
                }
            }
            DiffTag::Insert => {
                for line in new_lines {
                    push_prefixed_line(
                        &mut out,
                        "+  ",
                        line.trim_end_matches('\n'),
                        color,
                        Some(LineColor::Added),
                    );
                }
            }
            DiffTag::Replace => {
                for line in old_lines {
                    push_prefixed_line(
                        &mut out,
                        "-  ",
                        line.trim_end_matches('\n'),
                        color,
                        Some(LineColor::Removed),
                    );
                }
                for line in new_lines {
                    push_prefixed_line(
                        &mut out,
                        "+  ",
                        line.trim_end_matches('\n'),
                        color,
                        Some(LineColor::Added),
                    );
                }
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
                for line in old_lines {
                    push_prefixed_line(
                        &mut out,
                        "-  ",
                        line.trim_end_matches('\n'),
                        color,
                        Some(LineColor::Removed),
                    );
                }
                for line in new_lines {
                    push_prefixed_line(
                        &mut out,
                        "+  ",
                        line.trim_end_matches('\n'),
                        color,
                        Some(LineColor::Added),
                    );
                }
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

#[cfg(test)]
mod tests {
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
            compress: false,
            logfile: None,
            screenshot_dir: "/tmp".to_string(),
            screenshot_format: ScreenshotFormatArg::Text,
            shell: "sh -c".to_string(),
            differences: DiffModeArg::None,
            limit: 500,
            checkpoint_interval: 12,
            command: vec!["mock".to_string()],
        }
    }
}
