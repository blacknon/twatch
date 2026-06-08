// Copyright (c) 2026 Blacknon. All rights reserved.
// Use of this source code is governed by an MIT license
// that can be found in the LICENSE file.

use std::path::PathBuf;

use anyhow::{Context, Result};
use regex::Regex;

use super::{App, DiffMode};
use crate::aftercommand::{AfterCommandConfig, AfterCommandRuntime};
use crate::child_bindings::{ChildBinding, compile_child_bindings};
use crate::cli::Cli;
use crate::keymap::{KeyBinding, compile_keymap};
use crate::screenshot::ScreenshotFormat;

pub(super) struct AppConfig {
    pub child_pause_supported: bool,
    pub diff_mode: DiffMode,
    pub history_limit: usize,
    pub checkpoint_interval: usize,
    pub compress: bool,
    pub logfile: Option<String>,
    pub replay_mode: bool,
    pub screenshot_dir: PathBuf,
    pub screenshot_format: ScreenshotFormat,
    pub snapshot_on: Option<String>,
    pub snapshot_on_regex: Option<Regex>,
    pub snapshot_on_change_cells: Option<usize>,
    pub snapshot_once: bool,
    pub aftercommand_runtime: Option<AfterCommandRuntime>,
    pub command_display: String,
    pub debug: bool,
    pub hide_header: bool,
    pub replay_indicator: bool,
    pub keymap: Vec<KeyBinding>,
    pub child_bindings: Vec<ChildBinding>,
}

impl AppConfig {
    pub(super) fn from_cli(cli: &Cli, child_pause_supported: bool) -> Result<Self> {
        let snapshot_on_regex = cli
            .snapshot_on_regex
            .as_ref()
            .map(|pattern| {
                Regex::new(pattern).with_context(|| format!("invalid snapshot regex: {pattern}"))
            })
            .transpose()?;
        let aftercommand_regex = cli
            .aftercommand_regex
            .as_ref()
            .map(|pattern| {
                Regex::new(pattern)
                    .with_context(|| format!("invalid aftercommand regex: {pattern}"))
            })
            .transpose()?;
        let command_display = App::command_display_from_cli(cli);
        let keymap = compile_keymap(&cli.keymap).context("invalid --keymap")?;
        let child_bindings = compile_child_bindings(&cli.bind).context("invalid --bind")?;

        Ok(Self {
            child_pause_supported,
            diff_mode: cli.differences.into(),
            history_limit: cli.limit,
            checkpoint_interval: cli.checkpoint_interval.max(1),
            compress: cli.compress,
            logfile: cli.replay.clone().or(cli.logfile.clone()),
            replay_mode: cli.replay.is_some(),
            screenshot_dir: PathBuf::from(&cli.screenshot_dir),
            screenshot_format: cli.screenshot_format.into(),
            snapshot_on: cli.snapshot_on.clone(),
            snapshot_on_regex,
            snapshot_on_change_cells: cli.snapshot_on_change_cells,
            snapshot_once: cli.snapshot_once,
            aftercommand_runtime: cli.aftercommand.as_ref().map(|hook| {
                AfterCommandRuntime::new(AfterCommandConfig {
                    hook: hook.clone(),
                    shell: cli.shell.clone(),
                    command_display: command_display.clone(),
                    regex: aftercommand_regex.clone(),
                    changed_cells: cli.aftercommand_change_cells,
                    every: cli.aftercommand_every,
                    debounce_ms: cli.aftercommand_debounce_ms,
                    timeout_ms: cli.aftercommand_timeout_ms,
                })
            }),
            command_display,
            debug: cli.debug,
            hide_header: cli.hide_header,
            replay_indicator: cli.replay_indicator,
            keymap,
            child_bindings,
        })
    }
}
