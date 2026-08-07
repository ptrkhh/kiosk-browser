# P2-E — WRITER, Round 2

Every citation below was re-run by me in-session at HEAD `1decd59` before I took a position.
Banked items (E2 incl. the `#[cfg]`-in-`generate_handler!` rebuttal, E4 API feasibility +
Windows parent-walk, exit code `80`, the interlock arithmetic at shipped defaults, the
config-deletion restatement, E1 durability + zero taxonomy delta, scenario 18's number) are
not re-argued.

11 objections, 11 positions. Nothing is left silent.

---

## OB-1 — REVISE (HIGH, E1)

**Conceded in full.** I read both files:

- `crates/kiosk-main/build.rs` (9 lines, read whole):
  `.app_manifest(tauri_build::AppManifest::new().commands(&["verify_pin"]))`, with a comment
  saying exactly why (`verify_pin` is an app-crate command and needs the entry so
  tauri-build autogenerates `allow-verify-pin`).
- `crates/kiosk-main/capabilities/default.json`:
  `"permissions": ["core:default", "allow-verify-pin"]`.
- Tier 4 confirmed independently: `tauri-macros-2.6.3/src/command/handler.rs:35` calls
  `filter_unused_commands`, which at `:91-95` reads
  `tauri_utils::acl::read_allowed_commands()` (`tauri-utils-2.9.3/src/acl/mod.rs:412-421`,
  returning `AllowedCommands { commands, has_app_acl }` at `:404-410`) and
  `command_defs.retain(...)`s away anything not in the set, announcing it with a `println!`.
  A dropped command falls through `handler.rs`'s `_ => { return false; }`.

The Critic's Q3 point is the one that matters: E1's own `try/catch`-and-degrade-anyway design
(correct per C4) converts an ACL-stripped command into a **silent** dead telemetry path, and
that makes E8's headline criterion *vacuously* true. That is a C9 defect in my own gate.

**Replacement, concrete.** E1's change set gains two lines and one assertion:

1. `build.rs` → `.commands(&["verify_pin", "media_error"])`.
2. `capabilities/default.json` → `"permissions": ["core:default", "allow-verify-pin",
   "allow-media-error"]`.
3. **The gate that makes it non-silent:** scenario 18's *positive* precondition. Before the
   soak clock starts, the fixture loads the offline page with a deliberately absent
   `kiosk-offline.mp4` (the `error`/`no_progress` path, already reachable — the
   `kioskasset` handler at `main.rs:998-1001` reads the file and fails when absent) and
   asserts **one `media.error` appears on the spool**. Only then does the soak begin, with
   the criterion inverted to zero. A criterion that cannot fail is replaced by a criterion
   with a proven-live producer. This is ~5 lines of fixture and it kills the vacuity for
   OB-1 *and* is the same mechanism that answers OB-4's vacuity.

**Ownership, stated:** the ACL amendment is part of the same `main.rs:990` edit E1 already
owns. P2-B's `csp::report_violation` needs the identical pair of entries; E1 lands the
`build.rs` + capability edit for `media_error`, **B appends its own two entries** in the same
files. Recorded as a second B↔E integration edge alongside the `generate_handler!` list.

## OB-2 — REVISE (HIGH, E5 ⊕ E8)

**Conceded: the contradiction is mine and it is in one turn block.** Both branches the Critic
draws are correct. I take neither — I delete the clause that created the contradiction and
fix the fixture instead.

**(i) The invented clause is withdrawn.** "raises N so that dwell ≥ 300 s regardless of a fast
sample cadence" is deleted. **N = 5, fixed.** Dwell = `5 × health_sample_s`. At the shipped
default (60) that is 300 s, which is the arithmetic the Critic has already passed clean
against `healthy_run_s = 120`. There is now one N and one dwell formula.

**(ii) 18-W accelerates the *launcher* too, not just the sampler.** With dwell at 50 s the
Critic's timeline is right — 6 restarts by ≈347 s, inside `WINDOW_S = 600`. The fixture
already writes its own `kiosk.ini`; `healthy_run_s` is a `[kiosk]` bootstrap key with **no
range validation** — `bootstrap.rs:113-118` parses it through `number()` (`:74-89`), which
applies no bounds at all, default 120, and `dist-template/kiosk.ini:10` sets 120. So 18-W
sets **`healthy_run_s = 30`**. Then 30 < 50: the Armed-tick reset at `watchdog.rs:232-239`
fires on every run before the cap does, `restarts` is cleared each time, and the escalation
is unreachable at the accelerated cadence for the same structural reason it is unreachable at
shipped defaults. One line in a fixture; no product change; the interlock's *form* is
preserved (`dwell > healthy_run_s`) rather than special-cased.

**(iii) (b) and (c) are split into two runs — they genuinely cannot share a fixture.** The
Critic is right that a re-tripping cap resets the nightly-reload timer.

| Run | Config | Asserts |
|---|---|---|
| **18-W(b)** | leaking page; `max_webview_mem_mb = 256`; `health_sample_s = 10`; `healthy_run_s = 30`; `nightly_reload` unset | `webview_rss_mb` climbs and is reported; breach → **exit 80** → launcher restart with `watchdog.restart{code:80}` on the spool; **no** `watchdog.safe_mode` |
| **18-W(c)** | leaking page; `max_webview_mem_mb = 0` (**off** — range-valid, `validate.rs:108-114`, tested `:267`); `nightly_reload` a few minutes ahead | zero restarts; `webview_rss_mb` after the reload below the pre-reload peak |

Each run proves one of parent §10's assertions; "bounded RSS" is the series both emit. The
`no watchdog.safe_mode` assertion in (b) turns the interlock into something the gate actually
checks, which is what it was missing.

**(iv) The promised host test is withdrawn — the Critic is right that it cannot observe what
it pins.** `kiosk-main` never reads `healthy_run_s`; a test comparing against a hardcoded
`120` would keep passing after any launcher change. Replaced by what *is* observable: the
pure latch (`observe()` — N=5 consecutive, `cap_mb == 0` disables, fires once then resets)
gets its host test, and the `dwell > healthy_run_s` relation is pinned **by 18-W(b)'s
`no watchdog.safe_mode` assertion**, i.e. by the gate that exercises both processes together.
That is the only place the relation is observable at all.

## OB-3 — REVISE (HIGH, E8/E5) — ownership boundary stated, and the second half taken as risk

**On the wiring owner:** the Critic checked P2-F Round 1 and was correct — F had no Windows
job, and "wired by F" was a hand-off to a spec that did not contain the receiver. **P2-F's
Round 2 turn block has since adopted it as F7** (`P2F-R2-writer.md:369`, and the OB-2 section
`:93-107`), and I re-read it before answering.

**Boundary, stated explicitly so two specs do not both claim one job:**

| Artefact | Owner |
|---|---|
| The feature under test (E4 subtree RSS, E5 cap/latch/exit 80) | **E** |
| Scenario 18-W's fixture, parameters and assertions (the body) | **E** |
| The `endurance` job that runs it on `windows-latest`, its runner, artifacts, scheduling | **F** (F7) |

This matches F7 verbatim — *"F owns the job; E owns the body and the feature"* — and F states
the reciprocal hard dependency: if E4/E5/18-W are withdrawn, F7 is unrunnable and parent §10's
Windows-soak row returns to UNOWNED in the ledger rather than silently passing. I adopt that
clause as E-side spec text too, so neither spec can quietly drop it.

**I also accept F's consequential ask** (`P2F-R2-writer.md:138-142`): **E stops pinning a CI
duration.** My "scheduled CI 8 h+" is deleted; E8 now reads *"scheduled CI: multi-hour,
duration set by F within the hosted-runner cap."* E's pass criteria are duration-agnostic, F
owns the wall clock. That is a deletion from E.

**On the second half — E5's C8 migration precondition — ACCEPT-AS-DOCUMENTED-RISK, and my
R1 wording is downgraded.** 18-W runs a *deliberately leaking page at cap 256*. It proves the
**mechanism**; it does not produce a baseline for **1500 against real fleet content**. My R1
sentence — "1500 is a *measured* number for the fleet rather than an inherited one" —
overclaimed and I withdraw it. Corrected position:

- What P2 actually delivers before enforcement: (1) 18-W proves breach→restart works;
  (2) E4 ships `webview_rss_mb` in every `health.sample`, so fleets have the number;
  (3) **P2-G H5** (`p2g…:96`: *"≥72 h offline-video soak, RSS trend, loop count"*) gives a
  real-hardware, real-image RSS trend — but for *offline-video* content, not the operator's
  site.
- **Residual risk, named:** a fleet whose real site legitimately exceeds 1500 MB of summed
  webview RSS gets restarts it did not have in P1. **Carried by:** the deployment, at
  G's hardware-checklist sign-off, with the shipped levers (`0` = off, or raise within
  `[256, 8192]`) reachable by signed config. **Not** silently mitigated by an ordering claim
  that the evidence does not support.

