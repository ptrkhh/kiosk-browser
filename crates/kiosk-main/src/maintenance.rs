//! Nightly-reload timer (spec `maintenance.nightly_reload`): reloads the site once a
//! day at a local wall-clock "HH:MM" so long-running page state resets. All the
//! DST/timezone math lives in `kiosk_core::maintenance::next_fire`; this is just the
//! loop that sleeps until the next fire and calls back.

use chrono::Utc;
use kiosk_core::maintenance::next_fire;
use tokio_util::sync::CancellationToken;

/// Runs until `cancel` fires or `hhmm` is `None`/unparseable.
///
/// `reload` is called once per fire, on the app's own schedule — never twice for the
/// same calendar day, because `next_fire` always returns a strictly-future instant and
/// this loop recomputes it fresh after every fire (no day-tracking state needed on top).
///
/// `warn_once` is called exactly once, only if `hhmm` is non-empty but `next_fire`
/// can't parse it (bad "HH:MM" or unknown IANA zone) — the caller is expected to emit
/// `config.warn{field:"maintenance.nightly_reload"}` from it. `hhmm: None` means the
/// feature is off and returns immediately without calling either closure.
// ponytail: a plain loop over next_fire; no cron lib for a single daily reload.
pub async fn run(
    hhmm: Option<String>,
    tz: Option<String>,
    reload: impl Fn() + Send,
    warn_once: impl Fn() + Send,
    cancel: CancellationToken,
) {
    let Some(hhmm) = hhmm else { return }; // None = off
    loop {
        let now = Utc::now();
        let Some(fire) = next_fire(&hhmm, tz.as_deref(), now) else {
            warn_once();
            return;
        };
        let dur = (fire - now).to_std().unwrap_or_default();
        tokio::select! {
            _ = cancel.cancelled() => return,
            _ = tokio::time::sleep(dur) => reload(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[tokio::test]
    async fn none_returns_immediately_without_reloading() {
        let reloads = Arc::new(AtomicUsize::new(0));
        let warns = Arc::new(AtomicUsize::new(0));
        let (r, w) = (reloads.clone(), warns.clone());
        run(
            None,
            None,
            move || {
                r.fetch_add(1, Ordering::SeqCst);
            },
            move || {
                w.fetch_add(1, Ordering::SeqCst);
            },
            CancellationToken::new(),
        )
        .await;
        assert_eq!(reloads.load(Ordering::SeqCst), 0);
        assert_eq!(warns.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn cancel_before_fire_returns_without_reloading() {
        let reloads = Arc::new(AtomicUsize::new(0));
        let r = reloads.clone();
        let cancel = CancellationToken::new();
        cancel.cancel(); // already cancelled: the select must take the cancel branch
        run(
            Some("04:00".to_string()),
            Some("UTC".to_string()),
            move || {
                r.fetch_add(1, Ordering::SeqCst);
            },
            || panic!("warn_once must not fire for valid input"),
            cancel,
        )
        .await;
        assert_eq!(reloads.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn unparseable_hhmm_warns_once_and_returns_without_reloading() {
        let reloads = Arc::new(AtomicUsize::new(0));
        let warns = Arc::new(AtomicUsize::new(0));
        let (r, w) = (reloads.clone(), warns.clone());
        run(
            Some("nope".to_string()),
            None,
            move || {
                r.fetch_add(1, Ordering::SeqCst);
            },
            move || {
                w.fetch_add(1, Ordering::SeqCst);
            },
            CancellationToken::new(),
        )
        .await;
        assert_eq!(reloads.load(Ordering::SeqCst), 0);
        assert_eq!(warns.load(Ordering::SeqCst), 1);
    }
}
