# P2-E — CRITIC, Round 1

No frame dispute. Every objection below rests on a check I ran at HEAD `1decd59`; where I
checked a claim of the Writer's and it held, I say so and raise nothing.

## Objection index

| ID | Change | Objection (one line) | Sev | Evidence tier |
|---|---|---|---|---|
| OB-1 | E1 | A new `#[tauri::command]` is stripped/denied by the app ACL unless `build.rs` and `capabilities/default.json` are amended — E1 names neither, and the failure is silent | HIGH | 3 (`build.rs`, `capabilities/default.json`) + 4 (`tauri-macros`, `tauri-utils`) |
| OB-2 | E5 ⊕ E8 | The non-escalation interlock and scenario 18-W's acceleration are mutually exclusive; on 18-W's stated numbers the launcher escalates to safe mode | HIGH | 3 (`watchdog.rs`, launcher defaults) + Writer's own text |
| OB-3 | E8/E5 | Scenario 18-W has no wiring owner: P2-F's `endurance` workflow has no Windows job at all — so E5's own C8 migration precondition cannot execute | HIGH | 2 (P2-F design §2, line 38) |
| OB-4 | E3 | The steady-state predicate's `\|\| currentTime === 0` clause is blind to the exact PF-05 stall; scenario 18's core assertion passes vacuously | HIGH | 3 (`offline.html:65-68`) + Writer's E3 text |
| OB-5 | E5 | `std::process::exit(80)` races the logger thread; the explaining record (`webview_rss_mb`) is not durable — contradicting the stated replacement for the withdrawn event | MED | 3 (`telemetry.rs`, `pinpad.rs`) |
| OB-6 | E4 | RSS is not additive: the subtree sum double-counts the engine's shared pages, by a platform-dependent amount, against a fixed 1500 MB cap — undeclared C3 divergence | MED | 4 (`sysinfo` linux/windows process impls) |
| OB-7 | E4 | `refresh_processes` has an undeclared process-global side effect on Linux (`setrlimit(RLIMIT_NOFILE)`) and retains one open FD per process, forever | MED | 4 (`sysinfo-0.32.1`) |
| OB-8 | E7 | Removing `gstreamer1.0-libav` most likely fires the shipped `error` listener, i.e. `kind:"error"`; E7 pins the assertion to `kind:"no_progress"` | MED | 3 (`offline.html:49-51`) + 1 (parent §3.4) |
| OB-9 | E6 | With the `loop` attribute present `ended` never fires and the buggy seek is what `loop` does — the contingency requires an unstated change and only relocates the seek | MED | 3 (`offline.html:26`) + 5 (HTML media spec) |
| OB-10 | E3 | Two undeclared holes: fixed-interval aliasing on a looping clip → false-positive degrade; the `readyState >= 2` guard → fail-open on the frozen-frame case | MED | 3 (`offline.html:33-36`) |
| OB-11 | E1 | `State<Telemetry>` needs a `.manage(telem.clone())` that `main.rs:988-989` does not have | LOW | 3 (`main.rs:988-990`) |

---

## OB-1 — The Tauri ACL silently deletes the bridge (vs E1, HIGH)

**What breaks.** E1 adds `#[tauri::command] fn media_error(...)` and one entry in
`generate_handler!`. That is not sufficient to make the command callable in *this* app.

`crates/kiosk-main/build.rs` (read in full, 10 lines) declares the app command manifest
explicitly:

```rust
let attrs = tauri_build::Attributes::new()
    .app_manifest(tauri_build::AppManifest::new().commands(&["verify_pin"]));
```

and `crates/kiosk-main/capabilities/default.json` grants exactly
`"permissions": ["core:default", "allow-verify-pin"]`.

Tier 4: `tauri-macros-2.6.3/src/command/handler.rs:35` calls
`filter_unused_commands(plugin_name, &mut command_defs)` (`:90-140`), which reads
`tauri_utils::acl::read_allowed_commands()` (`tauri-utils-2.9.3/src/acl/mod.rs:413-421`)
and **`command_defs.retain(...)` drops any command not in the allowed set**, emitting only
a `println!("Removed unused commands from application: …")`. `has_app_acl` is true here —
this app ships a capability file and an app manifest, which is exactly why
`allow-verify-pin` exists at all (the build.rs comment says so).

