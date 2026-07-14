//! The Cloud Logging `entries:write` client (spec §6, TEL-01/02/05).
//!
//! Three properties are load-bearing here.
//!
//! 1. **A 401 forces exactly ONE refresh-and-retry** (TEL-05). A credential that
//!    401s persistently must surface as an `Err` the caller backs off on, not a
//!    retry loop that hammers the endpoint.
//! 2. **The `Date` header of EVERY response feeds the trusted clock** (TEL-01) -
//!    success and failure alike. On a dead-CMOS kiosk the very first response we
//!    ever get is likely a failure, and its `Date` is what bootstraps the clock.
//!    It is harvested through [`TrustedClock::observe_http_date`], which is a
//!    deliberately strict fail-closed IMF-fixdate gate; never parse the header
//!    here.
//! 3. **Timestamps are clamped before sending** (TEL-02). Cloud Logging rejects
//!    an ENTIRE batch containing any timestamp more than 24 h in the future, so
//!    one wildly-fast device clock would otherwise poison every batch forever
//!    and lose all telemetry permanently.
//!
//! On `partialSuccess`: Google documents that when any entry fails, "the response
//! status is the response status of one of the failed entries" - i.e. a partial
//! failure is a NON-2xx response carrying a `WriteLogEntriesPartialErrors` detail
//! keyed by the entries' zero-based index. So it is *not* possible for the API to
//! tell us "all good" while having dropped entries. We treat any non-2xx as an
//! `Err` and, when the body carries the partial-errors detail, we name the failed
//! indices in the error so the caller can see them rather than silently losing
//! them. Retrying the whole batch is safe: `insertId` is Cloud Logging's dedup
//! key, so the entries that DID land are deduplicated on the retry (TEL-03).

use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};

use crate::logging::auth::{AuthError, Secret, TokenSource};
use crate::logging::entry::LogEntry;
use crate::logging::time::TrustedClock;
use crate::logging::transport::{HttpResponse, Transport, TransportError};

pub const ENTRIES_WRITE_URL: &str = "https://logging.googleapis.com/v2/entries:write";

/// Cloud Logging's default bucket retention. Entries older than this are silently
/// discarded server-side, so the clamp floor is derived from it (TEL-02).
pub const RETENTION_DAYS: i64 = 30;

/// The 401 path is allowed exactly one extra attempt: mint-invalidate-retry.
/// Two attempts total, and never more - a persistently-401ing credential must
/// error out, not spin.
const MAX_ATTEMPTS: usize = 2;

/// Google's error bodies are unbounded and end up in a logged error string.
const MAX_ERROR_BODY_BYTES: usize = 512;

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    /// No answer at all (DNS/TCP/TLS/timeout). Spool and back off.
    #[error("entries:write unreachable: {0}")]
    Transport(#[from] TransportError),
    /// Could not obtain a bearer token. Emits `token.error` semantics.
    #[error("could not obtain an access token: {0}")]
    Auth(#[from] AuthError),
    /// Still 401 after one forced refresh-and-retry. The credential is bad;
    /// retrying in a loop would achieve nothing.
    #[error("entries:write still returned 401 after one forced token refresh")]
    Unauthorized,
    /// `partialSuccess` reported per-entry failures. The named entries were NOT
    /// written. Surfaced rather than swallowed: the caller must not commit a
    /// batch off the spool on the strength of a response that rejected some of it.
    #[error("entries:write rejected {} of the entries (indices {failed_indices:?}): HTTP {status}: {body}", failed_indices.len())]
    PartialFailure {
        status: u16,
        failed_indices: Vec<usize>,
        body: String,
    },
    /// Any other non-2xx (429, 5xx, an un-detailed 4xx). Spool and back off.
    #[error("entries:write returned HTTP {status}: {body}")]
    Http { status: u16, body: String },
}

fn truncate_for_error(body: &str) -> String {
    if body.len() <= MAX_ERROR_BODY_BYTES {
        return body.to_string();
    }
    let mut end = MAX_ERROR_BODY_BYTES;
    while end > 0 && !body.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}... [{} bytes truncated]", &body[..end], body.len() - end)
}

