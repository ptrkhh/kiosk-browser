# VERIFIER REPORT — P2-E (`2026-08-06-p2e-offline-video-soak-design.md`)

Role: verification only. No proposals, no opinions. Every entry is a mechanically
checkable claim from the spec, checked against tier 1–4 sources.

Environment note: repo at `/home/user/kiosk-browser`, HEAD `1decd59`. Host is
**Ubuntu 24.04.4**, `pkg-config --exists webkit2gtk-4.1` → **false**, `ldconfig -p |
grep -c webkit2gtk` → **0**, no `gst-inspect-1.0`, no `weston`, no `cage`. **There is no
WebKitGTK/GStreamer/compositor runtime in this environment**, so every runtime-behavior
claim about WebKitGTK is UNVERIFIABLE here by construction.

---

## 1. `bundled/offline.html` — the cited file and lines

Real path: **`/home/user/kiosk-browser/crates/kiosk-main/bundled/offline.html`** (72 lines).
There is no `bundled/` at the repo root.

### 1.1 Path citation — **DRIFT (LOW)**
Spec front-matter and Scope both write `bundled/offline.html`. The file is at
`crates/kiosk-main/bundled/offline.html`. Every other file citation in the P2 spec set is
crate-qualified or `file.rs:NNN`-style; this one is not resolvable as written from repo root.

### 1.2 `fallback(why)` exists — **VERIFIED**
```
40	        function fallback(why) {
41	          if (degraded) return;
42	          degraded = true;
43	          v.style.display = "none";
```

### 1.3 Static-splash degrade — **VERIFIED**
`v.style.display = "none"` (line 43) over `body { background: #000 }` (line 11), with the
in-file comment at lines 33-36 naming it: *"must degrade to the black \"offline\" splash —
this page's own black background — never a hung/frozen frame. The `<video>` is simply
hidden; the body stays black."*

### 1.4 The `:44-47` "recorded gap" comment — **VERIFIED**, verbatim:
```
44	          // Durable media.error telemetry from the page needs an IPC command bridge
45	          // (not wired in D2a — app-origin pages get no custom IPC yet); the console
46	          // marker is the honest minimal signal until that lands.
47	          console.error("media.error:", why);
```
The comment body is **44-46**; 47 is the `console.error` it describes. Citing `:44-47` as
"the recorded gap" is fair. **VERIFIED.**

### 1.5 `stalled`/`emptied` wired, cited `:52-56` — **VERIFIED (mechanism) / DRIFT (range)**
```
52	        v.addEventListener("stalled", function () {
53	          fallback("video stalled");
54	        });
55	        v.addEventListener("emptied", function () {
56	          fallback("video emptied");
57	        });
```
Both handlers exist and call `fallback`. The cited range `:52-56` **truncates the `emptied`
handler mid-statement** — the true range is `:52-57`. Off-by-one.

Also present but **not cited by E**: the `error` handler at `:49-51` — which is the arch-09
event the parent names first (`error`/`stalled`/`emptied`).

### 1.6 `play()` rejection, cited `:59-62` — **VERIFIED (mechanism) / DRIFT (range)**
```
58	        // Autoplay policy is satisfied by muted+playsinline, but honor the rejection.
59	        var p = v.play();
60	        if (p && typeof p.catch === "function") {
61	          p.catch(function (e) {
62	            fallback("play() rejected: " + e);
63	          });
64	        }
```
True range `:59-64` (or `:58-64` with the comment). Cited `:59-62` cuts the closing braces.

### 1.7 3 s progress watchdog, cited `:65-67` — **VERIFIED (mechanism) / DRIFT (range)**
```
65	        // Watchdog: currentTime must advance within 3 s of load, else assume no decode.
66	        setTimeout(function () {
67	          if (!degraded && v.currentTime === 0) fallback("no playback progress in 3s");
68	        }, 3000);
```
True range `:65-68`. Cited `:65-67` omits the `3000` argument — the literal that makes it a
*3 s* watchdog.

E's Component 2 claim that "the 3 s watchdog covers startup only" is **VERIFIED** — it is a
one-shot `setTimeout`, not an interval, and its predicate is `v.currentTime === 0`, which is
only meaningful pre-first-frame.

### 1.8 **Citation fragility vs P2-A — DRIFT (MED), undeclared**
P2-A design line 112 states: *"`offline.html` picks the mp4 URL by `location.protocol`
(page-local JS, no serve-time templating)."* The file at HEAD hardcodes
`src="http://kioskasset.localhost/kiosk-offline.mp4"` (line 30) with no such JS. P2-A therefore
**modifies this file and will shift every line number E cites**, and E declares itself as
"Builds on P2-A". Every `:NN-NN` in E is a citation against a pre-A file.

---

## 2. `health.rs` — what exists, and the least-mechanism question

Real path: **`/home/user/kiosk-browser/crates/kiosk-main/src/health.rs`** (71 lines).

### 2.1 What it polls and on what tick — **VERIFIED**
`health.rs:34-45`: a `tokio::time::interval(Duration::from_secs(period_s.clamp(10, 3600)))`
with `MissedTickBehavior::Delay`, driven by `logging.health_sample_s` (default 60,
`schema.rs:250` / `main.rs:720`). Each tick calls `kiosk_core::metrics::sample(...)` then
`telem.health(kiosk_core::metrics::to_fields(&s, dropped()))`. D2e metrics pipeline
**exists** and is exactly as E describes in shape (`metrics.rs:20-56`, `telemetry.rs:153`).