**When.** The first build after E1 lands, if `"media_error"` is not added to
`AppManifest::commands` *and* `allow-media-error` is not added to the capability's
`permissions`. Build succeeds. Runtime `invoke("media_error")` falls through
`generate_handler!`'s `_ => { return false; }` (`handler.rs:176-179`) and rejects.

**Why it matters.** E1's own design puts the invoke inside `try/catch` and lets the degrade
path run regardless (correct per C4) — so the rejection is swallowed and **nothing anywhere
reports that media telemetry is dead**. arch-09's "emit a `media.error` log" stays
unfulfilled with no signal, which is the silent-failure class the parent names (Q3). Worse,
it makes E8's headline pass criterion — "zero `media.error` in the spool over the soak
window" — *vacuously true*: a criterion that cannot fail is not a gate (C9).

This also lands on the integration point E claims to own. P2-B's `csp::report_violation`
has the identical requirement. E declares itself the owner of converting `main.rs:990` to a
multi-entry list; the ACL amendment belongs in the same change and is currently named by
neither spec.

**Falsifiable by:** showing `has_app_acl == false` for this build, or showing an
already-present `media_error` permission. Neither exists at HEAD.

## OB-2 — The interlock and 18-W's acceleration cannot both be true (vs E5 ⊕ E8, HIGH)

**What breaks.** E5 states the latch "reads the effective `health_sample_s` and raises N so
that dwell ≥ 300 s **regardless of a fast sample cadence**." E8 scenario 18-W states
`health_sample_s` is set to **10** to accelerate and therefore "**dwell shrinks to 50 s**."
Those are contradictory statements about the same latch in the same turn block. Take either
branch:

*Branch A — the 300 s floor holds.* 18-W's acceleration knob is disarmed: dwell is 300 s no
matter what `health_sample_s` is, so `health_sample_s = 10` buys nothing and E8's stated
50 s is simply wrong. Assertion (c) (nightly reload lowers `webview_rss_mb` below the
pre-reload peak) also becomes unreachable in the same fixture, because with `max_webview_mem_mb`
at the range floor 256 and a deliberately leaking page, the cap re-trips every ~300 s and
each restart resets the nightly-reload timer. (b) and (c) cannot share one fixture.

*Branch B — dwell really is 50 s.* Then the interlock is broken. Verified against the real
FSM: `restarts.clear()` happens only on the Armed tick at
`watchdog.rs:232-239`, gated on `now - spawned_at >= healthy_run_s`, and `spawned_at` is
reset on every respawn (`:241-245`). With dwell 50 s and `healthy_run_s` 120 s
(`kiosk-launcher/src/main.rs:120`, `dist-template/kiosk.ini`, `config/bootstrap.rs:113-118`)
the healthy-run reset **never fires**: every cap exit pushes into `self.restarts`
(`watchdog.rs:147-149`), backoff doubles 1→2→4→8→16→32 (`:161`), and the 6th restart inside
`WINDOW_S = 600` (`:80`) sets `self.safe = true` and emits `WatchdogEvent::SafeMode`
(`:150-156`) → `watchdog.safe_mode` **CRITICAL** (`event.rs:146-150`) → `Action::SpawnSafe`
(`:236-240`). Rough timeline at dwell 50 s: 5 restarts by ≈281 s, the 6th at ≈347 s — well
inside the window.

**When.** Every run of scenario 18-W as specified.

**Why it matters.** 18-W is, in the Writer's own words, "the only test that proves the cap
works end-to-end". As written it either cannot accelerate or it proves the *escalation* the
E5 interlock declares unreachable, and reports the device as CRITICAL-degraded. C9: a gate
that cannot run as specified is a feasibility defect. This is not a strike against the
interlock arithmetic at shipped defaults — see Clean passes — it is a strike against E's own
gate contradicting it.

Secondary, same objection: E5 says "N = 5" *and* "raises N so that dwell ≥ 300 s"; those
are two different N's. And the host test E5 promises —
"asserts `N × health_sample_s_min (10) ≥ healthy_run_s` is *checked*, not assumed" — cannot
observe `healthy_run_s`: it is the launcher's bootstrap value, `kiosk-main` never reads it
(E5 concedes this), so the test would compare against a **hardcoded copy of 120** and would
keep passing after any launcher-side change. The invariant's pin does not observe the thing
it pins.

