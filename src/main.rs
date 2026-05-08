use std::io::Write;
use std::time::Duration;

use anyhow::Result;
use clap::Parser;
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::execute;
use twatch::app::App;
use twatch::batch;
use twatch::cli::Cli;
use twatch::runner::{DemoRunner, FrameSource, PtyRunner, ReplayRunner};

fn main() -> Result<()> {
    let cli = Cli::parse();
    if cli.batch {
        return run_batch(cli);
    }

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

    execute!(std::io::stdout(), EnableMouseCapture)?;
    let terminal = ratatui::init();
    let result = App::new(&cli, source)?.run(terminal);
    ratatui::restore();
    execute!(std::io::stdout(), DisableMouseCapture)?;
    result
}

fn run_batch(cli: Cli) -> Result<()> {
    let (width, height) = batch::terminal_size(&cli);
    let mut source = build_source(&cli, SourceBuildMode::Batch, width, height)?;

    let interval = Duration::from_secs_f64(cli.interval.max(0.2));
    let mut stdout = std::io::stdout();
    let mut emitted = 0usize;
    let mut previous = None;

    loop {
        if cli.batch_count.is_some_and(|count| emitted >= count) {
            break;
        }

        std::thread::sleep(interval);
        let frame = source.capture(width, height)?;
        let snapshot = batch::prepare_snapshot(&frame.snapshot, cli.batch_crop);
        let rendered = batch::render_output(&cli, &frame.label, &snapshot, previous.as_ref());
        stdout.write_all(rendered.as_bytes())?;
        stdout.flush()?;
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
        SourceBuildMode::Batch => cli.aftercommand.clone(),
    };

    Ok(Box::new(PtyRunner::spawn(
        &cli.shell,
        cli.command.join(" "),
        aftercommand,
        width,
        height,
    )?))
}