/// Prepares a response body for inclusion in an error string: strip the bearer
/// token, THEN truncate.
///
/// The token strip is not theoretical. A misconfigured proxy, a captive portal,
/// or a `logging.googleapis.com` that some middlebox has redirected can echo the
/// request - headers and all - back in its error body. That body goes verbatim
/// into a `ClientError`, and a `ClientError` gets logged. `Secret` protects the
/// token everywhere it is *held*; this protects it where it comes *back to us*
/// from the network, which no amount of `Debug` redaction on our own types can
/// cover.
///
/// Scrub before truncating, so a token sitting across the truncation boundary is
/// still removed rather than half-printed.
fn sanitize_for_error(body: &str, token: &Secret) -> String {
    let scrubbed = body.replace(token.expose(), "<redacted-bearer-token>");
    truncate_for_error(&scrubbed)
}

/// Pulls the zero-based indices out of a `WriteLogEntriesPartialErrors` detail,
/// if the body carries one. Anything unexpected yields `None` - a body we cannot
/// read is just an ordinary HTTP failure, which is still an `Err`, so failing to
/// parse can never turn into silent data loss.
fn partial_error_indices(body: &str) -> Option<Vec<usize>> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    let details = v.get("error")?.get("details")?.as_array()?;
    for d in details {
        if let Some(errors) = d.get("logEntryErrors").and_then(|e| e.as_object()) {
            let mut idx: Vec<usize> = errors.keys().filter_map(|k| k.parse().ok()).collect();
            idx.sort_unstable();
            return Some(idx);
        }
    }
    None
}

/// Writes batches of [`LogEntry`] to Cloud Logging.
pub struct GclClient {
    token_source: TokenSource,
    transport: Arc<dyn Transport>,
    clock: TrustedClock,
}

impl GclClient {
    pub fn new(
        token_source: TokenSource,
        transport: Arc<dyn Transport>,
        clock: TrustedClock,
    ) -> Self {
        Self {
            token_source,
            transport,
            clock,
        }
    }

    /// POSTs `entries` to `entries:write`.
    ///
    /// `Ok(())` means Cloud Logging accepted the whole batch, and only then may
    /// the caller commit them off the spool. Every other outcome - network,
    /// 429, 5xx, a persistent 401, or a `partialSuccess` per-entry rejection -
    /// is an `Err`, so the caller spools and backs off.
    pub fn write(&mut self, entries: &[LogEntry]) -> Result<(), ClientError> {
        if entries.is_empty() {
            return Ok(());
        }

        let body = serde_json::to_string(&serde_json::json!({
            "entries": self.clamped(entries),
            "partialSuccess": true,
        }))
        .expect("a LogEntry batch is always serializable");

        let mut invalidated = false;
        for attempt in 0..MAX_ATTEMPTS {
            let token = self.token_source.token()?;
            let authorization = format!("Bearer {}", token.expose());
            let response = self.transport.post(
                ENTRIES_WRITE_URL,
                &[
                    ("Authorization", authorization.as_str()),
                    ("Content-Type", "application/json"),
                ],
                &body,
            );

            // Harvest the Date from EVERY response - success or failure - before
            // any branch can return (TEL-01).
            let response = match response {
                Ok(r) => {
                    self.harvest_date(&r);
                    r
                }
                Err(e) => return Err(e.into()),
            };

            if (200..300).contains(&response.status) {
                // Google documents that a partialSuccess rejection comes back as
                // the status of one of the FAILED entries - i.e. a non-2xx - so
                // this branch "should" be unreachable. We check anyway: `Ok(())`
                // is the caller's licence to commit the batch off the spool, and
                // committing an entry that was never written is UNRECOVERABLE.
                // "Never a silent success" must be an invariant of our code, not
                // an inference about someone else's server. The cost is parsing
                // `{}` once per successful flush.
                if let Some(failed_indices) = partial_error_indices(&response.body) {
                    return Err(ClientError::PartialFailure {
                        status: response.status,
                        failed_indices,
                        body: sanitize_for_error(&response.body, &token),
                    });
                }
                return Ok(());
            }

            if response.status == 401 && !invalidated && attempt + 1 < MAX_ATTEMPTS {
                // Exactly one forced refresh-and-retry (TEL-05).
                self.token_source.invalidate();
                invalidated = true;
                continue;
            }

            if response.status == 401 {
                return Err(ClientError::Unauthorized);
            }

            let body = sanitize_for_error(&response.body, &token);
            return Err(match partial_error_indices(&response.body) {
                Some(failed_indices) => ClientError::PartialFailure {
                    status: response.status,
                    failed_indices,
                    body,
                },
                None => ClientError::Http {
                    status: response.status,
                    body,
                },
            });
        }

        // Unreachable: the loop only continues on the single 401 refresh path,
        // which is bounded by MAX_ATTEMPTS.
        Err(ClientError::Unauthorized)
    }

