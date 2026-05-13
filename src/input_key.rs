// Copyright (c) 2026 Blacknon. All rights reserved.
// Use of this source code is governed by an MIT license
// that can be found in the LICENSE file.

use anyhow::{Context, Result};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub(crate) struct KeyPress {
    pub(crate) code: KeyCode,
    pub(crate) modifiers: KeyModifiers,
}

impl From<KeyEvent> for KeyPress {
    fn from(value: KeyEvent) -> Self {
        Self {
            code: value.code,
            modifiers: value.modifiers,
        }
    }
}

pub(crate) fn parse_key_press(value: &str) -> Result<KeyPress> {
    let (modifiers, key_name) = parse_key_name(value)?;
    let code = parse_key_code(key_name, modifiers)?;
    Ok(KeyPress { code, modifiers })
}

pub(crate) fn parse_key_bytes(value: &str) -> Result<Vec<u8>> {
    let normalized = value.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        anyhow::bail!("key binding token must not be empty");
    }

    Ok(match normalized.as_str() {
        "up" => b"\x1b[A".to_vec(),
        "down" => b"\x1b[B".to_vec(),
        "right" => b"\x1b[C".to_vec(),
        "left" => b"\x1b[D".to_vec(),
        "home" => b"\x1b[H".to_vec(),
        "end" => b"\x1b[F".to_vec(),
        "pageup" => b"\x1b[5~".to_vec(),
        "pagedown" => b"\x1b[6~".to_vec(),
        "insert" => b"\x1b[2~".to_vec(),
        "delete" => b"\x1b[3~".to_vec(),
        "enter" => vec![b'\r'],
        "tab" => vec![b'\t'],
        "esc" => vec![0x1b],
        "space" => vec![b' '],
        "backspace" => vec![0x7f],
        "f1" => b"\x1bOP".to_vec(),
        "f2" => b"\x1bOQ".to_vec(),
        "f3" => b"\x1bOR".to_vec(),
        "f4" => b"\x1bOS".to_vec(),
        "f5" => b"\x1b[15~".to_vec(),
        "f6" => b"\x1b[17~".to_vec(),
        "f7" => b"\x1b[18~".to_vec(),
        "f8" => b"\x1b[19~".to_vec(),
        "f9" => b"\x1b[20~".to_vec(),
        "f10" => b"\x1b[21~".to_vec(),
        "f11" => b"\x1b[23~".to_vec(),
        "f12" => b"\x1b[24~".to_vec(),
        token if token.starts_with("ctrl-") => parse_ctrl_key(token)?,
        token => parse_literal_key(token)?,
    })
}

fn parse_key_name(value: &str) -> Result<(KeyModifiers, &str)> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        anyhow::bail!("key binding token must not be empty");
    }

    let mut modifiers = KeyModifiers::empty();
    let mut rest = trimmed;
    loop {
        let lower = rest.to_ascii_lowercase();
        if let Some(stripped) = lower.strip_prefix("ctrl-") {
            modifiers.insert(KeyModifiers::CONTROL);
            rest = &rest[(rest.len() - stripped.len())..];
            continue;
        }
        if let Some(stripped) = lower.strip_prefix("alt-") {
            modifiers.insert(KeyModifiers::ALT);
            rest = &rest[(rest.len() - stripped.len())..];
            continue;
        }
        if let Some(stripped) = lower.strip_prefix("shift-") {
            modifiers.insert(KeyModifiers::SHIFT);
            rest = &rest[(rest.len() - stripped.len())..];
            continue;
        }
        break;
    }

    if rest.is_empty() {
        anyhow::bail!("key binding token must not be empty");
    }

    Ok((modifiers, rest))
}

fn parse_key_code(value: &str, modifiers: KeyModifiers) -> Result<KeyCode> {
    let normalized = value.to_ascii_lowercase();
    match normalized.as_str() {
        "up" => Ok(KeyCode::Up),
        "down" => Ok(KeyCode::Down),
        "left" => Ok(KeyCode::Left),
        "right" => Ok(KeyCode::Right),
        "home" => Ok(KeyCode::Home),
        "end" => Ok(KeyCode::End),
        "pageup" => Ok(KeyCode::PageUp),
        "pagedown" => Ok(KeyCode::PageDown),
        "tab" => Ok(KeyCode::Tab),
        "backtab" => Ok(KeyCode::BackTab),
        "backspace" => Ok(KeyCode::Backspace),
        "enter" => Ok(KeyCode::Enter),
        "esc" => Ok(KeyCode::Esc),
        "insert" => Ok(KeyCode::Insert),
        "delete" => Ok(KeyCode::Delete),
        "space" => Ok(KeyCode::Char(' ')),
        "plus" => Ok(KeyCode::Char('+')),
        "minus" => Ok(KeyCode::Char('-')),
        name if name.starts_with('f') => {
            let num = name[1..]
                .parse::<u8>()
                .with_context(|| format!("unsupported function key: {value}"))?;
            Ok(KeyCode::F(num))
        }
        _ => parse_char_code(value, modifiers),
    }
}

fn parse_char_code(value: &str, modifiers: KeyModifiers) -> Result<KeyCode> {
    let mut chars = value.chars();
    let ch = chars
        .next()
        .with_context(|| format!("unsupported key binding token: {value}"))?;
    if chars.next().is_some() {
        anyhow::bail!("unsupported key binding token: {value}");
    }

    let code = if modifiers.contains(KeyModifiers::SHIFT) && ch.is_ascii_alphabetic() {
        ch.to_ascii_uppercase()
    } else {
        ch
    };

    Ok(KeyCode::Char(code))
}

fn parse_ctrl_key(value: &str) -> Result<Vec<u8>> {
    let suffix = &value[5..];
    if suffix.len() != 1 {
        anyhow::bail!("ctrl binding must have one character: {value}");
    }
    let ch = suffix.as_bytes()[0];
    if !ch.is_ascii_alphabetic() && ch != b'\\' && ch != b'[' && ch != b']' {
        anyhow::bail!("unsupported ctrl binding: {value}");
    }
    Ok(match ch {
        b'\\' => vec![0x1c],
        b'[' => vec![0x1b],
        b']' => vec![0x1d],
        _ => vec![ch.to_ascii_uppercase() - b'@'],
    })
}

fn parse_literal_key(value: &str) -> Result<Vec<u8>> {
    if value.chars().count() != 1 {
        anyhow::bail!("unsupported key binding token: {value}");
    }
    Ok(value.as_bytes().to_vec())
}

#[cfg(test)]
mod tests {
    use super::{KeyPress, parse_key_bytes, parse_key_press};
    use crossterm::event::{KeyCode, KeyModifiers};

    #[test]
    fn parses_shift_letter_keypress() {
        let key = parse_key_press("shift-s").unwrap();
        assert_eq!(
            key,
            KeyPress {
                code: KeyCode::Char('S'),
                modifiers: KeyModifiers::SHIFT,
            }
        );
    }

    #[test]
    fn parses_ctrl_bytes_like_twrap() {
        assert_eq!(parse_key_bytes("ctrl-g").unwrap(), vec![0x07]);
        assert_eq!(parse_key_bytes("down").unwrap(), b"\x1b[B".to_vec());
    }
}
