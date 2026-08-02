//! Nightly maintenance scheduling helpers.

use chrono::{DateTime, Duration, Local, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;

/// The next UTC instant at which local wall-clock `hhmm` ("HH:MM") occurs strictly after
/// `now`, in IANA `tz` (or system local when `None`). `None` on unparseable input.
/// DST-safe: resolves the local time through the zone, taking the earliest valid instant
/// (a spring-forward gap rolls to the next day; fall-back ambiguity takes the earlier).
pub fn next_fire(hhmm: &str, tz: Option<&str>, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    let (h, m) = hhmm.split_once(':')?;
    let time = NaiveTime::from_hms_opt(h.parse().ok()?, m.parse().ok()?, 0)?;
    match tz {
        Some(name) => next_in_zone(time, name.parse::<Tz>().ok()?, now),
        None => next_in_zone(time, Local, now),
    }
}

/// Search up to a few days forward (covers a spring-forward skipped hour) for the earliest
/// valid local instant of `time`, in `zone`, that is strictly after `now`.
fn next_in_zone<Z: TimeZone>(
    time: NaiveTime,
    zone: Z,
    now: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    let local_now = now.with_timezone(&zone);
    for add in 0..4 {
        let date = local_now.date_naive() + Duration::days(add);
        let naive = date.and_time(time);
        // earliest valid instant for this local wall-clock in this zone
        if let Some(dt) = zone.from_local_datetime(&naive).earliest() {
            let as_utc = dt.with_timezone(&Utc);
            if as_utc > now {
                return Some(as_utc);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utc(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    #[test]
    fn rolls_to_today_when_hhmm_is_still_ahead() {
        // 03:00 UTC now, fire 04:00 UTC → today 04:00.
        let n = next_fire("04:00", Some("UTC"), utc("2026-07-01T03:00:00Z")).unwrap();
        assert_eq!(n, utc("2026-07-01T04:00:00Z"));
    }
    #[test]
    fn rolls_to_tomorrow_when_hhmm_already_passed() {
        let n = next_fire("04:00", Some("UTC"), utc("2026-07-01T05:00:00Z")).unwrap();
        assert_eq!(n, utc("2026-07-02T04:00:00Z"));
    }
    #[test]
    fn applies_the_iana_zone_offset() {
        // 04:00 in Asia/Jakarta (UTC+7) = 21:00 UTC the previous day.
        let n = next_fire("04:00", Some("Asia/Jakarta"), utc("2026-07-01T00:00:00Z")).unwrap();
        assert_eq!(n, utc("2026-07-01T21:00:00Z")); // 2026-07-02 04:00 +07:00
    }
    #[test]
    fn strictly_future_and_dst_spring_forward_resolves() {
        // US/Eastern spring-forward 2026-03-08: 02:30 does not exist locally; must still return
        // a valid strictly-future instant, not panic or None.
        let now = utc("2026-03-08T06:00:00Z"); // 01:00 EST
        let n = next_fire("02:30", Some("America/New_York"), now).unwrap();
        assert!(n > now, "always strictly future");
    }
    #[test]
    fn bad_input_is_none_not_panic() {
        assert!(next_fire("nope", Some("UTC"), utc("2026-07-01T00:00:00Z")).is_none());
        assert!(next_fire("04:00", Some("Not/AZone"), utc("2026-07-01T00:00:00Z")).is_none());
        assert!(next_fire("25:00", Some("UTC"), utc("2026-07-01T00:00:00Z")).is_none());
    }
}
