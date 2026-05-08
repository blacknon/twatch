use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crossterm::event::{KeyEvent, MouseEvent};

use crate::app::{App, InputTargetFocus, InputTraceEvent, InputTraceKind, ResizeTraceEvent};

impl App {
    pub(super) fn record_child_key_event(&mut self, key: KeyEvent) {
        self.record_input_event(InputTraceKind::Key {
            code: key.code,
            modifiers: key.modifiers,
        });
    }

    pub(super) fn record_child_mouse_event(&mut self, mouse: MouseEvent) {
        self.record_input_event(InputTraceKind::Mouse {
            kind: mouse.kind,
            column: mouse.column,
            row: mouse.row,
        });
    }

    fn record_input_event(&mut self, kind: InputTraceKind) {
        let event = InputTraceEvent {
            seq: self.next_input_seq,
            timestamp_unix_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or(Duration::ZERO)
                .as_millis() as u64,
            kind,
            target_focus: InputTargetFocus::Child,
        };
        self.next_input_seq += 1;
        self.pending_input_events.push(event.clone());
        self.input_trace.push_back(event);
        while self.input_trace.len() > 256 {
            self.input_trace.pop_front();
        }
    }

    pub(crate) fn note_resize_event(
        &mut self,
        new_width: u16,
        new_height: u16,
        source: &'static str,
    ) {
        let (old_width, old_height) = self
            .current_snapshot
            .as_ref()
            .map(|snapshot| (snapshot.width(), snapshot.height()))
            .unwrap_or((new_width, new_height));

        if old_width == new_width && old_height == new_height {
            return;
        }

        let event = ResizeTraceEvent {
            timestamp_unix_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or(Duration::ZERO)
                .as_millis() as u64,
            old_width,
            old_height,
            new_width,
            new_height,
            source,
        };
        self.pending_resize_event = Some(event.clone());
        self.resize_trace.push_back(event);
        while self.resize_trace.len() > 64 {
            self.resize_trace.pop_front();
        }
    }
}