## OB-3 — Scenario 18-W has no owner, and E5's C8 mitigation depends on it (vs E8/E5, HIGH)

**What breaks.** E8 assigns 18-W to E for design and says it is "**wired by F**". P2-F's
design (tier 2, read in full through §4) has no Windows scheduled job. Its `endurance`
workflow is exactly two jobs (F design §2): "(a) **full matrix** … in a `debian:12`
container; (b) **soak** — E's protocol at 8 h+, same container". Its per-PR exclusion list
(line 38) names "**E 18**" — not 18-W. F's only Windows surfaces are `build-windows` (§ current
state) and the MSI in `release` (§3). There is no Windows runner anywhere in F's scheduled
endurance design.

**When.** At integration: E hands a Windows soak to a spec that does not contain one.

**Why it matters, twice.**
1. Parent §10's Windows-runner memory soak is a P2-row obligation with three named
   assertions. Under frame §2 an item no spec owns is a HIGH integration defect. E has
   named it, which is progress on the draft, but naming is not owning: the gate cannot run.
2. It collapses E5's entire C8 answer. E5's migration design is "ordering is the mitigation,
   not a flag — E5's enforcement merges only after the Windows soak … has produced an RSS
   series, so 1500 is a *measured* number for the fleet rather than an inherited one." If
   18-W has no runner, that precondition never clears and the fleet-wide 1500 MB cap ships
   against an unmeasured baseline anyway. C8 puts that burden on the Writer.

Note for the record: the hypothesis that this duplicates an F-owned Windows leak soak is
**not** supported — I looked for it; F has no such job. The defect is absence, not
duplication.

## OB-4 — E3's predicate is blind to the failure it exists to catch (vs E3, HIGH)

**What breaks.** E3's steady-state check, verbatim: "every 5 s, if not `degraded` and not
`v.paused`, assert `v.currentTime !== last || v.currentTime === 0` progressed".

The `|| v.currentTime === 0` disjunct makes **any** sample at exactly 0 count as progress.
The PF-05 / Debian #1062012 failure class the parent names is the seek-to-0 loop path: the
element wraps, `currentTime` becomes 0, and decode does not resume. Two consecutive samples
both reading 0.0 are precisely a stuck seek-to-0 — and E3 classifies them as healthy. A
stall parked at the loop origin is structurally undetectable by this monitor.

The same expression is the *failure* predicate in the shipped startup watchdog —
`if (!degraded && v.currentTime === 0) fallback("no playback progress in 3s")`
(`offline.html:66-68`) — and E3 makes it a *success* predicate in the same file. That
inversion is the tell.

**When.** Every loop-boundary stall, i.e. the primary risk this sub-project exists to gate.

**Why it matters.** Scenario 18's core pass criterion is "zero `media.error` in the spool" —
so with OB-1 and OB-4 the gate is vacuous by two independent mechanisms. And E6's
"mechanical, not judgment" trigger rule — "any `media.error` with `kind:"stall"` **at a loop
boundary**" — can never be satisfied, because the monitor that would produce the trigger
evidence is deaf at the loop boundary. The contingency's activation condition is
unreachable by construction.

## OB-5 — The exit races the record that explains it (vs E5, MED)

**What breaks.** E5 step 3 is `std::process::exit(80)` on the latch, and E5 asserts: "The
RSS number that tripped it is in the preceding `health.sample` (`webview_rss_mb`, INFO) —
written to the spool before the clean exit, no fsync required because the process exits
deliberately."

Verified, tier 3: `Telemetry::health` (`telemetry.rs:153`) → `emit` (`:66-68`) →
`self.tx.try_send(LogReq{..})` on a **bounded** `std::sync::mpsc::sync_channel(CHANNEL_CAPACITY = 256)`
(`:30`, `:33-37`, `:239`), drained by the logger on a **separate blocking OS thread**
(`:325-345`, "this is a plain blocking loop over a `std::sync::mpsc` channel"). `emit` never
touches the spool. `std::process::exit` runs no destructors and joins no thread.

So the health task's sequence "emit `health.sample`; latch fires; `exit(80)`" gives **no
ordering guarantee at all** that the entry reached `Spool::append`. INFO is additionally not
fsynced (`event.rs:21-26` / `spool.rs:1-7`), and `try_send` drops silently on a full channel.

