//! Telemetry — Google Cloud Logging (spec §6).

pub mod auth;
pub mod client;
pub mod entry;
pub mod event;
pub mod ratelimit;
pub mod spool;
pub mod time;
pub mod transport;

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::{Map, Value};

use crate::logging::client::{ClientError, GclClient};
use crate::logging::entry::{EntryContext, LogEntry};
use crate::logging::event::Event;
use crate::logging::ratelimit::{Admit, RateLimiter};
use crate::logging::spool::{Spool, SpoolError};
use crate::logging::time::TrustedClock;

/// Flush triggers (spec TEL-07): every 10 s or 100 entries, whichever first.
pub const FLUSH_INTERVAL: Duration = Duration::from_secs(10);
pub const FLUSH_ENTRIES: usize = 100;

/// The most entries one `entries:write` carries. Equal to the entry trigger, so a
/// steady stream flushes in one request.
pub const MAX_BATCH: usize = 100;

const BACKOFF_MIN: Duration = Duration::from_secs(1);
const BACKOFF_MAX: Duration = Duration::from_secs(300);

/// How many times an entry NAMED by a permanent per-entry rejection is retried
/// before it is quarantined off the spool. See [`Logger::flush`].
pub const MAX_REJECT_ATTEMPTS: u32 = 3;

/// Bounded evidence kept about quarantined entries, for the operator.
const MAX_REMEMBERED_QUARANTINES: usize = 32;

/// The Logger's durable side-state, inside the spool root.
const REJECT_STATE_FILE: &str = "reject.json";
const REJECT_STATE_TMP: &str = "reject.json.tmp";

#[derive(Debug, thiserror::Error)]
pub enum LoggerError {
    #[error("spool failure: {0}")]
    Spool(#[from] SpoolError),
    #[error("write failure: {0}")]
    Client(#[from] ClientError),
}

/// Is this HTTP status a statement about the *entry* ("this will never be
/// accepted") or about the *server* ("try again later")?
///
/// 408 (request timeout) and 429 (rate limited) are 4xx but are explicitly
/// retryable; every 5xx is. Everything else in the 4xx range is a rejection of
/// the request's content, and re-sending the same bytes will get the same
/// answer forever. A 2xx that nevertheless carries per-entry errors (see
/// `client.rs`) also names entries the server refused to store: that is a
/// content verdict, not a capacity one.
fn status_is_permanent(status: u16) -> bool {
    !matches!(status, 408 | 429) && !(500..600).contains(&status)
}

/// A single quarantined entry: what we dropped, and why.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Quarantined {
    pub insert_id: String,
    pub status: u16,
    pub attempts: u32,
}

/// The Logger's durable side-state, next to the spool's own (`spool/reject.json`).
///
/// It is persisted for the same reason T4 persists `dropped_expired`: **the
/// device whose telemetry matters most is the one the watchdog is killing every
/// few seconds**, and an in-memory retry budget resets on every boot. A poison
/// entry would then never reach `MAX_REJECT_ATTEMPTS` (that takes ~3 flushes),
/// would be re-sent from the spool on every boot, and would wedge the drain
/// FOREVER — the exact failure this design exists to prevent, reached through the
/// back door of a restart loop. The loss counters are persisted for the mirror
/// reason: a quarantine followed by a kill before the next `health.sample` would
/// otherwise erase its own evidence.
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
struct RejectState {
    /// insert_id -> how many times a PERMANENT per-entry rejection named it.
    #[serde(default)]
    attempts: BTreeMap<String, u32>,
    #[serde(default)]
    dropped_rejected: u64,
    #[serde(default)]
    quarantined: Vec<Quarantined>,
}

impl RejectState {
    fn load(dir: &Path) -> RejectState {
        fs::read_to_string(dir.join(REJECT_STATE_FILE))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// Write-then-rename, fsynced — the same durability recipe as the spool's
    /// `seq`. A failure is swallowed: it degrades this to the old in-memory
    /// behavior, and it must never propagate into `log()`.
    fn persist(&self, dir: &Path) {
        let Ok(text) = serde_json::to_string(self) else {
            return;
        };
        let tmp = dir.join(REJECT_STATE_TMP);
        let write = File::create(&tmp).and_then(|mut f| {
            f.write_all(text.as_bytes())?;
            f.sync_all()
        });
        if write.is_err() {
            return;
        }
        let _ = fs::rename(&tmp, dir.join(REJECT_STATE_FILE));
        // Make the rename itself durable, not just the file's contents.
        let _ = File::open(dir).map(|f| f.sync_all());
    }
}

/// The telemetry facade: rate-limit → stamp → spool → batch → drain (spec §6).
///
/// The **spool is the buffer**. `log()` writes every entry through to the spool
/// (fsynced there and then for WARNING+, TEL-10) and only counts how many are
/// pending; `flush()` drains the spool and hands the batch to the client, and
/// entries are removed *only* after Cloud Logging confirmed the write. There is
/// deliberately no in-memory batch that a `SIGKILL` could take with it.
///
/// `Send`, not `Sync`: it is designed to be *owned* by a dedicated thread
/// (TEL-10 — independent of the frequently-hung webview thread). It does not
/// spawn that thread.
pub struct Logger {
    ctx: EntryContext,
    spool: Spool,
    client: GclClient,
    limiter: RateLimiter,
    clock: TrustedClock,

    /// Entries known to be awaiting delivery — see [`Logger::pending`].
    pending: usize,
    last_flush: Instant,

    /// Set after a failed flush; no flush is attempted before it elapses.
    backoff: Duration,
    retry_after: Option<Instant>,

