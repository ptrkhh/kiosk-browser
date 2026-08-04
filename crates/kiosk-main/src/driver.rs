//! The driver actor + `EffectSink` seam: executes the FSM's effects. Wired into
//! `main.rs` (Task 6): `TauriSink` is the production `EffectSink`, constructed there
//! together with a `Driver` and spawned via [`run`].

use crate::effect;
use kiosk_core::app::state::{Effect, Event as AppEvent, Machine};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// Executes the effects the FSM returns. The production impl (`TauriSink`, Task 6) drives
/// the webview; tests use a recording fake. Sync: the webview marshals internally.
pub trait EffectSink {
    fn dispatch(&mut self, effect: Effect);
}

/// SEC-09 Critical 2 fix: once the credential-DACL reload gate navigates to
/// `safe.html`, NOTHING may navigate the webview away again — but cancelling
/// `fetch_probe_cancel` (see `main.rs`'s `hold_safe_after_credential_violation`) only
/// narrows the race, it doesn't close it: `probe::run`'s in-flight `probe_once` and its
/// unconditional `tx.send` aren't raced against cancellation at all, and `driver::run`'s
/// `select!` has no `biased;`, so an `AppEvent` already buffered in the channel at the
/// moment the token fires can still be `recv`'d and dispatched. Hardening each producer
/// (race `probe_once` itself, add `biased;` here, check `cancel.is_cancelled()` before
/// `tx.send`) is three separate fixes for one hole. Every navigation, from whatever
/// producer, present or future, passes through `EffectSink::dispatch` — so wrapping the
/// sink and refusing to forward once latched closes the whole class at the one place
/// it all funnels through.
///
/// SEC-09 final review, FIX 2: `fetch_probe_cancel` is scoped to `fetch::run`/
/// `probe::run` ONLY — `driver::run` (below) is deliberately handed the top-level
/// shutdown token instead and is never cancelled by a credential-DACL violation, so
/// this latch (not task exit) is the ONLY thing standing between a post-violation
/// `AppEvent` and the webview. `ClearProfile`/`RefetchConfig` pass through
/// unconditionally (see `dispatch` below) precisely because the driver task keeps
/// running: the idle-clear privacy control depends on it.
pub struct SafeLatchedSink<S> {
    inner: S,
    latched: Arc<AtomicBool>,
}

impl<S: EffectSink> SafeLatchedSink<S> {
    pub fn new(inner: S) -> Self {
        Self {
            inner,
            latched: Arc::new(AtomicBool::new(false)),
        }
    }

    /// A `Clone`+`Send` handle sharing the SAME latch, so a task that never touches the
    /// sink itself (the credential-violation handler, which navigates through its own
    /// unwrapped `TauriSink` clone before the window exists in `driver::run`'s task) can
    /// still trip it.
    pub fn latch_handle(&self) -> SafeModeLatch {
        SafeModeLatch(self.latched.clone())
    }
}

impl<S: EffectSink> EffectSink for SafeLatchedSink<S> {
    fn dispatch(&mut self, effect: Effect) {
        // Only navigating effects are gated: `effect::page_for` returns `Some` exactly
        // for the ones that can move the webview off `safe.html` (`Navigate`,
        // `ShowVideo`, `ShowSplash`, `ShowErrorPage`). `RefetchConfig`/`ClearProfile`
        // never touch the page and must keep flowing even once latched -- in
        // particular `ClearProfile`, which drives the idle-clear privacy control
        // (`TauriSink` -> `clear::clear()` -> `AppEvent::ProfileCleared`) and has
        // nothing to do with the navigation race this latch closes.
        if effect::page_for(&effect).is_some() && self.latched.load(Ordering::SeqCst) {
            return;
        }
        self.inner.dispatch(effect);
    }
}

/// Trips a `SafeLatchedSink`'s latch from elsewhere. Cheap to clone; carries no borrow
/// of the sink.
#[derive(Clone)]
pub struct SafeModeLatch(Arc<AtomicBool>);