## OB-4 — REVISE (HIGH, E3) — the predicate is replaced, not patched

**Conceded without qualification.** `|| v.currentTime === 0` makes the stuck-at-zero seek —
the *exact* PF-05 / #1062012 class — read as healthy, and the Critic's tell is right: the same
expression is the *failure* predicate at `offline.html:66-68` and I made it a success
predicate in the same file. That disjunct was noise I added; it has no defence.

**Replacement: stop sampling `currentTime` on a timer at all.** The monitor becomes a
progress *counter* driven by the engine:

```js
var ticks = 0, lastT = 0, wrapAt = 0, misses = 0;
v.addEventListener("timeupdate", function () {
  if (v.currentTime < lastT) wrapAt = Date.now();   // a loop wrap; we never seek
  lastT = v.currentTime; ticks++;
});
setInterval(function () {
  if (degraded || v.paused) { misses = 0; return; }
  if (ticks > 0) { ticks = 0; misses = 0; return; }
  if (++misses >= 2) fallback("stall", { at_loop_boundary: Date.now() - wrapAt < 12000 });
}, 5000);
```

Why this is correct where the old one was not:

- **A loop wrap produces `timeupdate` events**, so a healthy loop always advances `ticks`.
  A hung decode produces **none** — `timeupdate` fires only when `currentTime` changes.
  "Looped" and "stalled" are now distinguished by the presence of engine activity, not by
  comparing floats across a wrap.
