# Changelog

## 0.1.5

- Add `--record-stdin` so `twatch` can record terminal output from stdin without spawning a child PTY
- Add `--size WIDTH,HEIGHT` for fixed-size stdin recording and document the tmux-oriented JSONL recording flow
- Store `--record-stdin` traces as checkpoints plus deltas instead of writing a full snapshot on every frame
- Compact in-memory cell symbols, compress delta history when `-C` is enabled, and raise the default checkpoint interval to reduce long-running memory pressure
- Add transparent `.jsonl.gz` log read/write support and make the tmux plugin default to `archive` compression so active pane replay stays faster
- Compress large snapshot and delta payloads inside JSONL records so plain `.jsonl` traces also shrink without hiding frame metadata
- Open replay after preloading the first 256 frames and continue loading the rest in the background to reduce large-log startup delay
- Spill older active `--record-stdin` frames into a compact sidecar during long-running sessions so live `.jsonl` logs grow more slowly
- Add tunable `--record-stdin-spill-every` and `--record-stdin-spill-retain` controls, and store deltas in a more compact run/style-table form to shrink active tmux logs further
- Treat `-L 0` as unlimited history retention so tmux replay can avoid trimming large recordings on startup
- Write active JSONL records in a more compact array-based format while keeping backward-compatible readers for older object-based logs

This release is focused on tmux-oriented recording and replay scalability.
It adds the first backend-oriented building block for `tmux pipe-pane` style integrations by letting `twatch` ingest terminal output from stdin and store it as replayable traces.

Notes:

- `--record-stdin` requires `--logfile` and cannot be combined with `--batch`, `--replay`, or a child command
- Replay readers remain backward-compatible with older object-based JSONL logs

## 0.1.4

- Add customizable `twatch` keymaps with `-K/--keymap KEY=ACTION`, including pane-specific navigation, snapshot actions, pause control, and app-input mode switching
- Add `twrap`-style child key overrides with `-k/--bind FROM=TO`, supporting key aliases, comma-separated key sequences, `text:...`, and `screenshot`
- Remove the unused `--interval` option and simplify batch capture timing so PTY-backed sources wait on real source updates instead of a polling interval
- Expand README and in-app help to document the current interaction model, custom keymap actions, and child key override behavior
- Add regression coverage for custom keymaps and child binding overrides, including passthrough interception and custom exit/app-input flows
- Refresh Rust CI and release workflows to avoid cached `cargo` shims on GitHub Actions and make macOS runner behavior more reliable
- Add consistent source headers across Rust modules touched during this release

This release is centered on input control and operational polish.
It makes `twatch` more usable with editor-like workflows, shell-heavy sessions, and wrapped TUIs that benefit from custom local shortcuts or remapped child input.

Notes:

- `--interval` was removed because the current PTY workflow is event-driven and no longer relies on that option
- The new input customization layer is the main user-facing change in this release

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
