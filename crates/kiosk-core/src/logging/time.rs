//! Trusted time (spec TEL-01). A kiosk with a dead CMOS battery on a
//! 443-only LAN cannot reach NTP, and a JWT with a bad `iat`/`exp` is rejected
//! by Google. Every HTTP response carries a mandatory `Date` header, and the
//! prober already makes a request every 10-30 s - so we take the clock from
//! traffic we are already sending. This is a FALLBACK; NTP stays the primary.

use chrono::{DateTime, Utc};
use std::sync::{Arc, Mutex};

/// Beyond this, the JWT `iat`/`exp` are outside Google's tolerance and the
/// caller emits `clock.skew` (WARNING).
pub const SKEW_THRESHOLD_SECONDS: i64 = 30;

#[derive(Debug, thiserror::Error)]
pub enum TimeError {
    #[error("unparseable HTTP Date header: {0}")]
    BadDate(String),
}

/// Offset (in seconds) between server time observed via HTTP `Date` headers
/// and the local clock, harvested from traffic we already send. Cheaply
/// cloneable and safe to share between the prober thread (writer) and the
/// logger thread (reader).
#[derive(Debug, Clone)]
pub struct TrustedClock {
    offset_seconds: Arc<Mutex<Option<i64>>>,
}

impl TrustedClock {
    pub fn new() -> Self {
        Self {
            offset_seconds: Arc::new(Mutex::new(None)),
        }
    }

    /// Parses an HTTP `Date` header and records the offset between the server's
    /// clock and ours.
    ///
    /// We accept ONLY IMF-fixdate (`Sun, 12 Jul 2026 08:30:00 GMT`), which is
    /// what RFC 9110 6.6.1 mandates for a `Date` header - and we check for the
    /// literal, uppercase, case-sensitive `GMT` suffix ourselves BEFORE handing
    /// anything to chrono.
    ///
    /// This strictness is deliberate. `chrono`'s RFC 2822 parser implements the
    /// obsolete-zone grammar, so it will happily accept `... 08:30:00 EST` as a
    /// real -05:00 offset (and `PST` as -08:00, and bare military letters). A
    /// `Date` from a misconfigured reverse proxy, a captive portal, or a hostile
    /// responder would then shift our harvested offset by hours *through the
    /// success path* - silently poisoning every log timestamp and every JWT
    /// `iat`/`exp` while looking perfectly healthy. Falling through to a bare
    /// `parse_from_rfc2822` is exactly that bug, so we do not do it.
    ///
    /// We therefore fail closed: anything that is not IMF-fixdate - an obsolete
    /// zone, a numeric zone, a lowercase `gmt`, a legal-but-obsolete RFC 850 or
    /// asctime date, or junk - is rejected and leaves any established offset
    /// untouched. This is a clock *harvester*, not a general HTTP date parser:
    /// declining to update the clock is always safe, adopting a wrong one is not.
    pub fn observe_http_date(&self, header_value: &str) -> Result<(), TimeError> {
        let bad = || TimeError::BadDate(header_value.to_string());

        // IMF-fixdate always ends in the literal uppercase `GMT`. Only once we
        // have confirmed that do we rewrite it to the numeric zone RFC 2822
        // requires; every other shape is refused outright.
        let prefix = header_value.strip_suffix(" GMT").ok_or_else(bad)?;
        let normalized = format!("{prefix} +0000");

        let server_utc = DateTime::parse_from_rfc2822(&normalized)
            .map_err(|_| bad())?
            .with_timezone(&Utc);

        let local_utc = Utc::now();
        let offset = server_utc.timestamp() - local_utc.timestamp();

        *self.offset_seconds.lock().unwrap() = Some(offset);
        Ok(())
    }

    pub fn offset_seconds(&self) -> Option<i64> {
        *self.offset_seconds.lock().unwrap()
    }

    /// The trusted current UTC instant, or `None` until an offset has been
    /// established. This must never guess.
    pub fn trusted_utc(&self) -> Option<DateTime<Utc>> {
        let offset = self.offset_seconds()?;
        Some(Utc::now() + chrono::Duration::seconds(offset))
    }

    pub fn is_skewed(&self) -> bool {
        self.offset_seconds()
            .map(|offset| offset.abs() > SKEW_THRESHOLD_SECONDS)
            .unwrap_or(false)
    }
}

impl Default for TrustedClock {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A fixed, known-good HTTP-date (RFC 7231 IMF-fixdate).
    const HTTP_DATE: &str = "Sun, 12 Jul 2026 08:30:00 GMT";