    /// Durable: survives the SIGKILL that a crash loop delivers. See [`RejectState`].
    state: RejectState,
}

impl Logger {
    pub fn new(
        ctx: EntryContext,
        mut spool: Spool,
        client: GclClient,
        limiter: RateLimiter,
        clock: TrustedClock,
    ) -> Logger {
        let state = RejectState::load(spool.dir());
        // Whatever a previous process left spooled is still awaiting delivery, so
        // the entry trigger must see it. Cheap: it is bounded by MAX_BATCH.
        let pending = spool.drain_batch(MAX_BATCH).map(|b| b.len()).unwrap_or(0);
        Logger {
            ctx,
            spool,
            client,
            limiter,
            clock,
            pending,
            last_flush: Instant::now(),
            backoff: BACKOFF_MIN,
            retry_after: None,
            state,
        }
    }

    /// Record an event. **Infallible from the caller's perspective**: telemetry
    /// must never take down the kiosk, so every failure below (a full disk, a
    /// read-only spool directory, a serialization error) is swallowed here — it
    /// is the one place in the codebase where swallowing is the correct answer,
    /// because the alternative is a `?` in the caller's hot path that turns a
    /// logging problem into a kiosk outage.
    ///
    /// A WARNING-or-above entry is on disk and fsynced *before this returns*
    /// (TEL-10): a watchdog `SIGKILL` runs neither the panic hook nor `Drop`,
    /// so the `watchdog.safe_mode` line explaining why a device died has to be
    /// durable by the time the caller gets control back.
    pub fn log(&mut self, event: Event, fields: Map<String, Value>) {
        if matches!(self.limiter.admit(event), Admit::Suppress) {
            return;
        }
        self.emit(event, fields);
    }

    /// Build, spool and count one entry, bypassing the rate limiter. Used by
    /// `log()` (post-admission) and by the coalesced summaries, which must never
    /// themselves be suppressed — a summary that got rate-limited away would
    /// hide the very flood it exists to report.
    fn emit(&mut self, event: Event, fields: Map<String, Value>) {
        let Ok(seq) = self.spool.next_seq() else {
            // The counter could not be reserved. Emitting anyway would risk
            // reusing an insertId, which makes Cloud Logging silently dedup a
            // real entry away — worse than dropping this one.
            return;
        };
        let entry = LogEntry::new(event, &self.ctx, seq, &self.clock, fields);
        if self.spool.append(&entry).is_err() {
            return;
        }
        self.pending += 1;
    }

    /// Drain the spool and write it to Cloud Logging.
    ///
    /// On success the batch is committed off the spool. On failure the entries
    /// stay spooled — with their `insertId`s intact, so the retry is byte-
    /// identical and Cloud Logging dedups whatever already landed (TEL-03) —
    /// and the next flush is delayed by an exponential backoff.
    ///
    /// ## Poison entries (the reason this is not a two-line function)
    ///
    /// Cloud Logging can reject *individual* entries: an oversized payload, a
    /// malformed field. Such an entry is rejected identically forever. The naive
    /// handler — "any error, keep the whole batch, retry" — therefore wedges the
    /// drain on the oldest poison entry and the device stops sending telemetry
    /// entirely, which looks exactly like a quiet, healthy device. That silent,
    /// permanent telemetry death is the worst failure mode in this module.
    ///
    /// So a `PartialFailure` is triaged, never blindly retried:
    ///
    /// * **The status decides transient vs permanent** (`status_is_permanent`).
    ///   A 429/5xx names entries the server had no *capacity* for; nothing is
    ///   wrong with them, so the whole batch is kept and retried, and no entry
    ///   is charged an attempt. Only a permanent status (a 4xx that is not
    ///   408/429, or the "impossible" 2xx-with-per-entry-errors) is a verdict on
    ///   the entries themselves.
    /// * **The entries NOT named were accepted.** They are committed off the
    ///   spool immediately — otherwise a poison entry would drag its innocent
    ///   batch-mates into an infinite retry with it.
    /// * **A named entry is charged an attempt, not dropped on the spot.**
    ///   Retrying it costs one request and could still succeed (Google's
    ///   per-entry codes are not reliably classifiable, and we may have been
    ///   handed a status that misdescribes the entry). After
    ///   `MAX_REJECT_ATTEMPTS` distinct rejections it is quarantined: committed
    ///   off the spool so the drain moves on.
    /// * **A quarantine is never silent.** It increments `dropped_rejected()`
    ///   and is recorded in `quarantined()`. This mirrors the spec's own idiom
    ///   for local loss (`spool.dropped_expired`, "surfaced in the next
    ///   `health.sample` — loss is visible, not silent"); P1-D reports both
    ///   counters from the same place.
    /// * **An UNATTRIBUTABLE permanent failure commits nothing.** If
    ///   `failed_indices` is empty, or names only indices outside the batch, we
    ///   have been told the batch failed but not *which* entries — and "not named
    ///   ⇒ accepted" would then quietly commit the ENTIRE batch off the spool for
    ///   entries Cloud Logging never stored. (Reachable, not theoretical: T7's
    ///   parser yields `Some(vec![])` for a `logEntryErrors: {}` body or for keys
    ///   that are not integers.) Silent loss of a whole batch is strictly worse
    ///   than a wedge, which is at least visible as a quiet device — so the whole
    ///   batch is kept, nothing is charged, and we back off.
    /// * The retry budget and the loss counters are **persisted** — see
    ///   [`RejectState`]; a crash loop must not reset them.
    pub fn flush(&mut self) -> Result<(), LoggerError> {
        let batch = match self.spool.drain_batch(MAX_BATCH) {
            Ok(b) => b,
            Err(e) => {
                // A broken spool is a failure like any other: back off rather
                // than re-entering on every tick.
                self.last_flush = Instant::now();
                self.fail();
                return Err(e.into());
            }
        };
        self.last_flush = Instant::now();
        self.pending = self.pending.saturating_sub(batch.len());
        if batch.is_empty() {
            self.succeed();
            return Ok(());
        }

        match self.client.write(&batch) {
            Ok(()) => {
                self.spool.commit_drained(&batch)?;
                let mut changed = false;
                for e in &batch {
                    changed |= self.state.attempts.remove(&e.insert_id).is_some();
                }
                if changed {
                    self.state.persist(self.spool.dir());
                }
                self.succeed();
                Ok(())
            }
            Err(ClientError::PartialFailure {
                status,
                failed_indices,
                body,
            }) => {
                // Nothing was delivered, so these entries are still awaiting it.
                self.pending += batch.len();
                let attributable =
                    status_is_permanent(status) && failed_indices.iter().any(|i| *i < batch.len());
                if attributable {
                    self.triage_rejections(&batch, &failed_indices, status)?;
                }
                self.fail();
                Err(LoggerError::Client(ClientError::PartialFailure {
                    status,
                    failed_indices,
                    body,
                }))
            }
            Err(e) => {
                // Nothing is committed: the entries stay spooled with the same
                // insertIds and are retried after the backoff.
                self.pending += batch.len();
                self.fail();
                Err(e.into())
            }
        }
    }

