# P2-E — Offline Video on WebKitGTK: Proof, Soak, and the Endurance Set (Design)

> Fifth sub-project of P2. Parent spec of record:
> `docs/superpowers/specs/2026-07-05-kiosk-browser-design.md` (rev 2), §3.4 (offline video,
> arch-09, `:285-292`), §6 (taxonomy `:661`, `:671`; rate-limits `:625-627`), §9 P2 row
> (`:839`), §10 (soak/endurance `:870-875`), §11 (PF-05 `:893`, memory `:900`). **Builds on
> P2-A** (the offline page renders, the `kioskasset` origin serves). E builds almost no
> player logic: `crates/kiosk-main/bundled/offline.html` already carries arch-09's failure
> wiring — the `error`/`stalled`/`emptied` listeners, the `play()` rejection `.catch`, the
> 3 s progress watchdog, and the static-splash degrade in `fallback()`. E proves that path
> on WebKitGTK, closes its one recorded gap, and picks up the parent roadmap's remaining P2
> endurance and maintenance rows.

**Status:** rev 2, 2026-08-07 — adversarial design review; see
`docs/superpowers/reviews/2026-08-07-p2b-p2g-adversarial-review/`.

**Citation convention.** P2-A rewrites `offline.html`'s mp4-URL selection and shifts every
line, so this spec cites that file by **symbol** (`fallback()`, the `error` listener, the
progress watchdog) and never by line number. Every other citation is `file.rs:NNN` at HEAD.

## Goal