### 2.2 "`health.rs` gains an RSS sample" — **FALSE (location)**
`health.rs:1-5`, verbatim module doc:
```
//! Periodic `health.sample` timer (spec §6, P1-D2e Task 2). BASIC (P1) fields only —
//! CPU %, mem used/total, disk-free for the data-dir volume, uptime, and
//! `spool_dropped_expired`. Webview-process RSS / `max_webview_mem_mb` enforcement
//! is P2 and does not belong here. All sampling logic lives in
//! `kiosk_core::metrics` (Task 1); this module only owns the tick/cancel loop.
```
`health.rs` **owns no sampling logic at all** — it is a tick/cancel loop. The RSS sample
attaches to `kiosk_core::metrics::HealthSample` (`metrics.rs:6-12`) + `metrics::sample`
(`:20-44`) + `metrics::to_fields` (`:47-56`), i.e. **`kiosk-core`, not `kiosk-main/health.rs`**.
E names the wrong file, and the file it names says so in its own first paragraph.

### 2.3 The doc comment also pre-names the *webview* process — **FALSE (wrong process)**
The existing code, the D2e design (`2026-07-27-p1d2e-observability-design.md:24, :127`), the
D2e plan (`plans/2026-07-27-p1d2e-observability.md:17`), and the parent §6 table all say
**webview-process RSS**:

- parent §6, line 671: `` | `health.sample` | INFO | CPU %, mem, disk free, uptime, **webview RSS**, `spool.dropped_expired` (P2) | ``
- D2e design line 24: *"— NOT webview RSS, and it does not enforce `max_webview_mem_mb`."*
- D2e design line 127: *"Deferred: webview-process RSS + `max_webview_mem_mb` enforcement (P2)"*

E proposes `/proc/self/status` `VmRSS` — that is **kiosk-main's own process**, not the
webview. On WebKitGTK the content runs in a separate `WebKitWebProcess` (P2-A scenario 4
explicitly kills `WebKitWebProcess` as a distinct process); on Windows WebView2 runs in
separate `msedgewebview2.exe` processes. `/proc/self/status` and a `GetProcessMemoryInfo` on
the current process both measure the **wrong process** for the requirement as written.
Consequence: a webview leak — the exact failure mode parent §11 line 900 names — would be
largely invisible to E's sampler, and `max_webview_mem_mb` would never trip.

### 2.4 `sysinfo` already a dependency, already exposes per-process RSS — **VERIFIED; `/proc/self/status` is redundant**
- `crates/kiosk-main/Cargo.toml`: `sysinfo = "0.32.1"`; `crates/kiosk-core/Cargo.toml`:
  `sysinfo = "0.32"`. Parent §4's claim that `metrics/` uses it holds.
- `health.rs` already **holds a `sysinfo::System` across ticks** (`health.rs:24-25, 42`) —
  the object a per-process refresh would use is already in hand and already refreshed.
- Vendored `sysinfo-0.32.1` exposes, cross-platform:
  - `System::refresh_processes(ProcessesToUpdate, bool)` — `src/common/system.rs:289`
  - `System::process(Pid) -> Option<&Process>` — `:400`
  - `Process::memory() -> u64` — `:1314`, documented as *"[size of the resident set]"*
  - `Process::parent()` (for reaching the webview child), `get_current_pid()` — `:2295`

So both platform-specific mechanisms E proposes (`/proc/self/status` parsing **and**
`GetProcessMemoryInfo` FFI) are already served by an existing dependency that is already
instantiated at the exact call site. **The `/proc/self/status` parse is redundant** against
tier-3+4 evidence. This is a Q2 (least mechanism) finding with a verified existing-mechanism
counterexample, not a preference.

---

## 3. Telemetry

### 3.1 `media.error` event + severity already exist — **VERIFIED**
Not `kiosk-main/src/telemetry.rs` — the taxonomy lives in
**`crates/kiosk-core/src/logging/event.rs`**:
- `Event::MediaError` variant — `event.rs:42`
- `Event::MediaError => "media.error"` — `event.rs:72`
- severity arm: `MediaError` is in the `Severity::Warning` group — `event.rs:100`
- pinned table row: `(Event::MediaError, "media.error", Severity::Warning)` — `event.rs:137`
- parent §6 line 661: `` | `media.error` | WARNING | offline video failed to decode/play; fell back to static splash | ``

E's "Telemetry event `media.error` (WARNING) per parent §3.4/§6" — **VERIFIED**, nothing to add.

**Additional fact E does not state:** `Event::MediaError` is a **dead variant today**. Grep
over `crates/**/*.rs` finds it only in `event.rs` (definition, name, severity, taxonomy row).
There is **no `Telemetry::media_error(...)` method** — `telemetry.rs` `pub fn` list is
`net_online, net_offline, app_start, app_stop, config_error, config_applied, panic, nav_error,
nav_blocked, focus_lost, webview_crash, health, config_warn`. E's bridge would be the first
emit site; a `Telemetry` method is required and is not named in E's Components.

