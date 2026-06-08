// Copyright (c) 2026 Blacknon. All rights reserved.
// Use of this source code is governed by an MIT license
// that can be found in the LICENSE file.

use std::borrow::Cow;
use std::fs;
use std::io::Write;
use std::panic::{self, PanicHookInfo};
use std::path::PathBuf;
use std::sync::Once;
use std::sync::mpsc::RecvTimeoutError;
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
use twatch::logging::{
    LogRecord, active_spill_path, append_delta_record, append_recent_record, compact_active_log,
    pack_log_as_compact_archive,
};
use twatch::runner::{FrameSource, PipeRunner, PtyRunner, ReplayRunner, SourceEvent};
use twatch::screen::ScreenSnapshot;

const BATCH_POLL_INTERVAL: Duration = Duration::from_millis(500);
const RECORD_STDIN_SETTLE_DELAY: Duration = Duration::from_millis(16);
fn main() -> Result<()> {
    let cli = Cli::parse();
    if cli.pack_logfile.is_some() || cli.pack_output.is_some() {
        return run_pack_log(cli);
    }
    if cli.record_stdin {
        return run_record_stdin(cli);
    }
    if cli.batch {
        return run_batch(cli);
    }

    install_panic_hook();
    run_interactive(cli)
}

fn run_pack_log(cli: Cli) -> Result<()> {
    let Some(input_path) = cli.pack_logfile.as_deref() else {
        anyhow::bail!("--pack-logfile requires an input path");
    };
    let Some(output_path) = cli.pack_output.as_deref() else {
        anyhow::bail!("--pack-output requires an output path");
    };
    pack_log_as_compact_archive(input_path, output_path)
}

