use std::io::{Read, Write};
use std::sync::atomic::AtomicU64;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, RwLock};
use std::thread;

use anyhow::{Context, Result};
use crossterm::event::{KeyEvent, MouseEvent};
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};

#[cfg(windows)]
use crate::cli::default_shell;
use crate::process_control;
use crate::runner::capture_hook::CaptureHook;
use crate::runner::encode::{key_to_bytes, mouse_to_bytes};
use crate::runner::{CaptureFrame, FrameSource, SourceEvent, time_label, unix_timestamp_millis};
use crate::screen::{Cell, ScreenSnapshot, Style, TermColor};

const SCROLLBACK_LINES: usize = 10_000;

pub struct PtyRunner {
    master: Box<dyn MasterPty + Send>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    child: Box<dyn Child + Send + Sync>,
    state: Arc<RwLock<TerminalState>>,
    stats: Arc<PtyStats>,
    dirty: Arc<AtomicBool>,
    capture_hook: Option<CaptureHook>,
    last_snapshot: Option<ScreenSnapshot>,
    last_size: (u16, u16),
    child_paused: bool,
    update_rx: Option<Receiver<SourceEvent>>,
}

struct TerminalState {
    parser: vt100::Parser,
}

struct PtyStats {
    bytes_read: AtomicU64,
    read_events: AtomicU64,
    dsr_responses: AtomicU64,
    first_bytes: Mutex<Vec<u8>>,
}

impl PtyStats {
    fn new() -> Self {
        Self {
            bytes_read: AtomicU64::new(0),
            read_events: AtomicU64::new(0),
            dsr_responses: AtomicU64::new(0),
            first_bytes: Mutex::new(Vec::new()),
        }
    }

    fn record_bytes(&self, bytes: &[u8]) {
        let mut first = self.first_bytes.lock().expect("pty stats poisoned");
        if first.len() >= 16 {
            return;
        }
        let remaining = 16usize.saturating_sub(first.len());
        first.extend(bytes.iter().take(remaining).copied());
    }