**When.** Every cap trip; racy, so intermittently.

**Why it matters.** The record that explains the restart is exactly what E5 promises
survives. The repo already knows this shape and orders it correctly:
`pinpad.rs:150-156` — "Exit AFTER the persist above: the success-reset lockout should be
durable on disk before the process that reset it goes away." E5 has no such ordering step.

**This is also where a concession went too far** — see the section below.

## OB-6 — The subtree sum is not a resident set, and the over-count differs by platform (vs E4, MED)

**What breaks.** E4 sums `Process::memory()` over the descendant subtree and compares the
sum to `max_webview_mem_mb`. Tier 4, both backends read:

- Linux: `sysinfo-0.32.1/src/unix/linux/process.rs:574-576` — RSS field of
  `/proc/<pid>/stat` × `page_size_b`.
- Windows: `src/windows/process.rs:298-300` — `pi.WorkingSetSize` from
  `SYSTEM_PROCESS_INFORMATION`.

Both are **total** resident/working sets, including shared pages. WebKitGTK's
`WebKitWebProcess` / `WebKitNetworkProcess` / GPU process each resident-map the same
`libwebkit2gtk-4.1.so` / `libjavascriptcoregtk` / GStreamer plugin text; WebView2's browser
+ N renderers + GPU + utility processes each map the same `msedge.dll` family. Summing
counts that shared text once per helper. sysinfo 0.32 exposes no PSS/private-bytes
alternative (`Process` has `memory()` and `virtual_memory()` only).

**When.** Every sample, systematically, in one direction (over-report).

**Why it matters.** The comparison target is not E's to move: parent §5.2 pins 1500 and E5
correctly refuses to change it. So the inflation lands entirely on the enforcement threshold
— the cap fires earlier than "the webview used 1500 MB". And because the helper-process
count differs materially between WebKitGTK (≈3) and WebView2 (≈5-8), the *same* configured
number means a different real memory ceiling on each platform. That is a divergence C3
requires stated in both directions, and E4 states only the reparenting risk. Minimum fix is
a declaration plus an interpretation note on the RSS series; that is cheap and E4 does not
have it.

## OB-7 — `refresh_processes` mutates the process's own resource limits (vs E4, MED)

**What breaks.** Tier 4, `sysinfo-0.32.1`:

- `src/unix/linux/system.rs:22-46` — `remaining_files()` is a `OnceLock` whose initialiser
  calls `libc::getrlimit(RLIMIT_NOFILE)` and then **`libc::setrlimit(RLIMIT_NOFILE, hard)`**,
  raising the process's soft FD limit to the hard limit, and budgets half of it to sysinfo.
- `src/unix/linux/process.rs:120` — `stat_file: Option<FileCounter>` is **retained per
  tracked process**; `:364-367`, `:502-518`, `:929-945` keep the `/proc/<pid>/stat` handle
  open across refreshes (`System::refresh_processes`'s own doc, `common/system.rs:278-280`:
  "⚠️ On Linux, `sysinfo` keeps the `stat` files open by default").
- `remaining_files` is referenced **only** from `process.rs` (`:24`, `:934`, `:962`) — so
  today the kiosk never triggers any of this: `metrics::sample` calls only
  `refresh_cpu_usage` / `refresh_memory` / `disks.refresh()`.

**When.** The first health tick after E4 lands, permanently thereafter.

**Why it matters.** E4's headline is least-mechanism: "No new dependency, no new `windows`
feature, no `unsafe`, no `#[cfg]`." True at the source level, and the side effect is real
anyway: a hardened kiosk process silently gets its `RLIMIT_NOFILE` raised by a metrics
library and then holds one FD per system process for the life of the run. In the *endurance*
sub-project, a permanent per-process FD retention is exactly the class of resource growth
scenario 18 is meant to detect, and it will now be present in the baseline. A named, cheap
knob exists — `sysinfo::set_open_files_limit` (`src/lib.rs:140`), documented as "call the
function before any call to the processes update" — and E4 mentions neither the effect nor
the knob.

## OB-8 — The deliberate-miss run asserts the wrong `kind` (vs E7, MED)

**What breaks.** E7: removing `gstreamer1.0-libav` and asserting "**one `media.error`
`{kind:"no_progress"}` reaches the spool**."

