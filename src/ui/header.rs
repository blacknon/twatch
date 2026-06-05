// Copyright (c) 2026 Blacknon. All rights reserved.
// Use of this source code is governed by an MIT license
// that can be found in the LICENSE file.

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::app::{App, DiffMode, FilterMode};

use super::{
    BADGE_BLUE, BADGE_CYAN, BADGE_DIM_TEXT, BADGE_GREEN, BADGE_GREY, BADGE_MAGENTA, BADGE_TEXT,
    COMMAND_ACCENT, HEADER_BG, HEADER_SUB_BG, TIMESTAMP_FG,
};

pub(super) fn draw_header_line_one(frame: &mut Frame<'_>, app: &App, area: Rect) {
    frame.render_widget(
        Paragraph::new("").style(Style::default().bg(HEADER_BG)),
        area,
    );

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(10), Constraint::Length(24)])
        .split(area);

    let command_text = format!("Event {}", app.command_display());
    let command_line = Line::from(vec![
        Span::styled(" ", Style::default().bg(HEADER_BG)),
        Span::styled(
            truncate_text(
                &command_text,
                usize::from(chunks[0].width).saturating_sub(1),
            ),
            Style::default().fg(Color::White).bg(HEADER_BG),
        ),
    ]);

    let timestamp_line = Line::from(vec![Span::styled(
        format!(
            " {} ",
            app.current_label().unwrap_or("---- -- -- --:--:--.---")
        ),
        Style::default()
            .fg(TIMESTAMP_FG)
            .bg(HEADER_BG)
            .add_modifier(Modifier::BOLD),
    )]);

    frame.render_widget(Paragraph::new(command_line), chunks[0]);
    frame.render_widget(
        Paragraph::new(timestamp_line).alignment(Alignment::Right),
        chunks[1],
    );
}

