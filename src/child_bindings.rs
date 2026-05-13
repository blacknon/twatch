// Copyright (c) 2026 Blacknon. All rights reserved.
// Use of this source code is governed by an MIT license
// that can be found in the LICENSE file.

use anyhow::{Context, Result};

use crate::input_key::{KeyPress, parse_key_bytes, parse_key_press};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ChildBindingAction {
    Send(Vec<u8>),
    SaveSnapshot,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ChildBinding {
    pub(crate) trigger: KeyPress,
    pub(crate) action: ChildBindingAction,
}

pub(crate) fn compile_child_bindings(specs: &[String]) -> Result<Vec<ChildBinding>> {
    let mut bindings = Vec::new();
    for spec in specs {
        let (left, right) = spec
            .split_once('=')
            .with_context(|| format!("binding must be FROM=TO: {spec}"))?;
        let trigger = parse_key_press(left.trim())?;
        let action = parse_child_binding_action(right.trim())?;
        bindings.push(ChildBinding { trigger, action });
    }
    Ok(bindings)
}

fn parse_child_binding_action(value: &str) -> Result<ChildBindingAction> {
    if value.eq_ignore_ascii_case("screenshot") {
        return Ok(ChildBindingAction::SaveSnapshot);
    }

    if let Some(text) = value.strip_prefix("text:") {
        return Ok(ChildBindingAction::Send(text.as_bytes().to_vec()));
    }

    let key_list = value.strip_prefix("send:").unwrap_or(value);
    let mut bytes = Vec::new();
    for item in key_list.split(',') {
        bytes.extend_from_slice(&parse_key_bytes(item.trim())?);
    }
    Ok(ChildBindingAction::Send(bytes))
}

#[cfg(test)]
mod tests {
    use super::{ChildBindingAction, compile_child_bindings};

    #[test]
    fn parses_twrap_style_bindings() {
        let bindings = compile_child_bindings(&[
            "j=down".to_string(),
            "ctrl-t=screenshot".to_string(),
            "g=text:gg".to_string(),
        ])
        .unwrap();

        assert!(
            bindings
                .iter()
                .any(|binding| binding.action == ChildBindingAction::Send(b"\x1b[B".to_vec()))
        );
        assert!(
            bindings
                .iter()
                .any(|binding| binding.action == ChildBindingAction::SaveSnapshot)
        );
        assert!(
            bindings
                .iter()
                .any(|binding| binding.action == ChildBindingAction::Send(b"gg".to_vec()))
        );
    }
}