    fn expected() -> DateTime<Utc> {
        DateTime::parse_from_rfc2822("Sun, 12 Jul 2026 08:30:00 +0000")
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn no_offset_until_a_date_is_observed() {
        let c = TrustedClock::new();
        assert_eq!(c.offset_seconds(), None);
        assert_eq!(c.trusted_utc(), None, "must not guess before it knows");
        assert!(!c.is_skewed(), "unknown skew is not skewed");
    }

    #[test]
    fn observing_a_date_establishes_the_offset() {
        let c = TrustedClock::new();
        c.observe_http_date(HTTP_DATE).expect("valid HTTP date");
        assert!(c.offset_seconds().is_some());
        let t = c.trusted_utc().expect("clock is established");
        // Trusted time must be within a second or two of the observed instant.
        let delta = (t - expected()).num_seconds().abs();
        assert!(
            delta <= 2,
            "trusted_utc drifted from the observed Date: {delta}s"
        );
    }

    #[test]
    fn a_malformed_date_is_rejected_and_does_not_corrupt_the_offset() {
        let c = TrustedClock::new();
        c.observe_http_date(HTTP_DATE).unwrap();
        let before = c.offset_seconds();
        c.observe_http_date("not a date").expect_err("must reject");
        assert_eq!(
            c.offset_seconds(),
            before,
            "a bad header must never move an established clock"
        );
    }

    #[test]
    fn a_later_observation_replaces_the_earlier_offset() {
        let c = TrustedClock::new();
        c.observe_http_date("Sun, 12 Jul 2026 08:30:00 GMT")
            .unwrap();
        let first = c.offset_seconds().unwrap();
        c.observe_http_date("Sun, 12 Jul 2026 09:30:00 GMT")
            .unwrap();
        let second = c.offset_seconds().unwrap();
        assert_eq!(second - first, 3600, "the newer Date must win");
    }

    #[test]
    fn is_skewed_trips_past_the_jwt_tolerance() {
        let c = TrustedClock::new();
        // Force a large offset by observing a Date far from the local clock.
        c.observe_http_date("Sun, 12 Jul 2020 08:30:00 GMT")
            .unwrap();
        assert!(
            c.is_skewed(),
            "a multi-year offset must be reported as skew (offset {:?})",
            c.offset_seconds()
        );
    }

    /// RFC 2822's grammar accepts obsolete zone abbreviations (`EST` => -05:00,
    /// `PST` => -08:00, and bare military letters). An HTTP `Date` is always
    /// IMF-fixdate (RFC 9110 6.6.1) and therefore always ends in `GMT`, so any
    /// other zone means the header is not a trustworthy HTTP-date. Accepting one
    /// would silently shift the harvested offset by hours through the SUCCESS
    /// path -- poisoning every log line and JWT while looking healthy. Rejecting
    /// is always safe: we simply keep whatever offset we already had.
    #[test]
    fn obsolete_rfc2822_zones_are_rejected_not_silently_accepted() {
        for bad in [
            "Sun, 12 Jul 2026 08:30:00 EST",
            "Sun, 12 Jul 2026 08:30:00 PST",
            "Sun, 12 Jul 2026 08:30:00 CST",
            "Sun, 12 Jul 2026 08:30:00 A", // bare military-letter zone
        ] {
            let c = TrustedClock::new();
            c.observe_http_date(HTTP_DATE).unwrap();
            let before = c.offset_seconds();
            assert!(
                c.observe_http_date(bad).is_err(),
                "{bad} must be rejected, not silently accepted as an offset"
            );
            assert_eq!(
                c.offset_seconds(),
                before,
                "{bad} must not move an established clock"
            );
        }
    }

    #[test]
    fn a_numeric_zone_is_rejected_because_it_is_not_imf_fixdate() {
        let c = TrustedClock::new();
        c.observe_http_date("Sun, 12 Jul 2026 08:30:00 +0500")
            .expect_err("a numeric zone is not a legal HTTP Date");
        assert_eq!(c.offset_seconds(), None);
    }

    #[test]
    fn a_lowercase_gmt_is_rejected() {
        let c = TrustedClock::new();
        c.observe_http_date("Sun, 12 Jul 2026 08:30:00 gmt")
            .expect_err("RFC 9110 mandates the uppercase literal GMT");
        assert_eq!(c.offset_seconds(), None);
    }

    #[test]
    fn clock_is_shareable_across_threads() {
        let c = TrustedClock::new();
        let c2 = c.clone();
        std::thread::spawn(move || c2.observe_http_date(HTTP_DATE).unwrap())
            .join()
            .unwrap();
        assert!(
            c.offset_seconds().is_some(),
            "an observation on the prober thread must be visible to the logger thread"
        );
    }
}
