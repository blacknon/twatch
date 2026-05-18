// Copyright (c) 2026 Blacknon. All rights reserved.
// Use of this source code is governed by an MIT license
// that can be found in the LICENSE file.

use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph};

use crate::app::{App, FocusPane};

use super::{BORDER_ACTIVE, BORDER_IDLE, PANEL_BG, SELECTION_BG};
use crate::ui::dialogs::centered_rect;

pub(super) fn draw_history_overlay(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let width = app.history_overlay_width();
    let overlay = Rect::new(
        area.right().saturating_sub(width),
        area.y,
        width,
        area.height,
    );

    if !app.ui.show_history {
        frame.render_widget(
            Paragraph::new("Hi")
                .alignment(Alignment::Center)
                .style(Style::default().fg(BORDER_IDLE).bg(PANEL_BG)),
            overlay,
        );
        return;
    }

    frame.render_widget(Clear, overlay);
    let title = if app.ui.focus == FocusPane::History {
        " History "
    } else {
        " history "
    };
    let visible_rows = usize::from(overlay.height.saturating_sub(2));
    let (start, end) = app.history_overlay_window(visible_rows);

    let mut items: Vec<ListItem<'_>> = Vec::with_capacity(end.saturating_sub(start));

    for row in start..end {
        if row == 0 {
            items.push(
                ListItem::new(Line::from("  latest")).style(if app.follow_latest {
                    Style::default().fg(Color::White).bg(SELECTION_BG)
                } else {
                    Style::default().fg(BORDER_ACTIVE)
                }),
            );
        } else {
            let index = app.filtered_indices()[row - 1];
            let meta = app.history_metadata(index);
            let style = if !app.follow_latest && index == app.selected_index {
                Style::default().fg(Color::White).bg(SELECTION_BG)
            } else if meta.changed {
                Style::default().fg(Color::Indexed(221))
            } else {
                Style::default().fg(Color::Gray)
            };
            items.push(
                ListItem::new(Line::from(format!(
                    "{} {}",
                    if !app.follow_latest && index == app.selected_index {
                        ">"
                    } else {
                        " "
                    },
                    meta.label
                )))
                .style(style),
            );
        }
    }

    let mut state = ListState::default();
    state.select(Some(app.selected_history_row_in_window(visible_rows)));
    let list = List::new(items)
        .block(
            Block::default()
                .title(title)
                .style(Style::default().bg(PANEL_BG))
                .borders(Borders::LEFT)
                .border_style(if app.ui.focus == FocusPane::History {
                    Style::default().fg(BORDER_ACTIVE)
                } else {
                    Style::default().fg(BORDER_IDLE)
                }),
        )
        .highlight_symbol(">");
    frame.render_stateful_widget(list, overlay, &mut state);
}

pub(super) fn draw_history_details(frame: &mut Frame<'_>, app: &App, area: Rect) {
    if !app.ui.show_history_details {
        return;
    }

    let Some(meta) = app.selected_history_metadata() else {
        return;
    };

    let marker = if app.follow_latest {
        "latest"
    } else {
        "selected"
    };
    let lines = vec![
        Line::from(format!("view: {marker}")),
        Line::from(format!("timestamp: {}", meta.label)),
        Line::from(format!("frame seq: {}", meta.frame_seq)),
        Line::from(format!("changed cells: {}", meta.changed_cell_count)),
        Line::from(format!(
            "input events: {}",
            meta.input_event_count_since_prev
        )),
        Line::from(format!("screen size: {}x{}", meta.width, meta.height)),
        Line::from(format!(
            "resized: {}",
            if meta.resized { "yes" } else { "no" }
        )),
        Line::from(format!(
            "input summary: {}",
            if meta.input_summary.is_empty() {
                "-"
            } else {
                meta.input_summary.as_str()
            }
        )),
        Line::from(if meta.resized {
            format!(
                "resize info: {}x{} -> {}x{} ({})",
                meta.resize_from_width,
                meta.resize_from_height,
                meta.resize_to_width,
                meta.resize_to_height,
                meta.resize_source
            )
        } else {
            "resize info: -".to_string()
        }),
        Line::from(""),
        Line::from("Shift+S: toggle details"),
        Line::from("Ctrl+S: save snapshot"),
    ];

    let popup = centered_rect(56, 52, area);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines)
            .block(
                Block::default()
                    .title(" Frame Info ")
                    .borders(Borders::ALL)
                    .style(Style::default().bg(PANEL_BG)),
            )
            .style(Style::default().fg(Color::White).bg(PANEL_BG)),
        popup,
    );
}