    /// The permanent-rejection half of `flush`'s triage. See its docs.
    ///
    /// The caller has already established that at least one `failed_indices`
    /// entry is IN RANGE, so "not named ⇒ accepted" is a sound inference here and
    /// nowhere else. Out-of-range indices are ignored (they name no entry we can
    /// charge), never wrapped or clamped onto an innocent entry.
    fn triage_rejections(
        &mut self,
        batch: &[LogEntry],
        failed_indices: &[usize],
        status: u16,
    ) -> Result<(), SpoolError> {
        let mut accepted: Vec<LogEntry> = Vec::new();
        let mut quarantine: Vec<LogEntry> = Vec::new();

        for (i, entry) in batch.iter().enumerate() {
            if !failed_indices.contains(&i) {
                // Cloud Logging stored it. Committing it is not merely safe, it
                // is required: leaving it behind would retry it forever
                // alongside the poison entry.
                accepted.push(entry.clone());
                self.state.attempts.remove(&entry.insert_id);
                continue;
            }
            let attempts = self
                .state
                .attempts
                .entry(entry.insert_id.clone())
                .or_insert(0);
            *attempts += 1;
            if *attempts >= MAX_REJECT_ATTEMPTS {
                let attempts = *attempts;
                self.state.attempts.remove(&entry.insert_id);
                self.state.dropped_rejected += 1;
                if self.state.quarantined.len() < MAX_REMEMBERED_QUARANTINES {
                    self.state.quarantined.push(Quarantined {
                        insert_id: entry.insert_id.clone(),
                        status,
                        attempts,
                    });
                }
                quarantine.push(entry.clone());
            }
        }

        // The budget and the counters must be durable BEFORE the entries leave
        // the spool: a kill in between must not lose the evidence of the drop.
        self.state.persist(self.spool.dir());

        // One commit call: `commit_drained` matches on insertId, so committing
        // the accepted and the quarantined together is exactly "remove these".
        accepted.extend(quarantine);
        if !accepted.is_empty() {
            self.pending = self.pending.saturating_sub(accepted.len());
            self.spool.commit_drained(&accepted)?;
        }
        Ok(())
    }

    /// Called on a timer by the owning thread. Flushes when 10 s have elapsed OR
    /// 100 entries are pending (spec TEL-07), and emits any coalesced rate-limit
    /// summaries first so a throttled loop stays visible in the very batch its
    /// suppression produced.
    pub fn tick(&mut self) {
        self.emit_summaries();

        if let Some(retry_after) = self.retry_after {
            if Instant::now() < retry_after {
                return;
            }
        }
        let due = self.pending >= FLUSH_ENTRIES || self.last_flush.elapsed() >= FLUSH_INTERVAL;
        if due {
            // `tick` is on the same "telemetry cannot kill the kiosk" footing as
            // `log`. The error is already recorded as a backoff.
            let _ = self.flush();
        }
    }

    /// Turn each suppressed-event count into a real entry (TEL-09), so the loop
    /// is bounded but never hidden.
    fn emit_summaries(&mut self) {
        for (event, suppressed) in self.limiter.take_summaries() {
            let mut fields = Map::new();
            fields.insert("suppressed_count".into(), Value::from(suppressed));
            fields.insert("rate_limited".into(), Value::Bool(true));
            self.emit(event, fields);
        }
    }

    fn succeed(&mut self) {
        self.backoff = BACKOFF_MIN;
        self.retry_after = None;
    }

    fn fail(&mut self) {
        self.retry_after = Some(Instant::now() + self.backoff);
        self.backoff = (self.backoff * 2).min(BACKOFF_MAX);
    }

    /// Entries Cloud Logging permanently refused and we therefore dropped, so the
    /// rest of the telemetry could flow. Loss is visible, never silent: P1-D
    /// surfaces this in `health.sample` next to `spool.dropped_expired`.
    /// Durable across a restart: a quarantine followed by a watchdog kill before
    /// the next `health.sample` must not erase its own evidence.
    pub fn dropped_rejected(&self) -> u64 {
        self.state.dropped_rejected
    }

    /// A bounded record of which entries were quarantined and why. Also durable.
    pub fn quarantined(&self) -> &[Quarantined] {
        &self.state.quarantined
    }

    /// Entries lost to the spool's drop-oldest budget (TEL-07).
    pub fn dropped_expired(&self) -> u64 {
        self.spool.dropped_expired()
    }