    fn harvest_date(&self, response: &HttpResponse) {
        if let Some(date) = response.header("Date") {
            let _ = self.clock.observe_http_date(date);
        }
    }

    /// Timestamp hygiene (TEL-02): clamp every timestamp into
    /// `[now − retention + 1h, now]`.
    ///
    /// The upper bound is the important one: Cloud Logging rejects the ENTIRE
    /// batch if any entry is stamped more than 24 h in the future, so a device
    /// with a fast clock would otherwise lose all telemetry forever. The lower
    /// bound keeps an entry just inside retention rather than letting the server
    /// silently discard it. An entry with no timestamp is sent without one -
    /// Logging then assigns receive time. An unparseable timestamp (which would
    /// poison the batch just as badly) is dropped to `None` for the same reason.
    ///
    /// # Two clauses of TEL-02 are deliberately NOT implemented
    ///
    /// Both are recorded here rather than silently skipped, so the next reader is
    /// not misled into believing the spec is implemented as written.
    ///
    /// 1. **"An entry created before trusted time existed is rewritten once using
    ///    the current offset and persisted."** Not done: a `None` timestamp stays
    ///    `None` forever, so such an entry permanently falls back to Cloud
    ///    Logging's server receive time.
    ///
    ///    Rewriting would mean back-dating an entry from an offset learned *after*
    ///    it was written, which is a guess: the only thing we would actually know
    ///    is the offset at *send* time, not the one that was true at *log* time,
    ///    and the gap between them is exactly the unbounded interval in which the
    ///    device had no clock. Server receive time is a real, monotone, honest
    ///    bound (the entry was logged at or before it); a synthesised timestamp is
    ///    a fiction that reads as fact. It would also make an entry's bytes — and
    ///    therefore the spool line, which is written once and fsynced — mutable
    ///    after the fact, for no gain over what the server already stamps.
    ///
    /// 2. **"Entries older than retention are dropped locally, incrementing
    ///    `spool.dropped_expired`."** Not done: they are clamped UP to the
    ///    retention floor and sent, so `dropped_expired` never counts age-expiry
    ///    (it counts only the spool's own drop-oldest budget evictions).
    ///
    ///    An entry can only be older than 30 days if the device was offline for
    ///    30 days — which is precisely the outage an operator most needs to see the
    ///    telemetry for. Dropping it destroys the only evidence of the event;
    ///    clamping it costs a timestamp that is wrong by however long the entry sat
    ///    undelivered, while keeping the payload, the severity and the `insertId`.
    ///    The entry's true time is recoverable anyway — it is embedded in the
    ///    monotone `insertId` sequence and in the payload. Preferring a slightly
    ///    wrong timestamp over no entry at all is the same trade the rest of this
    ///    subsystem makes everywhere else.
    fn clamped(&self, entries: &[LogEntry]) -> Vec<LogEntry> {
        let Some(now) = self.clock.trusted_utc() else {
            // No trusted time: we have nothing to clamp against. (The token mint
            // will fail with NoTrustedTime anyway, so this batch does not fly.)
            return entries.to_vec();
        };
        let floor = now - Duration::days(RETENTION_DAYS) + Duration::hours(1);

        entries
            .iter()
            .map(|e| {
                let mut e = e.clone();
                e.timestamp = e.timestamp.as_deref().and_then(|ts| {
                    let parsed: DateTime<Utc> =
                        DateTime::parse_from_rfc3339(ts).ok()?.with_timezone(&Utc);
                    Some(parsed.clamp(floor, now).to_rfc3339())
                });
                e
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::schema::UrlDetail;
    use crate::logging::auth::ServiceAccount;
    use crate::logging::entry::{EntryContext, LogEntry};
    use crate::logging::event::Event;
    use serde_json::{Map, Value};
    use std::collections::VecDeque;
    use std::sync::{LazyLock, Mutex};

    /// Generated once per test binary, for the same reason as in `auth`: a
    /// committed PEM is the highest-signal secret-scanner tripwire there is.
    static TEST_KEY_PEM: LazyLock<String> = LazyLock::new(|| {
        use rsa::pkcs8::{EncodePrivateKey, LineEnding};
        use rsa::RsaPrivateKey;
        let mut rng = rand::thread_rng();
        RsaPrivateKey::new(&mut rng, 2048)
            .expect("generate test RSA key")
            .to_pkcs8_pem(LineEnding::LF)
            .expect("encode pem")
            .to_string()
    });

    const TOKEN_A: &str = "ya29.TOKEN-A";
    const TOKEN_B: &str = "ya29.TOKEN-B";
    const NOW_HTTP_DATE: &str = "Sun, 12 Jul 2026 08:30:00 GMT";

    fn now_utc() -> DateTime<Utc> {
        DateTime::parse_from_rfc2822("Sun, 12 Jul 2026 08:30:00 +0000")
            .unwrap()
            .with_timezone(&Utc)
    }

    fn sa() -> ServiceAccount {
        ServiceAccount::from_json(
            &serde_json::json!({
                "private_key": *TEST_KEY_PEM,
                "client_email": "kiosk-logger@test-project.iam.gserviceaccount.com",
                "token_uri": "https://oauth2.googleapis.com/token",
            })
            .to_string(),
        )
        .expect("fixture parses")
    }

    /// One fake for BOTH endpoints: it routes on the URL, so the token endpoint's
    /// canned answers cannot be consumed by an `entries:write` call (or vice
    /// versa) and the write-attempt count is exact.
    #[derive(Clone)]
    struct CapturedWrite {
        body: String,
        headers: Vec<(String, String)>,
    }

    struct FakeTransport {
        writes: Mutex<Vec<CapturedWrite>>,
        write_responses: Mutex<VecDeque<Result<HttpResponse, String>>>,
        mints: Mutex<usize>,
        tokens: Mutex<VecDeque<String>>,
    }

    impl FakeTransport {
        fn new(write_responses: Vec<Result<HttpResponse, String>>) -> Arc<Self> {
            Arc::new(Self {
                writes: Mutex::new(Vec::new()),
                write_responses: Mutex::new(write_responses.into()),
                mints: Mutex::new(0),
                tokens: Mutex::new(vec![TOKEN_A.to_string(), TOKEN_B.to_string()].into()),
            })
        }

        fn write_count(&self) -> usize {
            self.writes.lock().unwrap().len()
        }

        fn mint_count(&self) -> usize {
            *self.mints.lock().unwrap()
        }

        /// The JSON body of the Nth `entries:write`.
        fn write_body(&self, n: usize) -> Value {
            let w = self.writes.lock().unwrap()[n].clone();
            serde_json::from_str(&w.body).expect("the request body must be JSON")
        }

        fn write_header(&self, n: usize, name: &str) -> Option<String> {
            self.writes.lock().unwrap()[n]
                .headers
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(name))
                .map(|(_, v)| v.clone())
        }
    }

    impl Transport for FakeTransport {
        fn post(
            &self,
            url: &str,
            headers: &[(&str, &str)],
            body: &str,
        ) -> Result<HttpResponse, TransportError> {
            if url == ENTRIES_WRITE_URL {
                self.writes.lock().unwrap().push(CapturedWrite {
                    body: body.to_string(),
                    headers: headers
                        .iter()
                        .map(|(k, v)| (k.to_string(), v.to_string()))
                        .collect(),
                });
                let mut r = self.write_responses.lock().unwrap();
                let next = if r.len() > 1 {
                    r.pop_front().unwrap()
                } else {
                    r.front().expect("ran out of canned writes").clone()
                };
                return next.map_err(TransportError::Network);
            }

            // The token endpoint. Each call mints a distinct token so a test can
            // see that a refresh actually happened.
            *self.mints.lock().unwrap() += 1;
            let mut tokens = self.tokens.lock().unwrap();
            let token = if tokens.len() > 1 {
                tokens.pop_front().unwrap()
            } else {
                tokens.front().expect("token").clone()
            };
            Ok(HttpResponse {
                status: 200,
                headers: vec![],
                body: format!(r#"{{"access_token":"{token}","expires_in":3600}}"#),
            })
        }
    }

    fn ok(status: u16) -> Result<HttpResponse, String> {
        Ok(HttpResponse {
            status,
            headers: vec![],
            body: "{}".into(),
        })
    }

    fn resp(status: u16, body: &str, date: Option<&str>) -> Result<HttpResponse, String> {
        Ok(HttpResponse {
            status,
            headers: date
                .map(|d| vec![("Date".to_string(), d.to_string())])
                .unwrap_or_default(),
            body: body.into(),
        })
    }

    fn established_clock() -> TrustedClock {
        let c = TrustedClock::new();
        c.observe_http_date(NOW_HTTP_DATE).unwrap();
        c
    }

    fn client(t: Arc<FakeTransport>, clock: TrustedClock) -> GclClient {
        GclClient::new(TokenSource::new(sa(), t.clone(), clock.clone()), t, clock)
    }

    fn ctx() -> EntryContext {
        EntryContext {
            project_id: "proj".into(),
            device_id: "lobby-01".into(),
            site: "hq".into(),
            region: "asia-southeast1".into(),
            app_version: "0.1.0".into(),
            config_revision: Some(1),
            url_detail: UrlDetail::Path,
        }
    }

    fn entry(seq: u64, clock: &TrustedClock) -> LogEntry {
        LogEntry::new(Event::AppStart, &ctx(), seq, clock, Map::new())
    }

    fn entries_of(body: &Value) -> &Vec<Value> {
        body.get("entries").unwrap().as_array().unwrap()
    }

    #[test]
    fn a_successful_write_posts_every_entry_with_partial_success() {
        let t = FakeTransport::new(vec![ok(200)]);
        let clock = established_clock();
        let mut c = client(t.clone(), clock.clone());

        let batch = vec![entry(1, &clock), entry(2, &clock), entry(3, &clock)];
        c.write(&batch).expect("200 is a success");

        assert_eq!(t.write_count(), 1);
        let body = t.write_body(0);
        assert_eq!(body.get("partialSuccess").unwrap(), &Value::Bool(true));
        let sent = entries_of(&body);
        assert_eq!(sent.len(), 3, "every entry must be posted");
        let ids: Vec<&str> = sent
            .iter()
            .map(|e| e.get("insertId").unwrap().as_str().unwrap())
            .collect();
        assert_eq!(ids, vec!["lobby-01-1", "lobby-01-2", "lobby-01-3"]);
    }

    #[test]
    fn the_bearer_token_is_sent_in_the_authorization_header() {
        let t = FakeTransport::new(vec![ok(200)]);
        let clock = established_clock();
        let mut c = client(t.clone(), clock);
        c.write(&[entry(1, &established_clock())]).unwrap();

        assert_eq!(
            t.write_header(0, "Authorization").as_deref(),
            Some(format!("Bearer {TOKEN_A}").as_str())
        );
        assert_eq!(
            t.write_header(0, "Content-Type").as_deref(),
            Some("application/json")
        );
    }

    #[test]
    fn a_401_triggers_exactly_one_refresh_and_retry() {
        let t = FakeTransport::new(vec![ok(401), ok(200)]);
        let clock = established_clock();
        let mut c = client(t.clone(), clock.clone());

        c.write(&[entry(1, &clock)]).expect("the retry succeeds");

        assert_eq!(t.write_count(), 2, "one 401 => exactly one retry");
        assert_eq!(t.mint_count(), 2, "the token source re-minted exactly once");
        assert_eq!(
            t.write_header(0, "Authorization").as_deref(),
            Some(format!("Bearer {TOKEN_A}").as_str())
        );
        assert_eq!(
            t.write_header(1, "Authorization").as_deref(),
            Some(format!("Bearer {TOKEN_B}").as_str()),
            "the retry must carry a FRESH token, not the rejected one"
        );
    }

    #[test]
    fn a_persistent_401_gives_up_rather_than_looping() {
        let t = FakeTransport::new(vec![ok(401)]); // 401s forever
        let clock = established_clock();
        let mut c = client(t.clone(), clock.clone());

        let err = c.write(&[entry(1, &clock)]).expect_err("must not loop");
        assert!(matches!(err, ClientError::Unauthorized), "got {err:?}");
        // A literal 2, deliberately NOT `MAX_ATTEMPTS`: asserting against the
        // constant would make this test agree with any bound the implementation
        // happened to choose, including a large one. Two attempts is the contract.
        assert_eq!(
            t.write_count(),
            2,
            "a persistently-401ing credential must be bounded at 2 attempts (one \
             forced refresh-and-retry), not retried in a loop"
        );
    }

    #[test]
    fn a_5xx_is_an_error_so_the_caller_spools() {
        for status in [429u16, 500, 503] {
            let t = FakeTransport::new(vec![resp(status, "backend unavailable", None)]);
            let clock = established_clock();
            let mut c = client(t.clone(), clock.clone());
            match c.write(&[entry(1, &clock)]) {
                Err(ClientError::Http { status: s, .. }) => assert_eq!(s, status),
                other => panic!("expected an Http error for {status}, got {other:?}"),
            }
            assert_eq!(t.write_count(), 1, "a 5xx/429 must not be retried in-line");
        }

        // A dead network is an Err too, not a panic.
        let t = FakeTransport::new(vec![Err("dns failure".into())]);
        let clock = established_clock();
        let mut c = client(t.clone(), clock.clone());
        assert!(matches!(
            c.write(&[entry(1, &clock)]),
            Err(ClientError::Transport(_))
        ));
    }

    #[test]
    fn the_date_header_of_every_response_updates_the_trusted_clock() {
        // (a) On success.
        let t = FakeTransport::new(vec![resp(200, "{}", Some("Sun, 12 Jul 2026 08:30:00 GMT"))]);
        let clock = TrustedClock::new();
        // A clock with no offset yet cannot mint a JWT, so seed it far away and
        // check the write's Date MOVES it. (This also proves the harvest is not
        // a no-op on an already-established clock.)
        clock
            .observe_http_date("Sun, 12 Jul 2026 06:30:00 GMT")
            .unwrap();
        let before = clock.offset_seconds().unwrap();
        let mut c = client(t, clock.clone());
        c.write(&[entry(1, &clock)]).expect("200");
        let after = clock.offset_seconds().unwrap();
        assert_eq!(
            after - before,
            7200,
            "the Date of a SUCCESS response must update the trusted clock"
        );

        // (b) On failure - the load-bearing half. A dead-CMOS kiosk's first
        // response is quite likely a failure, and its Date is what bootstraps it.
        let t = FakeTransport::new(vec![resp(
            500,
            "boom",
            Some("Sun, 12 Jul 2026 06:30:00 GMT"),
        )]);
        let clock = established_clock(); // 08:30
        let before = clock.offset_seconds().unwrap();
        let mut c = client(t, clock.clone());
        c.write(&[entry(1, &clock)]).expect_err("500");
        let after = clock.offset_seconds().unwrap();
        assert_eq!(
            after - before,
            -7200,
            "the Date of a FAILURE response must ALSO update the trusted clock"
        );
    }

    #[test]
    fn a_future_timestamp_is_clamped_so_it_cannot_poison_the_batch() {
        let t = FakeTransport::new(vec![ok(200)]);
        let clock = established_clock();
        let mut c = client(t.clone(), clock.clone());

        let mut e = entry(1, &clock);
        e.timestamp = Some((now_utc() + Duration::hours(48)).to_rfc3339());
        c.write(&[e]).unwrap();

        let body = t.write_body(0);
        let sent = entries_of(&body)[0]
            .get("timestamp")
            .expect("a stamped entry keeps its timestamp")
            .as_str()
            .unwrap()
            .to_string();
        let sent = DateTime::parse_from_rfc3339(&sent)
            .unwrap()
            .with_timezone(&Utc);
        // The tolerance absorbs the trusted clock's whole-second offset plus the
        // test's own runtime; it is orders of magnitude tighter than the +48 h
        // this must not be, and than the 24 h that poisons the batch.
        assert!(
            sent <= now_utc() + Duration::seconds(60),
            "+48h must be clamped to <= now (Cloud Logging rejects the WHOLE batch \
             for any timestamp > 24h in the future); got {sent}"
        );
        assert!(
            sent >= now_utc() - Duration::seconds(60),
            "clamping to `now` must not throw the timestamp into the past: {sent}"
        );
    }

    #[test]
    fn an_ancient_timestamp_is_clamped_up_to_just_inside_retention() {
        let t = FakeTransport::new(vec![ok(200)]);
        let clock = established_clock();
        let mut c = client(t.clone(), clock.clone());

        let mut e = entry(1, &clock);
        e.timestamp = Some((now_utc() - Duration::days(400)).to_rfc3339());
        c.write(&[e]).unwrap();

        let ts = entries_of(&t.write_body(0))[0]
            .get("timestamp")
            .unwrap()
            .as_str()
            .unwrap()
            .to_string();
        let ts = DateTime::parse_from_rfc3339(&ts)
            .unwrap()
            .with_timezone(&Utc);
        let floor = now_utc() - Duration::days(RETENTION_DAYS) + Duration::hours(1);
        assert!(
            (ts - floor).num_seconds().abs() <= 5,
            "an entry older than retention must be clamped to the floor \
             (or Logging silently discards it); got {ts}, expected ~{floor}"
        );
    }

    #[test]
    fn an_entry_without_a_timestamp_is_sent_without_one() {
        let t = FakeTransport::new(vec![ok(200)]);
        let clock = established_clock();
        let mut c = client(t.clone(), clock.clone());

        // An entry created before trusted time existed has no timestamp; Logging
        // then assigns receive time (TEL-02). We must NOT invent one.
        let mut e = entry(1, &clock);
        e.timestamp = None;
        c.write(&[e]).unwrap();

        let body = t.write_body(0);
        let sent = &entries_of(&body)[0];
        assert!(
            sent.get("timestamp").is_none(),
            "a timestamp-less entry must be posted WITHOUT a timestamp field, got {sent}"
        );
    }

    /// A timestamp in the clamp window must be passed through untouched, or the
    /// clamp test above would pass for a client that simply overwrote everything
    /// with `now`.
    #[test]
    fn a_timestamp_already_in_range_is_left_alone() {
        let t = FakeTransport::new(vec![ok(200)]);
        let clock = established_clock();
        let mut c = client(t.clone(), clock.clone());

        let stamp = (now_utc() - Duration::hours(3)).to_rfc3339();
        let mut e = entry(1, &clock);
        e.timestamp = Some(stamp.clone());
        c.write(&[e]).unwrap();

        let ts = entries_of(&t.write_body(0))[0]
            .get("timestamp")
            .unwrap()
            .as_str()
            .unwrap()
            .to_string();
        let sent = DateTime::parse_from_rfc3339(&ts).unwrap();
        let orig = DateTime::parse_from_rfc3339(&stamp).unwrap();
        assert_eq!(sent, orig, "an in-range timestamp must not be rewritten");
    }

    /// The gap the plan flagged: with `partialSuccess: true` Cloud Logging can
    /// accept some entries and reject others. Google documents that in that case
    /// "the response status is the response status of one of the failed entries"
    /// and the body carries `WriteLogEntriesPartialErrors.logEntryErrors` keyed by
    /// the entries' zero-based index. So a partial failure is a NON-2xx. Treating
    /// it as success would `commit_drained` the rejected entries off the spool -
    /// silent data loss. It must be an Err that NAMES the failed indices.
    #[test]
    fn a_partial_failure_is_an_error_and_never_a_silent_drop() {
        let body = serde_json::json!({
            "error": {
                "code": 400,
                "message": "Log entry with size 300K exceeds maximum size of 256.0K",
                "status": "INVALID_ARGUMENT",
                "details": [{
                    "@type": "type.googleapis.com/google.logging.v2.WriteLogEntriesPartialErrors",
                    "logEntryErrors": {
                        "1": {"code": 3, "message": "Log entry too large"},
                        "2": {"code": 3, "message": "Log entry too large"}
                    }
                }]
            }
        })
        .to_string();

        let t = FakeTransport::new(vec![resp(400, &body, None)]);
        let clock = established_clock();
        let mut c = client(t.clone(), clock.clone());

        let batch = vec![entry(1, &clock), entry(2, &clock), entry(3, &clock)];
        match c.write(&batch) {
            Err(ClientError::PartialFailure {
                status,
                failed_indices,
                ..
            }) => {
                assert_eq!(status, 400);
                assert_eq!(
                    failed_indices,
                    vec![1, 2],
                    "the caller must be told exactly which entries were rejected"
                );
            }
            other => panic!(
                "a partialSuccess per-entry rejection must be an Err naming the \
                 failed entries, not a silent success; got {other:?}"
            ),
        }
        assert_eq!(t.write_count(), 1, "a 4xx must not be retried in-line");
    }

    #[test]
    fn an_empty_batch_makes_no_request() {
        let t = FakeTransport::new(vec![ok(200)]);
        let clock = established_clock();
        let mut c = client(t.clone(), clock);
        c.write(&[]).expect("an empty batch is trivially written");
        assert_eq!(t.write_count(), 0);
        assert_eq!(t.mint_count(), 0, "and must not even mint a token");
    }

    /// A REFLECTING endpoint - a misconfigured proxy, a captive portal, a
    /// middlebox that has hijacked `logging.googleapis.com` - echoes our request
    /// back to us in its error body, headers and all. That body goes verbatim
    /// into a `ClientError`, and a `ClientError` gets logged. So the bearer token
    /// arrives back over the network inside data we are about to log.
    ///
    /// The canned body below therefore ACTUALLY CONTAINS the token. (It did not
    /// before: the body was the string "upstream said: whatever", which never
    /// held the token, so the test could only ever have failed if the client
    /// concatenated the header into the error itself - it could not fail for the
    /// threat its own name described. That gap was hiding a REAL leak: the body
    /// was passed verbatim to `truncate_for_error`, so this test now fails
    /// against the previous implementation. `sanitize_for_error` is the fix.)
    ///
    /// `Secret`'s redaction cannot help here: the token in the body is a plain
    /// `String` that came from the network, not a `Secret` we hold.
    #[test]
    fn a_reflecting_endpoint_cannot_echo_the_bearer_token_into_an_error() {
        for status in [500u16, 400, 429] {
            let echoed = format!(
                "your request was: POST /v2/entries:write\r\n\
                 Authorization: Bearer {TOKEN_A}\r\n\
                 Content-Type: application/json"
            );
            assert!(
                echoed.contains(TOKEN_A),
                "the fixture must really contain the token, or this test proves nothing"
            );

            let t = FakeTransport::new(vec![resp(status, &echoed, None)]);
            let clock = established_clock();
            let mut c = client(t, clock.clone());
            let err = c.write(&[entry(1, &clock)]).expect_err("non-2xx");

            let rendered = format!("{err} {err:?}");
            assert!(
                !rendered.contains(TOKEN_A),
                "the bearer token came back from the endpoint and leaked into a \
                 logged error ({status}): {rendered}"
            );
            assert!(
                rendered.contains("<redacted-bearer-token>"),
                "the token should be redacted in place, not merely absent by luck: {rendered}"
            );
        }
    }

    /// The last hole in "never a silent success".
    ///
    /// Google documents that a `partialSuccess` rejection comes back as the status
    /// of one of the FAILED entries, so a 200-with-partial-errors should not
    /// happen. But `Ok(())` is the caller's licence to `commit_drained` the batch
    /// off the spool, and committing an entry that was never written is
    /// unrecoverable. If that documented behavior is ever wrong, or ever changes,
    /// a 2xx path that never looks at the body loses those entries silently.
    ///
    /// So the invariant must live in our code, not in Google's documentation.
    #[test]
    fn a_2xx_carrying_partial_errors_is_still_not_a_silent_success() {
        let body = serde_json::json!({
            "error": {
                "code": 400,
                "status": "INVALID_ARGUMENT",
                "details": [{
                    "@type": "type.googleapis.com/google.logging.v2.WriteLogEntriesPartialErrors",
                    "logEntryErrors": { "2": {"code": 3, "message": "Log entry too large"} }
                }]
            }
        })
        .to_string();

        let t = FakeTransport::new(vec![resp(200, &body, None)]);
        let clock = established_clock();
        let mut c = client(t.clone(), clock.clone());

        let batch = vec![entry(1, &clock), entry(2, &clock), entry(3, &clock)];
        match c.write(&batch) {
            Err(ClientError::PartialFailure {
                status,
                failed_indices,
                ..
            }) => {
                assert_eq!(status, 200);
                assert_eq!(failed_indices, vec![2]);
            }
            other => panic!(
                "a 2xx whose body reports per-entry failures must NOT be reported as \
                 success - the caller would commit the rejected entries off the spool \
                 and they would be gone; got {other:?}"
            ),
        }
    }

    /// The other side of that check: an ordinary 200 (empty JSON body) is still a
    /// plain success, and the partial-errors probe must not turn it into an error.
    #[test]
    fn an_ordinary_2xx_is_still_a_success() {
        let t = FakeTransport::new(vec![resp(200, "{}", None)]);
        let clock = established_clock();
        let mut c = client(t.clone(), clock.clone());
        c.write(&[entry(1, &clock)])
            .expect("an empty 200 body is an unqualified success");
        assert_eq!(t.write_count(), 1);
    }

    #[test]
    fn an_oversized_error_body_is_truncated() {
        let t = FakeTransport::new(vec![resp(500, &"A".repeat(100_000), None)]);
        let clock = established_clock();
        let mut c = client(t, clock.clone());
        let err = c.write(&[entry(1, &clock)]).expect_err("500");
        let rendered = err.to_string();
        assert!(
            rendered.len() < 1200,
            "not truncated: {} bytes",
            rendered.len()
        );
        assert!(rendered.contains("truncated"));
    }

    #[test]
    fn a_token_failure_is_reported_and_no_write_is_attempted() {
        let t = FakeTransport::new(vec![ok(200)]);
        // A fresh clock has never seen a Date, so the JWT cannot be signed.
        let mut c = client(t.clone(), TrustedClock::new());
        assert!(matches!(
            c.write(&[entry(1, &TrustedClock::new())]),
            Err(ClientError::Auth(AuthError::NoTrustedTime))
        ));
        assert_eq!(t.write_count(), 0);
    }
}