    fn preview(&self) -> String {
        let first = self.first_bytes.lock().expect("pty stats poisoned");
        if first.is_empty() {
            return "none".to_string();
        }
        first
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

impl PtyRunner {
    pub fn spawn(
        shell: &str,
        command: &[String],
        aftercommand: Option<String>,
        width: u16,
        height: u16,
    ) -> Result<Self> {
        let size = pty_size(width, height);
        let pty_system = native_pty_system();
        let pair = pty_system.openpty(size).context("failed to open PTY")?;
        let mut command_builder = build_command(shell, command)?;
        command_builder.env("TERM", child_term_env());
        let child = pair
            .slave
            .spawn_command(command_builder)
            .context("failed to spawn child in PTY")?;
        drop(pair.slave);

        let reader = pair
            .master
            .try_clone_reader()
            .context("failed to clone PTY reader")?;
        let writer = pair
            .master
            .take_writer()
            .context("failed to take PTY writer")?;
        let writer = Arc::new(Mutex::new(writer));

        let state = Arc::new(RwLock::new(TerminalState {
            parser: vt100::Parser::new(height.max(1), width.max(1), SCROLLBACK_LINES),
        }));
        let stats = Arc::new(PtyStats::new());
        let dirty = Arc::new(AtomicBool::new(true));
        let (update_tx, update_rx) = mpsc::sync_channel(2);
        start_reader_thread(
            state.clone(),
            stats.clone(),
            dirty.clone(),
            reader,
            writer.clone(),
            update_tx,
        );

        Ok(Self {
            master: pair.master,
            writer,
            child,
            state,
            stats,
            dirty,
            capture_hook: aftercommand
                .map(|hook| CaptureHook::new(shell, display_command(command), hook)),
            last_snapshot: None,
            last_size: (width.max(1), height.max(1)),
            child_paused: false,
            update_rx: Some(update_rx),
        })
    }

    fn snapshot(&self) -> ScreenSnapshot {
        self.snapshot_with_scrollback(None)
    }

    fn snapshot_with_scrollback(&self, scrollback_offset: Option<usize>) -> ScreenSnapshot {
        let mut state = self.state.write().expect("terminal state poisoned");
        let previous_scrollback = state.parser.screen().scrollback();
        if let Some(offset) = scrollback_offset {
            state.parser.screen_mut().set_scrollback(offset);
        }
        let screen = state.parser.screen();
        let (rows, cols) = screen.size();
        let mut snapshot = ScreenSnapshot::new(cols, rows);
        let (cursor_row, cursor_col) = screen.cursor_position();
        snapshot.set_cursor_state(cursor_col, cursor_row, !screen.hide_cursor());
        snapshot.set_screen_mode(screen.alternate_screen(), screen.scrollback());

        for row in 0..rows {
            for col in 0..cols {
                let Some(cell) = screen.cell(row, col) else {
                    continue;
                };
                if cell.is_wide_continuation() {
                    snapshot.set_cell(col, row, Cell::blank());
                    continue;
                }
                let symbol = if cell.has_contents() {
                    cell.contents()
                } else {
                    " "
                };
                snapshot.set_cell(
                    col,
                    row,
                    Cell {
                        symbol: symbol.to_string(),
                        style: Style {
                            fg: map_color(cell.fgcolor()),
                            bg: map_color(cell.bgcolor()),
                            bold: cell.bold(),
                            italic: cell.italic(),
                            underline: cell.underline(),
                            inverted: cell.inverse(),
                        },
                    },
                );
            }
        }

        if scrollback_offset.is_some() {
            state
                .parser
                .screen_mut()
                .set_scrollback(previous_scrollback);
        }

        snapshot
    }

    fn maybe_run_aftercommand(&self, output: &str, changed: bool) -> Result<()> {
        self.capture_hook
            .as_ref()
            .map_or(Ok(()), |hook| hook.maybe_run(output, changed))
    }
}

impl FrameSource for PtyRunner {
    fn capture(&mut self, width: u16, height: u16) -> Result<CaptureFrame> {
        self.resize(width, height)?;
        let dirty = self.dirty.swap(false, Ordering::Relaxed);
        let snapshot = if dirty || self.last_snapshot.is_none() {
            self.snapshot()
        } else {
            self.last_snapshot.clone().expect("snapshot must exist")
        };
        let raw_output = snapshot.lines().join("\n");
        let changed = self.last_snapshot.as_ref() != Some(&snapshot);
        self.last_snapshot = Some(snapshot.clone());
        self.maybe_run_aftercommand(&raw_output, changed)?;

        Ok(CaptureFrame {
            label: time_label(),
            timestamp_unix_ms: unix_timestamp_millis(),
            snapshot,
            raw_output,
            changed,
        })
    }

    fn view_snapshot(
        &mut self,
        width: u16,
        height: u16,
        scrollback_offset: usize,
    ) -> Result<Option<ScreenSnapshot>> {
        self.resize(width, height)?;
        Ok(Some(self.snapshot_with_scrollback(Some(scrollback_offset))))
    }

    fn resize(&mut self, width: u16, height: u16) -> Result<()> {
        let size = (width.max(1), height.max(1));
        if self.last_size == size {
            return Ok(());
        }
        self.master
            .resize(pty_size(size.0, size.1))
            .context("failed to resize PTY")?;
        let mut state = self.state.write().expect("terminal state poisoned");
        state.parser.screen_mut().set_size(size.1, size.0);
        self.last_size = size;
        self.dirty.store(true, Ordering::Relaxed);
        Ok(())
    }

    fn send_key(&mut self, key: KeyEvent) -> Result<()> {
        let bytes = {
            let state = self.state.read().expect("terminal state poisoned");
            key_to_bytes(key, state.parser.screen().application_cursor())
        };
        if bytes.is_empty() {
            return Ok(());
        }
        let mut writer = self.writer.lock().expect("writer poisoned");
        writer
            .write_all(&bytes)
            .context("failed to write key to PTY")?;
        writer.flush().ok();
        Ok(())
    }

    fn send_mouse(&mut self, event: MouseEvent, body_row_offset: u16) -> Result<bool> {
        let encoded = {
            let state = self.state.read().expect("terminal state poisoned");
            mouse_to_bytes(
                state.parser.screen().mouse_protocol_mode(),
                state.parser.screen().mouse_protocol_encoding(),
                event,
                body_row_offset,
            )
        };
        let Some(bytes) = encoded else {
            return Ok(false);
        };
        let mut writer = self.writer.lock().expect("writer poisoned");
        writer
            .write_all(&bytes)
            .context("failed to write mouse event to PTY")?;
        writer.flush().ok();
        Ok(true)
    }

    fn toggle_child_pause(&mut self) -> Result<Option<bool>> {
        let pid = self
            .child
            .process_id()
            .context("PTY child process id is unavailable")?;
        if self.child_paused {
            process_control::resume_process(pid).context("failed to resume child process")?;
        } else {
            process_control::suspend_process(pid).context("failed to pause child process")?;
        }
        self.child_paused = !self.child_paused;
        Ok(Some(self.child_paused))
    }

    fn supports_child_pause(&self) -> bool {
        true
    }

    fn debug_status(&self) -> Option<String> {
        Some(format!(
            "pty rx={} ev={} dsr={} first={}",
            self.stats.bytes_read.load(Ordering::Relaxed),
            self.stats.read_events.load(Ordering::Relaxed),
            self.stats.dsr_responses.load(Ordering::Relaxed),
            self.stats.preview(),
        ))
    }

    fn terminate(&mut self) -> Result<()> {
        match self.child.kill() {
            Ok(()) => Ok(()),
            Err(err) if is_child_exit_error(&err) => Ok(()),
            Err(err) => Err(err).context("failed to terminate child PTY process"),
        }
    }

    fn has_pending_update(&self) -> bool {
        self.dirty.load(Ordering::Relaxed)
    }

    fn is_event_driven(&self) -> bool {
        true
    }

    fn take_update_receiver(&mut self) -> Option<Receiver<SourceEvent>> {
        self.update_rx.take()
    }
}

fn start_reader_thread(
    state: Arc<RwLock<TerminalState>>,
    stats: Arc<PtyStats>,
    dirty: Arc<AtomicBool>,
    mut reader: Box<dyn Read + Send>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    update_tx: SyncSender<SourceEvent>,
) {
    thread::spawn(move || {
        let mut buffer = [0u8; 8192];
        let mut control_tail = Vec::new();
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(count) => {
                    stats.bytes_read.fetch_add(count as u64, Ordering::Relaxed);
                    stats.read_events.fetch_add(1, Ordering::Relaxed);
                    stats.record_bytes(&buffer[..count]);
                    handle_terminal_queries(
                        &state,
                        &stats,
                        &writer,
                        &mut control_tail,
                        &buffer[..count],
                    );
                    let mut state = state.write().expect("terminal state poisoned");
                    state.parser.process(&buffer[..count]);
                    dirty.store(true, Ordering::Relaxed);
                    match update_tx.try_send(SourceEvent::Updated) {
                        Ok(()) | Err(TrySendError::Full(_)) => {}
                        Err(TrySendError::Disconnected(_)) => break,
                    }
                }
                Err(err) if is_transient_pty_read_error(&err) => {
                    thread::sleep(std::time::Duration::from_millis(10));
                }
                Err(err) => {
                    let _ = update_tx.send(SourceEvent::ClosedWithError(err.to_string()));
                    return;
                }
            }
        }
        let _ = update_tx.send(SourceEvent::Closed);
    });
}