Removing `libav` removes `avdec_h264` while `qtdemux` (`-good`) and `h264parse` (`-bad`)
remain installed, so the container demuxes and parses and then fails to link a decoder —
a bus error, not silence. WebKitGTK maps a pipeline error to an `error` event on the
element, and `offline.html:49-51` already handles it: `fallback("video error event")`, which
under E1's enumeration is `kind:"error"`. E7's assertion would then never see
`no_progress`, and the run fails for the wrong reason — or, if written loosely, passes while
asserting a mechanism that did not fire.

The opposite branch is also live: parent §3.4 line 288-290 (tier 1) says a missing element
yields "a silent black video", i.e. no error event, which is what `no_progress` assumes.
**Both branches are unverifiable in this environment** (no GStreamer, no WebKitGTK — the
verification report §0 establishes this), and E7 states its branch in the indicative with no
declared assumption and no plan-time check.

**Why it matters.** E7 is the only per-PR contribution E makes and it is the assertion the
Writer calls "the whole reason arch-09 exists". Pinning it to one `kind` bets the gate on an
unverified runtime behaviour. The correct spec text is the outcome, not the label: *exactly
one `media.error` of any enumerated kind, plus degrade to splash, observed on the spool* —
which is what the parent's requirement actually says and which is robust to either branch.

## OB-9 — The contingency relocates the seek rather than removing it (vs E6, MED)

**What breaks.** Two things E6 does not state.

1. The shipped element carries the `loop` attribute (`offline.html:25-31`, `loop` at
   `:26`). Per the HTML media element spec, when the end of the resource is reached and
   `loop` is set, the element seeks to the earliest position and **does not fire `ended`**.
   E6's primary trigger is `ended`. So E6 requires removing `loop` from both elements — an
   unstated prerequisite. More sharply: *while `loop` is present, the engine performs
   exactly the seek-to-0 that #1062012 names*, so the double-buffer changes nothing.
2. "the other resets in background" is that seek. E6's property is carefully worded as "no
   **visible** element ever seeks" — true, and insufficient. If the background reset is the
   thing that hangs, the next `ended` swaps to a not-ready element and the stall reappears
   one loop later, now with two frozen elements. The two candidate resets —
   `currentTime = 0` (a seek) vs `load()` (a full resource reset, which does not use the
   seek path) — have materially different exposure to the named bug, and E6 picks neither.

**Why it matters.** E6 is the parent-approved mitigation for the top-ranked risk in §11. Its
stated defining property does not hold as designed, and the choice that determines whether
it addresses #1062012 at all is left unmade and unnamed as an open decision.

Dependent defect: E6's trigger rule is unreachable while OB-4 stands.

## OB-10 — E3's other two holes: a false-positive gate and a fail-open guard (vs E3, MED)

**(a) Fixed-interval aliasing on a looping clip → false positive.** A 5 s sampler against a
looping video whose duration divides ≈5 s reads a near-identical `currentTime` on
consecutive samples. `currentTime !== last` is an exact float comparison; the aliasing case
is not exotic for a hand-authored loop clip (5.000 s, 2.500 s), and even without exact
equality the predicate has no tolerance parameter and no second confirming sample. Result:
`fallback("stall")` on a perfectly healthy video → the element is hidden → **the kiosk shows
the black splash instead of the video**, and scenario 18 fails. A gate that fails a good
build is a C9 defect in the same class as one that passes a bad one. The fix is trivial
(count `seeked`/wrap events, or track cumulative played time, or use an interval coprime
with the clip) and E3 specifies none.

**(b) The `readyState >= 2` guard is fail-open.** E3 only reports when
`readyState >= 2` (HAVE_CURRENT_DATA). A stall in which the pipeline drops back to
HAVE_METADATA is suppressed — no report, no degrade, and the element stays on screen
showing its last frame. That is precisely the outcome `offline.html:33-36` says must never
happen ("never a hung/frozen frame"). E3 offers no argument that the four existing arch-09
listeners cover that class (`stalled` is a fetch-side event and the resource here is a local
custom-scheme read). Undeclared assumption, silent in the field (Q3).

## OB-11 — `State<Telemetry>` has no `.manage()` (vs E1, LOW)