### 3.2 `health.memory_cap` — **FALSE (does not exist, and adding it is more than E states)**
Not in `Event`, not in `TAXONOMY`, not in parent §6's 23-row table.

The actual mechanism for adding an event + severity:
1. New `Event` variant (`event.rs:31-54`).
2. New arm in `Event::name()` (`:59-83`) — **exhaustive match, no catch-all**, so the compiler
   forces this (E0004).
3. New arm in `Event::severity()` (`:87-113`) — same, compiler-forced.
4. **New row in `TAXONOMY`** (`event.rs:125-161`) — *not* compiler-forced.
5. **Bump the hardcoded count** in the table-driven test.

### 3.3 The event→severity table-driven test (parent TEL-06) — **VERIFIED, and E must touch it**
`event.rs:118-121` header comment, verbatim:
> `/// The spec's table, verbatim. If you change this, you are changing the`
> `/// contract with the fleet's log-based metrics and alerting.`

`event.rs:157-160`:
```rust
#[test]
fn taxonomy_table_still_covers_all_23_spec_events() {
    assert_eq!(TAXONOMY.len(), 23, "spec §6 defines 23 events");
}
```
Parent §6's table has exactly **23** rows (counted, lines 651-672) — the pin is real and
current. Adding `health.memory_cap` therefore requires a **parent §6 table amendment** plus
the `23 → 24` bump plus a TAXONOMY row. E's spec is **silent on all of it**.

*(RT-09 in parent §10 is the live token-exchange smoke, line 876-877 — not the taxonomy test.
E does not cite RT-09; noting for the record that the taxonomy pin is TEL-06's, at
`event.rs:1-4`: "The mapping is table-driven and asserted by a test … pinned deliberately.")*

### 3.4 "rate-capped by the standard Logger bucket" — **FALSE**
`crates/kiosk-core/src/logging/ratelimit.rs:51-64`, `caps()` — the complete list:
```rust
/// `(event, per_minute, burst)` caps, verbatim from spec TEL-09. Every event
/// not listed here is uncapped.
pub fn caps() -> &'static [(Event, u32, u32)] {
    &[
        (Event::NavBlocked, 10, 20),
        (Event::NavError, 10, 20),
        (Event::WebviewCrash, 6, 6),
        (Event::FocusLost, 10, 20),
    ]
}
```
`RateLimiter::admit` (`:129-134`) returns `Admit::Allow` for any event with no bucket.
**`media.error` has no bucket and is uncapped today.** Parent §6 line 626 lists the same
three defaults (`nav.blocked`/`nav.error` 10/min burst 20; `webview.crash` 6/min). Adding a
`media.error` cap is a TEL-09 / parent §6 change E does not declare.

This matters against E's own Component 2: the loop-boundary monitor emits on every stall. The
`degraded` latch (`offline.html:41-42`) bounds it to one per page load, but a page-reload
loop (nightly reload, `webview.crash` recovery navigate-home, safe-mode cycling) is not bounded.

### 3.5 "spool flushed by the existing shutdown path" — **FALSE**
`main.rs:1234-1242`, the only shutdown path, verbatim comment:
```
// The graceful-exit path: on a locked-down kiosk tao usually tears the process
// down without ever reaching here, so this is best-effort only — WARNING+
// durability already rests on `Spool::append`'s synchronous fsync (see
// `install_panic_hook`), not on this `app.stop`.
.run(move |_app, event| {
    if let tauri::RunEvent::Exit = event {
        telem.app_stop();
        cancel.cancel();
    }
});
```
There is **no flush** in that handler — `telem.app_stop()` + `cancel.cancel()` only. And the
existing precedent for a self-inflicted exit, `pinpad.rs:156`, is a bare
`std::process::exit(86)` which **never reaches `RunEvent::Exit` at all**.

