# Changelog

## 0.1.1

- Fix child TUI exit handling so `twatch` can stop cleanly after the wrapped PTY closes
- Change watch-mode `changed` detection from PTY dirty-state tracking to snapshot comparison
- Avoid creating new history entries when the visible screen content did not change
- Keep the last captured frame visible when the wrapped child exits
- Fix remaining UI wording from `hwatch` to `twatch` in the exit dialog
- Update README examples to use the correct `--differences` option name
- Tighten regression coverage for unchanged-frame history behavior

This release is a small stability and polish update for the initial `0.1.x` series.
It does not add a major new feature, but it improves the core watch loop, history behavior, and release documentation.

## 0.1.0

Initial public release.

### Added

- PTY-based child TUI wrapping
- Event-driven screen capture
- `latest` plus history navigation
- Right-side history overlay
- String and regex filter
- Watch diff highlight
- Mouse and keyboard passthrough
- Batch mode
- Aftercommand hook
- JSONL history save/load
- In-memory compressed history
- Snapshot export as ANSI text or SVG

### Notes

- Mouse behavior still depends on the child TUI and the mouse protocol it enables
- Line/word diff is not included in this release
