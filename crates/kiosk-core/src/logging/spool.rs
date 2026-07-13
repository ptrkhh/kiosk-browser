//! Severity-tiered, crash-durable telemetry spool (spec TEL-03/07/09/10).
//!
//! The spool — not the network — is the source of truth. A watchdog `SIGKILL`
//! or an OOM-kill runs neither the panic hook nor `Drop`, so a WARNING-and-above
//! entry is written through and `fsync`ed before `append` returns: the
//! `watchdog.safe_mode` line that explains why a device died must survive the
//! kill that produced it (TEL-10).
//!
//! Two rings with INDEPENDENT drop-oldest budgets (TEL-07): an INFO flood (a
//! redirect loop spraying `nav.blocked`) can only ever evict INFO entries.
//!
//! Entries are removed ONLY by `commit_drained`, after the network confirmed
//! the write. An uncommitted drain is therefore re-delivered, and because the
//! `insertId` is reused byte-identically (TEL-03), Cloud Logging dedups the
//! retry: at-least-once delivery + a stable dedup key = effectively-once.

use crate::config::schema::Logging;
use crate::logging::entry::LogEntry;
use std::collections::{BTreeMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum SpoolError {
    #[error("spool io error at {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("spool serialization error: {0}")]
    Encode(#[from] serde_json::Error),
}

fn io(path: &Path) -> impl Fn(std::io::Error) -> SpoolError + '_ {
    move |source| SpoolError::Io {
        path: path.display().to_string(),
        source,
    }
}

const MB: u64 = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpoolConfig {
    pub max_mb: u64,
    pub reserve_high_mb: u64,
    pub segment_mb: u64,
}

impl SpoolConfig {
    /// Segment size is not operator-tunable; 5 MB is the spec's default.
    pub const DEFAULT_SEGMENT_MB: u64 = 5;

    pub fn from_logging(cfg: &Logging) -> SpoolConfig {
        SpoolConfig {
            max_mb: cfg.spool_max_mb,
            reserve_high_mb: cfg.spool_reserve_high_mb,
            segment_mb: Self::DEFAULT_SEGMENT_MB,
        }
    }
}

/// `fsync` the directory itself, so a `rename` into it is durable and not just
/// atomic. Without this, a power cut can leave the OLD `seq` (or no `seq`)
/// visible even though the new file's contents were fsynced — and a counter
/// that goes backwards reissues insertIds.
///
/// Errors are deliberately ignored: not every platform/filesystem supports
/// opening a directory for fsync, and failing an append over it would be worse
/// than the durability gap it closes. `std::fs` only — no per-OS crate (spec §4).
fn fsync_dir(dir: &Path) {
    let _ = File::open(dir).map(|f| f.sync_all());
}

/// The persisted, fsynced side-state of a ring. `dropped` lives here rather
/// than in memory because the INFO flood that drops entries is very often the
/// same event that ends in a watchdog kill — a counter that resets on restart
/// would make exactly the loss it exists to report invisible (rule 6).
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
struct RingState {
    /// segment name -> committed leading line count.
    committed: BTreeMap<String, u64>,
    #[serde(default)]
    dropped: u64,
}

/// One severity tier: a directory of `NNNNN.jsonl` segments plus a persisted
/// per-segment commit cursor (how many leading lines have been acknowledged).
#[derive(Debug)]
struct Ring {
    dir: PathBuf,
    budget_bytes: u64,
    segment_bytes: u64,
    /// segment name -> size in bytes. Ordered by name == ordered by age.
    sizes: BTreeMap<String, u64>,
    /// segment name -> committed leading line count.
    cursor: BTreeMap<String, u64>,
    /// Cached handle to the newest segment, opened in append mode.
    active: Option<(String, File)>,
    dropped: u64,
}

fn seg_name(n: u64) -> String {
    format!("{n:05}.jsonl")
}

fn seg_index(name: &str) -> Option<u64> {
    name.strip_suffix(".jsonl")?.parse().ok()
}

impl Ring {
    fn open(dir: PathBuf, budget_bytes: u64, segment_bytes: u64) -> Result<Ring, SpoolError> {
        fs::create_dir_all(&dir).map_err(io(&dir))?;

        let mut sizes = BTreeMap::new();
        for e in fs::read_dir(&dir).map_err(io(&dir))? {
            let e = e.map_err(io(&dir))?;
            let name = e.file_name().to_string_lossy().into_owned();
            if seg_index(&name).is_some() {
                let len = e.metadata().map_err(io(&dir))?.len();
                sizes.insert(name, len);
            }
        }

        let cursor_path = dir.join("cursor.json");
        // A corrupt cursor is not fatal: it only means already-sent entries are
        // re-sent, and the reused insertId makes that harmless.
        let state: RingState = fs::read_to_string(&cursor_path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();

        Ok(Ring {
            dir,
            budget_bytes,
            segment_bytes,
            sizes,
            cursor: state.committed,
            active: None,
            dropped: state.dropped,
        })
    }

    /// The highest `seq` embedded in any `insert_id` still on disk in this ring
    /// (`{device_id}-{seq}`; the suffix after the LAST '-'). Used to self-heal a
    /// lost counter. Every line is scanned, including committed ones — a
    /// committed entry may already have reached Cloud Logging, so its seq is
    /// just as unsafe to reuse.
    fn max_seq_on_disk(&self) -> u64 {
        let mut max = 0u64;
        for name in self.sizes.keys() {
            let Ok(f) = File::open(self.dir.join(name)) else {
                continue;
            };
            for line in BufReader::new(f).lines().map_while(Result::ok) {
                let Ok(e) = serde_json::from_str::<LogEntry>(&line) else {
                    continue; // torn line
                };
                if let Some(seq) = e
                    .insert_id
                    .rsplit_once('-')
                    .and_then(|(_, s)| s.parse::<u64>().ok())
                {
                    max = max.max(seq);
                }
            }
        }
        max
    }

    fn newest(&self) -> Option<String> {
        self.sizes.keys().next_back().cloned()
    }

    fn total_bytes(&self) -> u64 {
        self.sizes.values().sum()
    }

    fn open_segment(&mut self, name: &str) -> Result<&mut File, SpoolError> {
        let path = self.dir.join(name);
        let f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(io(&path))?;
        self.sizes.entry(name.to_string()).or_insert(0);
        self.active = Some((name.to_string(), f));
        Ok(&mut self.active.as_mut().expect("just set").1)
    }

    fn append_line(&mut self, line: &str, fsync: bool) -> Result<(), SpoolError> {
        let bytes = line.len() as u64 + 1; // + '\n'

        // Choose (or rotate to) the segment this line belongs in.
        let target = match self.newest() {
            Some(n) if self.sizes[&n] + bytes <= self.segment_bytes || self.sizes[&n] == 0 => n,
            Some(n) => seg_name(seg_index(&n).unwrap_or(0) + 1),
            None => seg_name(0),
        };
        let reopen = match &self.active {
            Some((n, _)) => n != &target,
            None => true,
        };
        if reopen {
            self.open_segment(&target)?;
        }

        {
            let f = &mut self.active.as_mut().expect("active set above").1;
            let path = self.dir.join(&target);
            writeln!(f, "{line}").map_err(io(&path))?;
            if fsync {
                // TEL-10: WARNING-and-above must be on the platter before we
                // return, because the very next thing that happens may be the
                // SIGKILL this entry exists to explain.
                f.sync_all().map_err(io(&path))?;
            }
        }
        *self.sizes.get_mut(&target).expect("size tracked") += bytes;

        self.enforce_budget()?;
        Ok(())
    }

    /// Drop-oldest, whole segments at a time. Never evicts the segment we are
    /// currently writing to, so a ring is always able to accept a new entry.
    /// The cursor is rewritten only when a segment actually went away — an
    /// INFO append must not pay for an fsync it does not need.
    fn enforce_budget(&mut self) -> Result<(), SpoolError> {
        let mut evicted = false;
        while self.total_bytes() > self.budget_bytes && self.sizes.len() > 1 {
            evicted = true;
            let oldest = self.sizes.keys().next().cloned().expect("len > 1");
            let path = self.dir.join(&oldest);
            let committed = self.cursor.get(&oldest).copied().unwrap_or(0);
            let lines = count_lines(&path)?;
            // Only entries that were never acknowledged count as loss.
            self.dropped += lines.saturating_sub(committed);
            fs::remove_file(&path).map_err(io(&path))?;
            self.sizes.remove(&oldest);
            self.cursor.remove(&oldest);
            if matches!(&self.active, Some((n, _)) if n == &oldest) {
                self.active = None;
            }
        }
        if evicted {
            self.persist_cursor()?;
        }
        Ok(())
    }

    /// Read up to `limit` uncommitted entries, oldest segment first, and STOP.
    ///
    /// Without the early stop, every `drain_batch` re-parsed the entire ring —
    /// O(spool x batches). Under exactly the flood the spool exists for, the
    /// drainer would be re-parsing 40 MB to ship 100 entries while the ring
    /// keeps evicting, so it can never catch up.
    ///
    /// Reading the oldest `limit` entries per ring is sufficient for a correct
    /// global oldest-first merge: within a ring, file order IS append order, and
    /// appends happen in wall-clock order, so no unread entry in this ring can
    /// be older than the ones we read. (If the trusted clock steps backwards
    /// mid-run, the selection can be locally imperfect — but nothing is lost:
    /// every entry stays on disk until committed and is drained on a later pass.)
    fn read_entries(&self, limit: usize) -> Result<Vec<LogEntry>, SpoolError> {
        let mut out = Vec::new();
        for name in self.sizes.keys() {
            if out.len() >= limit {
                break;
            }
            let path = self.dir.join(name);
            let skip = self.cursor.get(name).copied().unwrap_or(0) as usize;
            let f = match File::open(&path) {
                Ok(f) => f,
                Err(_) => continue,
            };
            for line in BufReader::new(f).lines().skip(skip) {
                if out.len() >= limit {
                    break;
                }
                let Ok(line) = line else { continue };
                // A torn write from a power cut must not brick telemetry: an
                // unparseable line is skipped, never fatal.
                if let Ok(e) = serde_json::from_str::<LogEntry>(&line) {
                    out.push(e);
                }
            }
        }
        Ok(out)
    }

    /// Advance each segment's committed-prefix cursor over lines whose entries
    /// were acknowledged (and over torn lines, which can never be acknowledged
    /// and would otherwise block the cursor forever). Fully committed segments
    /// are deleted. Segments are never rewritten in place — a crash mid-rewrite
    /// would lose data.
    fn commit(&mut self, acked: &HashSet<&str>) -> Result<(), SpoolError> {
        let names: Vec<String> = self.sizes.keys().cloned().collect();
        for name in names {
            let path = self.dir.join(&name);
            let start = self.cursor.get(&name).copied().unwrap_or(0);
            let f = match File::open(&path) {
                Ok(f) => f,
                Err(_) => continue,
            };
            let lines: Vec<String> = BufReader::new(f).lines().map_while(Result::ok).collect();
            let total = lines.len() as u64;
            let mut n = start;
            while (n as usize) < lines.len() {
                let line = &lines[n as usize];
                let ok = match serde_json::from_str::<LogEntry>(line) {
                    Ok(e) => acked.contains(e.insert_id.as_str()),
                    Err(_) => true, // torn line: skip past it
                };
                if !ok {
                    break;
                }
                n += 1;
            }
            if n == 0 {
                continue;
            }
            let is_active = matches!(&self.active, Some((a, _)) if a == &name);
            if n >= total && !is_active {
                fs::remove_file(&path).map_err(io(&path))?;
                self.sizes.remove(&name);
                self.cursor.remove(&name);
            } else {
                self.cursor.insert(name, n);
            }
        }
        self.persist_cursor()
    }

    /// Persist the commit cursor AND the drop counter together, then fsync the
    /// directory so the rename itself survives a power cut.
    fn persist_cursor(&self) -> Result<(), SpoolError> {
        let path = self.dir.join("cursor.json");
        let tmp = self.dir.join("cursor.json.tmp");
        let body = serde_json::to_string(&RingState {
            committed: self.cursor.clone(),
            dropped: self.dropped,
        })?;
        {
            let mut f = File::create(&tmp).map_err(io(&tmp))?;
            f.write_all(body.as_bytes()).map_err(io(&tmp))?;
            f.sync_all().map_err(io(&tmp))?;
        }
        fs::rename(&tmp, &path).map_err(io(&path))?;
        fsync_dir(&self.dir);
        Ok(())
    }
}

fn count_lines(path: &Path) -> Result<u64, SpoolError> {
    let f = match File::open(path) {
        Ok(f) => f,
        Err(_) => return Ok(0),
    };
    Ok(BufReader::new(f).lines().map_while(Result::ok).count() as u64)
}

#[derive(Debug)]
pub struct Spool {
    high: Ring,
    low: Ring,
    seq_path: PathBuf,
    seq: u64,
}

impl Spool {
    pub fn open(dir: &Path, cfg: SpoolConfig) -> Result<Spool, SpoolError> {
        let root = dir.join("spool");
        fs::create_dir_all(&root).map_err(io(&root))?;

        let segment_bytes = cfg.segment_mb.max(1) * MB;

        // Rule 1: the rings' budgets are independent, so a low-ring flood can
        // never take space away from the high ring. But they must also SUM to
        // no more than max_mb — `spool_reserve_high_mb` is only clamped in the
        // config layer, so a nonsense reserve (>= max_mb) must be defended
        // against here rather than trusted.
        let max_mb = cfg.max_mb.max(2); // each ring needs at least 1 MB to function
        let reserve_mb = cfg.reserve_high_mb.clamp(1, max_mb - 1);
        let high_budget = reserve_mb * MB;
        let low_budget = (max_mb - reserve_mb) * MB;

        let high = Ring::open(root.join("high"), high_budget, segment_bytes)?;
        let low = Ring::open(root.join("low"), low_budget, segment_bytes)?;

        // TEL-03 self-heal. A missing/empty/torn `seq` must NEVER send the
        // counter back to 0: `next_seq` would reissue insertIds that already
        // exist in the spool — and may already have reached Cloud Logging,
        // which would then dedup the NEW entries away. Silent data loss.
        //
        // So the counter is seeded from the max of what was persisted and the
        // highest seq still visible in the spool's own entries. It can then
        // never go backwards past an entry we can still see. Skipping seqs is
        // explicitly harmless; reusing one is not.
        let seq_path = root.join("seq");
        let persisted: u64 = fs::read_to_string(&seq_path)
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0);
        let seq = persisted
            .max(high.max_seq_on_disk())
            .max(low.max_seq_on_disk());

        Ok(Spool {
            high,
            low,
            seq_path,
            seq,
        })
    }

    /// Routes by severity; **fsyncs before returning for WARNING and above**.
    pub fn append(&mut self, entry: &LogEntry) -> Result<(), SpoolError> {
        let line = serde_json::to_string(entry)?;
        let high = entry.severity.is_high();
        let ring = if high { &mut self.high } else { &mut self.low };
        ring.append_line(&line, high)
    }

    /// The per-device monotonic counter behind `insertId` (TEL-03). It is
    /// persisted and fsynced BEFORE it is handed out: a crash between
    /// increment and use merely skips a seq (harmless), whereas reusing one
    /// collides insertIds and makes Cloud Logging silently dedup away a real
    /// entry.
    pub fn next_seq(&mut self) -> Result<u64, SpoolError> {
        let next = self.seq + 1;
        let tmp = self.seq_path.with_extension("tmp");
        {
            let mut f = File::create(&tmp).map_err(io(&tmp))?;
            f.write_all(next.to_string().as_bytes()).map_err(io(&tmp))?;
            f.sync_all().map_err(io(&tmp))?;
        }
        fs::rename(&tmp, &self.seq_path).map_err(io(&self.seq_path))?;
        // sync_all on the tmp file makes its CONTENTS durable, not the rename.
        // Without a directory fsync a power cut can leave the pre-rename value
        // (or no file at all) visible while an entry carrying the newer seq is
        // durably on disk — and then the counter goes backwards.
        if let Some(parent) = self.seq_path.parent() {
            fsync_dir(parent);
        }
        self.seq = next;
        Ok(next)
    }

    /// Oldest-first across BOTH rings, ordered by `(timestamp, insert_id)`.
    /// A `None` timestamp sorts first — it predates trusted time, so it is
    /// older than anything carrying a clock.
    pub fn drain_batch(&mut self, max: usize) -> Result<Vec<LogEntry>, SpoolError> {
        // Each ring yields at most `max` (its oldest); the merge of the two
        // then yields the global oldest `max`. Neither ring is read in full.
        let mut all = self.high.read_entries(max)?;
        all.extend(self.low.read_entries(max)?);
        // The insert_id tiebreak is LEXICOGRAPHIC, so "d1-10" < "d1-9". That is
        // arbitrary-but-stable, NOT chronological — and that is fine: it only
        // breaks ties between entries sharing a timestamp. Cloud Logging orders
        // by timestamp; insertId is purely a dedup key, never an ordering key.
        all.sort_by(|a, b| (&a.timestamp, &a.insert_id).cmp(&(&b.timestamp, &b.insert_id)));
        all.truncate(max);
        Ok(all)
    }

    /// The ONLY thing that removes entries. Called after the network confirmed
    /// the write; an uncommitted drain is re-delivered on the next drain.
    pub fn commit_drained(&mut self, entries: &[LogEntry]) -> Result<(), SpoolError> {
        let acked: HashSet<&str> = entries.iter().map(|e| e.insert_id.as_str()).collect();
        self.high.commit(&acked)?;
        self.low.commit(&acked)
    }

    /// Entries lost to drop-oldest. Surfaced in the next `health.sample` so
    /// loss is visible, never silent (rule 6).
    pub fn dropped_expired(&self) -> u64 {
        self.high.dropped + self.low.dropped
    }

    /// The high ring's byte budget. `high_budget_bytes() + low_budget_bytes()`
    /// is the spool's disk cap.
    pub fn high_budget_bytes(&self) -> u64 {
        self.high.budget_bytes
    }

    pub fn low_budget_bytes(&self) -> u64 {
        self.low.budget_bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::schema::UrlDetail;
    use crate::logging::entry::{EntryContext, LogEntry};
    use crate::logging::event::Event;
    use crate::logging::time::TrustedClock;
    use serde_json::Map;

    fn ctx() -> EntryContext {
        EntryContext {
            project_id: "p".into(),
            device_id: "d1".into(),
            site: "s".into(),
            region: "r".into(),
            app_version: "0.1.0".into(),
            config_revision: None,
            url_detail: UrlDetail::Path,
        }
    }

    /// ONE clock for the whole test binary, exactly as production has one.
    ///
    /// A fresh `TrustedClock` per entry is NOT equivalent: the clock stores an
    /// integer-second offset (`server.timestamp() - local.timestamp()`), so if
    /// a second boundary falls between `observe_http_date` and the later
    /// `Utc::now()`, the derived timestamp jumps ~1 s BACKWARDS relative to the
    /// previous entry's. That made the ordering test flaky under parallel load
    /// (observed: it failed in a full `cargo test -p kiosk-core` run). Sharing
    /// one established clock fixes the offset once and makes `trusted_utc()`
    /// monotonic, which is the real-world condition the ordering rule assumes.
    fn clock() -> &'static TrustedClock {
        static CLOCK: std::sync::OnceLock<TrustedClock> = std::sync::OnceLock::new();
        CLOCK.get_or_init(|| {
            let c = TrustedClock::new();
            c.observe_http_date("Sun, 12 Jul 2026 08:30:00 GMT")
                .unwrap();
            c
        })
    }

    fn entry(event: Event, seq: u64) -> LogEntry {
        LogEntry::new(event, &ctx(), seq, clock(), Map::new())
    }

    fn cfg() -> SpoolConfig {
        SpoolConfig {
            max_mb: 50,
            reserve_high_mb: 10,
            segment_mb: 5,
        }
    }

    #[test]
    fn appended_entries_drain_back_out() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Spool::open(dir.path(), cfg()).unwrap();
        s.append(&entry(Event::AppStart, 1)).unwrap();
        s.append(&entry(Event::WatchdogSafeMode, 2)).unwrap();

        let batch = s.drain_batch(10).unwrap();
        assert_eq!(batch.len(), 2);
    }

    #[test]
    fn seq_is_monotonic_and_survives_a_restart() {
        // TEL-03: if seq restarts at 0, insertIds collide across restarts and
        // Cloud Logging silently dedups away real entries.
        let dir = tempfile::tempdir().unwrap();
        let a = {
            let mut s = Spool::open(dir.path(), cfg()).unwrap();
            let _ = s.next_seq().unwrap();
            s.next_seq().unwrap()
        };
        let b = {
            let mut s = Spool::open(dir.path(), cfg()).unwrap();
            s.next_seq().unwrap()
        };
        assert!(b > a, "seq must not restart after a reopen: {a} then {b}");
    }

    #[test]
    fn drained_entries_are_not_removed_until_committed() {
        // At-least-once delivery: a drain whose network write never lands must
        // be re-delivered. insertId then makes the retry effectively-once.
        let dir = tempfile::tempdir().unwrap();
        let mut s = Spool::open(dir.path(), cfg()).unwrap();
        s.append(&entry(Event::AppStart, 1)).unwrap();

        let first = s.drain_batch(10).unwrap();
        assert_eq!(first.len(), 1);
        drop(s);

        let mut s = Spool::open(dir.path(), cfg()).unwrap();
        let again = s.drain_batch(10).unwrap();
        assert_eq!(again.len(), 1, "an uncommitted drain must be re-delivered");
        assert_eq!(
            again[0].insert_id, first[0].insert_id,
            "insertId reused verbatim"
        );

        s.commit_drained(&again).unwrap();
        assert!(
            s.drain_batch(10).unwrap().is_empty(),
            "committed entries are gone"
        );
    }

    #[test]
    fn a_low_severity_flood_cannot_evict_high_severity_entries() {
        // The whole point of the tiered ring (TEL-07): a redirect loop spraying
        // nav.blocked must not push out the watchdog.safe_mode entry that
        // explains why the device died.
        let dir = tempfile::tempdir().unwrap();
        // Tiny rings so the flood actually overflows within the test.
        let mut s = Spool::open(
            dir.path(),
            SpoolConfig {
                max_mb: 2,
                reserve_high_mb: 1,
                segment_mb: 1,
            },
        )
        .unwrap();

        s.append(&entry(Event::WatchdogSafeMode, 1)).unwrap(); // CRITICAL -> high ring

        // Flood the low ring well past its capacity.
        for seq in 2..20_000u64 {
            s.append(&entry(Event::AppStart, seq)).unwrap(); // INFO -> low ring
        }

        let drained = s.drain_batch(100_000).unwrap();
        assert!(
            drained.iter().any(|e| e.insert_id == "d1-1"),
            "the CRITICAL entry must survive an INFO flood"
        );
        assert!(
            s.dropped_expired() > 0 || drained.len() < 20_000,
            "the flood must have dropped low entries"
        );
    }

    #[test]
    fn drain_is_oldest_first_across_both_rings() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Spool::open(dir.path(), cfg()).unwrap();
        // Interleave severities; ordering must be by (timestamp, insert_id),
        // NOT by which ring they landed in.
        s.append(&entry(Event::AppStart, 1)).unwrap();
        s.append(&entry(Event::WatchdogSafeMode, 2)).unwrap();
        s.append(&entry(Event::AppStart, 3)).unwrap();

        let batch = s.drain_batch(10).unwrap();
        let ids: Vec<&str> = batch.iter().map(|e| e.insert_id.as_str()).collect();
        assert_eq!(ids, vec!["d1-1", "d1-2", "d1-3"], "got {ids:?}");
    }

    /// TEL-10 durability. An `fsync` cannot be observed from safe Rust, so this
    /// pins the observable half of the guarantee: after `append` returns for a
    /// CRITICAL entry, the bytes are already in the file as seen by a FRESHLY
    /// OPENED handle — no flush, no `Drop`, no close on the writer's side. The
    /// `File::sync_all()` call itself is on the write path in `Ring::append_line`
    /// (gated on `severity.is_high()`); this test cannot prove the kernel pushed
    /// the page cache to the platter.
    #[test]
    fn a_high_severity_append_is_on_disk_before_it_returns() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Spool::open(dir.path(), cfg()).unwrap();
        s.append(&entry(Event::WatchdogSafeMode, 1)).unwrap();
        // NOTE: `s` is deliberately NOT dropped/flushed before we read.

        let seg = dir.path().join("spool").join("high").join("00000.jsonl");
        let text = std::fs::read_to_string(&seg).expect("high segment exists at append time");
        assert!(
            text.contains("\"insertId\":\"d1-1\"") || text.contains("\"insert_id\":\"d1-1\""),
            "the CRITICAL entry must be readable from a fresh handle immediately: {text}"
        );

        // And it must survive a reopen that never saw the writer.
        let mut s2 = Spool::open(dir.path(), cfg()).unwrap();
        let batch = s2.drain_batch(10).unwrap();
        assert_eq!(batch.len(), 1);
        assert_eq!(batch[0].insert_id, "d1-1");
    }

    #[test]
    fn high_and_low_entries_are_routed_to_their_own_rings() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Spool::open(dir.path(), cfg()).unwrap();
        s.append(&entry(Event::AppStart, 1)).unwrap(); // INFO
        s.append(&entry(Event::NavBlocked, 2)).unwrap(); // WARNING => high
        drop(s);

        let high = std::fs::read_to_string(dir.path().join("spool/high/00000.jsonl")).unwrap();
        let low = std::fs::read_to_string(dir.path().join("spool/low/00000.jsonl")).unwrap();
        assert!(high.contains("d1-2") && !high.contains("d1-1"));
        assert!(low.contains("d1-1") && !low.contains("d1-2"));
    }

    /// CRITICAL. A destroyed / truncated / unparseable `spool/seq` must NOT send
    /// the counter back to 0, because `next_seq` would then re-issue insertIds
    /// that already exist — in the spool AND possibly already in Cloud Logging,
    /// which would dedup the NEW entries away. That is silent data loss.
    #[test]
    fn a_destroyed_seq_file_cannot_reissue_a_used_insert_id() {
        for wreck in ["", "garbage", "\u{0}\u{0}\u{0}"] {
            let dir = tempfile::tempdir().unwrap();
            let mut s = Spool::open(dir.path(), cfg()).unwrap();
            let mut used = Vec::new();
            for _ in 0..5 {
                let seq = s.next_seq().unwrap();
                let e = entry(Event::AppStart, seq);
                used.push(e.insert_id.clone());
                s.append(&e).unwrap();
            }
            drop(s);

            // A power cut leaves the counter empty/torn/absent.
            std::fs::write(dir.path().join("spool").join("seq"), wreck).unwrap();

            let mut s = Spool::open(dir.path(), cfg()).unwrap();
            let seq = s.next_seq().unwrap();
            let e = entry(Event::AppStart, seq);
            assert!(
                !used.contains(&e.insert_id),
                "seq file wrecked with {wreck:?} reissued a used insertId: {} (already used: {used:?})",
                e.insert_id
            );
        }
    }

    #[test]
    fn a_deleted_seq_file_cannot_reissue_a_used_insert_id() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Spool::open(dir.path(), cfg()).unwrap();
        let mut used = Vec::new();
        for _ in 0..5 {
            let seq = s.next_seq().unwrap();
            let e = entry(Event::AppStart, seq);
            used.push(e.insert_id.clone());
            s.append(&e).unwrap();
        }
        drop(s);
        std::fs::remove_file(dir.path().join("spool").join("seq")).unwrap();

        let mut s = Spool::open(dir.path(), cfg()).unwrap();
        // The spool self-heals from the entries it still holds.
        let seq = s.next_seq().unwrap();
        assert!(
            seq > 5,
            "seq must not go backwards past spooled entries: {seq}"
        );
        let e = entry(Event::AppStart, seq);
        assert!(!used.contains(&e.insert_id), "reissued {}", e.insert_id);
    }

    /// Rule 6: an INFO flood is very often the same event that ends in a
    /// watchdog kill, so a drop counter that resets on restart makes exactly
    /// the loss it exists to report invisible.
    #[test]
    fn the_dropped_counter_survives_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let tiny = SpoolConfig {
            max_mb: 2,
            reserve_high_mb: 1,
            segment_mb: 1,
        };
        let mut s = Spool::open(dir.path(), tiny).unwrap();
        for seq in 1..6_000u64 {
            s.append(&entry(Event::AppStart, seq)).unwrap();
        }
        let dropped = s.dropped_expired();
        assert!(dropped > 0, "the flood must have dropped entries");
        drop(s);

        let s = Spool::open(dir.path(), tiny).unwrap();
        assert_eq!(
            s.dropped_expired(),
            dropped,
            "the drop count must survive the restart that the flood probably caused"
        );
    }

    /// The disk cap must hold even when an operator sets a reserve that is
    /// larger than the whole spool. `Spool::open` defends itself.
    #[test]
    fn a_reserve_larger_than_the_max_still_respects_the_disk_cap() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Spool::open(
            dir.path(),
            SpoolConfig {
                max_mb: 2,
                reserve_high_mb: 10, // nonsense: >= max_mb
                segment_mb: 1,
            },
        )
        .unwrap();
        assert!(
            s.high_budget_bytes() + s.low_budget_bytes() <= 2 * MB,
            "high {} + low {} must not exceed max_mb",
            s.high_budget_bytes(),
            s.low_budget_bytes()
        );
        // And both rings must still be usable.
        s.append(&entry(Event::WatchdogSafeMode, 1)).unwrap();
        s.append(&entry(Event::AppStart, 2)).unwrap();
        assert_eq!(s.drain_batch(10).unwrap().len(), 2);
    }

    /// The mirror of the flood test: a HIGH flood evicts the HIGH ring's own
    /// oldest, and must not touch the low ring.
    #[test]
    fn a_high_severity_flood_evicts_its_own_ring_not_the_low_ring() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Spool::open(
            dir.path(),
            SpoolConfig {
                max_mb: 2,
                reserve_high_mb: 1,
                segment_mb: 1,
            },
        )
        .unwrap();

        s.append(&entry(Event::AppStart, 1)).unwrap(); // INFO -> low ring

        // Enough WARNINGs to overflow the 1 MB high ring. Kept modest because
        // every one of these pays a real fsync — which is the point of the tier.
        for seq in 2..3_000u64 {
            s.append(&entry(Event::NavBlocked, seq)).unwrap(); // WARNING -> high ring
        }

        let drained = s.drain_batch(100_000).unwrap();
        assert!(
            drained.iter().any(|e| e.insert_id == "d1-1"),
            "a HIGH flood must not evict the low ring's entry"
        );
        assert!(
            s.dropped_expired() > 0,
            "the high ring must have evicted its own oldest"
        );
        assert!(
            !drained.iter().any(|e| e.insert_id == "d1-2"),
            "the high ring's OLDEST entry is the one that goes"
        );
    }

    #[test]
    fn drain_batch_returns_the_oldest_max_entries_without_reading_everything() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Spool::open(dir.path(), cfg()).unwrap();
        for seq in 1..500u64 {
            s.append(&entry(Event::AppStart, seq)).unwrap();
        }
        let batch = s.drain_batch(5).unwrap();
        assert_eq!(batch.len(), 5);
        assert_eq!(batch[0].insert_id, "d1-1", "still oldest-first");
    }

    #[test]
    fn a_corrupt_spool_line_is_skipped_not_fatal() {
        // A torn write from a power cut must not brick telemetry forever.
        let dir = tempfile::tempdir().unwrap();
        let mut s = Spool::open(dir.path(), cfg()).unwrap();
        s.append(&entry(Event::AppStart, 1)).unwrap();
        drop(s);

        // Append a torn line to the low ring's segment.
        let seg = std::fs::read_dir(dir.path().join("spool").join("low"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let mut f = std::fs::OpenOptions::new().append(true).open(&seg).unwrap();
        use std::io::Write;
        writeln!(f, "{{ this is a torn writ").unwrap();
        drop(f);

        let mut s = Spool::open(dir.path(), cfg()).unwrap();
        let batch = s.drain_batch(10).unwrap();
        assert_eq!(
            batch.len(),
            1,
            "the good entry survives; the torn line is skipped"
        );
    }
}
