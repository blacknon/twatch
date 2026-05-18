// Copyright (c) 2026 Blacknon. All rights reserved.
// Use of this source code is governed by an MIT license
// that can be found in the LICENSE file.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::Color;

use crate::app::App;

mod dialogs;
mod header;
mod history;
mod watch;

pub(super) const HEADER_BG: Color = Color::Indexed(234);
pub(super) const HEADER_SUB_BG: Color = Color::Indexed(235);
pub(super) const PANEL_BG: Color = Color::Indexed(234);
pub(super) const BORDER_ACTIVE: Color = Color::Indexed(45);
pub(super) const BORDER_IDLE: Color = Color::Indexed(240);
pub(super) const SEARCH_BG: Color = Color::Indexed(220);
pub(super) const DIFF_BG: Color = Color::Rgb(238, 238, 238);
pub(super) const DIFF_FG: Color = Color::Black;
pub(super) const SELECTION_BG: Color = Color::Indexed(24);
pub(super) const COMMAND_ACCENT: Color = Color::Indexed(51);
pub(super) const TIMESTAMP_FG: Color = Color::Indexed(51);
pub(super) const BADGE_TEXT: Color = Color::Black;
pub(super) const BADGE_DIM_TEXT: Color = Color::Indexed(252);
pub(super) const BADGE_GREEN: Color = Color::Indexed(46);
pub(super) const BADGE_CYAN: Color = Color::Indexed(51);
pub(super) const BADGE_BLUE: Color = Color::Indexed(39);
pub(super) const BADGE_MAGENTA: Color = Color::Indexed(201);
pub(super) const BADGE_GREY: Color = Color::Indexed(238);

pub fn draw(frame: &mut Frame<'_>, app: &App) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(1),
        ])
        .split(area);

    header::draw_header_line_one(frame, app, chunks[0]);
    header::draw_header_line_two(frame, app, chunks[1]);

    watch::draw_watch(frame, app, chunks[2]);
    history::draw_history_overlay(frame, app, chunks[2]);
    history::draw_history_details(frame, app, area);

    if app.ui.show_help {
        dialogs::draw_help(frame, area);
    }
    if app.ui.show_exit_confirm {
        dialogs::draw_exit_confirm(frame, area);
    }
}
