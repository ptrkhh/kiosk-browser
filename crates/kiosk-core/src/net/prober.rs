//! The damped connectivity prober state machine (spec §3.3, arch-13, TEL-01).
//!
//! Layering (spec §4): no Tauri, no per-OS API, no sockets. This module does not make
//! the HTTP GET and does not own a timer — `kiosk-main` (P1-D) does both and feeds this
//! state machine one [`ProbeOutcome`] per request. [`reach`](super::reach) decides
//! *which URL* to probe; this module decides what a stream of results *means*.
//!
//! # Damping (spec §3.3)
//!
//! Two consecutive successes flip [`Link::Online`]; two consecutive failures flip
//! [`Link::Offline`]. This exists so the kiosk does not flap between the site and the
//! offline video on a marginal link — a visible flap is far more objectionable to a
//! viewer than a few extra seconds of showing either state. Concretely:
//!
//! - A single failure while [`Link::Online`] does **not** flip — the prober stays
//!   Online. This is the property a naive (undamped) implementation gets wrong, and the
//!   one most worth testing (see the `tests` module).
//! - A success **resets** an in-progress failure run, and symmetrically: `fail,
//!   succeed, fail` leaves the link Online, because the intervening success broke the
//!   run — the final `fail` is only the first of a fresh run, not the second of a
//!   continuing one.
//!
//! # The initial-state deviation (a deliberate judgment call, not an accident)
//!
//! A literal reading of "two consecutive successes flip online" means a perfectly
//! healthy kiosk shows the offline video for two full probe intervals (~20 s) on
//! *every* boot before it will admit it is online. That is a visible regression on
//! every healthy device — and damping exists to stop *flapping*, but on first boot
//! there is no prior state to flap *from*: there has never been an Online shown to flap
//! away from.
//!
//! So [`Prober`] starts in [`Link::Offline`] with an internal `Unknown` damping state
//! in which the **first** outcome ever recorded decides `link` immediately (one success
//! → Online; one failure → stays Offline, since Offline is already the starting point).
//! Two-consecutive damping applies to every transition **after** that first outcome.
//! [`Prober::record`]'s own tests exercise both the cold-start shortcut and the general
//! post-first-outcome damping rule separately, so the two are not conflated.
//!
//! # `Date` harvesting happens on every outcome (TEL-01)
//!
//! [`Prober::record`] feeds `outcome.date_header` to [`TrustedClock::observe_http_date`]
//! whenever one is present — on a **failing** probe exactly as on a succeeding one. This
//! is the mechanism that bootstraps the clock on a dead-CMOS device: the prober is the
//! one subsystem guaranteed to make an HTTP request every 10-30 s, and on a device that
//! actually needs this fallback, a large fraction of those probes are *failing* by
//! definition (that is why the clock is unset in the first place). Restricting the
//! harvest to successes would starve the clock on exactly the devices that need it.
//!
//! The header is never hand-parsed here — see [`TrustedClock::observe_http_date`]'s own
//! docs for why that would be a security-relevant bug, not a cosmetic one. A malformed
//! `Date` yields `Err`, which is swallowed: non-fatal by design, and it must never
//! affect the link/damping state, which is entirely independent of it.
//!
//! # The damping counter is in RAM, and that is correct — not an oversight
//!
//! Unlike several other counters in this codebase (e.g. `logging::mod::RejectState`,
//! which persists a retry budget across a crash because a fresh process must not forget
//! it), the damping run tracked here is deliberately **not** persisted. A fresh process
//! has no prior probe history to flap from — there is nothing to lose by starting over,
//! and inventing durability for it would just be state that outlives its own meaning
//! (a run recorded minutes before a restart says nothing about the link *now*).

use std::time::Duration;

use crate::config::schema::Network;
use crate::logging::time::TrustedClock;

/// One probe result, fed in by the caller. `reachable` is the caller's verdict on the
/// HTTP GET (e.g. a 204 from `connectivity_check_url`); `date_header` is that response's
/// `Date` header verbatim, when the response carried one at all — present on failing
/// responses (a captive portal's 200, a 5xx) just as often as on succeeding ones, and
/// absent only when the request never got an HTTP response to read a header from (a
/// connect timeout, DNS failure, TLS error).
#[derive(Debug, Clone)]
pub struct ProbeOutcome {
    pub reachable: bool,
    pub date_header: Option<String>,
}