- **Stuck at 0.0 is caught**: no `timeupdate`, `ticks` stays 0, two consecutive misses fire.
  That is the OB-4 case, now the primary detection path rather than the blind spot.
- `wrapAt` gives **E6 its mechanical trigger back**: a stall within one sample interval of the
  most recent wrap is flagged `at_loop_boundary`. OB-4's dependent defect against E6 is
  closed by the same three lines.

**Vacuity, closed twice.** With OB-1's positive precondition (a proven-live `media.error`
producer before the clock starts) and this predicate, scenario 18's "zero `media.error`"
criterion has both a producer that demonstrably works and a detector that is not deaf at the
loop boundary.

## OB-5 — REVISE (MED, E5) — by withdrawing the promise, not by adding a flush

**Conceded on the mechanism, and the Critic's tier-3 chain is right — I re-ran it and it is
worse than he states.** `Telemetry::emit` (`telemetry.rs:64-68`) is
`let _ = self.tx.try_send(...)` on a bounded `sync_channel(CHANNEL_CAPACITY = 256)`
(`:30-37`, `:239`), drained on a separate blocking OS thread; `process::exit` joins nothing.
Additionally: `main.rs:514-517` states `Logger::log` fsyncs **WARNING+** entries as it
processes them — an INFO `health.sample` is never fsynced, and on an *online* device it never
reaches the spool at all; it goes out in the ≤10 s GCL batch. So "written to the spool before
the clean exit" was wrong on two counts, not one.

