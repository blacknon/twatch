use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use serde::Serialize;
use time::{OffsetDateTime, format_description::FormatItem, macros::format_description};

use crate::screen::{Cell, ScreenSnapshot, Style, TermColor};

const TIME_FORMAT: &[FormatItem<'static>] =
    format_description!("[year]-[month]-[day] [hour]:[minute]:[second].[subsecond digits:3]");
const SCROLLBACK_LINES: usize = 10_000;

#[derive(Clone, Debug)]
pub struct CaptureFrame {
    pub label: String,
    pub snapshot: ScreenSnapshot,
    pub raw_output: String,
    pub changed: bool,
}

pub trait FrameSource {
    fn capture(&mut self, width: u16, height: u16) -> Result<CaptureFrame>;
    fn resize(&mut self, width: u16, height: u16) -> Result<()>;
    fn send_key(&mut self, key: KeyEvent) -> Result<()>;
    fn send_mouse(&mut self, event: MouseEvent, body_row_offset: u16) -> Result<()>;
    fn has_pending_update(&self) -> bool;
    fn is_event_driven(&self) -> bool;
    fn terminate(&mut self) -> Result<()>;
}

pub struct PtyRunner {
    master: Box<dyn MasterPty + Send>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    child: Box<dyn Child + Send + Sync>,
    state: Arc<RwLock<TerminalState>>,
    dirty: Arc<AtomicBool>,
    shell_program: String,
    shell_args: Vec<String>,
    command: String,
    aftercommand: Option<String>,
    last_snapshot: Option<ScreenSnapshot>,
    last_size: (u16, u16),
}

pub struct DemoRunner {
    sequence: u64,
    last_snapshot: Option<ScreenSnapshot>,
}

struct TerminalState {
    parser: vt100::Parser,
}

impl PtyRunner {
    pub fn spawn(
        shell: &str,
        command: String,
        aftercommand: Option<String>,
        width: u16,
        height: u16,
    ) -> Result<Self> {
        let size = pty_size(width, height);
        let pty_system = native_pty_system();
        let pair = pty_system.openpty(size).context("failed to open PTY")?;
        let mut command_builder = build_command(shell, &command)?;
        command_builder.env("TERM", "xterm-256color");
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

        let state = Arc::new(RwLock::new(TerminalState {
            parser: vt100::Parser::new(height.max(1), width.max(1), SCROLLBACK_LINES),
        }));
        let dirty = Arc::new(AtomicBool::new(true));
        start_reader_thread(state.clone(), dirty.clone(), reader);

        let (shell_program, shell_args) = parse_shell(shell);

        Ok(Self {
            master: pair.master,
            writer: Arc::new(Mutex::new(writer)),
            child,
            state,
            dirty,
            shell_program,
            shell_args,
            command,
            aftercommand,
            last_snapshot: None,
            last_size: (width.max(1), height.max(1)),
        })
    }

    fn snapshot(&self) -> ScreenSnapshot {
        let state = self.state.read().expect("terminal state poisoned");
        let screen = state.parser.screen();
        let (rows, cols) = screen.size();
        let mut snapshot = ScreenSnapshot::new(cols, rows);

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
                    " ".to_string()
                };
                snapshot.set_cell(
                    col,
                    row,
                    Cell {
                        symbol,
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

        snapshot
    }

    fn maybe_run_aftercommand(&self, output: &str, changed: bool) -> Result<()> {
        if !changed {
            return Ok(());
        }

        let Some(aftercommand) = &self.aftercommand else {
            return Ok(());
        };

        let payload = AfterCommandPayload {
            command: self.command.clone(),
            changed,
            output: output.to_string(),
            unix_timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or(Duration::ZERO)
                .as_secs(),
        };

        let mut cmd = std::process::Command::new(&self.shell_program);
        cmd.args(&self.shell_args)
            .arg(aftercommand)
            .env(
                "TWATCH_DATA",
                serde_json::to_string(&payload)
                    .context("failed to serialize aftercommand payload")?,
            )
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        let _ = cmd.status();
        Ok(())
    }
}

impl FrameSource for PtyRunner {
    fn capture(&mut self, width: u16, height: u16) -> Result<CaptureFrame> {
        self.resize(width, height)?;
        let dirty = self.dirty.swap(false, Ordering::Relaxed);
        let snapshot = if dirty || self.last_snapshot.is_none() {
            let snapshot = self.snapshot();
            self.last_snapshot = Some(snapshot.clone());
            snapshot
        } else {
            self.last_snapshot.clone().expect("snapshot must exist")
        };
        let raw_output = snapshot.lines().join("\n");
        let changed = dirty;
        self.maybe_run_aftercommand(&raw_output, changed)?;

        Ok(CaptureFrame {
            label: time_label(),
            snapshot,
            raw_output,
            changed,
        })
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
        state.parser.set_size(size.1, size.0);
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

    fn send_mouse(&mut self, event: MouseEvent, body_row_offset: u16) -> Result<()> {
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
            return Ok(());
        };
        let mut writer = self.writer.lock().expect("writer poisoned");
        writer
            .write_all(bytes.as_bytes())
            .context("failed to write mouse event to PTY")?;
        writer.flush().ok();
        Ok(())
    }

    fn terminate(&mut self) -> Result<()> {
        self.child
            .kill()
            .context("failed to terminate child PTY process")
    }

    fn has_pending_update(&self) -> bool {
        self.dirty.load(Ordering::Relaxed)
    }

    fn is_event_driven(&self) -> bool {
        true
    }
}

impl DemoRunner {
    pub fn new() -> Self {
        Self {
            sequence: 0,
            last_snapshot: None,
        }
    }
}

impl FrameSource for DemoRunner {
    fn capture(&mut self, width: u16, height: u16) -> Result<CaptureFrame> {
        self.sequence += 1;
        let phase = (self.sequence % 4) as usize;
        let states = [
            ("starting", "booting workers", "queue=12 inflight=1"),
            ("running", "workers healthy", "queue=8 inflight=3"),
            ("degraded", "worker-2 retrying", "queue=14 inflight=2"),
            ("recovered", "all workers healthy", "queue=7 inflight=4"),
        ];
        let (status, details, counters) = states[phase];

        let snapshot = ScreenSnapshot::from_text_lines(
            width,
            height,
            &[
                "service: demo-api",
                &format!("status: {status}"),
                details,
                counters,
                &format!("last tick: {:04}", self.sequence),
            ],
        );
        let raw_output = snapshot.lines().join("\n");
        let changed = self.last_snapshot.as_ref() != Some(&snapshot);
        self.last_snapshot = Some(snapshot.clone());

        Ok(CaptureFrame {
            label: time_label(),
            snapshot,
            raw_output,
            changed,
        })
    }

    fn resize(&mut self, _width: u16, _height: u16) -> Result<()> {
        Ok(())
    }

    fn send_key(&mut self, _key: KeyEvent) -> Result<()> {
        Ok(())
    }

    fn send_mouse(&mut self, _event: MouseEvent, _body_row_offset: u16) -> Result<()> {
        Ok(())
    }

    fn has_pending_update(&self) -> bool {
        false
    }

    fn is_event_driven(&self) -> bool {
        false
    }

    fn terminate(&mut self) -> Result<()> {
        Ok(())
    }
}

#[derive(Serialize)]
struct AfterCommandPayload {
    command: String,
    changed: bool,
    output: String,
    unix_timestamp: u64,
}

fn start_reader_thread(
    state: Arc<RwLock<TerminalState>>,
    dirty: Arc<AtomicBool>,
    mut reader: Box<dyn Read + Send>,
) {
    thread::spawn(move || {
        let mut buffer = [0u8; 8192];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(count) => {
                    let mut state = state.write().expect("terminal state poisoned");
                    state.parser.process(&buffer[..count]);
                    dirty.store(true, Ordering::Relaxed);
                }
                Err(_) => break,
            }
        }
    });
}