fn handle_terminal_queries(
    state: &Arc<RwLock<TerminalState>>,
    stats: &Arc<PtyStats>,
    writer: &Arc<Mutex<Box<dyn Write + Send>>>,
    control_tail: &mut Vec<u8>,
    bytes: &[u8],
) {
    let mut scan = Vec::with_capacity(control_tail.len() + bytes.len());
    scan.extend_from_slice(control_tail);
    scan.extend_from_slice(bytes);

    let mut index = 0usize;
    while index + 4 <= scan.len() {
        if scan[index..].starts_with(b"\x1b[6n") {
            let (row, col) = {
                let state = state.read().expect("terminal state poisoned");
                state.parser.screen().cursor_position()
            };
            let response = format!("\x1b[{};{}R", row.saturating_add(1), col.saturating_add(1));
            if let Ok(mut writer) = writer.lock() {
                if writer.write_all(response.as_bytes()).is_ok() {
                    let _ = writer.flush();
                    stats.dsr_responses.fetch_add(1, Ordering::Relaxed);
                }
            }
            index += 4;
        } else {
            index += 1;
        }
    }

    control_tail.clear();
    let keep = scan.len().min(3);
    control_tail.extend_from_slice(&scan[scan.len() - keep..]);
}

fn is_child_exit_error(err: &std::io::Error) -> bool {
    matches!(
        err.kind(),
        std::io::ErrorKind::NotFound
            | std::io::ErrorKind::BrokenPipe
            | std::io::ErrorKind::UnexpectedEof
            | std::io::ErrorKind::ConnectionAborted
            | std::io::ErrorKind::ConnectionReset
            | std::io::ErrorKind::NotConnected
    ) || matches!(err.raw_os_error(), Some(3) | Some(5))
}