What actually makes the event durable is `Spool::append`'s write-through fsync for
**high-severity (WARNING+) entries only** — `spool.rs:5` (*"entry is written through and
`fsync`ed before `append` returns"*), `spool.rs:248` (*"`append_line` fsyncs the FILE for a
high-severity entry"*), gated by `Severity::is_high()` = WARNING/ERROR/CRITICAL
(`event.rs:21-26`).

**Load-bearing consequence E does not state:** the durability of `health.memory_cap` depends
entirely on it being assigned **WARNING or above**. E never states its severity, and its
`health.*` sibling `health.sample` is **INFO** (`event.rs:94, :159`). An INFO
`health.memory_cap` would be silently lost on exit — the precise outcome E's Error-handling
section claims to prevent ("the restart never loses the event that explains it").

---

## 4. Config — `memory_max_mb` vs `max_webview_mem_mb`

### 4.1 **FALSE — the key is not new, and E's name contradicts the existing one.**

`maintenance.max_webview_mem_mb` **already exists, fully declared and validated**:

- **Schema** — `crates/kiosk-core/src/config/schema.rs:233-234`:
  ```rust
  #[serde(default = "d_max_mem")]
  pub max_webview_mem_mb: u64,
  ```
  under `struct Maintenance`.
- **Range validation** — `crates/kiosk-core/src/config/validate.rs:107-114`:
  ```rust
  // maintenance — {0} ∪ [256, 8192]
  let mem = cfg.maintenance.max_webview_mem_mb;
  if mem != 0 && !(256..=8192).contains(&mem) {
      errors.push(FieldError::new(
          "maintenance.max_webview_mem_mb",
          "must be 0 (off) or within [256, 8192]",
          mem.to_string(),
      ));
  }
  ```
- **RT-08 unimplemented-capability table** — `validate.rs:15-21`:
  ```rust
  const UNIMPLEMENTED: &[(&str, &str)] = &[
      ("content.inject_css", "P2"),
      ("content.inject_js", "P2"),
      ("content.pdf_view", "P1"),
      ("maintenance.max_webview_mem_mb", "P2"),
      ("maintenance.restart_app", "P2"),
  ];
  ```
  with the `config.warn` emitter at `validate.rs:181-183`.
- **Tests already pinned** — `validate.rs:267-270` (0 ok, 256 ok, 100 err, 9000 err).
- **Parent §5.2, line 538, verbatim:**
  `"max_webview_mem_mb": 1500            // 0 = off; {0} ∪ [256, 8192] (P2)`
- **Parent §10 line 872, verbatim:** *"asserts bounded RSS, that a `max_webview_mem_mb` breach
  fires a restart, and that nightly reload resets RSS."*
- **P1-F1 design line 108:** *"**`max_webview_mem_mb`** memory-cap restart (P2)"*.
- **P1-F1 plan line 17:** *"`restart_app` and `max_webview_mem_mb` are **P2** (spec §9) — do NOT
  implement them."*

E's *"New config key `memory_max_mb`"* is **FALSE** on three separate counts:

1. **Not new.** The key ships today, is range-validated, and is already tagged `"P2"` in the
   RT-08 table waiting for exactly this feature.
2. **Wrong name — a contradiction, not a synonym.** Adding `memory_max_mb` would create a
   *second* memory key. `Maintenance` carries `#[serde(flatten)] pub unknown: Map<..>`
   (`schema.rs:235-236`), so a config setting `max_webview_mem_mb` would keep landing on the
   validated-but-unimplemented key while the new one silently drove behavior — with the
   existing RT-08 `config.warn` still firing "feature unavailable in this build" for a feature
   that now exists.
3. **"default 0 = off — Windows fleets see zero behavior change until an operator opts in" is
   FALSE.** The shipped default is **1500**, pinned by `schema.rs:343`
   (`assert_eq!(c.maintenance.max_webview_mem_mb, 1500);`) and parent §5.2 line 538. Implementing
   the cap against the existing key means **every Windows fleet gets a 1500 MB cap enforced on
   upgrade with no operator action**. E's stated safety property does not hold for the key the
   parent actually names.

### 4.2 What implementing it actually touches (not stated by E)
`schema.rs` (unchanged — key exists), `validate.rs:15-21` (**remove** the
`maintenance.max_webview_mem_mb` row from `UNIMPLEMENTED`), `validate.rs:181-183` (remove its
match arm), plus the RT-08 warn-path tests. E's *"schema section placed at plan time beside its
consumers"* describes work that is already done and mis-describes the work that remains
(a **deletion** from `UNIMPLEMENTED`, not an addition to the schema).

---

## 5. Exit codes

### 5.1 Enumeration — complete, `grep` over `crates/**/*.rs`
| Code | Site | Meaning |
|---|---|---|
| `86` | `kiosk-main/src/pinpad.rs:156` (`std::process::exit(86)`) | technician exit (parent §9 P1 row, "emits exit code 86") |
| `86` | `kiosk-core/src/watchdog.rs:196-197` | `ChildExited{86}` → `Action::ExitLauncher{86}` |
| `86` | `packaging` systemd `RestartPreventExitStatus=86` (P2-C design line 87) | supervisor stop-restarting |
| `0` | `kiosk-launcher/src/main.rs:144`; `job.rs:24,243` | peer already supervises — deliberate success |
| `0` | `kiosk-main/src/cli.rs:31` | `--help`/`--version` style early exit |
| *pass-through* | `kiosk-launcher/src/main.rs:253` (`std::process::exit(code)`) | whatever `ExitLauncher{code}` carried |
| `2` | `kiosk-core/examples/kioskctl.rs:150` | example binary only, not shipped |

**There is no exit-code constants module.** `crates/kiosk-core/src/exit.rs` is *not* about exit
codes — it is the exit-gesture argon2id PIN gate + lockout (`verify_pin`, `Lockout`, `Gate`).
`86` is a bare literal at every site.

**Free code for a memory-cap restart: yes**, any non-0/non-86 value. But the space is
**not actually free** — P2-C design line 118-122 introduces signal-death mapping whose
*"Exact encoding chosen at plan time"* (line 122, and its open decision at line 182:
*"negative-signal vs sentinel"*). If C picks `128+signal`, the range 129-159 is consumed
(`watchdog.rs:315` already tests code `137` = SIGKILL). E's open decision #1 explicitly defers
to C, so this is a **declared** coordination dependency, not a defect — but both sides are
currently open, so no code is yet reserved.

### 5.2 "the launcher's existing FSM restarts it … No launcher change at all" — **VERIFIED, with an unstated consequence**
`crates/kiosk-core/src/watchdog.rs:193-199`:
```rust
Event::ChildExited { code, at } => {
    self.now = self.now.max(at);
    if code == 86 {
        return vec![Action::ExitLauncher { code: 86 }];
    }
    self.restart(code, at, "exit")
}
```
Any non-86 code restarts. Pinned by `watchdog.rs:311-320`
(`child_exited_passes_through_the_real_exit_code`, code 137) and `:323-338`
(`miss_with_child_exited_restarts_with_real_code`, code 1). **E's assumption holds.**

**Unstated consequence — the memory-cap restart is treated as a crash.** `restart()`
(`watchdog.rs:121-165`) unconditionally:
- emits `WatchdogEvent::Restart` → `watchdog.restart`, **Severity::Error** (`event.rs:138`);
- **doubles the backoff** (`self.backoff_s.saturating_mul(2).min(60)`, `:161`);
- pushes into the **sliding 10-minute crash-loop window** (`WINDOW_S: u64 = 600`, `:80`) and,
  at `> 5` restarts in the window, sets `self.safe = true` and emits `WatchdogEvent::SafeMode`
  → `watchdog.safe_mode`, **Severity::Critical** (`event.rs:146-150`), after which the
  launcher issues `Action::SpawnSafe` instead of `Action::SpawnMain` (`:236-240`).

So a device that legitimately trips the memory cap 6 times in 10 minutes is put into
**safe mode** and reported as a CRITICAL "device degraded". E's *"a memory-cap exit is just a
`ChildExited` with a greppable code"* is literally true and materially incomplete.

**Interaction with E's own soak pass criteria:** E's pass list includes *"process alive, no
launcher restarts"*. A memory-cap restart is a launcher restart. E does not say whether the
soak's cap is disabled, raised, or whether a cap trip fails the soak.

---

## 6. Tauri command registration and B's reporter

### 6.1 How commands are registered today — **VERIFIED, and there is exactly one**
`crates/kiosk-main/src/main.rs:988-990`:
```rust
tauri::Builder::default()
    .manage(pinpad_state)
    .invoke_handler(tauri::generate_handler![pinpad::verify_pin])
```
`pinpad::verify_pin` is the **sole** `#[tauri::command]` in the workspace
(`pinpad.rs:127-128`), registered **unconditionally / cross-platform**, with its state via
`.manage()`.

### 6.2 What B's spec actually says vs what E claims — **DRIFT (MED)**
E: *"A Tauri command (same page→telemetry pattern as B's CSP-violation reporter; **registered
cross-platform** — the page and its failure modes exist on both platforms)"*.

