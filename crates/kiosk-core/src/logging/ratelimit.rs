//! Per-event rate limiting with coalesced summaries (spec TEL-09).
//!
//! A redirect loop or a crash loop is *signal* — but at full volume it is a
//! firehose that costs money and buries the diagnosis in noise. Each capped
//! event type gets a token bucket (capacity = burst, refill = per-minute/60
//! tokens/sec). Once the bucket is empty the event is suppressed, but the
//! suppression is counted, and `take_summaries` hands back exactly one
//! `(Event, suppressed_count)` pair per event type so the caller can emit a
//! single coalesced entry per window. The loop stays visible — just bounded.
//!
//! Every other event (in particular `watchdog.safe_mode`) is uncapped: it is
//! low-volume by construction, and capping it would hide the very thing you
//! need to see when a device is dying.
//!
//! ## Deviation from the plan: `Instant`, not `TrustedClock`, drives refill
//!
//! The plan's sketch suggested refilling buckets from `TrustedClock`. That is
//! wrong for a rate limiter and this implementation does not do it:
//!
//! - `TrustedClock::trusted_utc()` returns `None` until an HTTP `Date` header
//!   has been observed — i.e. during early boot, which is exactly when a
//!   crash loop is most likely to happen. A limiter that can't run until the
//!   clock is established isn't a limiter during the window it matters most.
//! - `TrustedClock`'s offset can jump — forward *or backward* — the moment a
//!   new `Date` is observed (see `time.rs`: "a later observation replaces the
//!   earlier offset", and offsets can differ by hours). A backward jump would
//!   make `now - last_refill` negative, stalling or corrupting refill; a
//!   forward jump would instantly over-refill the bucket, defeating the cap.
//!
//! `std::time::Instant` is monotonic (never runs backward) and available
//! immediately at process start with no external dependency. It is the
//! correct clock for a refill *schedule*. `TrustedClock` remains exactly
//! what it's for: stamping wall-clock timestamps on emitted entries. This
//! module accepts a `TrustedClock` (per the task's interface, for future use
//! by callers that want to correlate rate-limiter state with wall time) but
//! deliberately does not use it for any timing decision.

use std::time::Instant;

use crate::logging::event::Event;
use crate::logging::time::TrustedClock;

/// The verdict for a single admitted event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admit {
    Allow,
    Suppress,
}

/// `(event, per_minute, burst)` caps, verbatim from spec TEL-09. Every event
/// not listed here is uncapped.
pub fn caps() -> &'static [(Event, u32, u32)] {
    &[
        (Event::NavBlocked, 10, 20),
        (Event::NavError, 10, 20),
        (Event::WebviewCrash, 6, 6),
    ]
}

struct Bucket {
    event: Event,
    capacity: f64,
    refill_per_sec: f64,
    tokens: f64,
    last_refill: Instant,
    suppressed: u64,
}

impl Bucket {
    fn new(event: Event, per_minute: u32, burst: u32) -> Self {
        Self {
            event,
            capacity: burst as f64,
            refill_per_sec: per_minute as f64 / 60.0,
            tokens: burst as f64,
            last_refill: Instant::now(),
            suppressed: 0,
        }
    }

    fn refill(&mut self, now: Instant) {
        let elapsed = now
            .saturating_duration_since(self.last_refill)
            .as_secs_f64();
        self.tokens = (self.tokens + elapsed * self.refill_per_sec).min(self.capacity);
        self.last_refill = now;
    }

    fn admit(&mut self) -> Admit {
        self.refill(Instant::now());
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            Admit::Allow
        } else {
            self.suppressed += 1;
            Admit::Suppress
        }
    }
}

/// Caps each event type with a token bucket and coalesces overflow into a
/// single suppressed-count summary per window (spec TEL-09).
pub struct RateLimiter {
    clock: TrustedClock,
    buckets: Vec<Bucket>,
}

impl RateLimiter {
    pub fn new(clock: TrustedClock) -> Self {
        let buckets = caps()
            .iter()
            .map(|(event, per_minute, burst)| Bucket::new(*event, *per_minute, *burst))
            .collect();
        Self { clock, buckets }
    }

    /// The `TrustedClock` this limiter was constructed with. Kept for callers
    /// that want to stamp wall-clock time alongside rate-limiter decisions;
    /// deliberately unused for the refill schedule itself (see module docs).
    pub fn clock(&self) -> &TrustedClock {
        &self.clock
    }

    pub fn admit(&mut self, event: Event) -> Admit {
        match self.buckets.iter_mut().find(|b| b.event == event) {
            Some(bucket) => bucket.admit(),
            None => Admit::Allow,
        }
    }

    /// Drains and returns the `(event, suppressed_count)` pairs for every
    /// bucket that suppressed at least one event since the last call.
    pub fn take_summaries(&mut self) -> Vec<(Event, u64)> {
        self.buckets
            .iter_mut()
            .filter(|b| b.suppressed > 0)
            .map(|b| {
                let count = b.suppressed;
                b.suppressed = 0;
                (b.event, count)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logging::event::Event;
    use crate::logging::time::TrustedClock;

    fn limiter() -> RateLimiter {
        let c = TrustedClock::new();
        c.observe_http_date("Sun, 12 Jul 2026 08:30:00 GMT")
            .unwrap();
        RateLimiter::new(c)
    }

    #[test]
    fn an_uncapped_event_is_always_allowed() {
        let mut r = limiter();
        // Capping a CRITICAL would hide the thing you most need to see.
        for _ in 0..1000 {
            assert!(matches!(r.admit(Event::WatchdogSafeMode), Admit::Allow));
        }
    }

    #[test]
    fn a_capped_event_is_allowed_up_to_its_burst_then_suppressed() {
        let mut r = limiter();
        let mut allowed = 0;
        for _ in 0..100 {
            if matches!(r.admit(Event::NavBlocked), Admit::Allow) {
                allowed += 1;
            }
        }
        assert_eq!(allowed, 20, "nav.blocked burst is 20 (spec TEL-09)");
    }

    #[test]
    fn suppressed_events_are_counted_and_surface_as_a_summary() {
        // The loop must remain VISIBLE — bounded, not hidden.
        let mut r = limiter();
        for _ in 0..100 {
            r.admit(Event::NavBlocked);
        }
        let summaries = r.take_summaries();
        let (event, count) = summaries
            .iter()
            .find(|(e, _)| *e == Event::NavBlocked)
            .expect("a suppressed event must produce a summary");
        assert_eq!(*event, Event::NavBlocked);
        assert_eq!(*count, 80, "100 attempts - 20 admitted = 80 suppressed");
    }

    #[test]
    fn taking_summaries_clears_them() {
        let mut r = limiter();
        for _ in 0..100 {
            r.admit(Event::NavBlocked);
        }
        assert!(!r.take_summaries().is_empty());
        assert!(
            r.take_summaries().is_empty(),
            "summaries are drained, not repeated"
        );
    }

    #[test]
    fn caps_match_the_spec() {
        let c = caps();
        let nav = c.iter().find(|(e, _, _)| *e == Event::NavBlocked).unwrap();
        assert_eq!((nav.1, nav.2), (10, 20));
        let crash = c
            .iter()
            .find(|(e, _, _)| *e == Event::WebviewCrash)
            .unwrap();
        assert_eq!(crash.1, 6);
    }
}