    /// **Entries awaiting delivery** — not "entries appended since the last
    /// flush". It counts everything logged (including whatever a previous process
    /// left spooled) and is reduced only by what a flush actually got *committed*;
    /// a failed flush puts its batch straight back. So after a flush that drained
    /// 100 of a 250-entry backlog it reads 150, the 100-entry trigger stays armed,
    /// and the backlog drains at one batch per tick instead of one per 10 s.
    ///
    /// It is a lower bound in one edge case: a `drain_batch` that itself failed
    /// leaves the count untouched. It is never an over-count of undelivered work.
    pub fn pending(&self) -> usize {
        self.pending
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::schema::UrlDetail;
    use crate::logging::auth::{ServiceAccount, TokenSource};
    use crate::logging::client::ENTRIES_WRITE_URL;
    use crate::logging::event::Severity;
    use crate::logging::spool::SpoolConfig;
    use crate::logging::transport::{HttpResponse, Transport, TransportError};
    use std::collections::VecDeque;
    use std::sync::{Arc, LazyLock, Mutex};

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

    const HTTP_DATE: &str = "Sun, 12 Jul 2026 08:30:00 GMT";

    struct FakeTransport {
        writes: Mutex<Vec<String>>,
        responses: Mutex<VecDeque<Result<HttpResponse, String>>>,
    }

    impl FakeTransport {
        fn new(responses: Vec<Result<HttpResponse, String>>) -> Arc<Self> {
            Arc::new(Self {
                writes: Mutex::new(Vec::new()),
                responses: Mutex::new(responses.into()),
            })
        }
        fn write_count(&self) -> usize {
            self.writes.lock().unwrap().len()
        }
        fn write_body(&self, n: usize) -> Value {
            serde_json::from_str(&self.writes.lock().unwrap()[n]).unwrap()
        }
        /// The insertIds of the Nth posted batch.
        fn insert_ids(&self, n: usize) -> Vec<String> {
            self.write_body(n)["entries"]
                .as_array()
                .unwrap()
                .iter()
                .map(|e| e["insert_id"].as_str().unwrap().to_string())
                .collect()
        }
    }

    impl Transport for FakeTransport {
        fn post(
            &self,
            url: &str,
            _headers: &[(&str, &str)],
            body: &str,
        ) -> Result<HttpResponse, TransportError> {
            if url == ENTRIES_WRITE_URL {
                self.writes.lock().unwrap().push(body.to_string());
                let mut r = self.responses.lock().unwrap();
                let next = if r.len() > 1 {
                    r.pop_front().unwrap()
                } else {
                    r.front().expect("ran out of canned responses").clone()
                };
                return next.map_err(TransportError::Network);
            }
            Ok(HttpResponse {
                status: 200,
                headers: vec![],
                body: r#"{"access_token":"ya29.TEST","expires_in":3600}"#.into(),
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

    fn resp(status: u16, body: &str) -> Result<HttpResponse, String> {
        Ok(HttpResponse {
            status,
            headers: vec![],
            body: body.into(),
        })
    }

    /// A `WriteLogEntriesPartialErrors` body naming `indices`.
    fn partial_body(indices: &[usize]) -> String {
        let mut errors = Map::new();
        for i in indices {
            errors.insert(
                i.to_string(),
                serde_json::json!({"code": 3, "message": "Log entry too large"}),
            );
        }
        serde_json::json!({
            "error": {
                "code": 400,
                "status": "INVALID_ARGUMENT",
                "details": [{
                    "@type": "type.googleapis.com/google.logging.v2.WriteLogEntriesPartialErrors",
                    "logEntryErrors": errors
                }]
            }
        })
        .to_string()
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

    fn clock() -> TrustedClock {
        let c = TrustedClock::new();
        c.observe_http_date(HTTP_DATE).unwrap();
        c
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
        .unwrap()
    }

    fn spool_at(dir: &std::path::Path) -> Spool {
        Spool::open(
            dir,
            SpoolConfig {
                max_mb: 50,
                reserve_high_mb: 10,
                segment_mb: 5,
            },
        )
        .expect("spool opens")
    }

    fn logger_with(dir: &std::path::Path, t: Arc<FakeTransport>) -> Logger {
        let c = clock();
        let client = GclClient::new(TokenSource::new(sa(), t.clone(), c.clone()), t, c.clone());
        Logger::new(ctx(), spool_at(dir), client, RateLimiter::new(c.clone()), c)
    }

    /// Every JSONL line under the spool root, of any ring.
    fn spooled_lines(dir: &std::path::Path) -> Vec<Value> {
        let mut out = Vec::new();
        for ring in ["high", "low"] {
            let d = dir.join("spool").join(ring);
            let Ok(rd) = std::fs::read_dir(&d) else {
                continue;
            };
            let mut names: Vec<_> = rd
                .filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| n.ends_with(".jsonl"))
                .collect();
            names.sort();
            for n in names {
                let text = std::fs::read_to_string(d.join(n)).unwrap();
                for line in text.lines().filter(|l| !l.trim().is_empty()) {
                    out.push(serde_json::from_str(line).unwrap());
                }
            }
        }
        out
    }

    /// What a fresh `Spool` over the same directory would still re-deliver, i.e.
    /// the uncommitted entries. NOT the same as `spooled_lines`: `commit_drained`
    /// advances a persisted cursor rather than rewriting segments, so a committed
    /// entry is still physically present in its file.
    fn uncommitted(dir: &std::path::Path) -> Vec<String> {
        spool_at(dir)
            .drain_batch(1000)
            .unwrap()
            .iter()
            .map(|e| e.insert_id.clone())
            .collect()
    }

    /// TEL-10. A watchdog SIGKILL runs neither the panic hook nor `Drop`, so the
    /// entry explaining why the device died has to be durable *by the time `log`
    /// returns* — not at the next flush, not at shutdown. This test reads the
    /// spool files straight off disk, with no cooperation from the `Logger` (no
    /// flush, no drop): what it asserts is exactly what a `kill -9` would leave.
    #[test]
    fn a_high_severity_event_is_on_disk_before_log_returns() {
        let dir = tempfile::tempdir().unwrap();
        let t = FakeTransport::new(vec![ok(200)]);
        let mut lg = logger_with(dir.path(), t.clone());

        lg.log(Event::WatchdogSafeMode, Map::new());

        let lines = spooled_lines(dir.path());
        assert_eq!(lines.len(), 1, "the entry must already be on disk");
        assert_eq!(lines[0]["json_payload"]["event"], "watchdog.safe_mode");
        assert_eq!(lines[0]["severity"], "CRITICAL");
        assert_eq!(
            t.write_count(),
            0,
            "and it must be durable WITHOUT the network having been touched"
        );
        // It must be in the PROTECTED (high) ring, or an INFO flood could evict it.
        let high = dir.path().join("spool").join("high");
        let bytes: u64 = std::fs::read_dir(&high)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".jsonl"))
            .map(|e| e.metadata().unwrap().len())
            .sum();
        assert!(bytes > 0, "a CRITICAL entry belongs in the protected ring");
    }

    #[test]
    fn flush_writes_the_spooled_entries_and_commits_them() {
        let dir = tempfile::tempdir().unwrap();
        let t = FakeTransport::new(vec![ok(200)]);
        let mut lg = logger_with(dir.path(), t.clone());

        lg.log(Event::AppStart, Map::new());
        lg.log(Event::NetOffline, Map::new());
        lg.flush().expect("200");

        assert_eq!(t.write_count(), 1);
        let ids = t.insert_ids(0);
        assert_eq!(ids.len(), 2, "both entries posted, got {ids:?}");

        // Committed: a second flush has nothing left to send.
        lg.flush().expect("nothing left");
        assert_eq!(
            t.write_count(),
            1,
            "a committed batch must not be sent again"
        );
        // And the same is true across a reopen — the commit is on disk.
        let mut re = spool_at(dir.path());
        assert!(
            re.drain_batch(100).unwrap().is_empty(),
            "the commit must be durable, not just in memory"
        );
    }

    #[test]
    fn a_failed_flush_keeps_the_entries_spooled_for_retry() {
        let dir = tempfile::tempdir().unwrap();
        // 500 first, then 200: the retry succeeds.
        let t = FakeTransport::new(vec![resp(500, "backend unavailable"), ok(200)]);
        let mut lg = logger_with(dir.path(), t.clone());

        lg.log(Event::AppStart, Map::new());
        lg.log(Event::WatchdogHang, Map::new());
        lg.flush().expect_err("500");

        let first = t.insert_ids(0);
        assert_eq!(first.len(), 2);

        // Nothing was committed away.
        assert_eq!(
            uncommitted(dir.path()).len(),
            2,
            "a failed flush must not lose entries"
        );

        lg.flush().expect("the retry succeeds");
        let second = t.insert_ids(1);
        assert_eq!(
            second, first,
            "the retry MUST reuse the same insertIds byte-identically — that is \
             the only thing that makes at-least-once delivery effectively-once \
             (TEL-03)"
        );
        // Both must be real, distinct ids, or the equality above is vacuous.
        assert_eq!(first[0], "lobby-01-1");
        assert_eq!(first[1], "lobby-01-2");
    }

    #[test]
    fn a_flush_fires_after_100_entries() {
        let dir = tempfile::tempdir().unwrap();
        let t = FakeTransport::new(vec![ok(200)]);
        let mut lg = logger_with(dir.path(), t.clone());

        for _ in 0..99 {
            lg.log(Event::AppStart, Map::new());
            lg.tick();
        }
        assert_eq!(
            t.write_count(),
            0,
            "99 entries and well under 10 s: nothing should have flushed yet"
        );

        lg.log(Event::AppStart, Map::new());
        lg.tick();
        assert_eq!(
            t.write_count(),
            1,
            "the 100th entry must trigger a flush (TEL-07)"
        );
        assert_eq!(t.insert_ids(0).len(), 100);
    }

    /// `log()` is called from the kiosk's hot paths. A logging failure must never
    /// become a kiosk failure — so a spool that cannot be written to (here: its
    /// directory is replaced by a FILE, so every create/append underneath it
    /// fails with ENOTDIR) must not panic and must not propagate.
    #[test]
    fn logging_never_panics_even_when_the_spool_is_broken() {
        let dir = tempfile::tempdir().unwrap();
        let t = FakeTransport::new(vec![ok(200)]);
        let mut lg = logger_with(dir.path(), t.clone());

        // Detonate the spool underneath the live Logger.
        std::fs::remove_dir_all(dir.path().join("spool")).unwrap();
        std::fs::write(dir.path().join("spool"), b"not a directory").unwrap();

        // Both tiers, and both entry paths.
        lg.log(Event::AppStart, Map::new());
        lg.log(Event::WatchdogSafeMode, Map::new());
        lg.tick(); // must not panic either
                   // A flush over a broken spool is an Err, never a panic.
        let _ = lg.flush();
    }

    #[test]
    fn rate_limited_events_produce_a_coalesced_summary_entry_on_tick() {
        let dir = tempfile::tempdir().unwrap();
        let t = FakeTransport::new(vec![ok(200)]);
        let mut lg = logger_with(dir.path(), t.clone());

        // nav.blocked: burst 20 (TEL-09). 60 attempts => 20 logged, 40 suppressed.
        for _ in 0..60 {
            lg.log(Event::NavBlocked, Map::new());
        }
        assert_eq!(lg.pending(), 20, "the cap must actually have bitten");

        lg.tick();
        lg.flush().unwrap();

        let sent = t.write_body(0);
        let entries = sent["entries"].as_array().unwrap();
        let summary = entries
            .iter()
            .find(|e| e["json_payload"]["suppressed_count"].is_number())
            .expect("a suppressed flood must surface as a coalesced summary entry");
        assert_eq!(summary["json_payload"]["event"], "nav.blocked");
        assert_eq!(summary["json_payload"]["suppressed_count"], 40);
        assert_eq!(
            entries.len(),
            21,
            "20 admitted + exactly ONE coalesced summary"
        );
        // The summary is a real entry: severity and insertId like any other.
        assert_eq!(summary["severity"], "WARNING");
        assert!(summary["insert_id"]
            .as_str()
            .unwrap()
            .starts_with("lobby-01-"));
    }

    #[test]
    fn an_end_to_end_pass_reaches_the_transport() {
        let dir = tempfile::tempdir().unwrap();
        let t = FakeTransport::new(vec![ok(200)]);
        let mut lg = logger_with(dir.path(), t.clone());

        let mut fields = Map::new();
        fields.insert("exit_code".into(), Value::from(86));
        lg.log(Event::WatchdogRestart, fields);

        // The 10 s timer has not elapsed, so force the flush the way the owning
        // thread's timer eventually would.
        lg.flush().expect("200");

        assert_eq!(t.write_count(), 1, "the request must have reached the wire");
        let body = t.write_body(0);
        assert_eq!(body["partialSuccess"], Value::Bool(true));
        let e = &body["entries"].as_array().unwrap()[0];
        assert_eq!(e["json_payload"]["event"], "watchdog.restart");
        assert_eq!(e["json_payload"]["exit_code"], 86);
        assert_eq!(e["log_name"], "projects/proj/logs/kiosk");
        assert_eq!(e["resource"]["labels"]["node_id"], "lobby-01");
        assert_eq!(e["insert_id"], "lobby-01-1");
    }

    // --- The poison entry (see `Logger::flush`) ---

    /// THE failure this task exists to prevent: Cloud Logging permanently rejects
    /// ONE entry, and the naive "keep the batch, retry" handler then retries the
    /// same batch forever. The device goes permanently, silently quiet.
    ///
    /// After a bounded number of attempts the poison entry must be quarantined so
    /// the healthy entries behind it can flow again.
    #[test]
    fn a_permanently_rejected_entry_is_quarantined_and_stops_wedging_the_drain() {
        let dir = tempfile::tempdir().unwrap();
        // Entry index 0 (the oldest) is poison: rejected 400 every single time.
        // Then, once it is gone, a plain 200.
        let t = FakeTransport::new(vec![
            resp(400, &partial_body(&[0])),
            resp(400, &partial_body(&[0])),
            resp(400, &partial_body(&[0])),
            ok(200),
        ]);
        let mut lg = logger_with(dir.path(), t.clone());

        lg.log(Event::AppStart, Map::new()); // lobby-01-1: the poison entry
        lg.log(Event::NetOffline, Map::new()); // lobby-01-2: innocent, WARNING

        for _ in 0..MAX_REJECT_ATTEMPTS {
            let _ = lg.flush();
        }

        assert_eq!(
            lg.dropped_rejected(),
            1,
            "the poison entry must eventually be dropped, or telemetry is dead \
             forever"
        );
        assert_eq!(lg.quarantined().len(), 1);
        assert_eq!(lg.quarantined()[0].insert_id, "lobby-01-1");
        assert_eq!(lg.quarantined()[0].status, 400);
        assert_eq!(lg.quarantined()[0].attempts, MAX_REJECT_ATTEMPTS);

        // The drain is unwedged: nothing is left to re-deliver, and a fresh entry
        // flies.
        assert!(
            uncommitted(dir.path()).is_empty(),
            "the poison entry (and its accepted batch-mate) must be off the spool"
        );
        lg.log(Event::AppStop, Map::new());
        lg.flush().expect("telemetry flows again");
        assert!(t.insert_ids(3).contains(&"lobby-01-3".to_string()));
    }

    /// The other half: a poison entry must NOT drag its innocent batch-mates into
    /// the retry with it. Cloud Logging ACCEPTED them (they are not in
    /// `failed_indices`); re-sending them forever is pure waste, and if the
    /// poison never cleared they would be stuck behind it.
    #[test]
    fn the_entries_a_partial_failure_accepted_are_committed_not_retried() {
        let dir = tempfile::tempdir().unwrap();
        let t = FakeTransport::new(vec![
            resp(400, &partial_body(&[1])), // only the SECOND entry is bad
            ok(200),
        ]);
        let mut lg = logger_with(dir.path(), t.clone());

        lg.log(Event::AppStart, Map::new()); // -1 accepted
        lg.log(Event::NetOffline, Map::new()); // -2 REJECTED
        lg.log(Event::AppStop, Map::new()); // -3 accepted
        let _ = lg.flush();

        assert_eq!(
            uncommitted(dir.path()),
            vec!["lobby-01-2".to_string()],
            "only the REJECTED entry may stay spooled; the accepted ones were \
             stored by Cloud Logging and must be committed"
        );

        let _ = lg.flush();
        assert_eq!(
            t.insert_ids(1),
            vec!["lobby-01-2".to_string()],
            "the retry must carry ONLY the rejected entry"
        );
    }

    /// A transient rejection must NOT be treated as poison. A 429/5xx says the
    /// server had no capacity, not that the entry is bad — dropping it after N
    /// attempts would throw away perfectly good telemetry during an outage,
    /// which is exactly when telemetry matters.
    #[test]
    fn a_transiently_rejected_entry_is_never_quarantined() {
        for status in [429u16, 500, 503] {
            let dir = tempfile::tempdir().unwrap();
            let t = FakeTransport::new(vec![resp(status, &partial_body(&[0]))]);
            let mut lg = logger_with(dir.path(), t.clone());

            lg.log(Event::AppStart, Map::new());
            for _ in 0..(MAX_REJECT_ATTEMPTS * 3) {
                let _ = lg.flush();
            }

            assert_eq!(
                lg.dropped_rejected(),
                0,
                "HTTP {status} is a capacity verdict, not a content verdict: the \
                 entry must be kept and retried forever, not quarantined"
            );
            assert_eq!(
                uncommitted(dir.path()),
                vec!["lobby-01-1".to_string()],
                "HTTP {status}: the entry must still be spooled for retry"
            );
        }
    }

    /// Quarantining is a last resort, not a first response: a rejection that
    /// clears on retry must not have cost the entry anything.
    #[test]
    fn a_rejection_that_later_succeeds_is_not_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let t = FakeTransport::new(vec![resp(400, &partial_body(&[0])), ok(200)]);
        let mut lg = logger_with(dir.path(), t.clone());

        lg.log(Event::AppStart, Map::new());
        let _ = lg.flush(); // attempt 1: rejected, kept
        assert_eq!(uncommitted(dir.path()), vec!["lobby-01-1".to_string()]);
        assert_eq!(lg.dropped_rejected(), 0, "one rejection is not a verdict");

        lg.flush().expect("the second attempt succeeds");
        assert_eq!(lg.dropped_rejected(), 0);
        assert_eq!(t.insert_ids(1), vec!["lobby-01-1".to_string()]);
    }

    /// The batch-loss hole. `partial_error_indices` (T7) builds its vec from the
    /// `logEntryErrors` keys, so an empty `logEntryErrors: {}` — or keys that are
    /// not integers — yields a `PartialFailure` with an EMPTY index set. The
    /// "not named ⇒ Cloud Logging accepted it" inference then applies to the whole
    /// batch and commits every entry off the spool, permanently, for entries that
    /// were never stored. Silent loss of an entire batch, no counter touched.
    ///
    /// An unattributable failure means we know the batch failed but not which
    /// entries. Nothing may be committed and nothing may be charged.
    #[test]
    fn an_unattributable_partial_failure_commits_nothing() {
        let unattributable = [
            // logEntryErrors: {} — the parser returns Some(vec![]).
            serde_json::json!({"error":{"code":400,"status":"INVALID_ARGUMENT","details":[{
                "@type":"type.googleapis.com/google.logging.v2.WriteLogEntriesPartialErrors",
                "logEntryErrors": {}
            }]}})
            .to_string(),
            // Indices that name no entry in a batch of 2.
            partial_body(&[7, 9]),
        ];

        for body in unattributable {
            let dir = tempfile::tempdir().unwrap();
            let t = FakeTransport::new(vec![resp(400, &body)]);
            let mut lg = logger_with(dir.path(), t.clone());

            lg.log(Event::AppStart, Map::new());
            lg.log(Event::NetOffline, Map::new());
            for _ in 0..(MAX_REJECT_ATTEMPTS * 2) {
                let _ = lg.flush();
            }

            assert_eq!(
                uncommitted(dir.path()),
                vec!["lobby-01-1".to_string(), "lobby-01-2".to_string()],
                "a permanent failure that names no entry in the batch must commit \
                 NOTHING — the entries were not stored, and committing them is \
                 unrecoverable silent loss of the whole batch. Body: {body}"
            );
            assert_eq!(
                lg.dropped_rejected(),
                0,
                "and nothing may be charged an attempt either"
            );
            assert!(lg.quarantined().is_empty());
        }
    }

    /// An out-of-range index alongside a real one must not derail the triage: the
    /// in-range entry is still charged, the bogus index is simply ignored.
    #[test]
    fn an_out_of_range_index_alongside_a_real_one_is_ignored_not_wrapped() {
        let dir = tempfile::tempdir().unwrap();
        let t = FakeTransport::new(vec![resp(400, &partial_body(&[1, 42]))]);
        let mut lg = logger_with(dir.path(), t.clone());

        lg.log(Event::AppStart, Map::new()); // -1: accepted
        lg.log(Event::NetOffline, Map::new()); // -2: rejected
        let _ = lg.flush();

        assert_eq!(
            uncommitted(dir.path()),
            vec!["lobby-01-2".to_string()],
            "index 42 names nothing and must not be wrapped onto an innocent entry"
        );
    }

    /// IMPORTANT-2. The device whose telemetry matters most is the one the
    /// watchdog is killing every few seconds. An in-memory retry budget resets on
    /// every boot, so the poison entry never reaches MAX_REJECT_ATTEMPTS (that
    /// takes ~3 flushes), is re-sent from the spool on every boot, and wedges the
    /// drain FOREVER — this design's own failure mode, through the back door of a
    /// restart loop.
    #[test]
    fn the_retry_budget_survives_a_restart() {
        let dir = tempfile::tempdir().unwrap();

        // Boot 1: two permanent rejections. Not yet at the limit.
        let t = FakeTransport::new(vec![resp(400, &partial_body(&[0]))]);
        let mut lg = logger_with(dir.path(), t.clone());
        lg.log(Event::AppStart, Map::new());
        let _ = lg.flush();
        let _ = lg.flush();
        assert_eq!(lg.dropped_rejected(), 0, "2 of 3 attempts: not yet dropped");
        drop(lg); // the watchdog's SIGKILL: no Drop hook does anything for us

        // Boot 2: ONE more rejection must be enough. If the budget reset, the
        // entry survives this flush and telemetry is wedged forever.
        let t = FakeTransport::new(vec![resp(400, &partial_body(&[0])), ok(200)]);
        let mut lg = logger_with(dir.path(), t.clone());
        let _ = lg.flush();

        assert_eq!(
            lg.dropped_rejected(),
            1,
            "the retry budget must be DURABLE: a crash loop resets an in-memory \
             counter on every boot, so the poison entry would never reach the \
             limit and would wedge the drain forever"
        );
        assert!(
            uncommitted(dir.path()).is_empty(),
            "and the drain must be unwedged"
        );
    }

    /// IMPORTANT-3. `dropped_rejected` is how the operator ever learns of the
    /// loss (P1-D surfaces it in `health.sample`, as T4 does for
    /// `spool.dropped_expired`). A quarantine followed by a kill before the next
    /// sample must not erase its own evidence.
    #[test]
    fn the_loss_counters_survive_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let t = FakeTransport::new(vec![resp(400, &partial_body(&[0]))]);
        let mut lg = logger_with(dir.path(), t.clone());
        lg.log(Event::AppStart, Map::new());
        for _ in 0..MAX_REJECT_ATTEMPTS {
            let _ = lg.flush();
        }
        assert_eq!(lg.dropped_rejected(), 1);
        assert_eq!(lg.quarantined()[0].insert_id, "lobby-01-1");
        drop(lg);

        let t = FakeTransport::new(vec![ok(200)]);
        let lg = logger_with(dir.path(), t);
        assert_eq!(
            lg.dropped_rejected(),
            1,
            "loss is reported, not swallowed — a counter the crash resets makes \
             the loss silent exactly when it happened"
        );
        assert_eq!(lg.quarantined()[0].insert_id, "lobby-01-1");
        assert_eq!(lg.quarantined()[0].status, 400);
    }

    /// IMPORTANT-4. `pending()` means "awaiting delivery". A flush that drained
    /// 100 of a larger backlog must leave the rest counted, or the 100-entry
    /// trigger disarms and the backlog crawls out at one batch per 10 s.
    #[test]
    fn pending_counts_undelivered_entries_not_appends_since_the_last_flush() {
        let dir = tempfile::tempdir().unwrap();
        let t = FakeTransport::new(vec![ok(200)]);
        let mut lg = logger_with(dir.path(), t.clone());

        for _ in 0..250 {
            lg.log(Event::AppStart, Map::new());
        }
        assert_eq!(lg.pending(), 250);

        lg.flush().unwrap();
        assert_eq!(
            lg.pending(),
            150,
            "a flush delivers at most MAX_BATCH; the remaining backlog is still \
             awaiting delivery and the entry trigger must stay armed"
        );
        lg.tick(); // still >= 100 pending => flushes again immediately
        assert_eq!(t.write_count(), 2);
        assert_eq!(lg.pending(), 50);

        // A failed flush puts its batch back: those entries are still undelivered.
        let dir = tempfile::tempdir().unwrap();
        let t = FakeTransport::new(vec![resp(500, "boom")]);
        let mut lg = logger_with(dir.path(), t);
        lg.log(Event::AppStart, Map::new());
        let _ = lg.flush();
        assert_eq!(lg.pending(), 1, "a failed flush delivered nothing");
    }

    /// A spool left behind by a killed process is still awaiting delivery, so the
    /// new Logger must count it — otherwise a restart hides a full backlog from
    /// the entry trigger.
    #[test]
    fn a_reopened_logger_counts_the_backlog_a_previous_process_left() {
        let dir = tempfile::tempdir().unwrap();
        let t = FakeTransport::new(vec![resp(500, "boom")]);
        let mut lg = logger_with(dir.path(), t);
        for _ in 0..3 {
            lg.log(Event::AppStart, Map::new());
        }
        let _ = lg.flush();
        drop(lg);

        let t = FakeTransport::new(vec![ok(200)]);
        let lg = logger_with(dir.path(), t);
        assert_eq!(lg.pending(), 3, "the orphaned spool is undelivered work");
    }

    #[test]
    fn a_severity_is_routed_to_the_ring_its_severity_demands() {
        let dir = tempfile::tempdir().unwrap();
        let t = FakeTransport::new(vec![ok(200)]);
        let mut lg = logger_with(dir.path(), t);
        lg.log(Event::AppStart, Map::new()); // INFO
        lg.log(Event::NetOffline, Map::new()); // WARNING

        let sev: Vec<Severity> = spooled_lines(dir.path())
            .iter()
            .map(|v| serde_json::from_value(v["severity"].clone()).unwrap())
            .collect();
        assert!(sev.contains(&Severity::Info));
        assert!(sev.contains(&Severity::Warning));
    }

    /// TEL-10: the Logger is owned by a dedicated thread, independent of the
    /// frequently-hung webview thread. If it stops being `Send`, that design is
    /// dead and this fails to compile.
    #[test]
    fn the_logger_is_send_so_a_dedicated_thread_can_own_it() {
        fn assert_send<T: Send>() {}
        assert_send::<Logger>();

        let dir = tempfile::tempdir().unwrap();
        let t = FakeTransport::new(vec![ok(200)]);
        let mut lg = logger_with(dir.path(), t.clone());
        std::thread::spawn(move || {
            lg.log(Event::AppStart, Map::new());
            lg.flush().unwrap();
        })
        .join()
        .unwrap();
        assert_eq!(t.write_count(), 1);
    }

    #[test]
    fn a_failed_flush_backs_off_instead_of_hammering_the_endpoint() {
        let dir = tempfile::tempdir().unwrap();
        let t = FakeTransport::new(vec![resp(500, "boom")]);
        let mut lg = logger_with(dir.path(), t.clone());

        for _ in 0..100 {
            lg.log(Event::AppStart, Map::new());
        }
        lg.tick(); // 100 entries => flush => 500 => backoff
        assert_eq!(t.write_count(), 1);

        for _ in 0..100 {
            lg.log(Event::AppStart, Map::new());
            lg.tick();
        }
        assert_eq!(
            t.write_count(),
            1,
            "inside the backoff window no further request may be made"
        );
    }
}
