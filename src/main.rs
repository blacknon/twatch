// Copyright (c) 2026 Blacknon. All rights reserved.
// Use of this source code is governed by an MIT license
// that can be found in the LICENSE file.

use std::borrow::Cow;
use std::fs;
use std::io::Write;
use std::panic::{self, PanicHookInfo};
use std::path::PathBuf;
use std::sync::Once;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use clap::Parser;
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::execute;
use regex::Regex;
use twatch::aftercommand::{AfterCommandConfig, AfterCommandEvent, AfterCommandRuntime};
use twatch::app::App;
use twatch::batch;
use twatch::cli::Cli;
use twatch::runner::{DemoRunner, FrameSource, PtyRunner, ReplayRunner};
use twatch::screen::ScreenSnapshot;

fn main() -> Result<()> {
    let cli = Cli::parse();
    if cli.batch {
        return run_batch(cli);
    }

    install_panic_hook();
    run_interactive(cli)
}

fn run_interactive(cli: Cli) -> Result<()> {
    let (width, height) = crossterm::terminal::size().unwrap_or((120, 40));
    let source = build_source(
        &cli,
        SourceBuildMode::Interactive,
        width,
        height.saturating_sub(2),
    )?;
    let app = App::new(&cli, source)?;

    execute!(std::io::stdout(), EnableMouseCapture)?;
    let _terminal_guard = TerminalRestoreGuard;
    let terminal = ratatui::init();
    app.run(terminal)
}

fn run_batch(cli: Cli) -> Result<()> {
    let (width, height) = batch::terminal_size(&cli);
    let mut source = build_source(&cli, SourceBuildMode::Batch, width, height)?;
    let mut aftercommand_runtime = build_batch_aftercommand_runtime(&cli)?;

    let interval = Duration::from_secs_f64(cli.interval.max(0.2));
    let mut stdout = std::io::stdout();
    let mut emitted = 0usize;
    let mut previous = None;
    let mut previous_full_snapshot: Option<ScreenSnapshot> = None;

    loop {
        if cli.batch_count.is_some_and(|count| emitted >= count) {
            break;
        }

        std::thread::sleep(interval);
        let frame = source.capture(width, height)?;
        if let Some(runtime) = &mut aftercommand_runtime {
            let changed_cell_count = frame
                .snapshot
                .changed_cell_count_since(previous_full_snapshot.as_ref());
            let _ = runtime.evaluate_and_enqueue(AfterCommandEvent {
                changed: frame.changed,
                output: frame.raw_output.clone(),
                timestamp_unix_ms: frame.timestamp_unix_ms,
                frame_seq: emitted as u64 + 1,
                width: frame.snapshot.width(),
                height: frame.snapshot.height(),
                changed_cell_count,
                last_input_summary: String::new(),
            })?;
        }
        let snapshot = batch::prepare_snapshot(&frame.snapshot, cli.batch_crop);
        let rendered = batch::render_output(&cli, &frame.label, &snapshot, previous.as_ref());
        stdout.write_all(rendered.as_bytes())?;
        stdout.flush()?;
        previous_full_snapshot = Some(frame.snapshot.clone());
        previous = Some(snapshot);
        emitted += 1;
    }

    source.terminate().ok();
    Ok(())
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum SourceBuildMode {
    Interactive,
    Batch,
}

fn build_source(
    cli: &Cli,
    mode: SourceBuildMode,
    width: u16,
    height: u16,
) -> Result<Box<dyn FrameSource>> {
    if mode == SourceBuildMode::Batch && cli.replay.is_some() {
        anyhow::bail!("--replay is not supported with --batch");
    }

    if let Some(path) = &cli.replay {
        return Ok(Box::new(ReplayRunner::from_log(path)?));
    }

    if cli.command.is_empty() {
        return Ok(Box::new(DemoRunner::new()));
    }

    let aftercommand = match mode {
        SourceBuildMode::Interactive => None,
        SourceBuildMode::Batch => None,
    };

    Ok(Box::new(PtyRunner::spawn(
        &cli.shell,
        &cli.command,
        aftercommand,
        width,
        height,
    )?))
}

fn build_batch_aftercommand_runtime(cli: &Cli) -> Result<Option<AfterCommandRuntime>> {
    let Some(hook) = &cli.aftercommand else {
        return Ok(None);
    };

    let regex = cli
        .aftercommand_regex
        .as_ref()
        .map(|pattern| {
            Regex::new(pattern).with_context(|| format!("invalid aftercommand regex: {pattern}"))
        })
        .transpose()?;

    let command_display = if cli.command.is_empty() {
        "demo".to_string()
    } else {
        cli.command.join(" ")
    };

    Ok(Some(AfterCommandRuntime::new(AfterCommandConfig {
        hook: hook.clone(),
        shell: cli.shell.clone(),
        command_display,
        regex,
        changed_cells: cli.aftercommand_change_cells,
        every: cli.aftercommand_every,
        debounce_ms: cli.aftercommand_debounce_ms,
        timeout_ms: cli.aftercommand_timeout_ms,
    })))
}

struct TerminalRestoreGuard;

impl Drop for TerminalRestoreGuard {
    fn drop(&mut self) {
        restore_terminal_state();
    }
}

fn restore_terminal_state() {
    ratatui::restore();
    let _ = execute!(std::io::stdout(), DisableMouseCapture);
}

fn install_panic_hook() {
    static PANIC_HOOK: Once = Once::new();

    PANIC_HOOK.call_once(|| {
        let previous = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            restore_terminal_state();
            if let Ok(path) = write_panic_report(info) {
                eprintln!("twatch panic report written to {}", path.display());
            }
            previous(info);
        }));
    });
}

fn write_panic_report(info: &PanicHookInfo<'_>) -> std::io::Result<PathBuf> {
    let path = panic_report_path();
    fs::write(&path, render_panic_report(info))?;
    Ok(path)
}

fn panic_report_path() -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    std::env::temp_dir().join(format!("twatch-panic-{timestamp}.log"))
}

fn render_panic_report(info: &PanicHookInfo<'_>) -> String {
    let thread = std::thread::current();
    let thread_name = thread.name().unwrap_or("unnamed");
    let payload = panic_payload(info);
    let location = info
        .location()
        .map(|location| {
            format!(
                "{}:{}:{}",
                location.file(),
                location.line(),
                location.column()
            )
        })
        .unwrap_or_else(|| "unknown".to_string());

    format!(
        "twatch panic report\nthread: {thread_name}\nlocation: {location}\npayload: {payload}\n"
    )
}

fn panic_payload<'a>(info: &'a PanicHookInfo<'a>) -> Cow<'a, str> {
    if let Some(payload) = info.payload().downcast_ref::<&str>() {
        Cow::Borrowed(payload)
    } else if let Some(payload) = info.payload().downcast_ref::<String>() {
        Cow::Borrowed(payload.as_str())
    } else {
        Cow::Borrowed("non-string panic payload")
    }
}

#[cfg(test)]
mod tests {
    use super::panic_report_path;

    #[test]
    fn panic_report_path_uses_temp_dir() {
        let path = panic_report_path();
        assert!(path.starts_with(std::env::temp_dir()));
        assert!(
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("twatch-panic-"))
        );
    }
}