/// The prober's public state (spec §3.3). P1-D emits `net.online` / `net.offline` on a
/// [`Prober::record`] flip and drives `Online ⇄ Offline` in the app state machine from
/// it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Link {
    Online,
    Offline,
}

/// The damping run tracker. Deliberately in-memory only — see the module docs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Damping {
    /// No outcome has been recorded by this process yet. The next one decides `link`
    /// immediately (the initial-state deviation).
    Unknown,
    /// `count` consecutive outcomes of `reachable` recorded since the run last broke
    /// (an opposite outcome arrived) or flipped `link`. Capped at 2: nothing above 2 is
    /// ever meaningful, since a flip only ever needs two, and capping keeps this a
    /// plain `u8` regardless of how long the link stays in one state.
    Run { reachable: bool, count: u8 },
}

/// The damped connectivity prober state machine. See the module docs for the rules.
///
/// `Debug` is derived so P1-D can log a `Prober` in a panic hook / health sample; every
/// field is already `Debug` ([`TrustedClock`], [`Link`], [`Damping`]).
#[derive(Debug)]
pub struct Prober {
    clock: TrustedClock,
    link: Link,
    damping: Damping,
}

impl Prober {
    pub fn new(clock: TrustedClock) -> Prober {
        Prober {
            clock,
            link: Link::Offline,
            damping: Damping::Unknown,
        }
    }

    /// Record one probe outcome and return `Some(new_state)` **only on a flip**; `None`
    /// otherwise. P1-D emits `net.online` / `net.offline` on a flip.
    ///
    /// TEL-01 (rule 2): `outcome.date_header`, when present, is fed to
    /// [`TrustedClock::observe_http_date`] unconditionally — success or failure alike —
    /// before anything below decides `link`. A malformed header is non-fatal (its `Err`
    /// is swallowed) and never influences the damping/link logic, which depends only on
    /// `outcome.reachable`.
    pub fn record(&mut self, outcome: ProbeOutcome) -> Option<Link> {
        if let Some(date) = &outcome.date_header {
            let _ = self.clock.observe_http_date(date);
        }

        match self.damping {
            Damping::Unknown => {
                // Rule 3: the first outcome this process ever sees decides `link`
                // immediately, bypassing the two-consecutive rule below. See the
                // module docs for why. `flip_to` still does the right thing on a cold
                // failure: the target (Offline) already equals the starting `link`, so
                // it correctly reports "no flip" rather than needing a special case.
                self.damping = Damping::Run {
                    reachable: outcome.reachable,
                    count: 1,
                };
                self.flip_to(Self::target_for(outcome.reachable))
            }
            Damping::Run { reachable, count } if reachable == outcome.reachable => {
                // The run continues: bump it (capped at 2) and flip once it reaches 2.
                let count = count.saturating_add(1).min(2);
                self.damping = Damping::Run { reachable, count };
                if count >= 2 {
                    self.flip_to(Self::target_for(reachable))
                } else {
                    None
                }
            }
            Damping::Run { .. } => {
                // Rule 1: the opposite outcome arrived, breaking the run. A lone
                // result never flips on its own — the run restarts at 1, and the
                // NEXT matching outcome would be the second of a fresh pair.
                self.damping = Damping::Run {
                    reachable: outcome.reachable,
                    count: 1,
                };
                None
            }
        }
    }

    pub fn link(&self) -> Link {
        self.link
    }

    fn target_for(reachable: bool) -> Link {
        if reachable {
            Link::Online
        } else {
            Link::Offline
        }
    }

    /// Sets `link` to `target`, returning `Some(target)` iff that is an actual change.
    /// `record`'s contract is "flips only" — re-affirming the state the prober is
    /// already in must stay silent.
    fn flip_to(&mut self, target: Link) -> Option<Link> {
        if self.link == target {
            None
        } else {
            self.link = target;
            Some(target)
        }
    }