P2-B design, lines 95-97, verbatim:
> The violation listener reports through a new **`#[cfg(not(windows))]`** Tauri command →
> `telem.nav_blocked("egress", blocked_uri)` — the same `REASON_EGRESS` label Windows
> emits (`egress.rs:22`) …

and B lines 109-110, verbatim:
> Windows is untouched: the native filter remains its sole boundary and the belt is **not**
> injected there — D2b's decision stands on its platform.

B's reporter is **Linux-gated**. E cites it as the pattern precedent while asserting
cross-platform registration; the *shape* (page → `#[tauri::command]` → `Telemetry` method) is a
genuine parallel, the **registration property E attributes to it is the opposite of B's**.
E's own Scope paragraph does declare its two cross-platform changes explicitly, so the frame's
C8 declaration requirement is met — but the "same as B" framing is not supported.

Mechanical note for the record: `tauri::generate_handler![]` takes a plain path list. B's
`#[cfg(not(windows))]` command and E's unconditional one cannot share one `generate_handler!`
invocation without a `cfg`-split of the whole `invoke_handler` line — a collision point between
B and E at the single call site `main.rs:990`. Neither spec mentions it.

(`telem.nav_blocked(reason, url)` does exist — `telemetry.rs:125`. B's citation VERIFIED.)

---

## 7. `GetProcessMemoryInfo` — new Windows dependency/feature?

### **VERIFIED — yes, it requires a new `windows` crate feature.**

- Vendored `windows-0.61.3`: `GetProcessMemoryInfo` is defined at
  `src/Windows/Win32/System/ProcessStatus/mod.rs:107` (linking `psapi.dll`), i.e. under the
  **`Win32_System_ProcessStatus`** feature (`windows-0.61.3/Cargo.toml:647`:
  `Win32_System_ProcessStatus = ["Win32_System"]`).
- `crates/kiosk-main/Cargo.toml` `[target.'cfg(windows)'.dependencies] windows` features:
  `Win32_Foundation`, `Win32_UI_WindowsAndMessaging`, `Win32_UI_Input_KeyboardAndMouse`,
  `Win32_System_LibraryLoader`, `Win32_System_Power`, `Win32_System_Registry`,
  `Win32_System_SystemInformation`, `Win32_Security`, `Win32_Security_Authorization`,
  `Win32_Storage_FileSystem`, `Win32_System_SystemServices`.
  **`Win32_System_ProcessStatus` is not among them.**

Not a *new crate* (C6 is not tripped), but a new feature flag plus an `unsafe` FFI block on the
Windows build — and, per §2.4, one that `sysinfo::Process::memory()` (already a dependency,
already instantiated at the call site) makes unnecessary.

---

## 8. Parent-spec traceability

### 8.1 §3.4 arch-09 / arch-10 — **VERIFIED (arch-09) / DRIFT (arch-10)**
§3.4 is at parent line **266** ("### 3.4 Offline video"). Line 278-283, verbatim:
> **Decode-failure robustness (arch-09):** `offline.html` wires the `<video>` element's
> `error`/`stalled`/`emptied` events and the `play()` promise rejection to fall back to the
> static splash and emit a `media.error` log; a watchdog timer also asserts playback
> progresses (`currentTime` advances within 3 s of load). This covers a corrupt/incompatible
> codec or a missing platform decoder, not just a missing file.

**arch-09 VERIFIED**, and E's characterisation of it is accurate — including that "emit a
`media.error` log" is the unfulfilled half (§3.1 above: the variant is dead code).

**arch-10 is NOT about offline video failure handling.** Parent lines 204 and 272-275:
> Android System WebView additionally requires the plugin to set
> `WebSettings.setMediaPlaybackRequiresUserGesture(false)` (its default `true` blocks kiosk
> autoplay — arch-10). This one WebSettings call is the only per-engine media configuration.

arch-10 is an **Android autoplay-gesture** requirement (P3). E's header cites "§3.4 (offline
video, arch-09/arch-10)" as its requirement basis; arch-10 discharges nothing E does and is
not a P2 item. **DRIFT (LOW).**

