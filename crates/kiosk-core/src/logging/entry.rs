//! LogEntry construction (spec TEL-02/03/04/08).

use crate::config::schema::UrlDetail;
use crate::logging::event::{Event, Severity};
use crate::logging::time::TrustedClock;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Debug, Clone, PartialEq)]
pub struct EntryContext {
    pub project_id: String,
    pub device_id: String,
    pub site: String,
    /// Defaults to `site` when `[kiosk] region` is unset (spec TEL-04).
    pub region: String,
    pub app_version: String,
    pub config_revision: Option<i64>,
    pub url_detail: UrlDetail,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResourceLabels {
    pub project_id: String,
    pub node_id: String,
    pub namespace: String,
    pub location: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Resource {
    /// Always `generic_node` (spec TEL-04).
    pub r#type: String,
    pub labels: ResourceLabels,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LogEntry {
    pub log_name: String,
    pub resource: Resource,
    pub labels: Map<String, Value>,
    pub severity: Severity,
    /// `None` when trusted time was not established at creation (spec TEL-02).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
    /// Cloud Logging's ONLY dedup key. Assigned once, reused verbatim on retry.
    pub insert_id: String,
    pub json_payload: Map<String, Value>,
}

impl LogEntry {
    pub fn new(
        event: Event,
        ctx: &EntryContext,
        seq: u64,
        clock: &TrustedClock,
        mut fields: Map<String, Value>,
    ) -> LogEntry {
        let mut labels = Map::new();
        labels.insert("app_version".into(), Value::from(ctx.app_version.clone()));
        labels.insert(
            "config_revision".into(),
            match ctx.config_revision {
                Some(r) => Value::from(r.to_string()),
                None => Value::from(""),
            },
        );
        labels.insert("device_id".into(), Value::from(ctx.device_id.clone()));
        labels.insert("site".into(), Value::from(ctx.site.clone()));

        fields.insert("event".into(), Value::from(event.name()));
        // The raw device clock is preserved even when it is wrong — it is the
        // evidence that tells an operator the device's clock is broken.
        fields.insert(
            "device_ts_raw".into(),
            Value::from(chrono::Utc::now().to_rfc3339()),
        );

        LogEntry {
            log_name: format!("projects/{}/logs/kiosk", ctx.project_id),
            resource: Resource {
                r#type: "generic_node".into(),
                labels: ResourceLabels {
                    project_id: ctx.project_id.clone(),
                    node_id: ctx.device_id.clone(),
                    namespace: ctx.site.clone(),
                    location: ctx.region.clone(),
                },
            },
            labels,
            severity: event.severity(),
            timestamp: clock.trusted_utc().map(|t| t.to_rfc3339()),
            insert_id: format!("{}-{}", ctx.device_id, seq),
            json_payload: fields,
        }
    }
}

/// Reduce a URL for logging and return `(redacted, url_sha256_8)` (spec TEL-08).
/// The query string is where tokens and PII live, and `nav.blocked` fires exactly
/// when a URL is most likely to carry one — so strip by default, hash for correlation.
pub fn redact_url(raw: &str, detail: UrlDetail) -> (String, String) {
    use sha2::{Digest, Sha256};

    let hash = {
        let mut h = Sha256::new();
        h.update(raw.as_bytes());
        let digest = h.finalize();
        hex8(&digest)
    };

    let redacted = match url::Url::parse(raw) {
        Err(_) => "<unparseable>".to_string(),
        Ok(u) => match detail {
            UrlDetail::Full => raw.to_string(),
            UrlDetail::Host => match u.host_str() {
                Some(h) => format!("{}://{}", u.scheme(), h),
                None => "<unparseable>".to_string(),
            },
            UrlDetail::Path => match u.host_str() {
                Some(h) => format!("{}://{}{}", u.scheme(), h, u.path()),
                None => "<unparseable>".to_string(),
            },
        },
    };

    (redacted, hash)
}

fn hex8(bytes: &[u8]) -> String {
    bytes
        .iter()
        .take(4)
        .map(|b| format!("{b:02x}"))
        .collect::<String>()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> EntryContext {
        EntryContext {
            project_id: "proj".into(),
            device_id: "lobby-01".into(),
            site: "hq".into(),
            region: "asia-southeast1".into(),
            app_version: "0.1.0+abc1234".into(),
            config_revision: Some(42),
            url_detail: UrlDetail::Path,
        }
    }

    fn established_clock() -> TrustedClock {
        let c = TrustedClock::new();
        c.observe_http_date("Sun, 12 Jul 2026 08:30:00 GMT")
            .unwrap();
        c
    }

    #[test]
    fn entry_carries_the_generic_node_resource_with_schema_keys_only() {
        let e = LogEntry::new(Event::AppStart, &ctx(), 1, &established_clock(), Map::new());
        assert_eq!(e.log_name, "projects/proj/logs/kiosk");
        assert_eq!(e.resource.r#type, "generic_node");
        assert_eq!(e.resource.labels.project_id, "proj");
        assert_eq!(e.resource.labels.node_id, "lobby-01");
        assert_eq!(e.resource.labels.namespace, "hq");
        assert_eq!(e.resource.labels.location, "asia-southeast1");
    }

    #[test]
    fn non_schema_identity_goes_in_entry_labels() {
        let e = LogEntry::new(Event::AppStart, &ctx(), 1, &established_clock(), Map::new());
        assert_eq!(e.labels.get("app_version").unwrap(), "0.1.0+abc1234");
        assert_eq!(e.labels.get("config_revision").unwrap(), "42");
        assert_eq!(e.labels.get("device_id").unwrap(), "lobby-01");
        assert_eq!(e.labels.get("site").unwrap(), "hq");
    }

    #[test]
    fn insert_id_is_device_id_and_seq() {
        let e = LogEntry::new(Event::AppStart, &ctx(), 7, &established_clock(), Map::new());
        assert_eq!(e.insert_id, "lobby-01-7");
    }

    #[test]
    fn severity_and_event_name_come_from_the_taxonomy() {
        let e = LogEntry::new(
            Event::WatchdogSafeMode,
            &ctx(),
            1,
            &established_clock(),
            Map::new(),
        );
        assert_eq!(e.severity, Severity::Critical);
        assert_eq!(e.json_payload.get("event").unwrap(), "watchdog.safe_mode");
    }

    #[test]
    fn timestamp_is_omitted_when_trusted_time_is_not_established() {
        // spec TEL-02: do not guess. Logging assigns receive time instead.
        let e = LogEntry::new(Event::AppStart, &ctx(), 1, &TrustedClock::new(), Map::new());
        assert_eq!(e.timestamp, None);
    }

    #[test]
    fn timestamp_is_rfc3339_when_trusted_time_is_established() {
        let e = LogEntry::new(Event::AppStart, &ctx(), 1, &established_clock(), Map::new());
        let ts = e.timestamp.expect("established clock => timestamp");
        chrono::DateTime::parse_from_rfc3339(&ts).expect("must be RFC3339");
    }

    #[test]
    fn raw_device_clock_is_preserved_for_forensics() {
        let e = LogEntry::new(Event::AppStart, &ctx(), 1, &established_clock(), Map::new());
        let raw = e
            .json_payload
            .get("device_ts_raw")
            .unwrap()
            .as_str()
            .unwrap();
        chrono::DateTime::parse_from_rfc3339(raw).expect("device_ts_raw must be RFC3339");
    }

    #[test]
    fn custom_fields_land_in_the_json_payload() {
        let mut f = Map::new();
        f.insert("exit_code".into(), Value::from(86));
        let e = LogEntry::new(Event::WatchdogRestart, &ctx(), 1, &established_clock(), f);
        assert_eq!(e.json_payload.get("exit_code").unwrap(), 86);
    }

    #[test]
    fn entry_round_trips_through_json_for_the_spool() {
        let e = LogEntry::new(Event::AppStart, &ctx(), 3, &established_clock(), Map::new());
        let text = serde_json::to_string(&e).unwrap();
        let back: LogEntry = serde_json::from_str(&text).unwrap();
        assert_eq!(
            back.insert_id, e.insert_id,
            "insertId MUST survive the spool byte-identically (TEL-03)"
        );
        assert_eq!(back.severity, e.severity);
        assert_eq!(back.timestamp, e.timestamp);
    }

    // --- URL redaction (TEL-08) ---

    #[test]
    fn path_detail_strips_query_and_fragment() {
        let (red, hash) = redact_url(
            "https://app.example.com/k/page?token=SECRET&id=9#frag",
            UrlDetail::Path,
        );
        assert_eq!(red, "https://app.example.com/k/page");
        assert!(
            !red.contains("SECRET"),
            "the token must never reach the log"
        );
        assert_eq!(hash.len(), 8);
    }

    #[test]
    fn host_detail_keeps_only_scheme_and_host() {
        let (red, _) = redact_url("https://app.example.com/k/page?x=1", UrlDetail::Host);
        assert_eq!(red, "https://app.example.com");
    }

    #[test]
    fn full_detail_keeps_everything() {
        let raw = "https://app.example.com/k?x=1#f";
        let (red, _) = redact_url(raw, UrlDetail::Full);
        assert_eq!(red, raw);
    }

    #[test]
    fn distinct_urls_get_distinct_hashes_even_when_redacted_identically() {
        let (a_red, a_hash) = redact_url("https://x.test/p?token=A", UrlDetail::Path);
        let (b_red, b_hash) = redact_url("https://x.test/p?token=B", UrlDetail::Path);
        assert_eq!(a_red, b_red, "redaction hides the difference...");
        assert_ne!(
            a_hash, b_hash,
            "...but the hash must still distinguish them"
        );
    }

    #[test]
    fn an_unparseable_url_is_not_logged_verbatim_and_does_not_panic() {
        let (red, hash) = redact_url("::: not a url :::", UrlDetail::Path);
        assert_eq!(red, "<unparseable>");
        assert_eq!(hash.len(), 8);
    }
}