impl SafeModeLatch {
    pub fn trip(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

/// Owns the single `Machine`. Not `Sync`; lives inside the driver task alone.
pub struct Driver {
    pub machine: Machine,
}

impl Driver {
    pub fn handle(&mut self, event: AppEvent, sink: &mut dyn EffectSink) {
        for effect in self.machine.on(event) {
            sink.dispatch(effect);
        }
    }
}

/// The driver task: drains the event channel until the channel closes or cancellation.
pub async fn run(
    mut rx: mpsc::Receiver<AppEvent>,
    mut driver: Driver,
    mut sink: Box<dyn EffectSink + Send>,
    cancel: CancellationToken,
) {
    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            maybe = rx.recv() => match maybe {
                Some(event) => driver.handle(event, sink.as_mut()),
                None => break,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kiosk_core::app::state::{
        Effect, Event as AppEvent, Machine, MachineConfig, DEFAULT_ERROR_RETRY_SECONDS,
    };
    // `Fallback` lives in `config::schema`; `app::state` only `use`s it privately (not
    // re-exported), so `kiosk_core::app::state::Fallback` (as the brief's snippet has it)
    // does not resolve outside the crate. Same type, its actual public home.
    use kiosk_core::config::schema::Fallback;

    #[derive(Default)]
    struct RecordingSink {
        effects: Vec<Effect>,
    }
    impl EffectSink for RecordingSink {
        fn dispatch(&mut self, effect: Effect) {
            self.effects.push(effect);
        }
    }

    /// Like `RecordingSink`, but its recording lives behind an `Arc<Mutex<_>>` so a test
    /// can keep inspecting it after the `Box<dyn EffectSink + Send>` it's wrapped in has
    /// moved into `run`.
    #[derive(Clone, Default)]
    struct SharedRecordingSink(std::sync::Arc<std::sync::Mutex<Vec<Effect>>>);
    impl EffectSink for SharedRecordingSink {
        fn dispatch(&mut self, effect: Effect) {
            self.0.lock().unwrap().push(effect);
        }
    }

    fn cfg() -> MachineConfig {
        MachineConfig {
            fallback: Fallback::Video,
            error_max_retries: 5,
            idle_clear: true,
            error_retry_seconds: DEFAULT_ERROR_RETRY_SECONDS,
        }
    }

    #[test]
    fn boot_with_config_navigates_home() {
        let mut d = Driver {
            machine: Machine::new(cfg()),
        };
        let mut sink = RecordingSink::default();
        d.handle(
            AppEvent::ConfigApplied {
                url: "https://home.test/".into(),
            },
            &mut sink,
        );
        assert_eq!(
            sink.effects,
            vec![Effect::Navigate("https://home.test/".into())]
        );
    }

    #[test]
    fn boot_without_config_shows_video() {
        let mut d = Driver {
            machine: Machine::new(cfg()),
        };
        let mut sink = RecordingSink::default();
        d.handle(AppEvent::ConfigUnavailable, &mut sink);
        assert_eq!(sink.effects, vec![Effect::ShowVideo]);
    }

    // I1: pins the FSM/driver contract for `Reconnected`, NOT a runtime event — D2a's
    // probe emits only `LinkChanged`, never `Reconnected`, so this path is dormant at
    // runtime (see `probe::run`/`main.rs` refetch note). Kept so a later sub-plan that
    // adds a `Reconnected` producer inherits a proven driver seam.
    #[test]
    fn offline_then_reconnect_refetches_then_navigates() {
        let mut d = Driver {
            machine: Machine::new(cfg()),
        };
        let mut sink = RecordingSink::default();
        d.handle(
            AppEvent::ConfigApplied {
                url: "https://home.test/".into(),
            },
            &mut sink,
        );
        d.handle(
            AppEvent::LinkChanged(kiosk_core::net::prober::Link::Offline),
            &mut sink,
        );
        sink.effects.clear();
        d.handle(AppEvent::Reconnected, &mut sink);
        assert_eq!(sink.effects, vec![Effect::RefetchConfig]);
    }

    /// SEC-09 Critical 2, closing the residual race: reproduces exactly what the review
    /// flagged as still open after `driver_probe_cancel.cancel()` alone -- an `AppEvent`
    /// already sitting in the channel (standing in for a `probe::run` result that landed
    /// just before/after cancellation) can still be `recv`'d by `driver::run`'s unbiased
    /// `select!` and dispatched, because cancelling the token stops the LOOP, not
    /// anything already buffered. Without `SafeLatchedSink` (i.e. boxing a plain
    /// `SharedRecordingSink` instead) this event reaches the sink -- see the RED run
    /// captured in the Task 3 report. With the wrap + `latch.trip()` (mirroring
    /// `hold_safe_after_credential_violation`'s call), it never does, regardless of how
    /// `select!` happens to schedule.
    #[tokio::test]
    async fn a_buffered_event_never_reaches_the_sink_once_safe_mode_latches() {
        let (tx, rx) = mpsc::channel::<AppEvent>(8);
        // Puts the FSM where `LinkChanged` actually produces an effect (Navigate), so a
        // no-op FSM transition can't hide a real gap.
        tx.try_send(AppEvent::ConfigApplied {
            url: "https://home.test/".into(),
        })
        .unwrap();
        tx.try_send(AppEvent::LinkChanged(
            kiosk_core::net::prober::Link::Offline,
        ))
        .unwrap();
        tx.try_send(AppEvent::LinkChanged(kiosk_core::net::prober::Link::Online))
            .unwrap();

        let sink = SharedRecordingSink::default();
        let effects = sink.0.clone();
        let latched = SafeLatchedSink::new(sink);
        let latch = latched.latch_handle();

        let cancel = CancellationToken::new();
        // Mirrors `hold_safe_after_credential_violation`'s sequence: trip the latch,
        // then cancel -- the exact ordering that leaves an unwrapped sink still
        // reachable via whatever `select!` schedules next.
        latch.trip();
        cancel.cancel();

        let d = Driver {
            machine: Machine::new(cfg()),
        };
        run(rx, d, Box::new(latched), cancel).await;

        let got = effects.lock().unwrap();
        assert!(
            got.is_empty(),
            "no Navigate/ShowVideo may reach the sink once safe mode has latched, even \
             for events already buffered ahead of cancellation: got {got:?}"
        );
    }

    /// Complement of the race test above: the latch must narrow to navigating effects
    /// only. `ClearProfile` is the idle-clear privacy control's actual data wipe
    /// (`TauriSink` -> `clear::clear()` -> `AppEvent::ProfileCleared`) and has nothing to
    /// do with `fetch::run`/`probe::run`, so it must keep reaching the inner sink even
    /// after `trip()` -- otherwise a device sitting in safe mode silently stops purging
    /// cookies/cache/local storage on every idle cycle. `RefetchConfig` is included for
    /// the same reason `effect::page_for` maps both to `None`. Fails against f8c458e,
    /// where `dispatch` blocked unconditionally once latched.
    #[test]
    fn non_navigating_effects_still_reach_the_sink_once_latched() {
        let mut sink = SafeLatchedSink::new(RecordingSink::default());
        sink.latch_handle().trip();

        sink.dispatch(Effect::ClearProfile { full: true });
        sink.dispatch(Effect::ClearProfile { full: false });
        sink.dispatch(Effect::RefetchConfig);
        // Still blocked: this is the property the race test above pins.
        sink.dispatch(Effect::ShowVideo);

        assert_eq!(
            sink.inner.effects,
            vec![
                Effect::ClearProfile { full: true },
                Effect::ClearProfile { full: false },
                Effect::RefetchConfig,
            ]
        );
    }
}