**Position: the promise is withdrawn; no ordering step is added.** Q1 — no requirement
demands that the tripping RSS value be durable. Parent §10 asks for *bounded RSS* (the
series, not one sample) and *a breach fires a restart* (the fact). The fact **is** durable and
is written by a surviving process: `watchdog.restart{code:80}` at ERROR, `code` a
first-class field (`sink.rs:203-217`). The number is best-effort, exactly like every other
INFO event, which is C4's doctrine.

**What I explicitly did *not* do, and why.** The `pinpad.rs:150-156` ordering pattern the
Critic cites is a *file persist* the calling thread performs itself, not a cross-thread
handshake — it does not transfer. The two ways to actually get the number durable both cost
more than the number is worth: (a) a new `LogReq::Flush` + ack round-trip through the logger
thread (new machinery on the durability path, for an INFO field); (b) reuse of
`telemetry::spool_boot_config_error`'s direct-`Spool::open` pattern (`telemetry.rs:291+`) —
**which would be a bug here**: that path exists precisely because no live `Logger` owns the
spool at boot-gate time; at cap-exit time one does, and a second concurrent `Spool` handle
appending to the same segments is corruption, not durability. Q2 says do neither.

**Restated E5 text:** *"the restart's cause is durable in `watchdog.restart{code:80}`, written
by the launcher. The `webview_rss_mb` series is delivered on the normal telemetry path,
best-effort like every INFO event; the sample immediately preceding the exit may be lost."*
That sentence is what E5 will say. The Withdrawals-section claim it replaces
("durable, greppable, emitted by a live process") is now scoped to the *fact* only, which is
the Critic's own conclusion and I adopt it.

## OB-6 — REVISE (MED, E4) — sum kept, divergence declared in both directions

**The mechanism is conceded exactly as cited.** I re-read both backends:
`sysinfo-0.32.1/src/unix/linux/process.rs:572-576` — RSS from `/proc/<pid>/stat` ×
`page_size_b`; `src/windows/process.rs:298-299` — `self.memory = pi.WorkingSetSize`. Both are
**total** resident/working sets including shared pages, and `Process` exposes only `memory()`
and `virtual_memory()` — no PSS, no private bytes.