fn run_interactive(cli: Cli) -> Result<()> {
    let (width, height) = crossterm::terminal::size().unwrap_or((120, 40));
    let header_rows = if cli.hide_header { 0 } else { 2 };
    let source = build_source(
        &cli,
        SourceBuildMode::Interactive,
        width,
        height.saturating_sub(header_rows),
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
    let is_event_driven = source.is_event_driven();
    let update_rx = source.take_update_receiver();
    let mut stdout = std::io::stdout();
    let mut emitted = 0usize;
    let mut previous = None;
    let mut previous_full_snapshot: Option<ScreenSnapshot> = None;

    loop {
        if cli.batch_count.is_some_and(|count| emitted >= count) {
            break;
        }

        if is_event_driven {
            match wait_for_batch_source_update(update_rx.as_ref(), &*source) {
                BatchLoopStep::Capture => {}
                BatchLoopStep::Break => break,
            }
        } else {
            std::thread::sleep(BATCH_POLL_INTERVAL);
        }

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

fn run_record_stdin(cli: Cli) -> Result<()> {
    if cli.batch {
        anyhow::bail!("--record-stdin cannot be combined with --batch");
    }
    if cli.replay.is_some() {
        anyhow::bail!("--record-stdin cannot be combined with --replay");
    }
    if !cli.command.is_empty() {
        anyhow::bail!("--record-stdin does not accept a child command");
    }
    let Some(path) = &cli.logfile else {
        anyhow::bail!("--record-stdin requires --logfile");
    };

    let (width, height) = stdin_record_size(&cli);
    let mut source = PipeRunner::from_stdin(width, height);
    let update_rx = source.take_update_receiver();
    let mut previous_snapshot: Option<ScreenSnapshot> = None;
    let mut next_frame_seq = 1u64;
    let spill_every = cli.record_stdin_spill_every;
    let spill_retain = cli.record_stdin_spill_retain.max(1);
    let checkpoint_interval = cli.checkpoint_interval.max(1) as u64;
    let recent_retain = spill_retain.max(16);

    loop {
        match wait_for_record_stdin_update(update_rx.as_ref(), &source) {
            BatchLoopStep::Capture => {}
            BatchLoopStep::Break => break,
        }

        let frame = source.capture(width, height)?;
        if !frame.changed && previous_snapshot.is_some() {
            continue;
        }

        let changed_cell_count = frame
            .snapshot
            .changed_cell_count_since(previous_snapshot.as_ref());
        let record = LogRecord {
            label: frame.label.clone(),
            changed: frame.changed,
            timestamp_unix_ms: frame.timestamp_unix_ms,
            frame_seq: next_frame_seq,
            width: frame.snapshot.width(),
            height: frame.snapshot.height(),
            changed_cell_count,
            input_event_count_since_prev: 0,
            resized: false,
            resize_from_width: 0,
            resize_from_height: 0,
            resize_to_width: 0,
            resize_to_height: 0,
            resize_source: "stdin".to_string(),
            snapshot: frame.snapshot.clone(),
        };

        append_delta_record(
            path,
            &record,
            previous_snapshot.as_ref(),
            checkpoint_interval,
        )?;
        append_recent_record(path, &record, recent_retain)?;
        previous_snapshot = Some(frame.snapshot);
        if spill_every > 0
            && active_spill_path(path).is_some()
            && next_frame_seq % spill_every as u64 == 0
        {
            let _ = compact_active_log(path, spill_retain);
        }
        next_frame_seq += 1;
    }

    source.terminate().ok();
    Ok(())
}

enum BatchLoopStep {
    Capture,
    Break,
}

fn wait_for_batch_source_update(
    update_rx: Option<&std::sync::mpsc::Receiver<SourceEvent>>,
    source: &dyn FrameSource,
) -> BatchLoopStep {
    let Some(update_rx) = update_rx else {
        return BatchLoopStep::Capture;
    };

    loop {
        match update_rx.recv() {
            Ok(SourceEvent::Updated) => return BatchLoopStep::Capture,
            Ok(SourceEvent::Closed) | Err(_) => {
                return if source.has_pending_update() {
                    BatchLoopStep::Capture
                } else {
                    BatchLoopStep::Break
                };
            }
            Ok(SourceEvent::ClosedWithError(_)) => {
                return if source.has_pending_update() {
                    BatchLoopStep::Capture
                } else {
                    BatchLoopStep::Break
                };
            }
        }
    }
}

fn wait_for_record_stdin_update(
    update_rx: Option<&std::sync::mpsc::Receiver<SourceEvent>>,
    source: &dyn FrameSource,
) -> BatchLoopStep {
    let Some(update_rx) = update_rx else {
        return BatchLoopStep::Capture;
    };

    loop {
        match update_rx.recv() {
            Ok(SourceEvent::Updated) => {
                let mut stream_closed = false;
                loop {
                    match update_rx.recv_timeout(RECORD_STDIN_SETTLE_DELAY) {
                        Ok(SourceEvent::Updated) => continue,
                        Ok(SourceEvent::Closed) | Err(RecvTimeoutError::Disconnected) => {
                            stream_closed = true;
                            break;
                        }
                        Ok(SourceEvent::ClosedWithError(_)) => {
                            stream_closed = true;
                            break;
                        }
                        Err(RecvTimeoutError::Timeout) => break,
                    }
                }

                return if stream_closed && !source.has_pending_update() {
                    BatchLoopStep::Break
                } else {
                    BatchLoopStep::Capture
                };
            }
            Ok(SourceEvent::Closed) | Err(_) => {
                return if source.has_pending_update() {
                    BatchLoopStep::Capture
                } else {
                    BatchLoopStep::Break
                };
            }
            Ok(SourceEvent::ClosedWithError(_)) => {
                return if source.has_pending_update() {
                    BatchLoopStep::Capture
                } else {
                    BatchLoopStep::Break
                };
            }
        }
    }
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
        anyhow::bail!("command is required unless --replay is used");
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

    let command_display = cli.command.join(" ");

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

fn stdin_record_size(cli: &Cli) -> (u16, u16) {
    cli.size
        .or(cli.batch_size)
        .map(|size| (size.width, size.height))
        .or_else(|| crossterm::terminal::size().ok())
        .unwrap_or((120, 40))
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
    use super::{SourceBuildMode, build_source, panic_report_path, stdin_record_size};
    use twatch::cli::{Cli, DiffModeArg, ScreenshotFormatArg, SizeSpec, default_shell};

    #[test]
    fn panic_report_path_uses_temp_dir() {
        let path = panic_report_path();
        assert!(path.starts_with(std::env::temp_dir()));
        assert!(
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("twatch-panic-"))
        );
    }

    #[test]
    fn stdin_record_size_prefers_explicit_size() {
        let cli = Cli {
            batch: false,
            batch_count: None,
            batch_size: None,
            batch_crop: None,
            batch_diff_only: false,
            batch_no_color: false,
            aftercommand: None,
            keymap: Vec::new(),
            bind: Vec::new(),
            aftercommand_regex: None,
            aftercommand_change_cells: None,
            aftercommand_every: None,
            aftercommand_debounce_ms: None,
            aftercommand_timeout_ms: 3000,
            compress: false,
            logfile: None,
            replay: None,
            pack_logfile: None,
            pack_output: None,
            record_stdin: true,
            record_stdin_spill_every: 64,
            record_stdin_spill_retain: 32,
            size: Some(SizeSpec {
                width: 90,
                height: 30,
            }),
            screenshot_dir: "/tmp".to_string(),
            screenshot_format: ScreenshotFormatArg::Text,
            snapshot_on: None,
            snapshot_on_regex: None,
            snapshot_on_change_cells: None,
            snapshot_once: false,
            shell: default_shell(),
            differences: DiffModeArg::None,
            limit: 500,
            checkpoint_interval: 12,
            debug: false,
            hide_header: false,
            replay_indicator: true,
            command: Vec::new(),
        };

        assert_eq!(stdin_record_size(&cli), (90, 30));
    }

    #[test]
    fn build_source_rejects_empty_command_without_replay() {
        let cli = Cli {
            batch: false,
            batch_count: None,
            batch_size: None,
            batch_crop: None,
            batch_diff_only: false,
            batch_no_color: false,
            aftercommand: None,
            keymap: Vec::new(),
            bind: Vec::new(),
            aftercommand_regex: None,
            aftercommand_change_cells: None,
            aftercommand_every: None,
            aftercommand_debounce_ms: None,
            aftercommand_timeout_ms: 3000,
            compress: false,
            logfile: None,
            replay: None,
            pack_logfile: None,
            pack_output: None,
            record_stdin: false,
            record_stdin_spill_every: 64,
            record_stdin_spill_retain: 32,
            size: None,
            screenshot_dir: "/tmp".to_string(),
            screenshot_format: ScreenshotFormatArg::Text,
            snapshot_on: None,
            snapshot_on_regex: None,
            snapshot_on_change_cells: None,
            snapshot_once: false,
            shell: default_shell(),
            differences: DiffModeArg::None,
            limit: 500,
            checkpoint_interval: 12,
            debug: false,
            hide_header: false,
            replay_indicator: true,
            command: Vec::new(),
        };

        let err = build_source(&cli, SourceBuildMode::Interactive, 80, 24)
            .err()
            .expect("empty command should be rejected");
        assert_eq!(
            err.to_string(),
            "command is required unless --replay is used"
        );
    }
}
