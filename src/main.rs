use anyhow::Result;
use clap::Parser;
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::execute;
use twatch::app::App;
use twatch::cli::Cli;
use twatch::runner::{DemoRunner, FrameSource, PtyRunner};

fn main() -> Result<()> {
    let cli = Cli::parse();
    if cli.batch {
        return run_batch(cli);
    }

    let (width, height) = crossterm::terminal::size().unwrap_or((120, 40));
    let source: Box<dyn FrameSource> = if cli.demo || cli.command.is_empty() {
        Box::new(DemoRunner::new())
    } else {
        Box::new(PtyRunner::spawn(
            &cli.shell,
            cli.command.join(" "),
            cli.aftercommand.clone(),
            width,
            height.saturating_sub(2),
        )?)
    };

    let mut stdout = std::io::stdout();
    execute!(stdout, EnableMouseCapture)?;
    let terminal = ratatui::init();
    let result = App::new(&cli, source)?.run(terminal);
    ratatui::restore();
    execute!(std::io::stdout(), DisableMouseCapture)?;
    result
}

fn run_batch(cli: Cli) -> Result<()> {
    let (width, height) = crossterm::terminal::size().unwrap_or((120, 40));
    let mut source: Box<dyn FrameSource> = if cli.demo || cli.command.is_empty() {
        Box::new(DemoRunner::new())
    } else {
        Box::new(PtyRunner::spawn(
            &cli.shell,
            cli.command.join(" "),
            cli.aftercommand,
            width,
            height.saturating_sub(2),
        )?)
    };

    std::thread::sleep(std::time::Duration::from_secs_f64(cli.interval.max(0.2)));
    let frame = source.capture(width, height.saturating_sub(2))?;
    print!(
        "{}",
        frame
            .snapshot
            .batch_render(&["twatch | batch mode", "search: <disabled> | focus: watch",])
    );
    Ok(())
}