**What the cap compares against, stated precisely (the Moderator's question):**

> On **both** platforms the cap compares `maintenance.max_webview_mem_mb` against the
> **arithmetic sum of `Process::memory()` over every descendant of the kiosk-main pid,
> excluding kiosk-main itself** — i.e. total resident (Linux) / working (Windows) set across
> the engine's helper processes, **with shared pages counted once per helper**. It is a
> *footprint proxy*, not a unique-set size.

**Why the sum is defensible and I do not narrow it.** The failure this cap exists to stop is
the machine running out of memory over weeks (parent §11 line 900) — and the machine dies on
*total* footprint, not on any one process's private set. Shared engine text is a **roughly
constant** offset: it does not grow with a leak. So the quantity that carries the requirement
— the trend — is unaffected by the over-count; only the absolute threshold is shifted, in one
direction (fires early), by a constant.

**The C3 declaration, both directions, which E4 was missing:**

- **Stricter than the configured number, on both platforms:** an effective ceiling below
  1500 MB of real distinct memory, by the shared-text offset.
- **Stricter on Windows than on Linux:** WebView2 runs more helper processes (browser + GPU +
  utility + N renderers) than WebKitGTK (web + network + GPU), so the same configured 1500 MB
  means a *lower* real ceiling on Windows. The same config key does not mean the same thing on
  the two platforms; that is now written down instead of discovered.
- **Interpretation note on the RSS series:** scenario 18 and 18-W record the **t=0 baseline**
  of `webview_rss_mb` as the first line of the artifact. The shared-text offset is visible
  there and is subtractable when reading a trend. This is the cheap fix the Critic names, and
  it also makes the offset an observed number per platform rather than an argued one.
- **Not narrowed to max-single-process:** on a one-page kiosk that would be nearly the same
  number with strictly less coverage (a leak in a GPU or utility helper would vanish), and it
  would still be an unshared-page over-count of one process. Q2/Q3: no gain.

## OB-7 — REVISE for the FD retention · ACCEPT-AS-DOCUMENTED-RISK for the `setrlimit` (MED, E4)

**Both effects verified by me, and the Critic's "today the kiosk never triggers this" is
right** — `metrics::sample` (`metrics.rs:26-28`) calls only `refresh_cpu_usage`,
`refresh_memory`, `disks.refresh()`; `remaining_files()` is reached only from
`unix/linux/process.rs`.

**FD retention — REVISE, one line.** `sysinfo::set_open_files_limit(0)` is called once at
startup, before the first process refresh. Verified this actually works rather than merely
existing: `FileCounter::new` (`unix/linux/process.rs:931-944`) does
`fetch_update(|remaining| if remaining > 0 { Some(remaining-1) } else { None })` and
**returns `None` when the budget is zero** — so with the limit at 0 no `stat` handle is
retained and sysinfo re-opens per refresh. `set_open_files_limit`'s own doc (`lib.rs:127-140`)
says "call the function before any call to the processes update" and that it returns `false`
on non-Linux, which is the correct no-op on Windows. The Critic's point that permanent
per-process FD retention would poison the *endurance* sub-project's own baseline is the
argument that decides this — it costs one line to not have it.

**`setrlimit(RLIMIT_NOFILE, hard)` — ACCEPT-AS-DOCUMENTED-RISK, because it is not avoidable
within sysinfo.** `remaining_files()` (`unix/linux/system.rs:22-46`) is a `OnceLock` whose
initialiser runs `getrlimit` then `setrlimit(RLIMIT_NOFILE, rlim_max)`; **`set_open_files_limit`
itself calls `remaining_files()`** (`lib.rs:141-160`), so the raise happens on the same path
that suppresses the retention. Any use of sysinfo's process API on Linux carries it.

- **What it is:** the soft `RLIMIT_NOFILE` is raised to the hard limit, once, at first process
  refresh. Nothing else changes.
- **Cheap mitigation, declared as an ask on the spec that already writes the unit file:**
  `LimitNOFILE=1024` (or the site's chosen value) in P2-C's systemd unit — its stated shape
  (`p2c…:83-89`) has `Type`/`ExecStart`/`Restart`/`RestartPreventExitStatus`/`RuntimeDirectory`
  and no `LimitNOFILE`. With soft == hard the raise is a no-op. **Ask on C/G**, one line.
- **Residual if declined:** a kiosk process whose soft FD limit is the systemd hard default
  instead of 1024. Carried by the deployment; recorded on G's checklist. I do not claim E4 is
  side-effect-free any more — my R1 headline ("no new dependency, no new feature, no `unsafe`,
  no `#[cfg]`") was true at the source level and is now qualified with this line in the spec.

## OB-8 — REVISE (MED, E7) — assertion rewritten to the outcome

**Conceded; I adopt the Critic's wording because it is the parent's requirement rather than my
guess at a runtime.** Both branches are genuinely unverifiable here (no GStreamer, no
WebKitGTK anywhere on this host), and E7 stated one in the indicative. The `error` branch is
in fact the more likely one — `qtdemux` (`-good`) and `h264parse` (`-bad`) survive the `libav`
removal, so the pipeline links, demuxes, parses, and fails at the decoder, and
`offline.html:49-51` already routes an `error` event into `fallback`.

**Replacement E7 assertion, verbatim spec text:**

> With `gstreamer1.0-libav` removed: **exactly one `media.error` of any enumerated kind
> reaches the spool, and the page degrades to the black splash.** The specific `kind` is
> recorded in the run artifact, not asserted.

Robust to either branch, and it is what parent §3.4 lines 288-290 actually requires ("a
missing element yields a silent black video, so packaging CI smoke-tests the offline path").
The recorded `kind` is how we learn which branch is real, at no cost to the gate.

## OB-9 — REVISE (MED, E6) — both unstated things are now stated, and the reset is chosen

**(1) `loop` removal — conceded as an unstated prerequisite, now spec.** `offline.html:26`
carries `loop`, and per the HTML media element spec a looping element seeks to the earliest
position and **does not fire `ended`**. So E6's primary trigger could never arm, and worse —
the Critic's sharper form — while `loop` is present the engine performs precisely the
seek-to-0 that #1062012 names, so the double-buffer would change nothing. **E6 removes `loop`
from both `<video>` elements**; the loop is driven entirely by the swap. Stated as a
prerequisite in the contingency's own text, not assumed.

**(2) The background reset — the choice is made: `load()`, not `currentTime = 0`.**
Conceded that "no *visible* element ever seeks" is true and insufficient, and that the two
candidates have materially different exposure. `currentTime = 0` **is** the seek path
#1062012 names — using it would relocate the bug into the background and surface it one loop
later with two frozen elements, exactly as described. `load()` re-runs the resource selection
algorithm: a fresh fetch and decode pipeline, not a seek. Against a local `kioskasset`
custom-scheme read, and with a full clip duration of budget before the element is needed, the
re-fetch cost is affordable. Spec text:

> On swap, the outgoing element is reset with `load()` — **never** `currentTime = 0`, which
> is the seek path #1062012 names. The incoming element must reach `canplaythrough` before it
> is swapped to front; if it has not by `duration − 0.25 s` of the visible element, the page
> degrades per arch-09 rather than showing a not-ready element. **Budget:** one clip duration;
> if `load()` cannot complete within it on target hardware, the fallback is the native-GL path
> (parent §3.4's second fallback), not `currentTime = 0`.

**(3) Trigger reachability** — restored by OB-4's `at_loop_boundary` flag. E6's activation
condition is satisfiable again.

## OB-10 — REVISE (MED, E3) — both holes closed by the OB-4 replacement

**(a) Fixed-interval aliasing → false positive. Conceded, and structurally removed.** The
Critic is right that an exact float comparison against a clip whose duration divides the
sample interval fails a healthy build — a C9 defect in the same class as passing a bad one.
The replacement predicate does not compare `currentTime` at all: it counts `timeupdate`
events, which a healthy looping element emits at ~4-66 Hz regardless of clip duration. There
is no interval/duration relationship left to alias against. The **two consecutive misses**
requirement (10 s) additionally supplies the confirming sample E3 lacked, so a single busy
main-thread tick cannot black the screen.

**(b) The `readyState >= 2` guard was fail-open. Conceded, and deleted rather than
re-specified.** A stall that drops the pipeline back to HAVE_METADATA would have been
suppressed — leaving the element on screen with its last frame, which is exactly what
`offline.html:33-36` says must never happen. The replacement predicate has **no `readyState`
guard**: absence of `timeupdate` while `!paused && !degraded` is a stall at any readyState.
A deletion closes the hole; I do not argue that the four existing arch-09 listeners cover the
class, because I cannot check it here and `stalled` is a fetch-side event against what is a
local custom-scheme read.

## OB-11 — REVISE (LOW, E1)

Conceded; verified by me — `grep -rn "\.manage(" crates/ --include=*.rs` returns exactly one
hit, `main.rs:989` `.manage(pinpad_state)`. `Telemetry` is `#[derive(Clone)]`
(`telemetry.rs:46-49`) and documented as a cheap `Clone + Send` handle, so
`.manage(telem.clone())` is added at the same site E1 is already editing. No fast-track veto
needed from me.

---

## On "where a concession went too far"

Accepted, with the correction narrowed. The Critic does not contest the withdrawal of
`health.memory_cap` and neither do I; his conclusion — that E5's real missing piece was an
ordering step rather than an event, and that a WARNING `health.memory_cap` would have lost the
same race down the same channel — is correct and I verified it. Where I land differently is
the remedy: per OB-5 I withdraw the *claim* rather than add the ordering, because no
requirement demands the tripping number be durable and both available ordering mechanisms cost
more than the field is worth. The fact stays durable in the launcher's event. So the
Withdrawals-section benefit is now stated as half: **durable for the fact, best-effort for the
number** — which is what the evidence supports.

---

## Change register after Round 2

Ⓓ = dependency edges moved this round.

| ID | State after R2 | Depends on |
|---|---|---|
| **E1** | **Revised (OB-1, OB-11).** Command + `Telemetry::media_error` + enumerated `kind` **plus** `build.rs` `.commands(&[…,"media_error"])`, `capabilities/default.json` `allow-media-error`, and `.manage(telem.clone())`. Non-vacuity proven by 18's positive precondition. | Ⓓ **P2-B now shares three files** (`main.rs:990`, `build.rs`, `capabilities/default.json`), not one; P2-A |
| **E2** | Unchanged (clean pass) | E1 |
| **E3** | **Rewritten (OB-4, OB-10).** `timeupdate`-counter monitor; 2 consecutive misses; no `readyState` guard; no float comparison; emits `at_loop_boundary` | E1, P2-A |
| **E4** | **Revised (OB-6, OB-7).** Subtree sum retained; C3 divergence declared in both directions + t=0 baseline in the artifact; `set_open_files_limit(0)` at startup; `setrlimit` accepted as documented risk | Ⓓ **ask on P2-C/G for `LimitNOFILE` in the unit** |
| **E5** | **Revised (OB-2, OB-5).** N = 5 fixed, dwell = `5 × health_sample_s`; the "raises N" clause deleted; the un-observable host test withdrawn; durability claim scoped to the fact | E4; P2-C (code space); Ⓓ **18-W(b) now pins the interlock** |
| **E6** | **Revised (OB-9).** `loop` removed from both elements (stated prerequisite); reset is **`load()`**, never `currentTime = 0`; `canplaythrough`-before-swap with a one-clip budget and a named fallback | Ⓓ **E3's `at_loop_boundary`** (trigger restored) |
| **E7** | **Revised (OB-8).** Asserts *exactly one `media.error` of any enumerated kind + degrade to splash*; the `kind` is recorded, not asserted | P2-G, P2-F |
| **E8** | **Revised (OB-2, OB-3).** 18 gains a positive precondition; **18-W splits into (b) and (c)**, (b) sets `healthy_run_s = 30` and asserts **no `watchdog.safe_mode`**, (c) runs cap-off; **CI duration un-pinned** ("multi-hour, set by F") | Ⓓ **P2-F F7 owns the Windows job; E owns the body — boundary stated both ways**; E1–E6; P2-G H5 |

**New in E's scope this round:** the `build.rs`/capability ACL entries (OB-1), scenario 18's
positive precondition (OB-1/OB-4), and `set_open_files_limit(0)` (OB-7).
**New withdrawals:** the "raises N" dwell clause (OB-2); the `healthy_run_s` host test (OB-2);
the "1500 is a measured number" migration claim (OB-3); the "8 h+" CI duration (OB-3);
"the RSS number is durable" (OB-5).
**New asks on siblings:** B — two ACL entries alongside its `generate_handler!` entry;
C/G — `LimitNOFILE` in the systemd unit.