fn build_command(shell: &str, command: &str) -> Result<CommandBuilder> {
    let parts = shell_words::split(shell).context("failed to parse shell command")?;
    if parts.is_empty() {
        return Ok(CommandBuilder::new(command));
    }

    let mut builder = CommandBuilder::new(&parts[0]);
    if shell.contains("{COMMAND}") {
        for arg in parts.iter().skip(1) {
            builder.arg(arg.replace("{COMMAND}", command));
        }
    } else {
        for arg in parts.iter().skip(1) {
            builder.arg(arg);
        }
        builder.arg(command);
    }
    Ok(builder)
}

fn parse_shell(shell: &str) -> (String, Vec<String>) {
    match shell_words::split(shell) {
        Ok(parts) if !parts.is_empty() => {
            (parts[0].clone(), parts.iter().skip(1).cloned().collect())
        }
        _ => ("sh".to_string(), vec!["-c".to_string()]),
    }
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

fn key_to_bytes(key: KeyEvent, application_cursor: bool) -> Vec<u8> {
    let mut bytes = Vec::new();

    if key.modifiers.contains(KeyModifiers::ALT)
        && !matches!(
            key.code,
            KeyCode::Left | KeyCode::Right | KeyCode::Up | KeyCode::Down
        )
    {
        bytes.push(0x1b);
    }

    match key.code {
        KeyCode::Backspace => bytes.push(0x7f),
        KeyCode::Enter => bytes.push(b'\r'),
        KeyCode::Left => bytes.extend_from_slice(if application_cursor {
            b"\x1bOD"
        } else {
            b"\x1b[D"
        }),
        KeyCode::Right => bytes.extend_from_slice(if application_cursor {
            b"\x1bOC"
        } else {
            b"\x1b[C"
        }),
        KeyCode::Up => bytes.extend_from_slice(if application_cursor {
            b"\x1bOA"
        } else {
            b"\x1b[A"
        }),
        KeyCode::Down => bytes.extend_from_slice(if application_cursor {
            b"\x1bOB"
        } else {
            b"\x1b[B"
        }),
        KeyCode::Home => bytes.extend_from_slice(b"\x1b[H"),
        KeyCode::End => bytes.extend_from_slice(b"\x1b[F"),
        KeyCode::PageUp => bytes.extend_from_slice(b"\x1b[5~"),
        KeyCode::PageDown => bytes.extend_from_slice(b"\x1b[6~"),
        KeyCode::Delete => bytes.extend_from_slice(b"\x1b[3~"),
        KeyCode::Insert => bytes.extend_from_slice(b"\x1b[2~"),
        KeyCode::F(1) => bytes.extend_from_slice(b"\x1bOP"),
        KeyCode::F(2) => bytes.extend_from_slice(b"\x1bOQ"),
        KeyCode::F(3) => bytes.extend_from_slice(b"\x1bOR"),
        KeyCode::F(4) => bytes.extend_from_slice(b"\x1bOS"),
        KeyCode::F(5) => bytes.extend_from_slice(b"\x1b[15~"),
        KeyCode::F(6) => bytes.extend_from_slice(b"\x1b[17~"),
        KeyCode::F(7) => bytes.extend_from_slice(b"\x1b[18~"),
        KeyCode::F(8) => bytes.extend_from_slice(b"\x1b[19~"),
        KeyCode::F(9) => bytes.extend_from_slice(b"\x1b[20~"),
        KeyCode::F(10) => bytes.extend_from_slice(b"\x1b[21~"),
        KeyCode::F(11) => bytes.extend_from_slice(b"\x1b[23~"),
        KeyCode::F(12) => bytes.extend_from_slice(b"\x1b[24~"),
        KeyCode::Tab => bytes.push(b'\t'),
        KeyCode::Esc => bytes.push(0x1b),
        KeyCode::Char(ch) => {
            if key.modifiers.contains(KeyModifiers::CONTROL) {
                if ch == ' ' {
                    bytes.push(0x00);
                } else if ch.is_ascii() {
                    bytes.push((ch.to_ascii_lowercase() as u8) & 0x1f);
                }
            } else {
                let mut utf8 = [0u8; 4];
                bytes.extend_from_slice(ch.encode_utf8(&mut utf8).as_bytes());
            }
        }
        _ => {}
    }

    bytes
}

fn mouse_to_bytes(
    mode: vt100::MouseProtocolMode,
    encoding: vt100::MouseProtocolEncoding,
    event: MouseEvent,
    body_row_offset: u16,
) -> Option<String> {
    if mode == vt100::MouseProtocolMode::None {
        return None;
    }
    if encoding != vt100::MouseProtocolEncoding::Sgr {
        return None;
    }

    let x = event.column.saturating_add(1);
    let y = event.row.saturating_sub(body_row_offset).saturating_add(1);

    let (code, suffix) = match event.kind {
        MouseEventKind::Down(MouseButton::Left) => (0, 'M'),
        MouseEventKind::Down(MouseButton::Middle) => (1, 'M'),
        MouseEventKind::Down(MouseButton::Right) => (2, 'M'),
        MouseEventKind::Up(MouseButton::Left)
        | MouseEventKind::Up(MouseButton::Middle)
        | MouseEventKind::Up(MouseButton::Right) => (0, 'm'),
        MouseEventKind::Drag(MouseButton::Left) => (32, 'M'),
        MouseEventKind::Drag(MouseButton::Middle) => (33, 'M'),
        MouseEventKind::Drag(MouseButton::Right) => (34, 'M'),
        MouseEventKind::Moved => (35, 'M'),
        MouseEventKind::ScrollUp => (64, 'M'),
        MouseEventKind::ScrollDown => (65, 'M'),
        _ => return None,
    };

    Some(format!("\x1b[<{};{};{}{}", code, x, y, suffix))
}

fn time_label() -> String {
    OffsetDateTime::now_local()
        .or_else(|_| Ok(OffsetDateTime::now_utc()))
        .and_then(|now| now.format(TIME_FORMAT))
        .unwrap_or_else(|_| "00:00:00".to_string())
}