### 8.2 §10 soak rows — **VERIFIED text / DRIFT in E's use of them**
Parent §10 lines 870-875, verbatim:
> - **Soak/endurance (scheduled CI, not per-PR):** a **Windows-runner job** drives looped
>   navigation + a deliberately leaking page with accelerated thresholds; asserts bounded
>   RSS, that a `max_webview_mem_mb` breach fires a restart, and that nightly reload resets
>   RSS. A ≥72 h real-hardware soak is a pre-release gate (RT-05). **Offline-video soak:**
>   multi-hour loop on the pinned Debian 12 image, assert no stall/black frame across loop
>   boundaries (PF-05).

The parent specifies **two distinct soaks**: (a) a **Windows-runner** memory soak whose named
assertions are *bounded RSS*, *`max_webview_mem_mb` breach fires a restart*, and *nightly reload
resets RSS*; and (b) the **Debian 12** offline-video loop soak (PF-05).

E's scenario 18 is a single Debian-12 soak. Its Pass criteria are *"zero `media.error`, process
alive, no launcher restarts, RSS delta … under a declared bound, loop count consistent with
wall-clock."* **None of the parent's three memory-soak assertions appear** — in particular
"a `max_webview_mem_mb` breach fires a restart" (the only test that proves the cap works) and
"nightly reload resets RSS". E takes ownership of the memory-cap feature but its soak asserts
*absence* of a breach, never that a breach behaves correctly. **DRIFT (MED)** — E's Testing
section does list a host test for the latch, so the pure logic is covered; the
end-to-end assertion the parent names is not.

### 8.3 §9 P2 row — **VERIFIED verbatim** (parent line 839):
> | **P2** | Linux + robustness | WebKitGTK parity (incl. pinch-gesture intercept, keep-awake
> at compositor), .deb + systemd + cage docs + §7.2 Linux hardening, idle reset (native),
> **memory cap restart + health-sampled RSS**, cross-platform webview-hang detection (JS
> ping), config-driven `inject_css`/`inject_js` knobs (behind signed config), remote log
> level, restart_app |

"memory cap restart + health-sampled RSS" **is assigned to P2 verbatim.** E's ownership claim
is traceable. **VERIFIED.**