fn is_transient_pty_read_error(err: &std::io::Error) -> bool {
    matches!(
        err.kind(),
        std::io::ErrorKind::Interrupted
            | std::io::ErrorKind::WouldBlock
            | std::io::ErrorKind::TimedOut
    )
}

fn build_command(shell: &str, command: &[String]) -> Result<CommandBuilder> {
    if command.is_empty() {
        anyhow::bail!("command cannot be empty");
    }

    if should_spawn_direct(shell) {
        let mut builder = CommandBuilder::new(&command[0]);
        builder.args(command.iter().skip(1));
        return Ok(builder);
    }

    let parts = shell_words::split(shell).context("failed to parse shell command")?;
    if parts.is_empty() {
        let mut builder = CommandBuilder::new(&command[0]);
        builder.args(command.iter().skip(1));
        return Ok(builder);
    }

    let command_text = display_command(command);
    let mut builder = CommandBuilder::new(&parts[0]);
    if shell.contains("{COMMAND}") {
        for arg in parts.iter().skip(1) {
            builder.arg(arg.replace("{COMMAND}", &command_text));
        }
    } else {
        for arg in parts.iter().skip(1) {
            builder.arg(arg);
        }
        builder.arg(command_text);
    }
    Ok(builder)
}

#[cfg(windows)]
fn should_spawn_direct(shell: &str) -> bool {
    shell_words::split(shell).ok() == shell_words::split(&default_shell()).ok()
}

#[cfg(not(windows))]
fn should_spawn_direct(_shell: &str) -> bool {
    false
}

fn display_command(command: &[String]) -> String {
    command.join(" ")
}

#[cfg(windows)]
fn child_term_env() -> &'static str {
    "xterm"
}

#[cfg(not(windows))]
fn child_term_env() -> &'static str {
    "xterm-256color"
}

fn map_color(color: vt100::Color) -> TermColor {
    match color {
        vt100::Color::Default => TermColor::Default,
        vt100::Color::Idx(index) => TermColor::Indexed(index),
        vt100::Color::Rgb(r, g, b) => TermColor::Rgb(r, g, b),
    }
}

