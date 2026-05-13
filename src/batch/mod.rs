// Copyright (c) 2026 Blacknon. All rights reserved.
// Use of this source code is governed by an MIT license
// that can be found in the LICENSE file.

use crate::cli::{Cli, CropSpec};
use crate::screen::ScreenSnapshot;

mod render;
#[cfg(test)]
mod tests;

pub fn terminal_size(cli: &Cli) -> (u16, u16) {
    cli.batch_size
        .map(|size| (size.width, size.height))
        .unwrap_or_else(|| crossterm::terminal::size().unwrap_or((120, 40)))
}

pub fn prepare_snapshot(snapshot: &ScreenSnapshot, crop: Option<CropSpec>) -> ScreenSnapshot {
    crop.map_or_else(
        || snapshot.clone(),
        |crop| snapshot.cropped(crop.x, crop.y, crop.width, crop.height),
    )
}

pub fn render_output(
    cli: &Cli,
    label: &str,
    current: &ScreenSnapshot,
    previous: Option<&ScreenSnapshot>,
) -> String {
    render::render_output(cli, label, current, previous)
}