pub(super) fn draw_header_line_two(frame: &mut Frame<'_>, app: &App, area: Rect) {
    frame.render_widget(
        Paragraph::new("").style(Style::default().bg(HEADER_SUB_BG)),
        area,
    );

    let filter_label = if app.is_search_mode() {
        if app.ui.filter_query.is_empty() {
            format!(
                "Filter: {}",
                match app.filter_mode() {
                    FilterMode::Plain => "/",
                    FilterMode::Regex => "*",
                }
            )
        } else {
            format!(
                "Filter: {}{}",
                match app.filter_mode() {
                    FilterMode::Plain => "/",
                    FilterMode::Regex => "*",
                },
                app.ui.filter_query
            )
        }
    } else if app.ui.filter_query.is_empty() {
        "Filter".to_string()
    } else {
        format!(
            "Filter: {}{}",
            match app.filter_mode() {
                FilterMode::Plain => "/",
                FilterMode::Regex => "*",
            },
            app.ui.filter_query
        )
    };
    let input_hint_long = if app.ui.app_input_mode {
        " | Input: app mode, Ctrl-g to return"
    } else {
        " | Input: press i to control app"
    };
    let input_hint_short = if app.ui.app_input_mode {
        " | Ctrl-g to return"
    } else {
        " | i to control app"
    };

    let hist_label = format!(
        "Hist {}/{} {} {}",
        format!("{:05}", app.display_filtered_history_len()),
        format!("{:05}", app.display_history_len()),
        if app.ui.show_history { "Open" } else { "Close" },
        if app.follow_latest { "Latest" } else { "Hold" }
    );
    let input_label = if app.ui.app_input_mode {
        "Input On".to_string()
    } else {
        "Input Off".to_string()
    };
    let capture_label = if app.paused {
        "Capture Pause".to_string()
    } else {
        "Capture Run".to_string()
    };
    let child_label = if !app.child_pause_supported {
        "Child N/A".to_string()
    } else if app.child_paused {
        "Child Stop".to_string()
    } else {
        "Child Run".to_string()
    };
    let diff_label = app.diff_mode.label().to_string();
    let right_width = badge_width(&hist_label)
        + 1
        + badge_width(&input_label)
        + 1
        + badge_width(&capture_label)
        + 1
        + badge_width(&child_label)
        + 1
        + badge_width(&diff_label);

    let available_left = usize::from(area.width).saturating_sub(right_width.saturating_add(1));
    let resize_summary = app.selected_resize_summary();
    let left_text = fit_header_left(
        &filter_label,
        input_hint_long,
        input_hint_short,
        app.ui
            .status_message
            .as_deref()
            .or(resize_summary.as_deref())
            .or_else(|| app.selected_input_summary()),
        available_left.saturating_sub(1),
    );
    let left = Line::from(vec![Span::styled(
        format!(" {}", left_text),
        Style::default()
            .fg(if app.ui.filter_query.is_empty() && !app.is_search_mode() {
                Color::Indexed(246)
            } else {
                COMMAND_ACCENT
            })
            .bg(HEADER_SUB_BG),
    )]);

    let right = Line::from(vec![
        status_badge(hist_label, BADGE_CYAN, true),
        gap(HEADER_SUB_BG),
        status_badge(input_label, BADGE_GREEN, true),
        gap(HEADER_SUB_BG),
        status_badge(
            capture_label,
            if app.paused {
                BADGE_MAGENTA
            } else {
                BADGE_GREEN
            },
            true,
        ),
        gap(HEADER_SUB_BG),
        status_badge(
            child_label,
            if app.child_paused {
                BADGE_MAGENTA
            } else {
                BADGE_GREEN
            },
            app.child_pause_supported,
        ),
        gap(HEADER_SUB_BG),
        status_badge(
            diff_label,
            match app.diff_mode {
                DiffMode::None => BADGE_MAGENTA,
                DiffMode::Watch => BADGE_BLUE,
            },
            true,
        ),
    ]);

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(right_width.min(usize::from(area.width)) as u16),
        ])
        .split(area);

    frame.render_widget(Paragraph::new(left), chunks[0]);
    frame.render_widget(Paragraph::new(right).alignment(Alignment::Right), chunks[1]);
}

fn status_badge<T: Into<String>>(label: T, bg: Color, active: bool) -> Span<'static> {
    let fg = if active { BADGE_TEXT } else { BADGE_DIM_TEXT };
    let bg = if active { bg } else { BADGE_GREY };
    Span::styled(
        format!("[{}]", label.into()),
        Style::default().fg(fg).bg(bg).add_modifier(Modifier::BOLD),
    )
}

fn gap(bg: Color) -> Span<'static> {
    Span::styled(" ", Style::default().bg(bg))
}

fn badge_width(label: &str) -> usize {
    UnicodeWidthStr::width(label) + 2
}

fn fit_header_left(
    filter_label: &str,
    long_hint: &str,
    short_hint: &str,
    status_hint: Option<&str>,
    max_width: usize,
) -> String {
    if let Some(status_hint) = status_hint {
        let status = format!("{filter_label} | {status_hint}");
        if display_width(&status) <= max_width {
            return status;
        }
    }

    let long = format!("{filter_label}{long_hint}");
    if display_width(&long) <= max_width {
        return long;
    }

    let short = format!("{filter_label}{short_hint}");
    if display_width(&short) <= max_width {
        return short;
    }

    truncate_text(filter_label, max_width)
}

fn truncate_text(text: &str, max_width: usize) -> String {
    if display_width(text) <= max_width {
        return text.to_string();
    }
    if max_width <= 1 {
        return String::new();
    }

    let keep = max_width.saturating_sub(1);
    let mut out = String::new();
    for ch in text.chars() {
        if display_width(&out) + UnicodeWidthChar::width(ch).unwrap_or(0) > keep {
            break;
        }
        out.push(ch);
    }
    out.push('…');
    out
}

fn display_width(text: &str) -> usize {
    UnicodeWidthStr::width(text)
}
