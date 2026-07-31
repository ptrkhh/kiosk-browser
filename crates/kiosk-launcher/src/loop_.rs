use kiosk_core::watchdog::{Action, Event, Watchdog};
use std::ops::ControlFlow;
use std::sync::mpsc::Receiver;

/// Executes the FSM's Actions. Real impl (Task 4) spawns/drains/logs; tests record.
/// Returns `Break(code)` when it handled `ExitLauncher`, to stop the loop.
///
/// # Dead code scope
/// `#[allow(dead_code)]` here is temporary: `main.rs` does not yet wire this trait
/// into the loop. Remove this allow when Task 4 (`LauncherSink` + assembly) drives `run()`.
#[allow(dead_code)]
pub trait ActionSink {
    fn dispatch(&mut self, action: Action) -> ControlFlow<i32>;
}

/// Drain events into the FSM; dispatch each Action. Returns the process exit code.
///
/// # Dead code scope
/// `#[allow(dead_code)]` here is temporary: `main.rs` does not yet wire this function
/// into the launcher. Remove this allow when Task 4 (`LauncherSink` + assembly) drives this.
#[allow(dead_code)]
pub fn run(
    rx: Receiver<Event>,
    mut wd: Watchdog,
    initial: Vec<Action>,
    sink: &mut dyn ActionSink,
) -> i32 {
    for a in initial {
        if let ControlFlow::Break(code) = sink.dispatch(a) {
            return code;
        }
    }
    while let Ok(ev) = rx.recv() {
        for a in wd.on(ev) {
            if let ControlFlow::Break(code) = sink.dispatch(a) {
                return code;
            }
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use kiosk_core::watchdog::{Action, Event, Watchdog, WatchdogConfig};
    use std::sync::mpsc;

    #[derive(Default)]
    struct RecordingSink {
        actions: Vec<Action>,
    }
    impl ActionSink for RecordingSink {
        fn dispatch(&mut self, a: Action) -> std::ops::ControlFlow<i32> {
            let stop = matches!(a, Action::ExitLauncher { .. });
            let code = if let Action::ExitLauncher { code } = a {
                code
            } else {
                0
            };
            self.actions.push(a);
            if stop {
                std::ops::ControlFlow::Break(code)
            } else {
                std::ops::ControlFlow::Continue(())
            }
        }
    }
    fn cfg() -> WatchdogConfig {
        WatchdogConfig {
            startup_grace_s: 90,
            healthy_run_s: 120,
            channel_grace_s: 30,
        }
    }

    #[test]
    fn dispatches_the_initial_spawn_then_exits_on_code_86() {
        let (wd, initial) = Watchdog::new(cfg());
        let (tx, rx) = mpsc::channel();
        tx.send(Event::Spawned { at: 0 }).unwrap();
        tx.send(Event::Ready).unwrap();
        tx.send(Event::ChildExited { code: 86, at: 30 }).unwrap();
        drop(tx);
        let mut sink = RecordingSink::default();
        let code = run(rx, wd, initial, &mut sink);
        assert_eq!(code, 86, "exit-86 stops the loop with code 86");
        assert!(
            sink.actions.iter().any(|a| matches!(a, Action::SpawnMain)),
            "initial spawn dispatched"
        );
        assert!(matches!(
            sink.actions.last(),
            Some(Action::ExitLauncher { code: 86 })
        ));
    }
}
