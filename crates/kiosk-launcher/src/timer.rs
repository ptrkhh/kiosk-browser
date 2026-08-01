//! A 1-second tick source feeding `watchdog::Event::Tick` into the
//! supervisor loop, so the pure FSM can detect missed heartbeats and
//! elapsed run time without ever reading a clock itself.

use crate::clock::now;
use kiosk_core::watchdog::Event;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::time::Duration;

/// Starts a detached thread that sends `Event::Tick{now}` on `tx` roughly
/// once per second until `cancel` is set to `true`, then stops without
/// sending further events. Does not panic if `tx`'s receiver is dropped
/// first (e.g. the supervisor loop already exited) — it just stops.
pub fn spawn_timer(tx: Sender<Event>, cancel: Arc<AtomicBool>) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(1));
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        if tx.send(Event::Tick { now: now() }).is_err() {
            break;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn ticks_until_cancelled() {
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        spawn_timer(tx, cancel.clone());

        let tick = rx
            .recv_timeout(Duration::from_secs(3))
            .expect("expected a Tick within 3s of a 1s-period timer");
        assert!(matches!(tick, Event::Tick { .. }));

        cancel.store(true, Ordering::Relaxed);
        // Draining leftover ticks. After cancellation takes effect (at most
        // one more tick may already be in flight), the channel must go
        // quiet: recv_timeout should time out rather than delivering
        // further ticks indefinitely.
        loop {
            match rx.recv_timeout(Duration::from_millis(1500)) {
                Ok(Event::Tick { .. }) => continue,
                Ok(_) => panic!("timer only ever sends Tick events"),
                Err(mpsc::RecvTimeoutError::Timeout) => break,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
    }
}
