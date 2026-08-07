# P2-E — WRITER, Round 1 (opening)

No frame dispute. Everything below was re-verified by me at HEAD `1decd59`; where I
disagree with the verification report I say so and cite. Where I do not, I concede.

## Change register

| ID | Change | Requirement discharged | Depends on |
|---|---|---|---|
| **E1** | `media.error` IPC bridge: one `#[tauri::command]` + `Telemetry::media_error(kind, at)`, enumerated `kind` only. **CROSS-PLATFORM (declared #1)** | parent §3.4 arch-09 ("emit a `media.error` log"); §6 `media.error` WARNING row | P2-A (edits same file); **P2-B** (shared `generate_handler!` at `main.rs:990`) |
| **E2** | One row in `ratelimit::caps()`: `(Event::MediaError, 6, 6)` | parent §6 rate-limit para (line 626); TEL-07/09 | E1 |
| **E3** | Loop-boundary self-monitoring in `crates/kiosk-main/bundled/offline.html` | parent §3.4 arch-09 (progress must be asserted, not only at startup); PF-05 | E1, P2-A |
| **E4** | **Webview-process** RSS in `kiosk_core::metrics` via the already-held `sysinfo::System` (pid-subtree sum). **CROSS-PLATFORM (declared #2a)** | parent §9 P2 row "health-sampled RSS"; §6 `health.sample` "webview RSS (P2)"; D2e deferral `:127` | none new (D2e pipeline exists) |
| **E5** | Memory-cap restart against the **shipped** `maintenance.max_webview_mem_mb`: latch + `UNIMPLEMENTED` deletion + exit code `80` + non-escalation interlock. **CROSS-PLATFORM (declared #2b)** | parent §9 P2 row "memory cap restart"; §5.2 line 538; §10 line 872; §11 line 900 | **E4**; P2-C (exit-code space); launcher FSM (unchanged) |
| **E6** | Double-buffered no-seek loop, conditional; mechanical trigger rule | parent §3.4 fallback ordering; §11 PF-05 (#1062012) | E3 (supplies the trigger evidence) |
| **E7** | Harness GStreamer environment + one deliberate missing-element run | parent §3.4 lines 285-290 ("a missing element yields a silent black video") | P2-G (`.deb` dep list), P2-F (harness) |
| **E8** | Soak protocol, scenario 18, pass criteria — **plus** the parent's Windows memory-soak assertions | parent §10 lines 870-875; RT-05; PF-05 | E1–E5, P2-F (scheduled wiring), P2-G (H5) |

Withdrawn from the draft: `health.memory_cap` event; `memory_max_mb` key; `/proc/self/status`
+ `GetProcessMemoryInfo`; arch-10 citation; "spool flushed by the existing shutdown path".
See §Withdrawals.

---

## E1 — `media.error` IPC bridge

**Proposal.** `crates/kiosk-main/src/media.rs`: one `#[tauri::command] fn media_error(kind:
String, at: f64, telem: State<Telemetry>)`. `kind` is **validated at the boundary** against a
closed set — `error | stalled | emptied | play_rejected | no_progress | stall` — mapping to a
`&'static str`; anything else is dropped, not logged. No free-form string crosses IPC (the
page's `fallback(why)` today interpolates engine text from `p.catch`, `offline.html:62`; that
text is not a stable label and must not become a log field — the `nav_blocked` precedent
enumerates its `reason` too, `telemetry.rs:128-135`). `Telemetry::media_error` is a new
method; `Event::MediaError` and its WARNING severity already exist.

`offline.html`'s `fallback()` gains one `window.__TAURI_INTERNALS__.invoke(...)` guarded by
`try/catch`, **before** the existing `console.error`, and the degrade path runs regardless —
telemetry is observation, never a dependency (C4). The `:44-46` comment is deleted in the
same change.

**Evidence (verified by me).**
- Tier 3: `Event::MediaError` is a **dead variant** — `grep -rn "MediaError\|media_error"
  crates/ --include=*.rs` returns exactly four hits, all in `event.rs` (`:43`, `:72`, `:101`,
  `:137`). No emitter, no `Telemetry` method. The verifier is right; the method is now an
  explicit deliverable of E1 rather than an implication.
- Tier 3: `event.rs:101` puts `MediaError` in the `Severity::Warning` arm; `:137` is the
  TAXONOMY row. WARNING ⇒ `Severity::is_high()` (`event.rs:21-26`) ⇒ write-through fsync in
  `Spool::append` (`spool.rs:1-7`). Durability is already correct for this event, unassisted.
- Tier 3: the single registration site is `main.rs:988-990`,
  `generate_handler![pinpad::verify_pin]`, unconditional.
- Tier 1: parent §6 line 661 — `media.error | WARNING | offline video failed to decode/play`.

**Dependencies.** P2-A rewrites the mp4-URL selection in the same file (A design line 112) —
E1 lands **after** A. P2-B: see the restated dependency below; E1 owns the conversion of
`main.rs:990` to a multi-entry list.

**Cross-platform justification (C8/Q4).** The page and its failure modes are identical on both
platforms; the *unfulfilled* half of arch-09 is unfulfilled on Windows too. Blast radius: one
new command, invoked only by a bundled app-origin page that only loads when the site is
unreachable; failure mode is a rejected promise the page already swallows.

## E2 — rate cap for `media.error`

**Proposal.** Add `(Event::MediaError, 6, 6)` to `ratelimit::caps()`, with a comment stating
the driver (a reload loop, not the page).

**Requirement.** parent §6 line 626 (rate-limiting) / TEL-07/09.

**Evidence.** Tier 3, verified: `ratelimit.rs:51-64` `caps()` contains exactly `NavBlocked`,
`NavError`, `WebviewCrash`, `FocusLost`; its own doc says *"Every event not listed here is
uncapped"*, and `admit()` (`:129-134`) returns `Allow` for an unbucketed event. My draft's
"rate-capped by the standard Logger bucket" was **false** — conceded.

Why 6/6: within one page load the `degraded` latch (`offline.html:41-42`) already bounds
emission to one, so the page is not the driver. The driver is repeated *page loads* —
`webview.crash`→navigate-home recovery, nightly reload, safe-mode cycling. Matching
`WebviewCrash`'s 6/min bucket caps the co-driver at its own rate.

**Is this a parent amendment?** No. Precedent is in-tree: `FocusLost` is a fourth cap that is
**not** in parent §6's "defaults:" list (line 626 names three), added in-code with a written
rationale at `ratelimit.rs:55-61`. `grep -n "TEL-09"` over the parent returns **no dedicated
TEL-09 clause** — only the "TEL-07/09" bullet at line 620 whose prose says *"defaults:"*, i.e.
an open list. Extending `caps()` with a rationale is established practice, not a contract
change. (If the Critic reads §6 line 626 as closed, the fallback is to leave `media.error`
uncapped and rely on the `degraded` latch — I do not prefer it, but it is not a defect.)

## E3 — loop-boundary self-monitoring

**Proposal.** Replace the one-shot `setTimeout` watchdog with a `setInterval` that keeps the
existing 3 s startup semantics and adds the steady-state check: every 5 s, if not `degraded`
and not `v.paused`, assert `v.currentTime !== last || v.currentTime === 0` progressed; a
non-advancing sample with `readyState >= 2` → `fallback("stall")` → E1 reports
`{kind:"stall", at:currentTime}` → degrade per arch-09.

**Requirement.** parent §3.4 arch-09; PF-05. Discharges the gap my draft correctly identified
and the verifier confirmed: the shipped watchdog is a one-shot `setTimeout` with predicate
`v.currentTime === 0` (`offline.html:65-68`), meaningful only pre-first-frame.

**Evidence.** Tier 3, read in full: `crates/kiosk-main/bundled/offline.html`, 72 lines. The
`error` listener at `:49-51` exists and my draft omitted it — corrected: **all four** arch-09
signals (`error`/`stalled`/`emptied`/`play()` rejection) route through `fallback` and all four
now report through E1.

**Open decision retained.** `ended`+`timeupdate` vs `requestVideoFrameCallback` — unchanged,
still a plan-time check, still "don't assume" (no WebKitGTK on any machine in this review;
`pkg-config --exists webkit2gtk-4.1` false).

**Dependencies.** E1 (the reporting channel), P2-A (same file).

## E4 — webview-process RSS, using the `System` that is already in hand

**Proposal.** `kiosk_core::metrics::sample` gains `sys.refresh_processes(ProcessesToUpdate::All,
true)` and a pure helper `webview_rss_bytes(map: &HashMap<Pid, Option<Pid>>, mem: &HashMap<Pid,
u64>, self_pid) -> u64` that sums the **descendant subtree** of the current pid (excluding
self). New `HealthSample.webview_rss_mb` + one `to_fields` key. No new dependency, no new
`windows` feature, no `unsafe`, no `#[cfg]`.

**Requirement.** parent §9 P2 row ("health-sampled RSS"); parent §6 line 671 (`health.sample`
… **webview RSS** … `(P2)`) — the field is *already specified*, so no taxonomy amendment;
D2e design line 127 defers exactly this to P2.

**Evidence (verified by me).**
- Tier 3: `health.rs:1-5` module doc says sampling logic lives in `kiosk_core::metrics` and
  that webview RSS *"is P2 and does not belong here."* My draft's "`health.rs` gains an RSS
  sample" named the wrong module and the module says so. **Conceded**; the change moves to
  `crates/kiosk-core/src/metrics.rs` (`HealthSample` `:6-12`, `sample` `:20-44`, `to_fields`
  `:47-56`). `health.rs` is untouched — it only owns the tick.
- Tier 3: `health.rs:24-25,42` holds `System` by value across ticks and passes `&mut sys` into
  `metrics::sample`; `metrics.rs:26-28` already calls `refresh_cpu_usage`/`refresh_memory`.
  The object is in hand at the exact call site.
- Tier 4, `sysinfo-0.32.1`, read: `System::refresh_processes(ProcessesToUpdate, bool)`
  `src/common/system.rs:289-293`; `System::processes()` `:386`; `Process::memory()` `:1314`
  documented as *resident set*; `Process::parent()` `:1356`; `get_current_pid()` `:2295`.
  Cross-platform, all of it.
- Tier 3: `sysinfo = "0.32.1"` in `crates/kiosk-main/Cargo.toml`, `"0.32"` in
  `crates/kiosk-core/Cargo.toml`.
- Tier 4: `windows-0.61.3` `Win32_System_ProcessStatus` is **not** among the 11 features
  declared in `kiosk-main/Cargo.toml` — I confirmed the list. `GetProcessMemoryInfo` would
  need a new feature and an `unsafe` block, for a number `sysinfo` already returns.

**Why the subtree, not a process name.** WebKitGTK runs content in `WebKitWebProcess`; WebView2
runs a browser process plus renderers. Both are **descendants of kiosk-main**, so a
parent-pointer walk down from `get_current_pid()` is engine-agnostic and needs no name
matching, no per-platform branch, and no knowledge of how many helper processes an engine
spawns. Cost: one `/proc` enumeration per `health_sample_s` (default 60 s, `schema.rs`
default / `validate.rs` range [10,3600]).

**Q2.** My draft proposed two platform-specific mechanisms where one already-instantiated
dependency covers both. The verifier's counterexample stands. This is the corrected design.

**Residual risk (declared).** If a webview helper is reparented (its intermediate parent dies
first), it drops out of the subtree for that sample. Bounded: it is a dying tree, and the
latch (E5) requires N consecutive samples, so a one-sample dip cannot trip or un-trip
anything. Pinning: the pure helper is host-tested over a synthetic pid map including an
orphan; the real number is observed in scenario 18's RSS series.

## E5 — memory-cap restart, against the key that already ships

**Restated against the real key.** There is **no new config key**. `maintenance.max_webview_mem_mb`
exists today and is fully declared, validated, documented and tested:

- `crates/kiosk-core/src/config/schema.rs:233-234` — `#[serde(default = "d_max_mem")] pub
  max_webview_mem_mb: u64` on `struct Maintenance`.
- `crates/kiosk-core/src/config/validate.rs:107-114` — `{0} ∪ [256, 8192]`.
- `validate.rs:15-21` — `UNIMPLEMENTED` contains `("maintenance.max_webview_mem_mb", "P2")`;
  the arm that detects "set to non-default" is `validate.rs:181-183`.
- `validate.rs:265-271` — `max_webview_mem_allows_zero_meaning_off_but_rejects_between`.
- `schema.rs:343` — `assert_eq!(c.maintenance.max_webview_mem_mb, 1500)`.
- parent §5.2 line 538 and parent §10 line 872 both name it.

I read all of it. My draft's *"New config key `memory_max_mb`"* was **false on every count**
and would have created a second memory key shadowed by `Maintenance`'s
`#[serde(flatten)] unknown` (`schema.rs:235-236`) — an operator's `max_webview_mem_mb` would
keep landing on the inert key while the new one drove behavior, with the RT-08 warning still
firing "feature unavailable" for a feature that now exists. Fully conceded and withdrawn.

**What the change actually is.**
1. `validate.rs:19` — **delete** the `("maintenance.max_webview_mem_mb", "P2")` row from
   `UNIMPLEMENTED`; `validate.rs:181-183` — delete its match arm; update the RT-08 warn-path
   tests. Schema and range validation are **unchanged**. Net config diff is a deletion.
2. `kiosk-core`: a pure latch `struct MemCap { over: u32 }` with
   `fn observe(&mut self, rss_mb: u64, cap_mb: u64) -> bool` — `cap_mb == 0` ⇒ always `false`;
   N consecutive samples strictly over ⇒ `true` once, then reset. N = 5.
3. `kiosk-main`: on `true`, `std::process::exit(80)`.

**Exit code — decided, not deferred.** `80`. Verified free: the complete set of exit-code
literals in the workspace is `86` (`pinpad.rs:156`, `watchdog.rs:196-197`), `0`
(`kiosk-launcher/src/main.rs:144`, `job.rs:24,243`, `cli.rs:31`), pass-through
(`kiosk-launcher/src/main.rs:253`), `2` (an example binary). `80` is below 86, outside
`128+signal` (129-159), and outside any negative-sentinel scheme — so it survives **whichever**
encoding P2-C picks for signal death (C design lines 118-122, open decision line 182). E
reserves `80`; C only has to not pick it. Open decision #1 is closed from E's side.

**The launcher interaction — I take the position the verifier says I owe.** The verifier is
correct that "no launcher change at all" is true-but-incomplete. `watchdog.rs:193-199` restarts
on any non-86 code, and `restart()` (`:121-165`) unconditionally emits `watchdog.restart`
(ERROR, `event.rs:138`), doubles backoff (`:161`), and pushes into the 600 s window with
`> 5 ⇒ safe = true` + `watchdog.safe_mode` CRITICAL + `Action::SpawnSafe` (`:148-157`,
`:236-240`). I verified all of it.

My position: **the escalation is unreachable from cap exits, by construction of the latch, and
that is the design — not an accident.** `restart()`'s sibling path at `watchdog.rs:232-239`
clears `restarts`, resets `backoff_s` to 1 and clears `safe` once an Armed run crosses
`healthy_run_s`. The latch's minimum dwell before it can fire is `N × health_sample_s` =
5 × 60 s = **300 s**, against a launcher default `healthy_run_s` of **120 s**
(`kiosk-launcher/src/main.rs:120`, pinned by `:289`). So every cap exit lands in a
window that the healthy-run path already cleared: `restarts.len()` becomes 1, never > 5.
Safe mode is not reachable. This makes the latch's dwell a **load-bearing invariant**, and it
gets a host test that asserts `N × health_sample_s_min (10) ≥ healthy_run_s` is *checked*, not
assumed — concretely: the latch reads the effective `health_sample_s` and raises N so that
dwell ≥ 300 s regardless of a fast sample cadence.

*Residual risk, declared:* an operator who raises `healthy_run_s` in `kiosk.ini` above the
dwell re-opens escalation. Bound: `healthy_run_s` is bootstrap config (`bootstrap.rs:26`), not
remote config — it is a technician-set value on the device. Documented in the operator note,
not defended in code (defending it needs kiosk-main to read the launcher's bootstrap file —
a coupling not worth buying).

**"Default 0 = off, Windows fleets see zero behavior change" — conceded, the property is
false.** The shipped default is 1500 (`schema.rs:343`; parent §5.2 line 538). I verified it.
I will not "fix" this by changing the default: parent §5.2 pins 1500 and parent §10 requires
*"a `max_webview_mem_mb` breach fires a restart"*, so a default of 0 would contradict the
parent twice. The real migration/rollout design, which replaces the false property:

- **Ordering is the mitigation, not a flag.** E4 (sample) and E5 (enforce) are separate tasks
  in a fixed order. E4 merges first and puts `webview_rss_mb` into every `health.sample`.
  E5's enforcement merges only after the Windows soak job (parent §10, wired by F) has
  produced an RSS series, so 1500 is a *measured* number for the fleet rather than an
  inherited one. If the series says 1500 is under the real working set, the corrective action
  is a parent §5.2 default amendment raised as a defect — not a silent divergence in E.
- **Operator lever already exists and already works.** `0` = off is range-valid today
  (`validate.rs:108-114`, tested `:267`), reachable via signed remote config.
- **Fleets that already set the key have already been warned.** `config.warn` "feature
  unavailable in this build" has fired for them since P1 (`validate.rs:181-183`) — the
  RT-08 table exists precisely so that the value starts taking effect at the phase it is
  tagged with. That is a designed migration, and E's job is to remove the row.
- **Fleets on the default get a stated behavior change** in the release note: from P2, a
  webview tree exceeding 1500 MB for 5 consecutive health samples restarts the app. That is
  what parent §9's "memory cap restart" means. I state it in both directions per C3: this is
  **stricter** than P1 Windows behavior, deliberately, and it is the parent's instruction.

**Severity/durability of the cap event — restated, and the event is withdrawn.** My draft's
*"spool flushed by the existing shutdown path"* is **false**: `main.rs:1234-1242` calls
`telem.app_stop()` + `cancel.cancel()` and nothing else, and its own comment disclaims
durability, resting it on `Spool::append`'s fsync — which `spool.rs:1-7` / `:240-252` gate on
`Severity::is_high()` (WARNING+, `event.rs:21-26`). And a `std::process::exit` never reaches
`RunEvent::Exit` at all (precedent: `pinpad.rs:156`). Conceded.

The consequence the verifier draws is right, and it kills the event rather than fixing it:
**the explaining record does not have to be written by the dying process.** The launcher
survives the exit and already writes `watchdog.restart` at **ERROR** — fsynced — carrying
`code` as a first-class field (`sink.rs:203-217`: `code`, `backoff_s`, `cause`). A restart
line reading `watchdog.restart{code: 80, cause: "exit"}` is durable, greppable, emitted by a
live process, and costs **zero** new taxonomy. So `health.memory_cap` is withdrawn (see
Withdrawals). The RSS number that tripped it is in the preceding `health.sample`
(`webview_rss_mb`, INFO) — written to the spool before the clean exit, no fsync required
because the process exits deliberately rather than being killed.

**Testing.** Host: latch (consecutive semantics, `0` disables, dwell ≥ 300 s, fires once);
subtree-sum helper over a synthetic pid map (normal, orphan, self-only); exit code is `80` and
never `86`.

## E6 — the contingency, designed now (PF-05)

Unchanged from the draft and unchallenged: two stacked `<video>` elements, both preloaded; on
`ended` the hidden one (already at 0, paused-ready) plays and swaps to front; the other resets
in background; no visible element ever seeks. **Trigger rule, mechanical:** any `media.error`
with `kind:"stall"` at a loop boundary during scenario 18 ⇒ the contingency task activates in
the plan. Native-GL path stays out unless double-buffering also fails on hardware (it forfeits
the one-HTML-path property, parent §3.4).

**Evidence.** Tier 1, verified verbatim: parent §11 line 893 — *"fallback = seamless no-seek
loop or native GL path"*; parent §3.4 lines 289-292 same ordering. E's ordering matches.

**Dependency.** E3 supplies the trigger evidence; conditional on the soak.

## E7 — harness GStreamer environment, including the deliberate miss

The four packages parent §3.4 lines 285-288 names —
`gstreamer1.0-plugins-{base,good,bad}`, `gstreamer1.0-libav` — installed in the smoke/soak
environment. One run **removes `libav`** and asserts the silent-black case is caught: the
progress watchdog fires, `fallback` degrades to the splash, and **one `media.error`
`{kind:"no_progress"}` reaches the spool**. That last clause is the point — post-E1 the
assertion is on the spool, not on a console line.

**Evidence.** Tier 1: parent lines 285-290 (*"a missing element yields a silent black video,
so packaging CI smoke-tests the offline path"*). Tier 2: P2-G design lines 36-37 carries the
identical four; three-way consistent, I checked.

**Dependencies.** P2-G owns the `.deb` declaration; P2-F owns the harness. E owns only the
assertion.

## E8 — soak protocol (scenario 18) and pass criteria

**Scenario 18** (free — A 1-7, B 8-12, C 13-15, D 16-17; P2-F line 38 independently names
"E 18"). Fixture: config-down boot → offline video. Durations: in-session ~2 h; scheduled CI
8 h+ in `debian:12` (F); hardware ≥72 h (G, H5, RT-05).

**Pass criteria, corrected.** Zero `media.error` in the spool; process alive; **zero launcher
restarts of any kind, including a memory-cap exit** — `offline.html` has no leak source, so a
cap trip during this soak is a real leak and **fails** the soak (this resolves the ambiguity
the verifier flagged; the cap is left at its default, not disabled); `webview_rss_mb` delta
over the window under a bound declared from the first-run baseline then pinned; loop count
consistent with wall-clock.

**The parent's memory assertions are not scenario 18's — and E declares them.** Parent §10
lines 870-875 specifies **two** soaks: a **Windows-runner** memory soak (leaking page,
accelerated thresholds, asserts bounded RSS + *a `max_webview_mem_mb` breach fires a restart* +
*nightly reload resets RSS*) and the **Debian 12** offline-video soak (PF-05). My draft
collapsed them and dropped all three named assertions. Conceded. Restated:

- **Scenario 18 = the Debian offline-video soak only.**
- **Scenario 18-W = the Windows memory soak**, owned by E (E owns the feature), wired by F.
  Fixture: a deliberately leaking local page; `max_webview_mem_mb` set to the range floor
  **256** and `health_sample_s` to **10** to accelerate (both range-valid today,
  `validate.rs:108-114,118-121`) — dwell shrinks to 50 s. Asserts: (a) `webview_rss_mb`
  climbs and is reported; (b) the breach produces **exit 80** and a launcher restart, with
  `watchdog.restart{code:80}` in the spool; (c) with `maintenance.nightly_reload` set a few
  minutes out, `webview_rss_mb` after the reload is below the pre-reload peak — the reload
  timer already ships (`crates/kiosk-main/src/maintenance.rs`, wired `main.rs:1189`).
  (b) is the only test that proves the cap works end-to-end; the verifier is right that its
  absence was the material gap, not the citation.

**`WEBKIT_DISABLE_COMPOSITING_MODE=1` — declared assumption, now pinned.** P2-A line 316
permits the flag *"in the smoke environment only"*. Scenario 18 is a soak, not a smoke run,
and disabling compositing plausibly moves the video off the accelerated path. **Rule:
scenario 18 runs without the flag.** If the harness cannot come up without it, that fact is
recorded in the soak artifact and the run is annotated as measuring the non-composited path.
Residual risk: a stall class that only appears with compositing enabled would be invisible in
CI — bounded by H5, which runs on real hardware with the real compositor.

**Testing / per-PR.** Soak is never per-PR (F line 38). Per-PR from E: the missing-decoder
case (E7) only, as a variant fixture — cost ~3 s, F decides inclusion.

---

## Response to the verification record

### FALSE (6) — all conceded, four with a replacement design

| # | Finding | Disposition |
|---|---|---|
| 1 | `memory_max_mb` is not new; contradicts `maintenance.max_webview_mem_mb` | **CONCEDE.** Key withdrawn. E5 restated against the shipped key; the config work is a **deletion** from `UNIMPLEMENTED` (`validate.rs:19`, `:181-183`), not a schema addition. I read `schema.rs:225-240,343`, `validate.rs:15-21,107-114,175-190,265-271`. |
| 2 | "default 0 = off / zero Windows behavior change" — default is 1500 | **CONCEDE the property.** It is false and I will not restore it by changing the default (that would contradict parent §5.2 line 538 and §10 line 872). Replacement = the four-part rollout in E5: E4-before-E5 ordering with the Windows soak supplying the number, the pre-existing `0` lever, the RT-08 warning as the already-served migration notice, and an explicit stated-stricter-than-Windows divergence per C3. |
| 3 | "`health.rs` gains an RSS sample" — wrong module | **CONCEDE.** `health.rs:1-5` says so in its own doc. Change relocates to `kiosk_core::metrics` (`metrics.rs:6-12,20-44,47-56`); `health.rs` untouched. |
| 4 | Wrong process measured (main, not webview) | **CONCEDE**, and the mechanism changes with it. Required quantity is webview-process RSS (parent §6 line 671; D2e `:24`,`:127`; `health.rs:3`). Design = subtree sum over `sysinfo`, per E4. Both proposed platform mechanisms withdrawn. |
| 5 | "rate-capped by the standard Logger bucket" | **CONCEDE.** `ratelimit.rs:51-64` + `admit()` `:129-134` verified. Replacement = E2, one `caps()` row, with the `FocusLost` precedent (`ratelimit.rs:55-61`) as the authority for extending the list in-code. |
| 6 | "spool flushed by the existing shutdown path" | **CONCEDE.** `main.rs:1234-1242` performs no flush; durability is `Spool::append` fsync gated on `is_high()`. Replacement = withdraw the event entirely; the durable, greppable record is the launcher's `watchdog.restart{code:80}` (ERROR, `sink.rs:209-217`) written by a **surviving** process. |

### The six called out head-on

1. **Key collision** — answered in E5. Read in full and restated. The remaining config work is
   a deletion.
2. **"Zero behavior change" is false** — conceded, migration design given in E5, no new default.
3. **Wrong process** — conceded; `sysinfo` subtree sum (E4). I verified `refresh_processes`
   `system.rs:289`, `processes()` `:386`, `Process::memory()` `:1314`, `parent()` `:1356`,
   `get_current_pid()` `:2295`, and that `Win32_System_ProcessStatus` is absent from
   `kiosk-main/Cargo.toml`'s feature list. Q2: one existing dependency, already instantiated at
   the call site, beats two new platform mechanisms plus a feature flag plus `unsafe`.
4. **Rate cap + durability** — both conceded; E2 and the withdrawal of `health.memory_cap`.
   On severity specifically: the verifier's point that an INFO `health.memory_cap` would be
   lost is exactly why the event is gone rather than promoted — promoting it to WARNING would
   have worked, but it would have cost a parent §6 amendment for a record the launcher already
   writes at ERROR.
5. **New telemetry costs more than E says** — accepted as the reason the new event dies.
   `Event::MediaError` being dead is folded into E1 as an explicit `Telemetry::media_error`
   deliverable. `health.memory_cap` would have required: an `Event` variant, two
   compiler-forced arms (`event.rs:59-83`, `:87-113` — both exhaustive, no catch-all), a
   `TAXONOMY` row (**not** compiler-forced), a bump of `assert_eq!(TAXONOMY.len(), 23)`
   (`event.rs:157-160`), and a parent §6 table amendment — I counted the parent table at lines
   651-672 and it is exactly 23 events. Net taxonomy delta of P2-E: **zero**.
6. **B-dependency misstated** — see below.

### The B dependency, restated — and one correction to the mechanical note

**Conceded:** P2-B's reporter is `#[cfg(not(windows))]` (B design lines 95-97), and B line
109-110 says *"Windows is untouched"*. My "registered cross-platform, same as B" was wrong on
the registration property. Correct statement: **the shape is shared (page →
`#[tauri::command]` → `Telemetry` method → existing event), the gating is opposite** — B's
command is Linux-only because CSP injection is Linux-only; E's is unconditional because
`offline.html` and its failure modes ship on both platforms.

**Correction, tier 4.** The verifier's mechanical note — that B's cfg'd command and E's
unconditional one *"cannot share one `generate_handler!` invocation without a `cfg`-split of
the whole `invoke_handler` line"* — is **not correct for the pinned version**. `Cargo.lock`
resolves `tauri-macros 2.6.3`. In
`tauri-macros-2.6.3/src/command/handler.rs`, `CommandDef` parses **outer attributes** ahead of
the path (`:12-22`, `let attrs = input.call(Attribute::parse_outer)?`), and codegen emits them
onto the generated match arm (`:155,159,169` and the repetition at `:177`:
`#(#(#attrs)* #command_name_macros => #wrappers!(#paths, #invoke),)*`). So

```rust
.invoke_handler(tauri::generate_handler![
    pinpad::verify_pin,
    media::media_error,
    #[cfg(not(windows))] csp::report_violation,
])
```

compiles: on Windows the arm — and the macro call inside it that would reference the
non-existent wrapper — is cfg-stripped before expansion.

**Dependency as declared:** E1 owns converting `main.rs:990` from a single-path list to a
multi-entry list and lands first; **B appends one cfg-attributed entry**. No cfg-split of the
builder chain, no ordering hazard beyond "E1 before B's reporter". If B lands first, the roles
swap and E appends — the resolution is symmetric. Flagged to the Moderator as a **cross-spec
integration point neither spec currently names**; E now names it.

### Launcher-side interaction — position taken

Stated in E5. Summary: "no launcher change at all" is true for the code (verified:
`watchdog.rs:193-199` restarts on any non-86; pinned by `:311-320`, `:323-338`) and materially
incomplete for the behavior (verified: `restart()` `:121-165` — ERROR log, backoff doubling
`:161`, 600 s window `:80`, `> 5 ⇒ safe_mode` CRITICAL `:148-157` + `SpawnSafe` `:236-240`). My
position is that the interaction is **designed against, not tolerated**: the latch's minimum
dwell (N × `health_sample_s`, floored at 300 s) exceeds the launcher's default `healthy_run_s`
of 120 s (`kiosk-launcher/src/main.rs:120`), and `watchdog.rs:232-239` clears the crash-loop
window on every healthy run, so a cap exit can never be the 6th entry in a 600 s window.
Backoff doubles once (1→2 s) and resets on the next healthy run. Safe mode is unreachable from
cap exits. This is a host-testable invariant on the latch and it is now spec.

### DRIFT (10) — dispositions

| Finding | Disposition |
|---|---|
| `bundled/offline.html` path not crate-qualified | **CONCEDE** → `crates/kiosk-main/bundled/offline.html` throughout. |
| `:52-56`, `:59-62`, `:65-67` off-by-one ranges | **CONCEDE**, and I go further: since P2-A rewrites the same file (A line 112) and shifts every line, **all line-number citations into `offline.html` are replaced by symbol references** — `fallback()`, the `error`/`stalled`/`emptied` listeners, the `play()` `.catch`, the progress watchdog. Kills the drift class permanently rather than re-numbering it. |
| Citations pre-date P2-A's edit | **CONCEDE**, handled by the same rule; E1/E3 land after A. |
| `error` handler at `:49-51` uncited | **ACCEPTED as an omission** — all four arch-09 signals now named in E3. |
| arch-10 cited but discharges nothing | **CONCEDE.** arch-10 is Android autoplay-gesture (parent lines 204, 272-275), P3. Dropped from E's header; requirement basis is §3.4 arch-09 + §9 P2 row + §10 + §11. |
| "§10:" quote is actually parent §11 line 900 | **CONCEDE.** Attribution corrected to §11 (Risks); the §9 P2-row citation (line 839) is the verbatim assignment and stands. |
| Soak omits the parent's three memory assertions | **CONCEDE**, materially. Fixed by splitting scenario 18 / 18-W in E8; (b) breach→restart is the assertion that proves the feature. |
| "B's reporter is cross-platform" | **CONCEDE** — restated above. |
| "A's scenario 3 render check" is a page check | **CONCEDE.** P2-A line 350: *"Offline-video in A is wiring-only … playback quality is P2-E's."* There is no per-PR video assertion to inherit. E's per-PR contribution is E7's missing-decoder case, and nothing else. |
| Scenario 18 free / hand-offs consistent | No dispute (VERIFIED). |

### UNVERIFIABLE (5)

Four were already pinned and I re-affirm the pins: `requestVideoFrameCallback` (open decision
#3, plan-time check); loop-boundary stalls (scenario 18 **is** the gate; parent §11 line 893
pre-declares the risk); Debian #1062012 (inherited from parent line 290/893, not E's burden);
GStreamer sufficiency (E7's install + deliberate-miss run).

The fifth — **`WEBKIT_DISABLE_COMPOSITING_MODE=1` soak-neutrality** — was genuinely unpinned.
**Now pinned** by E8's rule: scenario 18 runs without the flag; if the harness requires it,
the run is annotated and H5 (hardware, real compositor) is the covering gate. Declared as an
assumption with residual risk, not asserted.

### Undeclared assumptions (verifier §11, items 1-14)

1, 2 → conceded (E5). 3, 4, 5 → conceded (E4). 6 → conceded (E2). 7 → conceded (event
withdrawn). 8 → conceded + corrected (B dependency). 9 → position taken (E5 interlock).
10 → closed: exit code **80**, decided here, not deferred. 11 → moot: no new event, taxonomy
delta zero. 12 → conceded: `Telemetry::media_error` is now an explicit E1 deliverable.
13 → conceded: symbol references replace line numbers. 14 → pinned (E8).

**No item is left silent.**

---

## Withdrawals / restructuring

1. **`health.memory_cap` event — WITHDRAWN.** Reason: it would cost a parent §6 amendment, a
   `TAXONOMY` row and a `23 → 24` bump of a deliberately pinned test (`event.rs:118-121`
   header: *"you are changing the contract with the fleet's log-based metrics and alerting"*),
   to record something a **surviving** process already records durably at ERROR with the code
   as a field (`sink.rs:209-217`). Q2. Net taxonomy delta for P2-E is now zero.
2. **Config key `memory_max_mb` — WITHDRAWN.** The key ships
   (`schema.rs:234`) and is already tagged `"P2"` in `UNIMPLEMENTED` (`validate.rs:19`)
   waiting for this feature. Replaced by a deletion.
3. **`/proc/self/status` parse and `GetProcessMemoryInfo` FFI — WITHDRAWN.** Replaced by
   `sysinfo::Process::memory()` over the pid subtree. Removes a parser, an `unsafe` block, a
   new `windows` feature flag, and both `#[cfg]` branches.
4. **"Zero Windows behavior change" safety property — WITHDRAWN**, not repaired. Replaced by a
   stated, ordered rollout and an explicit C3 divergence note (stricter than P1 Windows, by
   parent instruction).
5. **arch-10 citation — WITHDRAWN** (Android/P3, discharges nothing here).
6. **All `offline.html` line-number citations — WITHDRAWN**, replaced by symbol references,
   because P2-A edits the same file.
7. **RESTRUCTURED: scenario 18 splits into 18 (Debian offline-video, PF-05) and 18-W (Windows
   memory soak, parent §10).** My draft's single soak silently dropped the parent's three named
   memory assertions while claiming ownership of the feature they test. 18-W is the gate that
   proves the cap works; without it E5 would ship a restart path never exercised end-to-end.
8. **RESTRUCTURED: E4 and E5 are ordered tasks, not one change.** The sampler must produce a
   number before the enforcer acts on 1500.
9. **NOT withdrawn:** E6 (contingency design), E7 (harness + deliberate miss), the
   mechanical contingency trigger, and E's `.deb`/hardware/CI hand-offs to G and F — all
   verified consistent three ways and unchallenged.
