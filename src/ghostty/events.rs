//! Bounded handoff of native actions that must mutate GTK outside Ghostty callbacks.

use std::collections::VecDeque;
use std::sync::{LazyLock, Mutex};

const CAPACITY: usize = 128;
const PER_TURN: usize = 16;

/// An owned pane identity; closed panes are ignored when GTK consumes the event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Event {
    Bell(u64),
    NewTerminalTab(u64),
}

#[derive(Default)]
struct Pending {
    events: VecDeque<Event>,
    dropped: usize,
}

impl Pending {
    /// Preserve FIFO order and coalesce only redundant bell attention for the same pane.
    fn push(&mut self, event: Event) -> bool {
        if matches!(event, Event::Bell(_)) && self.events.contains(&event) {
            return true;
        }
        if self.events.len() == CAPACITY {
            self.dropped = self.dropped.saturating_add(1);
            return false;
        }
        self.events.push_back(event);
        true
    }

    /// Take a bounded batch and overflow count, retaining later events for the next GTK turn.
    fn take(&mut self) -> (Vec<Event>, usize) {
        let count = self.events.len().min(PER_TURN);
        (
            self.events.drain(..count).collect(),
            std::mem::take(&mut self.dropped),
        )
    }
}

static PENDING: LazyLock<Mutex<Pending>> = LazyLock::new(|| Mutex::new(Pending::default()));

/// Queue a native action without GTK reentrancy; false means capacity or lock failure.
pub(crate) fn push(event: Event) -> bool {
    PENDING.lock().is_ok_and(|mut pending| pending.push(event))
}

/// Release the queue lock before the caller executes any GTK or native operation.
pub(crate) fn take() -> (Vec<Event>, usize) {
    PENDING
        .lock()
        .map(|mut pending| pending.take())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Bell coalescing must not discard other panes or repeated terminal creation requests.
    #[test]
    fn burst_order_and_coalescing() {
        let mut pending = Pending::default();
        for event in [
            Event::Bell(1),
            Event::Bell(2),
            Event::Bell(1),
            Event::NewTerminalTab(1),
            Event::NewTerminalTab(1),
        ] {
            assert!(pending.push(event));
        }
        assert_eq!(
            pending.take(),
            (
                vec![
                    Event::Bell(1),
                    Event::Bell(2),
                    Event::NewTerminalTab(1),
                    Event::NewTerminalTab(1)
                ],
                0
            )
        );
        assert!(pending.take().0.is_empty());
    }

    /// Capacity rejection is observable and draining bounds GTK work while restoring admission.
    #[test]
    fn bounded_capacity_and_turn_budget() {
        let mut pending = Pending::default();
        for pane in 0..CAPACITY as u64 {
            assert!(pending.push(Event::NewTerminalTab(pane)));
        }
        assert!(!pending.push(Event::Bell(500)));
        let (events, dropped) = pending.take();
        assert_eq!(events.len(), PER_TURN);
        assert_eq!(events[0], Event::NewTerminalTab(0));
        assert_eq!(dropped, 1);
        assert!(pending.push(Event::Bell(500)));
        assert_eq!(pending.take().1, 0);
        assert_eq!(pending.events.back(), Some(&Event::Bell(500)));
    }
}
