# Changelog

## 0.1.3

- Improve Windows compatibility for wrapped TUIs by fixing PTY startup, direct process spawning, terminal query handling, and repeated key/mouse input behavior
- Preserve cursor state in captured snapshots and history, and only render the terminal cursor when the live viewport is aligned with current output
- Add safer panic recovery with terminal restore and panic log output so mouse capture and terminal state do not stay broken after crashes
- Refine shell and main-screen behavior with automatic alternate-screen detection, lightweight watch scrolling in history, and better guardrails around mouse escape leakage
- Upgrade terminal rendering dependencies to `ratatui 0.30` and `vt100 0.16.2`, removing the need for local vendoring while enabling safer deep scrollback handling
- Add Docker-based demo support for recording `twatch -L 5000 -- zsh` sessions with `vhs`

This release focuses on stability and day-to-day usability across real terminal workflows.
It strengthens Windows behavior, improves shell-oriented sessions, and modernizes the terminal stack underneath `twatch`.

## 0.1.2

- Add replay mode for saved JSONL traces with read-only history browsing
- Add frame metadata for timestamps, sequence numbers, changed-cell counts, input summaries, and resize traces
- Add cell inspector, automatic snapshot triggers, and conditional asynchronous `aftercommand` triggers for TUI debugging
- Add child process pause/resume on `Shift+P` in addition to capture pause on `p`
- Improve long-running history behavior with batched trimming, cached snapshot lookup, update coalescing, and lighter history rendering
- Restrict mouse passthrough to `latest` view and refine history overlay sizing and selection behavior
- Split large `app`, `ui`, `runner`, and `aftercommand` modules into smaller units without changing core behavior
- Refresh README and in-app help text to match the current keybindings and debug-oriented workflow

This release turns `twatch` into a much stronger debugging tool for terminal UIs.
It expands traceability and replay features, improves responsiveness for longer sessions, and tightens the interaction model around history browsing and child control.

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
