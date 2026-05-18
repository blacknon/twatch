// Copyright (c) 2026 Blacknon. All rights reserved.
// Use of this source code is governed by an MIT license
// that can be found in the LICENSE file.

use anyhow::{Context, Result};

use crate::input_key::{KeyPress, parse_key_press};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum KeyAction {
    Up,
    WatchPaneUp,
    HistoryPaneUp,
    Down,
    WatchPaneDown,
    HistoryPaneDown,
    PageUp,
    WatchPanePageUp,
    HistoryPanePageUp,
    PageDown,
    WatchPanePageDown,
    HistoryPanePageDown,
    MoveTop,
    WatchPaneMoveTop,
    HistoryPaneMoveTop,
    MoveEnd,
    WatchPaneMoveEnd,
    HistoryPaneMoveEnd,
    ToggleFocus,
    FocusWatchPane,
    FocusHistoryPane,
    Quit,
    Reset,
    Delete,
    ClearExceptSelected,
    Cancel,
    ForceCancel,
    Help,
    ToggleViewHistoryPane,
    ToggleHistorySummary,
    ToggleDiffMode,
    SetDiffModeNone,
    SetDiffModeWatch,
    TogglePause,
    ToggleChildPause,
    ChangeFilterMode,
    ChangeRegexFilterMode,
    EnterAppInputMode,
    LeaveAppInputMode,
    ToggleInspector,
    SaveSnapshot,
    CycleSnapshotFormat,
    ScrollLeft,
    ScrollRight,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct KeyBinding {
    pub(crate) trigger: KeyPress,
    pub(crate) action: KeyAction,
}

pub(crate) fn compile_keymap(specs: &[String]) -> Result<Vec<KeyBinding>> {
    let mut bindings = Vec::new();
    for spec in specs {
        let (left, right) = spec
            .split_once('=')
            .with_context(|| format!("keymap must be KEY=ACTION: {spec}"))?;
        let trigger = parse_key_press(left.trim())?;
        let action = parse_key_action(right.trim())?;
        bindings.push(KeyBinding { trigger, action });
    }
    Ok(bindings)
}

fn parse_key_action(value: &str) -> Result<KeyAction> {
    Ok(match value.trim().to_ascii_lowercase().as_str() {
        "up" => KeyAction::Up,
        "watch_pane_up" => KeyAction::WatchPaneUp,
        "history_pane_up" => KeyAction::HistoryPaneUp,
        "down" => KeyAction::Down,
        "watch_pane_down" => KeyAction::WatchPaneDown,
        "history_pane_down" => KeyAction::HistoryPaneDown,
        "page_up" => KeyAction::PageUp,
        "watch_pane_page_up" => KeyAction::WatchPanePageUp,
        "history_pane_page_up" => KeyAction::HistoryPanePageUp,
        "page_down" => KeyAction::PageDown,
        "watch_pane_page_down" => KeyAction::WatchPanePageDown,
        "history_pane_page_down" => KeyAction::HistoryPanePageDown,
        "move_top" => KeyAction::MoveTop,
        "watch_pane_move_top" => KeyAction::WatchPaneMoveTop,
        "history_pane_move_top" => KeyAction::HistoryPaneMoveTop,
        "move_end" => KeyAction::MoveEnd,
        "watch_pane_move_end" => KeyAction::WatchPaneMoveEnd,
        "history_pane_move_end" => KeyAction::HistoryPaneMoveEnd,
        "toggle_focus" => KeyAction::ToggleFocus,
        "focus_watch_pane" => KeyAction::FocusWatchPane,
        "focus_history_pane" => KeyAction::FocusHistoryPane,
        "quit" => KeyAction::Quit,
        "reset" => KeyAction::Reset,
        "delete" => KeyAction::Delete,
        "clear_except_selected" => KeyAction::ClearExceptSelected,
        "cancel" => KeyAction::Cancel,
        "force_cancel" => KeyAction::ForceCancel,
        "help" => KeyAction::Help,
        "toggle_view_history_pane" => KeyAction::ToggleViewHistoryPane,
        "toggle_history_summary" => KeyAction::ToggleHistorySummary,
        "toggle_diff_mode" => KeyAction::ToggleDiffMode,
        "set_diff_mode_none" | "set_diff_mode_plane" => KeyAction::SetDiffModeNone,
        "set_diff_mode_watch" => KeyAction::SetDiffModeWatch,
        "toggle_pause" => KeyAction::TogglePause,
        "toggle_child_pause" => KeyAction::ToggleChildPause,
        "change_filter_mode" => KeyAction::ChangeFilterMode,
        "change_regex_filter_mode" => KeyAction::ChangeRegexFilterMode,
        "enter_app_input_mode" => KeyAction::EnterAppInputMode,
        "leave_app_input_mode" => KeyAction::LeaveAppInputMode,
        "toggle_inspector" => KeyAction::ToggleInspector,
        "save_snapshot" => KeyAction::SaveSnapshot,
        "cycle_snapshot_format" => KeyAction::CycleSnapshotFormat,
        "scroll_left" => KeyAction::ScrollLeft,
        "scroll_right" => KeyAction::ScrollRight,
        _ => anyhow::bail!("unsupported key action: {value}"),
    })
}

#[cfg(test)]
mod tests {
    use super::{KeyAction, compile_keymap};

    #[test]
    fn parses_hwatch_style_keymap() {
        let bindings =
            compile_keymap(&["ctrl-p=history_pane_up".to_string(), "q=quit".to_string()]).unwrap();
        assert_eq!(bindings.len(), 2);
        assert_eq!(bindings[0].action, KeyAction::HistoryPaneUp);
        assert_eq!(bindings[1].action, KeyAction::Quit);
    }
}
