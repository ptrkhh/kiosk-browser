//! Connectivity-probe task (spec §3.3, arch-13, P1-D2a Task 4): the timer loop that GETs
//! the resolved probe URL, feeds the outcome to kiosk-core's damped `Prober`, and — only
//! on a flip — emits both the matching telemetry call and the FSM event. The damping
//! decision itself (how many consecutive outcomes flip the link, the cold-start
//! shortcut) lives entirely in `kiosk_core::net::prober::Prober`; this module never
//! reimplements it, and it never hand-parses the `Date` header — `Prober::record`
//! harvests it into its own `TrustedClock` internally (TEL-01).
//!
//! Not yet wired into `main.rs` — Task 6 constructs the real `Prober`/`Network`/resolved
//! probe URL from a `Booted` and spawns [`run`]. Until then this module's public surface
//! has no caller outside its own tests (mirrors `driver.rs`/`boot.rs`/`telemetry.rs`/
//! `fetch.rs`'s Task 1/2/5/3 note).
#![allow(dead_code)]

use kiosk_core::app::state::Event as AppEvent;
use kiosk_core::config::schema::Network;
use kiosk_core::net::prober::{Link, ProbeOutcome, Prober};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// Feed one probe outcome to the damped prober; on a flip, produce the FSM event.
/// `Prober::record` harvests `outcome.date_header` into its own `TrustedClock`
/// internally (on failing probes exactly as on succeeding ones) — this function never
/// hand-parses it.
pub fn on_outcome(prober: &mut Prober, outcome: ProbeOutcome) -> Option<AppEvent> {
    prober.record(outcome).map(AppEvent::LinkChanged)
}

/// GETs `url` once. Any client-build or send failure collapses to an unreachable
/// outcome — the caller (the damped `Prober`) only needs "reachable or not", never why.
pub async fn probe_once(url: &str) -> ProbeOutcome {
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(5))
        .timeout(std::time::Duration::from_secs(10))
        .build();
    match client {
        Err(_) => ProbeOutcome {
            reachable: false,
            date_header: None,
        },
        Ok(client) => match client.get(url).send().await {
            Ok(resp) => {
                let date_header = resp
                    .headers()
                    .get("date")
                    .and_then(|v| v.to_str().ok())
                    .map(str::to_string);
                ProbeOutcome {
                    reachable: resp.status().is_success(),
                    date_header,
                }
            }
            Err(_) => ProbeOutcome {
                reachable: false,
                date_header: None,
            },
        },
    }
}

/// The probe loop: re-reads `prober.interval(&network)` every iteration (30 s online /
/// 10 s offline, spec §3.3) so a flip's new interval takes effect on the very next
/// sleep, GETs `probe_url`, and on a damped flip emits BOTH the matching
/// `net.online`/`net.offline` telemetry event and the FSM event.
///
/// `probe_url` is resolved via `kiosk_core::net::reach::resolve_probe_url` (arch-13:
/// unset check-url + private content host → probe the content origin instead) BEFORE
/// this task is spawned — a one-time decision keyed off the operator's config, not
/// something re-resolved probe to probe.
pub async fn run(
    mut prober: Prober,
    network: Network,
    probe_url: String,
    tx: mpsc::Sender<AppEvent>,
    telem: crate::telemetry::Telemetry,
    cancel: CancellationToken,
) {
    loop {
        let wait = prober.interval(&network);
        tokio::select! {
            _ = cancel.cancelled() => break,
            _ = tokio::time::sleep(wait) => {}
        }
        if let Some(ev) = on_outcome(&mut prober, probe_once(&probe_url).await) {
            match ev {
                AppEvent::LinkChanged(Link::Online) => telem.net_online(),
                AppEvent::LinkChanged(Link::Offline) => telem.net_offline(),
                _ => {}
            }
            let _ = tx.send(ev).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kiosk_core::logging::time::TrustedClock;

    fn outcome(ok: bool) -> ProbeOutcome {
        ProbeOutcome {
            reachable: ok,
            date_header: None,
        }
    }

    // Sequence verified directly against the REAL `Prober::record` (crates/kiosk-core/src/
    // net/prober.rs), not assumed: starting cold (`Damping::Unknown`, `link() == Offline`),
    // 1) a failure is the process's first-ever outcome, so `Damping::Unknown`'s cold-start
    //    rule decides `link` immediately -- target_for(false) == Offline, which already
    //    equals the starting link, so `flip_to` reports no flip (`None`), and the run
    //    becomes `Run { reachable: false, count: 1 }`.
    // 2) a success is the OPPOSITE of the running `false`-run, so it hits the "run broken"
    //    arm: resets to `Run { reachable: true, count: 1 }`, still `None` (only the first
    //    of a fresh success run).
    // 3) a second consecutive success matches the run, bumps count to 2, and *that* is what
    //    flips -- `flip_to(Online)` sees `link() == Offline != Online` and returns
    //    `Some(Online)`. Exactly one `LinkChanged(Online)`, on the third call.
    #[test]
    fn a_damped_flip_yields_a_linkchanged_event() {
        let mut p = Prober::new(TrustedClock::new());
        let first = on_outcome(&mut p, outcome(false));
        assert!(
            first.is_none(),
            "cold-start failure: no flip, Offline is already the start"
        );

        let second = on_outcome(&mut p, outcome(true));
        assert!(
            second.is_none(),
            "first success of a fresh run: no flip yet"
        );

        let third = on_outcome(&mut p, outcome(true));
        assert!(
            matches!(third, Some(AppEvent::LinkChanged(Link::Online))),
            "second consecutive success must damp-flip to Online: got {third:?}"
        );
    }

    #[test]
    fn a_non_flip_yields_no_event() {
        let mut p = Prober::new(TrustedClock::new());
        // Cold-start success flips Online immediately (rule 3) -- consumed, not asserted.
        let _ = on_outcome(&mut p, outcome(true));
        assert!(
            on_outcome(&mut p, outcome(true)).is_none(),
            "steady state (repeated success while already Online) emits nothing"
        );
    }
}