E1's signature is `fn media_error(kind: String, at: f64, telem: State<Telemetry>)`.
`main.rs:988-989` is `tauri::Builder::default().manage(pinpad_state)` — the sole `.manage`
call in the crate (verified by grep). E1 needs `.manage(telem.clone())` added at the same
site it is already converting. Not named. Polish; I do not veto a fast-track.

---

## Clean passes

**E2 — clean pass, and one of the Writer's rebuttals is correct.**

- The tier-4 rebuttal on `#[cfg]` inside `generate_handler!` **holds**. I read
  `tauri-macros-2.6.3/src/command/handler.rs` end to end: `CommandDef::parse` calls
  `input.call(Attribute::parse_outer)?` before the path (`:16-22`), the attrs are carried
  through `From<Handler> for TokenStream` (`:155-160`), and emitted onto the generated match
  arm at `:176` — `#(#(#attrs)* #command_name_macros => #wrappers!(#paths, #invoke),)*`.
  A `#[cfg(not(windows))]` entry is stripped with its arm, including the
  `__tauri_command_name_*` macro call inside it, before expansion. The verification report's
  mechanical note is wrong for the pinned version and the Writer is right. No cfg-split of
  the builder chain is needed. (The `filter_unused_commands` call in the same file is a
  *separate* mechanism and is OB-1; it does not affect this point.)
- The `(6, 6)` cap does **not** hide scenario 18's signal. `RateLimiter::admit`
  (`ratelimit.rs:129-134`) can only convert an event to `Suppress`; it cannot manufacture
  one. E8's criterion is "**zero** `media.error`", so any real stall storm still produces a
  first, admitted event, and `take_summaries` (`:138-150`) additionally surfaces the
  suppressed count. The hypothesis that a stall storm during a long soak would be hidden by
  the cap is not supported — I checked it and it does not hold.
- The `FocusLost` precedent for extending `caps()` in-code with a written rationale is real
  (`ratelimit.rs:55-61`) and parent §6's "defaults:" wording is open. No parent amendment.

**E4's API feasibility — clean, and one adverse hypothesis checked and dropped.**
`sysinfo = "0.32.1"` / `"0.32"` resolve to 0.32.1 with **default features**, and
`default = ["component","disk","network","system","user","multithread"]` (crate
`Cargo.toml:124-131`) — the `system` feature carries process enumeration on both platforms,
so no feature change is needed anywhere. `System::refresh_processes` (`common/system.rs:289-303`),
`processes()` (`:386-388`), `Process::memory()` (`:1314-1316`), `Process::parent()`
(`:1356-1358`), `get_current_pid()` (`:2295`) all exist and are cross-platform, exactly as
cited. On Windows the parent pointer is **not** handle-dependent: it comes from
`SYSTEM_PROCESS_INFORMATION.InheritedFromUniqueProcessId` for every process in one
`NtQuerySystemInformation` sweep (`src/windows/system.rs:306-312`, refreshed on every pass at
`:322-323`), so a parent-pointer walk from the app process reaches WebView2's browser process
and its renderers. **The hypothesis that the Windows subtree walk finds nothing and the cap
silently never fires is not supported and I am not raising it.** My E4 objections are
OB-6 (what the number means) and OB-7 (the side effect), not feasibility.

**E5 exit code `80` — clean.** Complete enumeration of `process::exit` in shipped crates:
`kiosk-launcher/src/main.rs:144` (0), `:253` (pass-through), `kiosk-main/src/cli.rs:31` (0),
`kiosk-main/src/pinpad.rs:156` (86); plus `watchdog.rs:196-197` (86 → `ExitLauncher`). `80`
collides with nothing, sits below 86, outside `128+signal`, and survives either encoding
P2-C picks. No std/shell reservation applies to an application exit code in this range that
is not also produced by a shell (`80` is not in the `126/127/128+n` reserved band).