### 8.4 §11 PF-05 / Debian #1062012 — **VERIFIED verbatim** (parent line 893):
> | WebKitGTK H.264 loop deps + seek-loop fragility (#1062012) | name exact gst packages in
> .deb; multi-hour loop soak on target image; fallback = seamless no-seek loop or native GL
> path |

E's contingency (double-buffered no-seek loop primary, native GL only on its failure) matches
the parent's fallback ordering. **VERIFIED.**

### 8.5 E's "§10:" quotation of the memory-cap row — **DRIFT (LOW, wrong section)**
E writes: *"the parent roadmap assigns these to P2 verbatim (**§10**: \"P1 nightly reload; P2
adds memory-cap restart + health-sampled RSS; validated by scheduled soak\")"*.

That string is **parent line 900, in §11 (Risks)**, not §10:
> | Webview memory leaks over weeks | P1 nightly reload; P2 adds memory-cap restart +
> health-sampled RSS; validated by scheduled soak (§10) |

The quoted text is **verbatim-correct**; the section attribution is wrong (§11, and the "(§10)"
inside the quote is the risk row's own forward-reference).

### 8.6 GStreamer package list — **VERIFIED exact**
Parent §3.4 lines 285-288: `gstreamer1.0-plugins-base`, `gstreamer1.0-plugins-good`
(qtdemux), `gstreamer1.0-plugins-bad` (h264parse), `gstreamer1.0-libav` (avdec_h264). E's
`gstreamer1.0-plugins-{base,good,bad}` + `gstreamer1.0-libav` = the same four.
P2-G design lines 36-37 carries the identical four *"verbatim"*. **VERIFIED, three-way consistent.**

E's missing-element exercise ("remove `libav`, assert the silent-black case is caught") maps to
parent line 288-290: *"a missing element yields a silent black video, so packaging CI
smoke-tests the offline path on the pinned Debian 12 image."* **VERIFIED.**

### 8.7 Ownership hand-offs — **VERIFIED, three-way consistent**
- E "Out: ≥72 h hardware soak (RT-05 — P2-G's checklist)" ↔ **P2-G design line 96**:
  `| H5 | ≥72 h offline-video soak, RSS trend, loop count; visual black-frame check | E / RT-05 |`
- E "Out: scheduled-CI wiring of the soak (P2-F)" ↔ **P2-F design line 48**:
  *"(b) **soak** — E's protocol at 8 h+, same container, RSS series retained"*
- E "short form per-PR is NOT run" ↔ **P2-F design line 38**: *"E 18 (soak is never per-PR)"*
- E "`.deb` GStreamer dependency declaration (P2-G)" ↔ **P2-G lines 36-37**. All **VERIFIED.**

### 8.8 "A scenario 3 entry path" — **VERIFIED (entry path) / DRIFT (what it asserts)**
P2-A scenario 3, verbatim: *"config/network down → offline fallback page loads from app
origin"*. The entry path matches E's fixture. But E's Testing section says *"the per-PR video
assertion stays **A's scenario 3 render check**"* — A's scenario 3 asserts **page** load, and
P2-A design line 350 states explicitly: *"Offline-video in A is wiring-only (asset serves, page
renders headless); **playback quality is P2-E's**."* There is no per-PR *video* assertion in A
to inherit. **DRIFT (LOW).**

---

## 9. Scenario numbering — **VERIFIED, no collision**

| Range | Owner | Evidence |
|---|---|---|
| 1–7 | P2-A | design line 312 *"scenarios 1–7 under weston headless, all blocking"*; line 326 |
| 8–12 | P2-B | design line 31 *"smoke scenarios 8–12"*; line 205 |
| 13–15 | P2-C | design line 21 *"smoke scenarios 13–15"*; scenarios 14, 15 at lines 155, 157 |
| 16–17 | P2-D | design line 25 *"smoke scenarios 16–17"*; line 137 |
| **18** | **P2-E** | free |
| — | P2-G | no scenario numbers (uses H1–H5 hardware-checklist IDs) |

E's scenario 18 collides with nothing, and **P2-F independently corroborates it**
(design line 38: *"E 18 (soak is never per-PR)"*). **VERIFIED.**

---

## 10. UNVERIFIABLE in this environment (with pinning mechanisms)

| Claim | Why unverifiable | Pinning mechanism |
|---|---|---|
| `requestVideoFrameCallback` support in WebKitGTK | No WebKitGTK anywhere on this host: `pkg-config --exists webkit2gtk-4.1` false; `ldconfig -p \| grep -c webkit2gtk` = 0; host is Ubuntu 24.04, not the Debian 12 floor. Tier-4 vendored sources are the *bindings* (`webkit2gtk-2.0.2`), not the engine — a JS API's presence is not observable from them. | **E already pins it correctly**: Open decision #3, *"WebKitGTK support for the latter — check at plan time, don't assume."* Declared assumption with a named check. **Acceptable per frame §4.4.** |
| Loop-boundary stalls on WebKitGTK's seek-to-0 path (`loop` attribute) | Same — no engine, no GStreamer (`gst-inspect-1.0` absent), no compositor (`weston`/`cage` absent). | Parent §11 line 893 pre-declares it as a *risk with a named mitigation*; E's scenario 18 **is** the pinning gate, and the contingency trigger is stated mechanically (*"any stall `media.error` at a loop boundary during the soak → the contingency task activates"*). **Acceptable.** |
| Debian bug **#1062012** contents/status | External tier-5 source; no verification attempted from this environment. | Cited by the **parent spec** at line 290 and line 893 — E inherits it from tier 1, does not originate it. Not E's burden. **N/A.** |
| GStreamer four-package sufficiency for H.264 loop decode | No GStreamer in environment. | E's Component 5 (install the four in the smoke/soak env; remove `libav` once and assert the black case is caught). **Declared and gated.** |
| Whether `WEBKIT_DISABLE_COMPOSITING_MODE=1` (P2-A smoke env) alters decode/stall behavior during an 8 h soak | No engine. **Not addressed by E at all** — P2-A design line 316 permits the flag "in the smoke environment only", and E's soak runs in that environment. | **No pinning mechanism in E.** Undeclared. |

---

## 11. Claims stated as fact that are actually undeclared assumptions

Separate list, per instruction. These are asserted in the indicative in E's text with no
"assume"/"pin at plan time" hedge and no named check.

1. **"New config key `memory_max_mb`"** — asserted as fact; contradicted by
   `schema.rs:234` / `validate.rs:19,108-114`. Also carries the buried assumption that E is
   free to name the key, when parent §5.2 line 538 and parent §10 line 872 name it.
2. **"default 0 = off — Windows fleets see zero behavior change until an operator opts in"** —
   asserted as a safety property; the parent-named key defaults to **1500** (`schema.rs:343`).
   The property is *assumed*, never checked against the shipped default.
3. **"`health.rs` gains an RSS sample per existing poll tick"** — assumes `health.rs` is where
   sampling lives; its own module doc (`health.rs:4-5`) says the opposite.
4. **"`/proc/self/status` `VmRSS` on Linux; `GetProcessMemoryInfo` on Windows"** — assumes no
   existing mechanism covers it. `sysinfo` (already a dependency, already held across ticks at
   the exact call site) exposes `Process::memory()` cross-platform. Assumption never stated,
   never checked.
5. **Implicitly, that main-process RSS is the quantity required** — every prior artifact says
   **webview-process** RSS (parent §6:671, D2e:24 and :127, `health.rs:3`). E never states it is
   substituting a different measurement, nor why.
6. **"rate-capped by the standard Logger bucket"** — asserted as existing behavior;
   `ratelimit.rs:51-64` has no `MediaError` bucket and `admit()` returns `Allow` for unbucketed
   events.
7. **"spool flushed by the existing shutdown path"** — asserted as existing behavior;
   `main.rs:1234-1242` performs no flush and its own comment disclaims durability. The real
   durability path (`Spool::append` fsync) is **severity-gated**, and E never assigns
   `health.memory_cap` a severity.
8. **"registered cross-platform … same pattern as B's CSP-violation reporter"** — B's is
   `#[cfg(not(windows))]` (B design line 95). The registration property is assumed from a
   precedent that has the opposite property.
9. **"No launcher change at all"** — true for the code, but assumes a memory-cap exit is
   behaviorally inert in the FSM. It is not: `watchdog.rs:121-165` applies backoff doubling and
   crash-loop→safe-mode escalation (`WINDOW_S = 600`, `> 5` restarts → `watchdog.safe_mode`
   CRITICAL + `Action::SpawnSafe`).
10. **"a dedicated restart exit code"** — assumes the code space is allocatable now; P2-C's
    signal-death encoding is still open (C design line 122, line 182) and may consume 129-159.
    *E does declare this one* as open decision #1 — listed here only because the Component-3
    prose states it as settled.
11. **Adding `health.memory_cap` is costless** — requires a parent §6 table amendment, a
    `TAXONOMY` row, and bumping the pinned `assert_eq!(TAXONOMY.len(), 23, "spec §6 defines 23
    events")` (`event.rs:157-160`). None mentioned.
12. **A `Telemetry::media_error(...)` method exists** — implied by "through which
    `offline.html`'s existing `fallback(why)` reports". `Event::MediaError` is a **dead variant**;
    no emitter exists anywhere in the workspace.
13. **Line citations into `offline.html` are stable** — P2-A (design line 112) rewrites the mp4
    URL selection in that same file, shifting every cited line. E declares it "builds on P2-A".
14. **`WEBKIT_DISABLE_COMPOSITING_MODE=1` is soak-neutral** — the flag is permitted in the smoke
    environment E's soak runs in (P2-A design line 316); E never mentions it.

---

## Verdict tally

| Verdict | Count |
|---|---|
| **VERIFIED** | **17** |
| **FALSE** | **6** |
| **DRIFT** | **10** |
| **UNVERIFIABLE** | **5** (4 with adequate pinning, 1 without) |

**VERIFIED (17):** `fallback(why)` exists · static-splash degrade · `:44-47` gap comment
verbatim · `stalled`/`emptied` wired · `play()` rejection wired · 3 s watchdog wired and
startup-only · health.rs tick/cadence + D2e metrics pipeline · `sysinfo` is a dependency and
exposes per-process RSS · `media.error` event+WARNING exist · TEL-06 table-driven test exists
and is count-pinned · FSM restarts on non-86 · exit-code enumeration complete · single
unconditional `invoke_handler` today · `Win32_System_ProcessStatus` is a required new feature ·
parent §9 P2 row assigns memory-cap+RSS to P2 verbatim · parent §11/#1062012 + fallback ordering ·
GStreamer four-package list exact and three-way consistent · scenario 18 free and F-corroborated ·
A/F/G ownership hand-offs consistent.

**FALSE (6):** `memory_max_mb` is not new and contradicts `maintenance.max_webview_mem_mb` ·
"default 0 = off / zero Windows behavior change" (default is 1500) · "health.rs gains an RSS
sample" (wrong module) · main-process RSS substituted for the required webview-process RSS ·
"rate-capped by the standard Logger bucket" (no bucket exists) · "spool flushed by the existing
shutdown path" (no flush exists; durability is severity-gated and E assigns no severity).

**DRIFT (10):** `bundled/offline.html` path · `:52-56` · `:59-62` · `:65-67` line ranges ·
line citations pre-date P2-A's edit to the same file · arch-10 cited but discharges nothing ·
§10 vs §11 misattribution of the memory-cap quote · soak omits the parent's three named memory
assertions · "B's reporter is cross-platform" (B is `#[cfg(not(windows))]`) · "A's scenario 3
render check" is a page check, not a video check.

**UNVERIFIABLE (5):** `requestVideoFrameCallback` in WebKitGTK (pinned by E open decision #3) ·
loop-boundary stall behavior (pinned by scenario 18 + parent §11) · Debian #1062012 (inherited
from parent, not E's burden) · GStreamer sufficiency (pinned by E Component 5) ·
`WEBKIT_DISABLE_COMPOSITING_MODE=1` soak-neutrality (**not pinned by E**).