fn pty_size(width: u16, height: u16) -> PtySize {
    PtySize {
        rows: height.max(1),
        cols: width.max(1),
        pixel_width: 0,
        pixel_height: 0,
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Result as IoResult, Write};
    use std::sync::atomic::Ordering;
    use std::sync::{Arc, Mutex, RwLock};

    use super::{
        PtyStats, TerminalState, build_command, child_term_env, handle_terminal_queries,
        is_transient_pty_read_error,
    };

    fn argv(builder: portable_pty::CommandBuilder) -> Vec<String> {
        builder
            .get_argv()
            .iter()
            .map(|value| value.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn build_command_uses_shell_on_unix_default() {
        #[cfg(not(windows))]
        assert_eq!(
            argv(build_command("sh -c", &["printf".into(), "hello".into()]).unwrap()),
            vec!["sh", "-c", "printf hello"]
        );
    }

    #[test]
    fn build_command_spawns_directly_for_windows_default_shell() {
        #[cfg(windows)]
        assert_eq!(
            argv(build_command("cmd /C", &["app.exe".into(), "hello world".into()]).unwrap()),
            vec!["app.exe", "hello world"]
        );
    }

    #[test]
    fn build_command_supports_placeholder_shell() {
        assert_eq!(
            argv(build_command("env WRAP={COMMAND}", &["echo".into(), "hi".into()]).unwrap()),
            vec!["env", "WRAP=echo hi"]
        );
    }

    #[test]
    fn child_term_env_matches_platform_expectation() {
        #[cfg(windows)]
        assert_eq!(child_term_env(), "xterm");

        #[cfg(not(windows))]
        assert_eq!(child_term_env(), "xterm-256color");
    }

    #[test]
    fn transient_pty_read_errors_are_retriable() {
        assert!(is_transient_pty_read_error(&std::io::Error::from(
            std::io::ErrorKind::Interrupted
        )));
        assert!(is_transient_pty_read_error(&std::io::Error::from(
            std::io::ErrorKind::WouldBlock
        )));
        assert!(!is_transient_pty_read_error(&std::io::Error::from(
            std::io::ErrorKind::BrokenPipe
        )));
    }

    #[test]
    fn pty_debug_status_reports_rx_counters() {
        let stats = PtyStats::new();
        stats.bytes_read.store(12, Ordering::Relaxed);
        stats.read_events.store(3, Ordering::Relaxed);
        stats.dsr_responses.store(1, Ordering::Relaxed);
        stats.record_bytes(&[0x1b, 0x5b, 0x3f, 0x31]);

        assert_eq!(
            format!(
                "pty rx={} ev={} dsr={} first={}",
                stats.bytes_read.load(Ordering::Relaxed),
                stats.read_events.load(Ordering::Relaxed),
                stats.dsr_responses.load(Ordering::Relaxed),
                stats.preview(),
            ),
            "pty rx=12 ev=3 dsr=1 first=1b 5b 3f 31"
        );
    }

    struct TestWriter {
        bytes: Arc<Mutex<Vec<u8>>>,
    }

    impl Write for TestWriter {
        fn write(&mut self, buf: &[u8]) -> IoResult<usize> {
            self.bytes.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> IoResult<()> {
            Ok(())
        }
    }

    #[test]
    fn responds_to_dsr_cursor_position_query() {
        let state = Arc::new(RwLock::new(TerminalState {
            parser: vt100::Parser::new(10, 20, 0),
        }));
        let stats = Arc::new(PtyStats::new());
        let captured = Arc::new(Mutex::new(Vec::new()));
        let writer = Arc::new(Mutex::new(Box::new(TestWriter {
            bytes: captured.clone(),
        }) as Box<dyn Write + Send>));
        let mut control_tail = Vec::new();

        handle_terminal_queries(&state, &stats, &writer, &mut control_tail, b"\x1b[6n");

        assert_eq!(*captured.lock().unwrap(), b"\x1b[1;1R");
        assert_eq!(stats.dsr_responses.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn vt100_scrollback_supports_offsets_beyond_visible_rows() {
        let mut parser = vt100::Parser::new(3, 8, 32);
        parser.process(b"1\n2\n3\n4\n5\n6\n7\n8\n");

        let visible_now: Vec<String> = parser.screen().rows(0, 8).collect();
        assert_eq!(visible_now.len(), 3);

        parser.screen_mut().set_scrollback(6);
        let older_rows: Vec<String> = parser.screen().rows(0, 8).collect();

        assert_eq!(parser.screen().scrollback(), 6);
        assert_eq!(older_rows.len(), 3);
        assert_ne!(older_rows, visible_now);
        assert!(older_rows.iter().any(|row| row.contains('2')));
    }
}
