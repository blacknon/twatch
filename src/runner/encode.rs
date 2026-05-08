use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

pub(super) fn key_to_bytes(key: KeyEvent, application_cursor: bool) -> Vec<u8> {
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

pub(super) fn mouse_to_bytes(
    mode: vt100::MouseProtocolMode,
    encoding: vt100::MouseProtocolEncoding,
    event: MouseEvent,
    body_row_offset: u16,
) -> Option<Vec<u8>> {
    if mode == vt100::MouseProtocolMode::None {
        return None;
    }

    let x = event.column.saturating_add(1);
    let y = event.row.saturating_sub(body_row_offset).saturating_add(1);

    let mode_name = format!("{mode:?}");
    let encoding_name = format!("{encoding:?}");

    let (code, sgr_suffix) = match event.kind {
        MouseEventKind::Down(MouseButton::Left) => (0, 'M'),
        MouseEventKind::Down(MouseButton::Middle) => (1, 'M'),
        MouseEventKind::Down(MouseButton::Right) => (2, 'M'),
        MouseEventKind::Up(MouseButton::Left)
        | MouseEventKind::Up(MouseButton::Middle)
        | MouseEventKind::Up(MouseButton::Right) => (3, 'm'),
        MouseEventKind::Drag(MouseButton::Left) => {
            if !supports_drag_tracking(&mode_name) {
                return None;
            }
            (32, 'M')
        }
        MouseEventKind::Drag(MouseButton::Middle) => {
            if !supports_drag_tracking(&mode_name) {
                return None;
            }
            (33, 'M')
        }
        MouseEventKind::Drag(MouseButton::Right) => {
            if !supports_drag_tracking(&mode_name) {
                return None;
            }
            (34, 'M')
        }
        MouseEventKind::Moved => return None,
        MouseEventKind::ScrollUp => (64, 'M'),
        MouseEventKind::ScrollDown => (65, 'M'),
        _ => return None,
    };

    if encoding_name.contains("Sgr") {
        return Some(format!("\x1b[<{};{};{}{}", code, x, y, sgr_suffix).into_bytes());
    }

    encode_legacy_mouse(code, x, y)
}

fn supports_drag_tracking(mode_name: &str) -> bool {
    mode_name.contains("Motion") || mode_name.contains("Drag")
}

fn encode_legacy_mouse(code: u16, x: u16, y: u16) -> Option<Vec<u8>> {
    let x = x.min(223);
    let y = y.min(223);
    let cb = u8::try_from(code).ok()?.saturating_add(32);
    let cx = u8::try_from(x).ok()?.saturating_add(32);
    let cy = u8::try_from(y).ok()?.saturating_add(32);
    Some(vec![0x1b, b'[', b'M', cb, cx, cy])
}