    /// `probe_offline_s` while offline, `probe_online_s` while online (spec §3.3).
    pub fn interval(&self, net: &Network) -> Duration {
        match self.link {
            Link::Online => Duration::from_secs(net.probe_online_s),
            Link::Offline => Duration::from_secs(net.probe_offline_s),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::schema::Network;

    const DATE_1: &str = "Sun, 12 Jul 2026 08:30:00 GMT";

    fn outcome(reachable: bool) -> ProbeOutcome {
        ProbeOutcome {
            reachable,
            date_header: None,
        }
    }

    fn outcome_with_date(reachable: bool, date: &str) -> ProbeOutcome {
        ProbeOutcome {
            reachable,
            date_header: Some(date.to_string()),
        }
    }

    fn new_prober() -> Prober {
        Prober::new(TrustedClock::new())
    }

    // ===================================================================================
    // Rule 3 — initial state (the deliberate deviation)
    // ===================================================================================

    #[test]
    fn one_success_from_cold_flips_online_immediately() {
        let mut p = new_prober();
        assert_eq!(p.link(), Link::Offline, "premise: starts Offline");

        let flip = p.record(outcome(true));

        assert_eq!(
            flip,
            Some(Link::Online),
            "the FIRST outcome this process ever sees must decide immediately \
             (rule 3) -- it must NOT wait for a second consecutive success"
        );
        assert_eq!(p.link(), Link::Online);
    }

    #[test]
    fn one_failure_from_cold_stays_offline() {
        let mut p = new_prober();

        let flip = p.record(outcome(false));

        assert_eq!(flip, None, "already Offline, so a failure is not a flip");
        assert_eq!(p.link(), Link::Offline);
    }

    // ===================================================================================
    // Rule 1 — damping
    // ===================================================================================

    #[test]
    fn from_online_a_single_failure_does_not_flip() {
        let mut p = new_prober();
        p.record(outcome(true)); // cold success -> Online
        assert_eq!(p.link(), Link::Online, "premise");

        let flip = p.record(outcome(false));

        assert_eq!(
            flip, None,
            "ONE failure from Online must NOT flip -- this is the damping \
             property a naive (undamped) implementation gets wrong"
        );
        assert_eq!(
            p.link(),
            Link::Online,
            "still Online after a single failure"
        );
    }

    #[test]
    fn from_cold_a_single_success_after_a_failure_does_not_flip() {
        // The mirror of `from_online_a_single_failure_does_not_flip`, on the offline side.
        // A cold failure leaves the link Offline in a `Run { reachable: false, count: 1 }`
        // state; a lone success then only STARTS a fresh success run — it must not flip
        // Online on its own. Two consecutive successes are required to flip up, exactly as
        // two consecutive failures are required to flip down.
        let mut p = new_prober();
        let cold = p.record(outcome(false)); // cold failure
        assert_eq!(
            cold, None,
            "already Offline, so a cold failure is not a flip"
        );
        assert_eq!(
            p.link(),
            Link::Offline,
            "premise: Offline after a cold failure"
        );

        let flip = p.record(outcome(true));

        assert_eq!(
            flip, None,
            "ONE success out of a cold Offline must NOT flip -- it only starts a fresh \
             success run; two consecutive successes are needed to flip Online"
        );
        assert_eq!(
            p.link(),
            Link::Offline,
            "still Offline after a single success"
        );
    }

    #[test]
    fn two_consecutive_failures_flip_offline() {
        let mut p = new_prober();
        p.record(outcome(true)); // -> Online

        let first = p.record(outcome(false));
        assert_eq!(first, None, "first of two -- no flip yet");
        assert_eq!(p.link(), Link::Online);

        let second = p.record(outcome(false));
        assert_eq!(
            second,
            Some(Link::Offline),
            "the SECOND consecutive failure must flip"
        );
        assert_eq!(p.link(), Link::Offline);
    }

    #[test]
    fn two_consecutive_successes_flip_online_from_a_damped_offline_state() {
        // Distinct from the cold-start test above: this Offline is reached via a
        // genuine two-consecutive-failure flip, so what follows exercises the general
        // post-first-outcome "two consecutive successes flip online" rule, not the
        // first-outcome shortcut.
        let mut p = new_prober();
        p.record(outcome(true)); // -> Online
        p.record(outcome(false));
        p.record(outcome(false)); // -> Offline (damped)
        assert_eq!(p.link(), Link::Offline, "premise");

        let first = p.record(outcome(true));
        assert_eq!(first, None, "first of two -- no flip yet");
        assert_eq!(p.link(), Link::Offline);

        let second = p.record(outcome(true));
        assert_eq!(
            second,
            Some(Link::Online),
            "the SECOND consecutive success must flip"
        );
        assert_eq!(p.link(), Link::Online);
    }

    #[test]
    fn a_success_resets_the_failure_run() {
        // fail, succeed, fail -> still Online: the intervening success breaks the run,
        // so the final fail is only the FIRST of a fresh run, not the second of a
        // continuing one.
        let mut p = new_prober();
        p.record(outcome(true)); // -> Online

        p.record(outcome(false)); // run = 1 failure
        p.record(outcome(true)); // run BROKEN -> resets to 1 success
        let last = p.record(outcome(false)); // run = 1 failure again (fresh)

        assert_eq!(
            last, None,
            "the run was broken by the intervening success, so this lone \
             failure must not flip"
        );
        assert_eq!(p.link(), Link::Online, "still Online");
    }

    #[test]
    fn a_failure_resets_the_success_run_the_other_direction() {
        // Symmetric case, starting from a genuinely damped Offline: succeed, fail,
        // succeed -> still Offline.
        let mut p = new_prober();
        p.record(outcome(true));
        p.record(outcome(false));
        p.record(outcome(false)); // -> Offline (damped)
        assert_eq!(p.link(), Link::Offline, "premise");

        p.record(outcome(true)); // run = 1 success
        p.record(outcome(false)); // run BROKEN -> resets to 1 failure
        let last = p.record(outcome(true)); // run = 1 success again (fresh)

        assert_eq!(
            last, None,
            "the run was broken by the intervening failure, so this lone \
             success must not flip"
        );
        assert_eq!(p.link(), Link::Offline, "still Offline");
    }

    #[test]
    fn steady_state_does_not_keep_reemitting_a_flip() {
        let mut p = new_prober();
        p.record(outcome(true)); // -> Online (the one flip)
        for _ in 0..5 {
            assert_eq!(
                p.record(outcome(true)),
                None,
                "already Online: repeated successes are not flips"
            );
        }
        assert_eq!(p.link(), Link::Online);
    }

    // ===================================================================================
    // interval()
    // ===================================================================================

    #[test]
    fn interval_uses_the_offline_value_while_offline() {
        let p = new_prober(); // starts Offline
        let net = Network {
            probe_online_s: 45,
            probe_offline_s: 7,
            ..Network::default()
        };
        assert_eq!(p.interval(&net), Duration::from_secs(7));
    }

    #[test]
    fn interval_uses_the_online_value_while_online() {
        let mut p = new_prober();
        p.record(outcome(true)); // -> Online
        let net = Network {
            probe_online_s: 45,
            probe_offline_s: 7,
            ..Network::default()
        };
        assert_eq!(p.interval(&net), Duration::from_secs(45));
    }

    // ===================================================================================
    // Rule 2 / TEL-01 — Date header harvesting
    // ===================================================================================

    #[test]
    fn a_date_header_on_a_failed_probe_still_reaches_the_clock() {
        let clock = TrustedClock::new();
        let mut p = Prober::new(clock.clone());
        assert_eq!(clock.offset_seconds(), None, "premise");

        p.record(outcome_with_date(false, DATE_1));

        assert!(
            clock.offset_seconds().is_some(),
            "TEL-01: even a FAILING probe's Date header must reach the trusted \
             clock -- this bootstraps the clock on a dead-CMOS device, where most \
             probe traffic is failing by definition"
        );
    }

    #[test]
    fn a_date_header_on_a_successful_probe_also_reaches_the_clock() {
        let clock = TrustedClock::new();
        let mut p = Prober::new(clock.clone());

        p.record(outcome_with_date(true, DATE_1));

        assert!(clock.offset_seconds().is_some());
    }

    #[test]
    fn a_malformed_date_does_not_panic_and_does_not_flip_anything() {
        let clock = TrustedClock::new();
        let mut p = Prober::new(clock.clone());
        p.record(outcome(true)); // -> Online, clock still unset
        assert_eq!(
            clock.offset_seconds(),
            None,
            "premise: no valid Date observed yet"
        );

        // A single failure carrying a garbage Date: must not panic, must not
        // establish the clock, and -- being a LONE failure from Online -- must not
        // flip either.
        let flip = p.record(outcome_with_date(false, "not a date"));

        assert_eq!(
            flip, None,
            "a single failure from Online must not flip, regardless of its Date \
             header"
        );
        assert_eq!(p.link(), Link::Online);
        assert_eq!(
            clock.offset_seconds(),
            None,
            "a malformed Date must never establish an offset"
        );
    }

    #[test]
    fn a_malformed_date_does_not_corrupt_an_already_established_clock() {
        let clock = TrustedClock::new();
        let mut p = Prober::new(clock.clone());
        p.record(outcome_with_date(true, DATE_1));
        let before = clock.offset_seconds();
        assert!(before.is_some(), "premise");

        p.record(outcome_with_date(false, "garbage"));

        assert_eq!(
            clock.offset_seconds(),
            before,
            "a bad header must never move an already-established offset"
        );
    }
}