**E5's interlock arithmetic at shipped defaults — clean.** Dwell `5 × 60 = 300 s` vs
`healthy_run_s = 120` (`kiosk-launcher/src/main.rs:120`, `config/bootstrap.rs:113-118`,
`dist-template/kiosk.ini`) and the Armed-tick reset at `watchdog.rs:232-239` (which does
reset `spawned_at` on respawn, `:241-245`) — safe mode is genuinely unreachable from cap
exits on a default fleet. The Writer's position is correct here; OB-2 is about E's own
18-W contradicting it, not about this arithmetic. His declared residual risk
(`healthy_run_s` is unbounded — `bootstrap.rs:75-91`'s `number()` applies no range at all,
and the repo's own test fixture at `main.rs:296` uses `healthy_run_s = 300`) is correctly
declared as a technician-set bootstrap value.

**E1 durability and taxonomy delta — clean.** `Event::MediaError` is in the
`Severity::Warning` arm (`event.rs:100-105`), `is_high()` covers WARNING (`:21-26`), and
`Spool::append` fsyncs high-severity entries (`spool.rs:1-7`) — no work needed. `TAXONOMY`
already carries the row and the pinned `assert_eq!(TAXONOMY.len(), 23)` (`:157-160`) does
not move. Net taxonomy delta zero, as claimed.

**E5's config work — clean as restated.** `maintenance.max_webview_mem_mb` exists
(`schema.rs:233-234`, default `d_max_mem() = 1500` at `:38-40`), is range-validated
(`validate.rs:107-114`), and sits in `UNIMPLEMENTED` (`:15-21`) with the warn emitter at
`:175-190` firing only when the value differs from the default. The remaining work really is
a deletion. The `#[serde(flatten)] unknown` shadowing hazard the Writer describes
(`schema.rs:235-236`) is real and his withdrawal of `memory_max_mb` is correct.

**Scenario 18 — clean.** No collision with A(1–7)/B(8–12)/C(13–15)/D(16–17), and F line 38
independently names "E 18". The "18-W" label itself is a new ID format, which I would
normally flag; it is folded into OB-3 instead, since the numbering is the least of that
change's problems.

---

## Where a concession went too far

**The withdrawal of `health.memory_cap` was justified on cost, but its stated replacement is
unsound — and the Writer traded a durable explaining record for a racy one.**

The *reasoning* for withdrawal is correct and I do not contest it: a new event costs a parent
§6 amendment, a `TAXONOMY` row and a `23 → 24` bump of a deliberately pinned test
(`event.rs:118-121`, `:157-160`), and `watchdog.restart{code, backoff_s, cause}` is genuinely
emitted at ERROR by a surviving process with `code` as a first-class field (verified:
`kiosk-launcher/src/sink.rs:201-217`, `event.rs:138`). Q2 is satisfied.

But the Writer then rests the *quantitative* half of the record — which RSS tripped the cap
— on the preceding INFO `health.sample`, asserting it is "written to the spool before the
clean exit". Per OB-5 that is false: `emit` is a `try_send` to another thread and
`process::exit` joins nothing. So after the withdrawal, the fleet gets `code: 80` and **no
number**, intermittently. `watchdog.restart` cannot carry the RSS — it is the launcher's
event and the launcher never saw the sample.

Note this does not rehabilitate the withdrawn event: a WARNING `health.memory_cap` would
have gone down the same channel and lost the same race. The correct conclusion is that
**E5's real missing piece is an exit ordering step, not an event** — the `pinpad.rs:150-156`
pattern ("Exit AFTER the persist above"), i.e. a drain/flush handshake with the logger thread
before `exit(80)`. E5 withdrew the event and did not add the ordering, so the Withdrawals
section's stated benefit ("durable, greppable, emitted by a live process") is only half
delivered: durable for the *fact*, lost for the *number*.

I am also recording, for the ledger, that no other concession looks over-broad to me. The
`memory_max_mb`, `health.rs`-module, `/proc/self/status`+`GetProcessMemoryInfo`,
"rate-capped by the standard bucket" and "spool flushed by the shutdown path" withdrawals
are all correct against tier-3/4 evidence I re-checked independently.

---

## Counts

| Severity | Count | IDs |
|---|---|---|
| HIGH | 4 | OB-1, OB-2, OB-3, OB-4 |
| MED | 6 | OB-5, OB-6, OB-7, OB-8, OB-9, OB-10 |
| LOW | 1 | OB-11 |

Clean passes: **E2** (with the Writer's `#[cfg]`-in-`generate_handler!` rebuttal affirmed as
correct), E4's API feasibility and cross-platform parent-walk, E5's exit code `80`, E5's
interlock arithmetic at shipped defaults, E5's config-deletion restatement, E1's durability
and zero taxonomy delta, and scenario 18's number.