The offline video loops for hours on WebKitGTK with no silent failure mode: a decode failure
degrades visibly *and observably* (spooled, not console-only), a loop boundary that stalls is
detected rather than mistaken for health (Debian #1062012 / PF-05 is the named risk), memory
is bounded by a cap whose level is measured rather than assumed, and the parent-approved
contingency is designed up front rather than improvised mid-soak.

## Scope

**In:** the `media.error` IPC bridge (E1–E2); loop-boundary self-monitoring in the page (E3);
**health-sampled webview RSS + memory-cap restart** (E4–E5), which parent §9's P2 row and
§11:900 assign verbatim and which E owns as the endurance sub-project; the double-buffered
no-seek loop *design* with a mechanical trigger (E6); the GStreamer harness environment
including one deliberate missing-element run (E7); the soak protocol and scenarios 18 /
18-W1 / 18-W2 (E8); and the two remaining pure-config P2-row knobs that live in the sections E
already owns — **`maintenance.restart_app`** (E9) and **remote log level** (E10).

**Out:** the ≥72 h hardware soak (RT-05 → P2-G H5); scheduled-CI wiring, runners, matrices and
artifacts (P2-F, jobs F5 and F7); the `.deb` GStreamer dependency declaration (P2-G, list
fixed by parent §3.4:285-288); the mp4 install path (P2-G G6).

**Two deliberate cross-platform changes, declared as such** — everything else in P2 is
Linux-gated (C8):

1. **E1's `media.error` bridge.** The page and its failure modes are identical on both
   platforms and arch-09's "emit a `media.error` log" (parent §3.4:280) is unfulfilled on
   both. Blast radius: one command, invoked only by a bundled app-origin page that loads only
   when the site is unreachable; its failure mode is a rejected promise the page already
   swallows.
2. **E4/E5's webview RSS + memory cap.** A named P2 roadmap row. This is the **only behaviour
   change** any P2 spec makes on Windows, and it is bounded by three things stated below:
   E4-before-E5 ordering, the 18-W2 floor gate, and the shipped `0` = off lever.

**Change register:** E1–E10. E lands in **two stages** (INT-1); cross-spec edges are tabulated
at the end, each in one direction only.

## Components

### E1 — the `media.error` bridge

`crates/kiosk-main/src/media.rs`, one command:

```rust
#[tauri::command]
fn media_error(kind: String, at: f64, ms_since_wrap: Option<f64>, telem: State<Telemetry>)
```

`kind` is validated **at the boundary** against a closed set — `error | stalled | emptied |
play_rejected | no_progress | stall` — mapping to a `&'static str`; anything else is dropped,
not logged. No free-form string crosses IPC: the page's `fallback(why)` today interpolates
engine text from the `play()` `.catch`, and that text is not a stable label. The precedent is
`nav_blocked`, whose `reason` is documented as *"a stable, greppable label"* taken from
`BlockReason::as_str()` (`telemetry.rs:119-126`). Numeric hygiene at
the same boundary: a non-finite or negative `at` / `ms_since_wrap` is recorded as `null`
rather than logged.

`Telemetry::media_error` is a **new method**: `Event::MediaError` is a dead variant today —
`grep -rn "MediaError\|media_error" crates/ --include=*.rs` returns four hits, all in
`event.rs`, with no emitter and no `Telemetry` method. The variant sits in the
`Severity::Warning` arm (`event.rs:101`) and already has its `TAXONOMY` row (`:137`), so
WARNING ⇒ `Severity::is_high()` (`event.rs:21-26`) ⇒ write-through `fsync` in `Spool::append`
(`spool.rs:1-7`, `:547`). Durability is correct unassisted, and the pinned
`assert_eq!(TAXONOMY.len(), 23, "spec §6 defines 23 events")` (`event.rs:179`) does not move.
**Net taxonomy delta for P2-E: zero.**

`offline.html`'s `fallback()` gains one
`window.__TAURI_INTERNALS__.invoke(...)` guarded by `try/catch`, **before** the existing
`console.error`, and the degrade path runs regardless — telemetry is observation, never a
dependency (C4). The comment recording the missing bridge is deleted in the same change.

**The ACL is part of the change, not an implication.** A `#[tauri::command]` plus a
`generate_handler!` entry is not sufficient in *this* app.
`tauri-macros-2.6.3/src/command/handler.rs:35` calls `filter_unused_commands` (`:90-140`),
which reads `tauri_utils::acl::read_allowed_commands()`
(`tauri-utils-2.9.3/src/acl/mod.rs:413-421`) and `retain`s away any command not in the allowed
set; a dropped command falls through `handler.rs`'s `_ => { return false; }` and the invoke
rejects at runtime after a green build. `has_app_acl` is true here — `crates/kiosk-main/build.rs`
declares `.app_manifest(AppManifest::new().commands(&["verify_pin"]))` and
`crates/kiosk-main/capabilities/default.json` grants `["core:default", "allow-verify-pin"]`.
So E1 edits **four** sites:

- `build.rs` → `.commands(&["verify_pin", "media_error"])`;
- `capabilities/default.json` → `+ "allow-media-error"`;
- `main.rs:990` → `generate_handler![pinpad::verify_pin, media::media_error]`;
- `main.rs:989` → `.manage(telem.clone())` beside the existing `.manage(pinpad_state)` — the
  sole `.manage` call in the crate. `Telemetry` is `#[derive(Clone)]` (`telemetry.rs:46-49`)
  and documented as a cheap `Send` handle.

Without those first two, E1's own `try/catch`-and-degrade-anyway design (correct per C4)
converts an ACL-stripped command into a **silent** dead telemetry path, and makes E8's
headline criterion — zero `media.error` over the soak — vacuously true. A criterion that
cannot fail is not a gate (C9); scenario 18's positive precondition (E8) is what proves the
producer is live.

**E1 depends on P2-A only.** The draft's declared P2-B edge is **deleted** (INT-10): B dropped
its `securitypolicyviolation` listener and its `#[cfg(not(windows))]` Tauri command in Round 1
and states that nothing needs to touch `main.rs:990`. E1 owns `main.rs:990`, `build.rs` and
`capabilities/default.json` alone.

### E2 — rate cap for `media.error`

One row in `ratelimit::caps()`: `(Event::MediaError, 6, 6)`, with a comment naming the driver.

The draft's claim that the event was already "rate-capped by the standard Logger bucket" was
**false**: `caps()` (`ratelimit.rs:52-63`) contains exactly `NavBlocked`, `NavError`,
`WebviewCrash` and `FocusLost`, its own doc says *"Every event not listed here is uncapped"*,
and `admit()` (`:129-134`) returns `Allow` for an unbucketed event.

Why 6/6: within one page load the `degraded` latch in `fallback()` bounds emission to one, so
the page is not the driver — repeated *page loads* are (`webview.crash` → navigate-home
recovery, nightly reload, safe-mode cycling), which is `WebviewCrash`'s own 6/min bucket.

**Not a parent amendment.** Parent §6:626 names three defaults and says *"defaults:"*, an open
list; `FocusLost` is a fourth cap added in-code with a written rationale
(`ratelimit.rs:57-61`) and is not in that list. Extending `caps()` with a rationale is
established practice. The cap cannot hide the soak's signal: `admit` can only convert an event
to `Suppress`, never manufacture one, E8's criterion is **zero**, and `take_summaries`
(`ratelimit.rs:138-150`) surfaces the suppressed count.

### E3 — loop-boundary self-monitoring in the page

The shipped watchdog is a one-shot `setTimeout` whose predicate is `v.currentTime === 0` — it
is meaningful only before the first frame and says nothing about steady state.

**The `currentTime`-sampling predicate is rejected and recorded.** An earlier revision asserted
`v.currentTime !== last || v.currentTime === 0` every 5 s. That is blind to the exact PF-05 /
#1062012 failure — the element wraps, `currentTime` becomes 0, decode does not resume, and two
consecutive 0.0 samples read as *healthy*. It is the shipped watchdog's own **failure**
predicate used as a **success** predicate in the same file. It also had two further holes: an
exact float comparison aliases against a clip whose duration divides the sample interval and
blacks the screen on a healthy build, and a `readyState >= 2` guard fails open on a stall that
drops the pipeline back to `HAVE_METADATA` — precisely the frozen frame `offline.html`'s own
header says must never happen.

**Replacement: count engine activity, do not sample the clock.**

```js
var ticks = 0, lastT = 0, wrapAt = 0, misses = 0;
v.addEventListener("timeupdate", function () {
  if (v.currentTime < lastT) wrapAt = Date.now();   // a loop wrap; we never seek
  lastT = v.currentTime; ticks++;
});
setInterval(function () {
  if (degraded || v.paused) { misses = 0; return; }
  if (ticks > 0) { ticks = 0; misses = 0; return; }
  if (++misses >= 2) fallback("stall", v.currentTime, Date.now() - wrapAt);
}, 5000);
```

- A healthy loop wrap emits `timeupdate` (≈4–66 Hz during playback, tens to hundreds per 5 s
  window); a hung decode emits none. "Looped" and "stalled" are distinguished by engine
  activity, not by comparing floats across a wrap.
- Stuck at 0.0 is the **primary** detection path, not the blind spot.
- No float comparison survives, so there is no interval/duration aliasing left; the two-miss
  requirement (10 s) is the confirming sample the old predicate lacked.
- No `readyState` guard: absence of `timeupdate` while `!paused && !degraded` is a stall at any
  readyState.
- The `degraded` check comes first, so the degrade path — which hides the element without
  pausing it — cannot re-trip the monitor.
- `wrapAt` yields `ms_since_wrap`, a **raw number**, which is what restores E6's mechanical
  trigger. The `12000` literal is **deleted from the page**; it lives once, in E6's activation
  rule (a threshold in the page would sit 2 s from the monitor's own ≥10 000 ms minimum
  detection latency on a `setInterval` the page cannot control, and any overrun would silently
  flip a genuine loop-boundary stall to "not at a boundary").

All four arch-09 signals — `error`, `stalled`, `emptied`, the `play()` rejection — plus the
startup watchdog and this monitor route through `fallback()` and therefore through E1.

**Open at plan time (values only):** `requestVideoFrameCallback` is not assumed to exist on
WebKitGTK and is not used; the design above needs only `timeupdate`. If a plan-time check finds
`requestVideoFrameCallback` present it changes nothing here.

### E4 — webview RSS, from the `System` already in hand

`kiosk_core::metrics::sample` (`metrics.rs:20-44`) gains
`sys.refresh_processes(ProcessesToUpdate::All, true)` and a pure helper that sums the
**descendant subtree** of the current pid, excluding self. `HealthSample` (`metrics.rs:6-12`)
gains `webview_rss_mb`; `to_fields` (`:47-56`) gains the matching key. **No new dependency, no
new `windows` feature, no `unsafe`, no `#[cfg]`** — with one declared side effect, below.

The change belongs in `kiosk-core`, not `kiosk-main`: `health.rs:1-5` states that all sampling
logic lives in `kiosk_core::metrics` and that webview RSS *"is P2 and does not belong here"*.
`health.rs` is untouched — it owns only the tick, and it already holds the `System` by value
across ticks and hands `&mut sys` to `metrics::sample` (`health.rs:25`, `:42`), which today
calls only `refresh_cpu_usage` / `refresh_memory` / `disks.refresh()` (`metrics.rs:26-28`).
The object is in hand at the exact call site.

**Both platform-specific mechanisms are withdrawn.** `/proc/self/status` `VmRSS` parsing and
the Windows `GetProcessMemoryInfo` FFI are redundant against a dependency already instantiated
here (Q2), and the latter would additionally need `Win32_System_ProcessStatus`, which is not
among the `windows` features `kiosk-main/Cargo.toml` declares, plus an `unsafe` block, for a
number `sysinfo` already returns. Verified in `sysinfo-0.32.1`: `System::refresh_processes`
(`src/common/system.rs:289-293`), `processes()` (`:386`), `Process::memory()` (`:1314`,
documented as resident set), `Process::parent()` (`:1356`), `Process::start_time()` (`:1384`),
`get_current_pid()` (`:2295`) — all cross-platform, all under default features.

**Why the subtree and not a process name.** WebKitGTK runs content in `WebKitWebProcess`;
WebView2 runs a browser process plus renderers. Both are descendants of kiosk-main, so a
parent-pointer walk from `get_current_pid()` is engine-agnostic: no name matching, no
per-platform branch, no knowledge of how many helpers an engine spawns. On Windows the parent
pointer comes from `SYSTEM_PROCESS_INFORMATION.InheritedFromUniqueProcessId` in the same
`NtQuerySystemInformation` sweep as the memory figure (`src/windows/system.rs:306-312`,
refreshed per pass at `:322-323`), so the walk needs no handles and reaches the WebView2
browser process and its renderers. Cost: one process enumeration per `health_sample_s`
(default 60, range `[10, 3600]`).

**PID-recycle guard.** Windows never rewrites `InheritedFromUniqueProcessId` when a parent
exits, and PIDs are recycled, so a naive walk can graft an unrelated tree onto the kiosk's —
an inflation vector. The helper **rejects any candidate child whose `start_time()` precedes its
claimed parent's**: one comparison in a helper we are writing anyway.

**What the number means — a declared footprint proxy, not a unique set.**

> On **both** platforms the cap compares `maintenance.max_webview_mem_mb` against the
> **arithmetic sum of `Process::memory()` over every descendant of the kiosk-main pid,
> excluding kiosk-main itself** — total resident (Linux, `sysinfo-0.32.1/src/unix/linux/
> process.rs:574-576`: `/proc/<pid>/stat` RSS × page size) or working set (Windows,
> `src/windows/process.rs:298-300`: `pi.WorkingSetSize`), **with shared pages counted once per
> helper**. It is a *footprint proxy*, not a unique-set size. `sysinfo` 0.32 exposes no PSS or
> private-bytes alternative.

**C3 divergence, both directions:**

- **Stricter than the configured number, on both platforms.** The effective ceiling is below
  1500 MB of real distinct memory by the shared-engine-text offset.
- **Stricter on Windows than on Linux.** WebView2 runs more helpers (browser + GPU + utility +
  N renderers) than WebKitGTK (web + network + GPU), so the same configured number means a
  *lower* real ceiling on Windows. The same config key does not mean the same thing on the two
  platforms; that is written down here instead of discovered in the field.
- **Interpretation note.** Scenarios 18, 18-W1 and 18-W2 record the **t=0 baseline** of
  `webview_rss_mb` as the first line of every soak artifact. The shared-text offset is visible
  there and is subtractable when reading a trend, which makes the offset an observed per-platform
  number rather than an argued one.

**Rationale, corrected.** The justification for summing is **Q1 traceability to parent §6:671's
literal "webview RSS"** and nothing else. The earlier "the machine dies on total footprint"
argument is **withdrawn**: the sum is strictly *above* total footprint by the over-count, and
the quantity that does track machine pressure already ships — `mem_used_mb` / `mem_total_mb`
in `HealthSample` (`metrics.rs:8-9`, from `sys.used_memory()`).

**Narrowing to a max-single-process reading — rejected, recorded.** On a one-page kiosk it
would be nearly the same number with strictly less coverage (a leak in a GPU or utility helper
would vanish) and would still over-count that one process's shared pages. Q2/Q3: no gain.

**Declared side effect, and the one line that bounds it.** `sysinfo`'s Linux process API
retains one `/proc/<pid>/stat` handle per tracked process across refreshes
(`src/unix/linux/process.rs:120`, `:364-367`, `:929-945`; `System::refresh_processes`'s own doc
at `common/system.rs:278-280` says so). Permanent per-process FD retention is exactly the class
of resource growth scenario 18 exists to detect, and it would sit in the baseline. So
**`sysinfo::set_open_files_limit(0)` is called once at startup, before the first process
refresh** (`lib.rs:127-168`): the budget lands at exactly 0, `FileCounter::new`
(`unix/linux/process.rs:931-944`) returns `None`, and `_get_stat_data` opens, reads and
**drops** the handle. Functionality preserved, retention gone. It is a no-op on Windows by
design.

*Accepted as documented risk:* `remaining_files()`'s `OnceLock` initialiser runs
`getrlimit`/`setrlimit(RLIMIT_NOFILE, hard)` (`unix/linux/system.rs:22-46`), and
`set_open_files_limit` itself calls it, so any use of sysinfo's Linux process API raises the
process's soft FD limit to the hard limit, once. The effect is a *raise*, so the mitigation
(`LimitNOFILE=` in P2-C's unit) buys very little; it is declared as a **non-blocking** ask on
P2-C/P2-G, and declining it costs essentially nothing. E4's "no new dependency, no `unsafe`,
no `#[cfg]`" headline is true at the source level and is qualified by this paragraph.

**E4 ships first and unconditionally** (see E5). Every fleet gets `webview_rss_mb` in
`health.sample` from that build, with no enforcement attached.

### E5 — memory-cap restart, against the key that already ships

**There is no new config key.** `maintenance.max_webview_mem_mb` ships today and is fully
declared, validated, documented and tested:

- `crates/kiosk-core/src/config/schema.rs:233-234` — `#[serde(default = "d_max_mem")] pub
  max_webview_mem_mb: u64` on `struct Maintenance`; `d_max_mem() = 1500` at `:38-40`, pinned
  by `assert_eq!(c.maintenance.max_webview_mem_mb, 1500)` at `:343`.
- `validate.rs:107-114` — range `{0} ∪ [256, 8192]`.
- `validate.rs:19` — `UNIMPLEMENTED` carries `("maintenance.max_webview_mem_mb", "P2")`; the
  non-default detector is the arm at `validate.rs:181-183`.
- `validate.rs:266-271` — `max_webview_mem_allows_zero_meaning_off_but_rejects_between`.
- Parent §5.2:538 and §10:872 both name it.

**The invented key `memory_max_mb` is withdrawn.** It was false on every count and would have
created a second memory key shadowed by `Maintenance`'s `#[serde(flatten)] unknown`
(`schema.rs:235-236`): an operator's `max_webview_mem_mb` would keep landing on the inert key
while the new one drove behaviour, with the RT-08 warning still reporting "feature unavailable"
for a feature that now exists.

**The config work is therefore a deletion.** Remove the `validate.rs:19` row and its
`validate.rs:181-183` arm; update the RT-08 warn-path tests. Schema and range validation are
unchanged.

**The latch.** `kiosk-core` exports `pub const MEM_CAP_N: u32 = 5` and a pure
`struct MemCap { over: u32 }` with `fn observe(&mut self, rss_mb: u64, cap_mb: u64) -> bool`:
`cap_mb == 0` ⇒ always `false`; `MEM_CAP_N` consecutive samples strictly over ⇒ `true` once,
then reset. `kiosk-main` calls `std::process::exit(80)` on `true`. There is one `N` and one
dwell formula: **dwell = `MEM_CAP_N × health_sample_s`**. (An earlier revision also said the
latch "raises N so that dwell ≥ 300 s regardless of sample cadence"; that clause is
**deleted** — it contradicted the accelerated 18-W1 fixture in the same breath.)

**Exit code 80 — decided, never 86.** The complete set of exit-code literals in the workspace
is `86` (`pinpad.rs:156`, `watchdog.rs:196-197`), `0` (`kiosk-launcher/src/main.rs:144`,
`job.rs:24`, `:243`, `cli.rs:31`), a pass-through (`kiosk-launcher/src/main.rs:253`), and `2`
in an example binary. `80` sits below 86, outside `128 + signal` (129–159) and outside any
negative-sentinel scheme, so it survives whichever encoding P2-C picks for signal death
(C7: `128 + signo`, never-86, `-2`). No self-restart may read as a technician exit.

**The launcher interaction is designed against, not tolerated.** `watchdog.rs:194-199` restarts
on any non-86 code, and `restart()` emits `watchdog.restart` (ERROR), doubles backoff, and
pushes into a 600 s window (`WINDOW_S`, `watchdog.rs:80`) where `> 5` sets `safe = true` and
logs `watchdog.safe_mode` CRITICAL (`:151-155`) → `Action::SpawnSafe`. **Escalation is
unreachable from cap exits by construction:** the Armed tick clears `restarts`, resets
`backoff_s` and clears `safe` once a run crosses `healthy_run_s` (`watchdog.rs:234-238`,
with `spawned_at` reset per respawn at `:243`). At shipped defaults dwell is `5 × 60 = 300 s`
against `healthy_run_s = 120` (`kiosk-launcher/src/main.rs:110-124`, pinned at `:285-290`), so
every cap exit lands in a window the healthy-run path already cleared: `restarts.len()` becomes
1, never > 5. Backoff doubles once and resets on the next healthy run.

**That relation is pinned where both values are observable — in `kiosk-launcher`, not
`kiosk-main`.** `kiosk-main` never reads `healthy_run_s`, so a host test there could only
compare against a hardcoded 120 and would keep passing after any launcher change; that test is
**withdrawn**. `kiosk-launcher/Cargo.toml:14` declares `kiosk-core.workspace = true` and the
launcher owns both real values, so:

```rust
#[test]
fn mem_cap_dwell_exceeds_the_launchers_healthy_run_window() {
    let dwell = MEM_CAP_N as u64 * RemoteConfig::default().logging.health_sample_s;
    assert!(dwell > watchdog_config(None).healthy_run_s,
        "a memory-cap exit must land after the crash-loop window has been cleared");
}
```

`RemoteConfig::default()` is `serde_json::from_str("{}")` (`schema.rs:303-307`), i.e. the same
value `schema.rs:345` already pins at 60; `watchdog_config(None).healthy_run_s` is 120. No
hardcoded copy of either number; the test fails if `d_health_sample`, `MEM_CAP_N` or the
launcher's default moves. 18-W1's `no watchdog.safe_mode` assertion is **kept alongside it**,
not replaced — the two pin different properties (the shipped-default relation, and the
cross-process exit → restart → window behaviour end to end).

*Residual, declared:* both sides are operator-settable — `health_sample_s` by signed remote
config, `healthy_run_s` by `kiosk.ini` with no range validation at all
(`bootstrap.rs:75-91`'s `number()` applies no bounds). `healthy_run_s` is a technician-set
bootstrap value on the device; it is documented in the operator note and not defended in code,
which would require kiosk-main to read the launcher's bootstrap file.

**"Default 0 = off, so Windows fleets see zero behaviour change" — withdrawn, not repaired.**
The shipped default is **1500**, not 0, so the property is false. It is not fixed by changing
the default: parent §5.2:538 pins 1500 and §10:872 requires that a breach fires a restart, so a
default of 0 would contradict the parent twice. What replaces it:

- **The level is underived, and E says so.** `grep -n "1500" docs/superpowers/specs/*.md`
  returns **exactly one line** across every spec — parent §5.2:538 — with no derivation and no
  stated measurand, and there is no observed RSS measurement anywhere in the repo. E4 binds
  that number to a quantity that over-counts shared pages, worst on Windows.
- **E4 ships first, unconditionally** — sampler only, no enforcement.
- **E5's enforcement half is merge-gated on a measurement.** Scenario **18-W2** records the
  steady-state Windows `webview_rss_mb` of its fixture at rest as a first-class artifact
  number. **If that floor is ≥ 750 MB (half of 1500), E5's enforcement does not ship: a defect
  is raised against parent §5.2's default instead.** Margin rationale: distinguishing a leak
  from a working set needs at least 2× headroom between healthy steady state and the cap;
  below that the cap fires on normal variance rather than on a leak.
- **Fleets that already set the key have already been warned.** `config.warn` "feature
  unavailable in this build" has fired for them since P1 (`validate.rs:181-183`); the RT-08
  table exists precisely so a value starts taking effect at the phase it is tagged with, and
  E's job is to remove the row.
- **The operator lever already ships and already works.** `0` = off is range-valid today
  (`validate.rs:107-114`, tested at `:267`) and reachable by signed remote config.
- **The release note states the change and hands over a derived rule:** from P2, a webview tree
  exceeding `max_webview_mem_mb` for `MEM_CAP_N` consecutive health samples restarts the app.
  *After upgrading to the E4 build, read your fleet's `health.sample.webview_rss_mb` p99 over
  one week and set `maintenance.max_webview_mem_mb` to roughly 2× that value within
  `[256, 8192]`, or to `0` to disable; until then the shipped default of 1500 applies.* The
  note also carries 18-W2's recorded floor ("your Windows engine baseline is ~X MB; your
  content adds on top") so the rule is actionable on day one rather than after a week of
  telemetry. Per C3 this is **stricter** than P1 Windows behaviour, deliberately, by the
  parent's instruction.

*Residual, named precisely and accepted:* 18-W2's fixture is a test page, not fleet content, so
its number is a **floor** on the healthy Windows working set — engine + helpers + a trivial
page. The floor gate catches the disqualifying case (even the floor is near 1500) and cannot
certify the general one. A Windows fleet whose real site drives the summed tree between that
floor and 1500 gets a clean, well-logged, permanent restart cycle every 300 s that it did not
have in P1. **Carried by the operator**, informed rather than surprised, via the sampler, the
release-note rule and the `0` lever. It is **not** carried by P2-G's hardware checklist, which
is Linux.

**Durability: the fact, not the number.** The draft's "spool flushed by the existing shutdown
path" is **false** — `main.rs:1235-1243` calls `telem.app_stop()` + `cancel.cancel()` and
nothing else, and its own comment disclaims durability, resting it on `Spool::append`'s
fsync; a `std::process::exit` never reaches `RunEvent::Exit` at all (precedent:
`pinpad.rs:156`). `Telemetry::emit` is `let _ = self.tx.try_send(...)` on a bounded
`sync_channel(256)` drained by a separate blocking OS thread (`telemetry.rs:28-37`, `:64-68`),
which `exit` joins not at all, and an INFO `health.sample` is never fsynced. So:

> The restart's cause is durable in `watchdog.restart{code: 80}` — ERROR, fsynced, written by
> the **surviving** launcher, with `code`, `backoff_s` and `cause` as first-class fields
> (`kiosk-launcher/src/sink.rs:209-217`). The `webview_rss_mb` series is delivered on the
> normal telemetry path, best-effort like every INFO event; the sample immediately preceding
> the exit may be lost.

**A `health.memory_cap` event — rejected, recorded.** It would cost an `Event` variant, two
compiler-forced match arms, a `TAXONOMY` row (*not* compiler-forced), a `23 → 24` bump of a
deliberately pinned test whose own header says *"you are changing the contract with the fleet's
log-based metrics and alerting"*, and a parent §6 table amendment — to record something a
surviving process already records durably at ERROR with the code as a field. A WARNING variant
would have lost the same cross-thread race down the same channel, so promoting it was not a
fix either. **An exit-ordering handshake — also rejected:** no requirement names the tripping
number durable (parent §10 asks for *bounded RSS*, a series, and *a breach fires a restart*, a
fact); `pinpad.rs:150-156`'s "exit after the persist" is a file write the calling thread
performs itself and does not transfer to a cross-thread flush; and reusing
`telemetry::spool_boot_config_error`'s direct-`Spool::open` pattern would be a **bug** here —
that path exists because no live `Logger` owns the spool at boot-gate time, and a second
concurrent `Spool` handle appending to the same segments is corruption, not durability.

### E6 — the double-buffered no-seek loop, designed now (PF-05)

Two stacked `<video>` elements, both preloaded; on `ended` the hidden one — already at 0 and
paused-ready — plays and swaps to front; the other resets in the background. Parent §11:893 and
§3.4:291-292 give the ordering verbatim (*"fallback = seamless no-seek loop or native GL
path"*) and E's matches.

Two things the design must state, because without them it does nothing:

1. **`loop` is removed from both elements.** The shipped element carries `loop`, and per the
   HTML media element spec a looping element seeks to the earliest position and **does not fire
   `ended`** — so E6's primary trigger could never arm. Worse: while `loop` is present the
   engine performs precisely the seek-to-0 that #1062012 names, so the double buffer would
   change nothing. The loop is driven entirely by the swap.
2. **The background reset is `load()`, never `currentTime = 0`.** `currentTime = 0` *is* the
   seek path #1062012 names; using it would relocate the bug into the background and surface it
   one loop later with two frozen elements. `load()` re-runs the resource selection algorithm —
   a fresh fetch and decode pipeline, not a seek — which is affordable against a local
   `kioskasset` custom-scheme read with a full clip duration of budget.

> The incoming element must reach `canplaythrough` before it is swapped to front; if it has not
> by `duration − 0.25 s` of the visible element, the page degrades per arch-09 rather than
> showing a not-ready element. **Budget:** one clip duration; if `load()` cannot complete within
> it on target hardware, the fallback is the native-GL path (parent §3.4's second fallback),
> **not** `currentTime = 0`.

**Activation rule — mechanical, not judgment, and the only place the threshold lives:** any
`media.error{kind: "stall"}` during scenario 18 whose `ms_since_wrap` is **< 12000** ⇒ the
contingency task activates in the plan. The native-GL path stays out unless double-buffering
also fails on hardware; it forfeits the one-HTML-path property and needs its own design round.

### E7 — harness GStreamer environment, including the deliberate miss

The four packages parent §3.4:285-288 names for the `.deb` —
`gstreamer1.0-plugins-{base,good,bad}`, `gstreamer1.0-libav` — are installed in the smoke and
soak environments. **One run removes `gstreamer1.0-libav`.** P2-G's design carries the identical
four for the `.deb`; three-way consistent.

> With `gstreamer1.0-libav` removed: **exactly one `media.error` of any enumerated kind reaches
> the spool, and the page degrades to the black splash.** The specific `kind` is **recorded in
> the run artifact, not asserted.**

**Pinning the assertion to one `kind` — rejected, recorded.** An earlier revision asserted
`kind: "no_progress"`. Removing `libav` removes `avdec_h264` while `qtdemux` (`-good`) and
`h264parse` (`-bad`) remain, so the pipeline demuxes and parses and then fails to link a
decoder — a bus error that WebKitGTK surfaces as an `error` event, which the shipped `error`
listener already routes into `fallback()` as `kind: "error"`. The opposite branch is also live:
parent §3.4:287-288 says a missing element yields *"a silent black video"*, which is what
`no_progress` assumes. Neither branch is verifiable without GStreamer and WebKitGTK, so the
spec asserts the **outcome** the parent actually requires and records the label. This is E's
only per-PR contribution (~3 s as a variant fixture; P2-F decides inclusion). E owns the
assertion; P2-G owns the `.deb` declaration, P2-F owns the harness.

### E8 — soak protocol: scenarios 18, 18-W1, 18-W2

**Scenario 18 — the Debian offline-video soak (PF-05).** Fixture: config-down boot → offline
video (P2-A scenario 3's entry path), on the `debian:12` nightly container.

- **Positive precondition, before the soak clock starts.** The fixture first loads the offline
  page with `kiosk-offline.mp4` deliberately **absent** — the `kioskasset` handler reads the
  file and returns 404 when it is missing (`main.rs:998-1010`) — and asserts **one
  `media.error` appears on the spool.** Only then does the soak begin, with the criterion
  inverted to zero. A criterion that cannot fail is replaced by one with a proven-live
  producer.
- **Pass:** zero `media.error` in the spool; process alive; **zero launcher restarts of any
  kind, including a memory-cap exit** — `offline.html` has no leak source, so a cap trip here
  is a real leak and **fails** the soak (the cap is left at its default, not disabled);
  `webview_rss_mb` delta over the window under a bound declared from the first-run baseline and
  then pinned; loop count consistent with wall-clock.
- **Durations:** in-session ~2 h minimum during execution; **scheduled CI multi-hour, duration
  set by P2-F within the hosted-runner cap** — E pins no CI wall clock and E's pass criteria
  are duration-agnostic; hardware ≥72 h (P2-G H5, RT-05).
- **`WEBKIT_DISABLE_COMPOSITING_MODE=1` is not set.** P2-A permits it *"in the smoke environment
  only"* (`2026-08-06-p2a-linux-bringup-design.md:315-316`); scenario 18 is a soak, and
  disabling compositing plausibly moves the video off the accelerated path. If the harness
  cannot come up without it, that fact is recorded in the artifact and the run is annotated as
  measuring the non-composited path. *Residual:* a stall class visible only with compositing
  enabled would be invisible in CI — bounded by H5, on real hardware with the real compositor.
- **Artifacts:** the t=0 `webview_rss_mb` baseline as the first line; on failure, the full spool
  and the compositor log.

**Scenarios 18-W1 and 18-W2 — the Windows memory soak (parent §10:870-875).** The parent
specifies **two** soaks, not one, and names three assertions for the Windows-runner job:
bounded RSS, a `max_webview_mem_mb` breach fires a restart, and nightly reload resets RSS. A
single fixture cannot carry them: with the cap at 256 against a leaking page the cap re-trips
every dwell and every restart resets the nightly-reload timer, so "post-reload RSS" is
unreachable — and at the default `healthy_run_s` the run drives the launcher to
`watchdog.safe_mode` on its sixth restart inside `WINDOW_S`. Hence two runs. (They were briefly
labelled 18-W(b)/(c); renamed to **18-W1 / 18-W2** because P2-F's `endurance` jobs are lettered
(a)/(b)/(c) and the labels collided.)

**Authoritative parameter table — this is the single source of truth.**

| | **18-W1** (breach → restart) | **18-W2** (nightly reload resets RSS) |
|---|---|---|
| Runner | `windows-latest` | `windows-latest` |
| Page | deliberately leaking, **is `content.url`** | deliberately leaking, **is `content.url`** |
| `maintenance.max_webview_mem_mb` | **256** | **0** (off) |
| `logging.health_sample_s` | **10** (dwell = 50 s) | default (60) |
| `kiosk.healthy_run_s` (`kiosk.ini`) | **30** | default (120) |
| `maintenance.nightly_reload` | **unset** | **a few minutes ahead** |
| `content.clear_data_on_reset` | off | **off** (default is `true`, so the fixture must set it `false`) |
| `--safe` | no | **no** |
| Asserts | `webview_rss_mb` climbs and is reported; breach → **exit 80** → launcher restart with `watchdog.restart{code: 80}` on the spool; **no `watchdog.safe_mode`** | zero restarts; post-reload `webview_rss_mb` < pre-reload peak; **post-reload URL == the leaking page**; **steady-state `webview_rss_mb` recorded** as a first-class artifact number (E5's floor gate) |

`max_webview_mem_mb: 0` and `256`, and `health_sample_s: 10`, are range-valid today
(`validate.rs:107-114`, `:118-121`, tested at `:267`). `healthy_run_s` is a `[kiosk]` bootstrap
key parsed through `bootstrap.rs:113-118`'s `number()`, which applies **no bounds**
(`dist-template/kiosk.ini` ships 120), so 18-W1's `30` is mechanically available; at dwell
50 > 30 the Armed-tick reset (`watchdog.rs:234-238`) fires before each cap trip and escalation
is unreachable at the accelerated cadence for the same structural reason it is unreachable at
shipped defaults. The interlock's *form* (`dwell > healthy_run_s`) is preserved rather than
special-cased, and the `no watchdog.safe_mode` assertion is what turns it into something the
gate actually checks.

**18-W2's fixture preconditions, stated because each one silently voids the assertion.** The
nightly timer sends `AppEvent::IdleExpired` **into the FSM** rather than reloading the webview
(`main.rs:1177-1194`, whose comment enumerates the outcomes):

1. **The device must be in `Online`** when the timer fires. `state.rs:306-311` is the only
   no-clear arm and `:296-304` the only clearing arm; every other state is a no-op, pinned by
   the tests at `state.rs:979-995`.
2. **The leaking page must be `content.url`.** The reload navigates to `self.home`, not "the
   current URL", so a fixture that leaks on some other page would swap to a lighter one and go
   green while proving nothing. This is a false-pass risk, so it gets a **second assertion**
   (post-reload URL equals the leaking page), not merely a precondition.
3. **`content.clear_data_on_reset` off** — otherwise `Effect::ClearProfile{full: true}` also
   frees memory and the drop cannot be attributed to the reload. Its default is `true`
   (`schema.rs:118`, `d_true`; asserted at `:325`), so the fixture must set it `false`.
4. **No `--safe`.** `main.rs:1184-1185`: *"`--safe` never spawns this"* — a safe-mode run has
   no reload timer at all.

**Ownership boundary, stated in both directions.**

> Scenario 18-W1/18-W2's parameters and assertions are defined **only** in this section. P2-F's
> `endurance` Windows job (F7) **references them by ID and must not restate them** — restating
> is what produced the drift this review found. **F owns the job, the runner, scheduling and
> artifacts; E owns the body, the parameters and the feature.** If E4/E5/E8 are withdrawn, F7
> is unrunnable and parent §10's Windows-soak row returns to UNOWNED in the ledger rather than
> silently passing. **F7's RSS series is an artifact, not a dependency: P2-F declares no edge
> onto E5.**

### E9 — `maintenance.restart_app` (parent §9 P2 row)

`maintenance.restart_app` ships as `Option<String>` (`schema.rs:230`) and is tagged
`("maintenance.restart_app", "P2")` in `UNIMPLEMENTED` (`validate.rs:20`, detector at
`:184-185`) — the same shape, in the same config section, as the key E5 lights up. E owns it
because the timer, the section and the exit path are all E's already.

The mechanism is **E5's exit path fired by a clock instead of by a threshold**, and the clock
already exists: `crates/kiosk-main/src/maintenance.rs` is a generic `next_fire`-driven "HH:MM"
loop, not a nightly-reload-specific one — `hhmm: None` returns immediately (feature off) and an
unparseable value calls `warn_once` exactly once, which the caller turns into
`config.warn{field}` (`maintenance.rs:10-20`). So E9 is a **second `tokio::spawn(maintenance::run(...))`**
at the same site as the nightly-reload timer (`main.rs:1188`), with `restart_app` as the time,
the same timezone, a `warn_once` naming `maintenance.restart_app`, and a callback that takes
E5's clean exit — the launcher's FSM restarts it exactly as it does a cap exit
(`watchdog.rs:194-199`). It sits inside the same `if safe {} else {}` split as the nightly
timer: a safe-mode run must not restart itself on a clock.

Config work is again a **deletion**: remove `validate.rs:20`'s row and its `:184-185` arm, and
update the RT-08 warn-path test. No new dependency, no cron library, no new module.

### E10 — remote log level (parent §9 P2 row)

`logging.level` parses, defaults and validates today and has **zero consumers**:
`schema.rs:247-248` (`#[serde(default = "d_level")] pub level: String`, `d_level() = "info"` at
`:41-43`, pinned at `:344`), `VALID_LOG_LEVELS` at `validate.rs:11`, and
`grep -rn "\.level\b" crates/kiosk-core/src/logging/ crates/kiosk-main/src/` returns **nothing**.
`Severity` already exists and is already ordered by use (`event.rs:10-15`, `is_high()` at
`:21-26`).

The delivery is a `>=` severity drop applied before the spool in `Telemetry` — parse
`logging.level` into a `Severity` once per config apply, drop entries below it. Roughly five
lines, platform-free, no new taxonomy, no schema change.

Note for the record: unlike `max_webview_mem_mb` and `restart_app`, `logging.level` was never
added to `UNIMPLEMENTED`, so an operator setting it has been silently ignored with no RT-08
warning since P1. There is no row to delete; the fix is the consumer itself.

E10 is not naturally E's. It is carried here because it lives in the same `logging` /
`maintenance` config surface E is already editing and costs one sentence to bundle with E9,
against standing up a sub-project for five lines.

## Testing

- **Host tests (`kiosk-core`):** `MemCap::observe` — `MEM_CAP_N` consecutive semantics,
  `cap_mb == 0` disables, fires once then resets; the subtree-sum helper over a synthetic pid
  map covering the normal tree, an orphan (reparented helper), self-only, and a **recycled
  pid** whose `start_time()` precedes its claimed parent's.
- **Host test (`kiosk-launcher`):** `mem_cap_dwell_exceeds_the_launchers_healthy_run_window`
  (E5) — the only place both shipped defaults are observable.
- **Host tests (`kiosk-main`):** `media_error`'s `kind` enumeration (each accepted label maps,
  anything else is dropped) and its numeric hygiene (non-finite/negative → `null`); the RT-08
  warn-path tests updated for the two deleted `UNIMPLEMENTED` rows.
- **Per-PR:** E7's missing-decoder case only, as a variant fixture. There is no per-PR video
  assertion to inherit — P2-A states that offline video in A is wiring-only and *"playback
  quality is P2-E's"*.
- **Scheduled:** scenario 18 (P2-F's `debian:12` soak job); 18-W1 and 18-W2 (P2-F's F7 on
  `windows-latest`).
- **Hardware:** P2-G H5 — ≥72 h offline-video soak, RSS trend, loop count; visual/black-frame
  checks are hardware-checklist items, since screenshot-based black-frame detection stays
  best-effort in the headless harness.

## Error handling

arch-09 is the doctrine: every media failure degrades to the static splash plus one spooled
event. The bridge failing degrades to `console.error` — today's behaviour — and never blocks
the degrade path (C4). A memory-cap exit is a clean `exit(80)`; the durable explanation is the
launcher's `watchdog.restart{code: 80}` at ERROR, and the number that tripped it is
best-effort like every INFO event. Telemetry is observation, never a dependency.

## Residual risks — each with a named carrier

| Risk | Carrier |
|---|---|
| Windows content between 18-W2's recorded floor and 1500 MB ⇒ a clean, well-logged 300 s restart cycle | **The operator**: E4 ships the number first, the release note gives the p99 × 2 rule and 18-W2's floor, `0` = off is range-valid and signed-config reachable |
| Both sides of the interlock are operator-settable (`health_sample_s` remote, `healthy_run_s` unbounded in `kiosk.ini`) | **Declared**; `healthy_run_s` is technician-set bootstrap config, documented in the operator note, not defended in code |
| `setrlimit(RLIMIT_NOFILE, hard)` on the first process refresh — unavoidable inside `sysinfo` | **Declared**; effect is a *raise*, so the `LimitNOFILE` ask on P2-C/P2-G is **non-blocking** and declining it costs essentially nothing |
| `watchdog.restart{code: 80}` carries the fact, not the tripping RSS number | **Declared**; no requirement names the number durable, and both ordering remedies cost more than the field is worth |
| A reparented webview helper drops out of one sample | **Bounded by the latch**: `MEM_CAP_N` consecutive samples, so a one-sample dip cannot trip or un-trip anything; covered by the synthetic-pid-map host test |
| A stall class visible only with compositing enabled | **P2-G H5** — real hardware, real compositor; scenario 18 runs without `WEBKIT_DISABLE_COMPOSITING_MODE` and annotates the artifact if it cannot |
| Debian #1062012 loop-boundary stalls on WebKitGTK | **Scenario 18 is the gate**; E6 is the pre-designed, mechanically-triggered contingency |

## Open decisions to resolve at plan time

Values and shims only; no mechanism is left unpinned.

- The declared `webview_rss_mb` delta bound for scenario 18's pass criterion — taken from the
  first in-session soak's baseline, then pinned.

## Change register and cross-spec edges

**E lands in two stages** (INT-1). E4's sampler and all three scenario bodies land in stage 1
with **no enforcement**; P2-F then ships F7 as `strategy.matrix.scenario: [18-W2]` and its first
green nightly records the floor; then **one commit** carries E5's enforcement branch in
`kiosk-main` **plus** the single line adding `18-W1` to `endurance.yml`'s matrix. No second
P2-F pass, no reopened spec.

| ID | Change | Discharges | Stage | Depends on |
|---|---|---|---|---|
| E1 | `media.error` command + `Telemetry::media_error` + enumerated `kind` + ACL entries + `.manage(telem.clone())`. **Cross-platform, declared #1** | parent §3.4:280 arch-09; §6:661 | 1 | **P2-A only** (stale P2-B edge deleted) |
| E2 | `(Event::MediaError, 6, 6)` in `ratelimit::caps()` | parent §6:625-627; TEL-07/09 | 1 | E1 |
| E3 | `timeupdate`-counter loop monitor; `currentTime` predicate withdrawn; emits raw `ms_since_wrap` | parent §3.4 arch-09; PF-05 | 1 | E1, P2-A |
| E4 | Webview-process RSS as a declared descendant-sum footprint proxy via the already-held `System`; `start_time()` recycle guard; `set_open_files_limit(0)`; C3 divergence + t=0 baseline | parent §9:839 "health-sampled RSS"; §6:671; P1-D2e deferral | 1 | none new; non-blocking `LimitNOFILE` ask on P2-C/P2-G |
| E5 | Memory-cap latch against the **shipped** `maintenance.max_webview_mem_mb`; `UNIMPLEMENTED` deletion; `pub const MEM_CAP_N = 5`; exit **80**; non-escalation interlock + launcher-side relation test. **Cross-platform, declared #2** | parent §9:839 "memory cap restart"; §5.2:538; §10:872; §11:900 | **2** | E4 (in); the first green nightly F7/18-W2 run (in, merge gate on the enforcement half only). **No outbound edge** |
| E6 | Double-buffered no-seek loop; `loop` removed; reset is `load()`; `canplaythrough` gate; mechanical `ms_since_wrap < 12000` trigger | parent §3.4:291-292; §11:893 (PF-05, #1062012) | 1 | E3 (supplies the trigger evidence) |
| E7 | Harness GStreamer set + one deliberate missing-element run; outcome asserted, `kind` recorded | parent §3.4:285-288 | 1 | P2-G (`.deb` list), P2-F (harness) |
| E8 | Scenario 18 + positive precondition; 18-W1 / 18-W2 with the authoritative parameter table and 18-W2's four preconditions | parent §10:870-875; RT-05; PF-05 | 1 (bodies) | E1–E6; P2-F F7 (job); P2-G H5 |
| E9 | `maintenance.restart_app` — a second `maintenance::run` timer taking E5's exit path; `UNIMPLEMENTED` deletion | parent §9:839 "restart_app"; `validate.rs:20` | 1 | E5's exit path (code, not merge order) |
| E10 | Remote log level — `logging.level` parsed into a `Severity` and applied as a `>=` drop in `Telemetry` | parent §9:839 "remote log level" | 1 | none |

**Edges, each stated once and in one direction:**

- **E8 → P2-F F7:** F7 runs 18-W1/18-W2 and **references E's parameter table by ID; it must not
  restate it.** F owns the job, runner, scheduling and artifacts.
- **F7's first green nightly 18-W2 ⇒ E5's enforcement branch** — a *merge gate* on E5's
  enforcement half only, satisfied at stage 2. F7's RSS series is an **artifact**, not a
  dependency; **P2-F declares no edge onto E5**, and the earlier "carried in both directions"
  formulation is deleted — it was the cycle.
- **E7 → P2-G G6:** the mp4 install path and the `.deb` GStreamer declaration.
- **E8 → P2-G H5:** the ≥72 h hardware soak (Linux only; it is **not** load-bearing for the
  Windows level).
- **E4 → P2-C/P2-G (non-blocking):** `LimitNOFILE=` in the systemd unit.
- **Deleted:** E1 → P2-B. B dropped the `#[cfg(not(windows))]` Tauri command and the
  `main.rs:990` edit in its Round 1; the edge pointed at a withdrawn change and fabricated a
  B-before-E ordering constraint that does not exist.

## Scope / defer

`.deb` GStreamer deps, the mp4 install path, hardware soak and visual checks → P2-G. Scheduled
CI wiring, runners, matrices, artifacts and all wall-clock durations → P2-F. The native-GL video
path → only on double-buffer failure on hardware, and it needs its own design round: it forfeits
the one-HTML-path property parent §3.4 protects. arch-10 (Android autoplay gesture) is **not**
discharged here and its citation is withdrawn — it is P3.
