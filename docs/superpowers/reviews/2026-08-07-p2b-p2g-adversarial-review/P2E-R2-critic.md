# P2-E — CRITIC, Round 2

Every disposition below was re-verified by me at HEAD `1decd59`. Banked items are not
re-argued. Nine acceptances, two counters, three new objections (one of which is the
Moderator's own question and is the only thing I would block a merge on).

## Disposition of Round-1 objections

| OB | Writer's move | My response | Status |
|---|---|---|---|
| OB-1 | Two ACL entries + scenario 18 positive precondition | Both verified; the precondition has a real shipped producer (`main.rs:1006-1010` returns 404 → `error` event). Vacuity is killed. | **ACCEPTED** |
| OB-2 | "raises N" clause deleted; 18-W(b) sets `healthy_run_s = 30`; host test withdrawn; (b)/(c) split | Deletion and split accepted. But (iv) is wrong: 18-W(b) at 50/30 pins the FSM's generic behaviour, not the shipped 300/120 relation — which is now pinned by **nothing**. An observable pin does exist, in `kiosk-launcher`. | **COUNTERED** |
| OB-3 | Boundary table; F adopts F7; C8 baseline downgraded to documented risk | Boundary *form* is right and stated both ways. But F7's spec text and E's 18-W now **describe different jobs** — F pins E's Round-1 parameters, the ones E just conceded cannot share a fixture. | **COUNTERED** |
| OB-4 | `currentTime` predicate replaced by a `timeupdate` counter | Verified correct on all three axes the Moderator names. This is a genuinely better design than a patch would have been. | **ACCEPTED** |
| OB-5 | Promise withdrawn, no flush added | Q1 holds — no requirement names the number durable — and both rejected remedies are correctly rejected; the second is a real corruption hazard. | **ACCEPTED** |
| OB-6 | Sum kept; C3 divergence declared both directions + t=0 baseline | He did exactly the minimum I named in R1, so the objection is discharged. One correction to the *justification*, which should not enter the ledger as established. | **ACCEPTED (with correction)** |
| OB-7 | `set_open_files_limit(0)`; `setrlimit` as documented risk | Verified end to end in the pinned crate — the claim holds precisely, including the "before the first refresh" precondition it depends on. Residual is owned and near-zero. | **ACCEPTED** |
| OB-8 | Assertion rewritten to the outcome, `kind` recorded not asserted | Adopts the parent's requirement instead of a guess. Nothing left. | **ACCEPTED** |
| OB-9 | `loop` removed (stated); reset is `load()`; `canplaythrough`-before-swap + named fallback | Both unstated things stated, the right branch chosen, and the not-ready-swap hole closed by the third clause I did not ask for. | **ACCEPTED** |
| OB-10 | Both holes closed structurally by the OB-4 replacement | Aliasing is removed by construction (no float comparison survives); the `readyState` guard is deleted rather than re-specified. | **ACCEPTED** |
| OB-11 | `.manage(telem.clone())` added | Verified: `main.rs:989` is still the only `.manage`. | **ACCEPTED** |

---

## OB-2 — COUNTERED: the shipped-default interlock is now pinned by nothing

**Accepted:** deleting the "raises N" clause is the right fix — there is now one N and one
dwell formula, and the contradiction is gone. `healthy_run_s = 30` in the fixture is
arithmetically sound and mechanically available: `bootstrap.rs:113-118` parses it through
`number()` (`:75-91`), which applies **no bounds of any kind**, and
`dist-template/kiosk.ini:10` ships 120. At 50 > 30 the Armed-tick reset
(`watchdog.rs:232-239`) fires before each cap trip and the escalation is unreachable. Good.

**What I counter is (iv).** E5's R1 host test was withdrawn — correctly; I made that
argument and it stands. But the replacement pin does not cover what was withdrawn:

> "the `dwell > healthy_run_s` relation is pinned **by 18-W(b)'s `no watchdog.safe_mode`
> assertion** … That is the only place the relation is observable at all."

18-W(b) runs at dwell 50 s / `healthy_run_s` 30 s. Its `no watchdog.safe_mode` assertion
therefore pins **`50 > 30`**, which is a property of `watchdog.rs`'s FSM and is already
covered by that module's own tests. It says nothing about the shipped relation
`N(5) × health_sample_s(60) = 300 > healthy_run_s(120)` — neither number appears anywhere in
the fixture. After R2 the shipped-default invariant, which E5 calls load-bearing, has **zero
pins**: no host test, and a gate that re-parameterises both sides of the relation before
exercising it.

**"The only place it is observable" is false, and the counterexample is cheap.** The
Writer's reasoning ("kiosk-main never reads `healthy_run_s`") is correct for a test in
`kiosk-main` and does not generalise. `crates/kiosk-launcher/Cargo.toml:13-15` declares
`kiosk-core.workspace = true`, and the launcher owns both real values:

- `watchdog_config(None).healthy_run_s` — the launcher's own function, `main.rs:110-124`,
  already asserted at `:287-290`.
- `RemoteConfig::default().logging.health_sample_s` — `kiosk-core`, default 60
  (`schema.rs:44-46`, pinned at `:345`), publicly reachable.

So a single test **in `kiosk-launcher`** asserting
`MEM_CAP_N * RemoteConfig::default().logging.health_sample_s > watchdog_config(None).healthy_run_s`
observes both real values with no hardcoded copy, and fails if either default moves. That
requires only that E export `MEM_CAP_N` (or a `dwell_s()`) publicly from `kiosk-core` — which
E5 needs anyway for the latch. One test, one `pub`.

**Severity MED.** Falsifiable by showing `kiosk-launcher` cannot reach one of the two values.
It can reach both.

## OB-3 — COUNTERED: the two specs now describe different jobs

**Accepted, and it is a real improvement:** the ownership table is stated in both directions,
F7 exists in F's Round 2, and F's "unrunnable-if-E-withdraws → returns to UNOWNED rather than
silently passing" clause is exactly the Q3 property the hand-off was missing. The
duration-unpinning is a clean deletion from E. I also accept the C8 downgrade as honest — see
NEW-1 for what it leaves behind.

**What fails is the Moderator's test: the two statements disagree.** F's revised F7
(`P2F-R2-writer.md:93-101`), verbatim:

> `endurance` job (c) runs **E's scenario 18-W** on `windows-latest`. Parameters and
> assertions are E's (`max_webview_mem_mb: 256`, `health_sample_s: 10`, `nightly_reload`
> set a few minutes ahead; asserts rising `webview_rss_mb`, exit 80 +
> `watchdog.restart{code:80}`, post-reload RSS below the pre-reload peak).

That is **one** job carrying E's **Round-1** parameters — cap 256 *and* `nightly_reload` set
*and* no `healthy_run_s` override. It is precisely the fixture E conceded four sections
earlier cannot exist: the cap re-trips every 50 s, the nightly-reload timer is reset by every
restart so "post-reload RSS" is unreachable, and with `healthy_run_s` at its default 120 the
run drives the launcher to `watchdog.safe_mode` (my R1 timeline, which the Writer accepted:
6 restarts by ≈347 s inside `WINDOW_S = 600`). F's spec text encodes the defect E just fixed.

Three concrete divergences, all checkable against the two turn blocks:

| Item | E R2 | F7 R2 |
|---|---|---|
| Number of runs | two — 18-W(b), 18-W(c) | one |
| `healthy_run_s` | **30** in (b) — load-bearing, without it the run escalates | not mentioned |
| `nightly_reload` | **unset** in (b); set only in (c), where `max_webview_mem_mb = 0` | set, alongside cap 256 |
| `no watchdog.safe_mode` | asserted in (b); E5 makes it the interlock's only pin | absent |

Label collision as a bonus: F calls its own third `endurance` job "(c)" while E now has a
scenario "18-W(c)" that is a *different* thing.

**Why it matters.** "F owns the job, E owns the body" is the right boundary and it does not
help if the body written in F's spec is a body E has retracted. An implementer reading F
builds the escalating single-run fixture. Both specs must be re-synced to the (b)/(c) split
in the same integration pass; the boundary clause should say the parameters live in E and are
*referenced*, not restated, by F — restating them is what produced the drift.

**Severity MED** (integration/consistency; the underlying correctness defect is already
identified and agreed, this is only about which document carries the fixed version).

---

## New objections

### NEW-1 — The enforcement level is unbounded on Windows, and both bounding mechanisms were withdrawn in the same turn (HIGH, E4 ⊕ E5)

This is the Moderator's question and it is the one thing in P2-E I would not merge as it
stands.

**The facts, checked.**

1. **The parent supplies a number with no quantity.** `grep -n "1500" ` over the parent
   returns exactly one line — §5.2 line 538, `"max_webview_mem_mb": 1500 // 0 = off; {0} ∪
   [256, 8192] (P2)`. There is no derivation, no stated measurand, and **no RSS measurement
   anywhere in the repo or in any spec** (grep over `docs/superpowers/specs/*.md` finds no
   observed number).
2. **E4 now binds that number to a quantity that is strictly larger than real memory.** Both
   backends re-read: `sysinfo-0.32.1/src/unix/linux/process.rs:574-576` (RSS × page size) and
   `src/windows/process.rs:298-299` (`pi.WorkingSetSize`). Summing over helpers counts shared
   engine text once per helper. E4's own C3 note says this is **worst on Windows**, because
   WebView2 runs more helper processes than WebKitGTK.
3. **Nothing in P2 measures the resulting healthy operating level on Windows.** 18-W(b) runs
   a *deliberately leaking* page at cap **256** — it proves the mechanism, not the level.
   18-W(c) runs with the cap **off**. Scenario 18 is Debian + the offline video. P2-G H5 is
   Linux hardware, offline-video content (`p2g…:96`). Real fleet content on Windows appears
   in no gate.
4. **Both mitigations are gone.** The R1 ordering claim ("1500 is a *measured* number for the
   fleet") is withdrawn as unsupported — correctly. What replaced it is "carried by the
   deployment, at G's hardware-checklist sign-off" — and G's checklist is Linux hardware.
   Meanwhile the enforcement still ships enabled at 1500 by default on every Windows fleet at
   upgrade, because `d_max_mem() = 1500` (`schema.rs:38-40`, pinned `:345`) and E5 deletes the
   RT-08 row that currently makes it inert.

**What breaks / when.** A Windows kiosk whose ordinary content page drives the WebView2 tree
past a *summed* 1500 MB — plausible for a content-rich site across a browser process, GPU
process, 2-4 renderers and utility processes, each carrying its own copy of the shared
`msedge.dll` working set — restarts every `5 × health_sample_s` (300 s at defaults),
indefinitely. Per the interlock that is *not* a crash loop and *not* safe mode: the launcher
restarts cleanly, clears the window on each healthy run, and logs `watchdog.restart{code:80}`
at ERROR. It is a well-behaved, well-logged, permanent five-minute outage cycle. That is a
field outage produced by a leak *mitigation*, on a platform where nothing measured the
threshold.

**Why it matters.** Frame §6 lists "cross-platform regression risk on Windows" as HIGH, and
C8 puts the burden on the Writer. The C3 divergence is now **declared** — I accept that, it
is what I asked for in R1 — but declaring a divergence is not bounding it. After R2 the
residual from OB-3 and the residual from OB-6 are the same residual, both marked "carried by
the deployment", and they compound into one unmeasured fleet-wide behaviour change.

**One correction to the justification, which I do not want banked as established.** E4 R2
argues the sum is defensible because "the machine dies on *total* footprint". The sum is not
total footprint — it is strictly **above** it, by exactly the shared-page over-count. And the
quantity that actually tracks machine pressure already ships: `mem_used_mb` / `mem_total_mb`
in `HealthSample` (`metrics.rs:8-9`, from `sys.used_memory()`). The sum's justification is
parent §6's literal "webview RSS" (traceability, Q1) — which is sound and is why I accept the
metric — not the OOM argument, which its own metric contradicts.

**The cheap fix, on a run E already owns.** 18-W(c) already runs on `windows-latest` with
`max_webview_mem_mb = 0`. Add one recorded observation to it: the steady-state
`webview_rss_mb` of the fixture at rest, reported in the artifact as a first-class number, and
a plan-time rule — *if the observed healthy sum is within a stated margin of 1500, raise a
defect against parent §5.2's default rather than shipping the enforcement*. That is zero new
runs and one assertion, it uses the escalation path E5 already names ("the corrective action
is a parent §5.2 default amendment raised as a defect"), and it converts "carried by the
deployment" into "carried by a gate E owns". Without something of this shape the enforcement
half of E5 is a change whose blast radius no gate observes.

**Two notes on the descendant walk itself, since the Moderator asked what it *finds*.**
The mechanism is sound and I do not object to it: on Windows the parent pointer comes from
`SYSTEM_PROCESS_INFORMATION.InheritedFromUniqueProcessId` in the same NT snapshot as the
memory figure and is re-read on every pass (`src/windows/system.rs:306-312`, `:322-323`), so
no handles are needed and the walk reaches the WebView2 browser process (a child of the app
process) and its renderers (grandchildren). And 18-W(b)'s "`webview_rss_mb` climbs and is
reported" now *is* the pin that the walk finds something on Windows — that gap from R1 is
closed. The one residual: Windows never rewrites `InheritedFromUniqueProcessId` when a parent
exits, and PIDs are recycled, so a full-subtree walk can graft an unrelated tree onto the
kiosk's — an inflation vector, which matters more now that NEW-1 shows the threshold is tight.
`Process::start_time()` exists (`common/system.rs:1384`); rejecting a child whose start time
precedes its claimed parent's is a one-comparison guard on a helper E is writing anyway.

### NEW-2 — 18-W(c) is unassertable without three fixture preconditions it does not state (MED, E8)

**What breaks.** 18-W(c) asserts "`webview_rss_mb` after the reload below the pre-reload
peak", driven by `maintenance.nightly_reload`. Read the actual reload path:

- `maintenance::run` fires a callback which sends **`AppEvent::IdleExpired` into the FSM**,
  not a webview reload (`main.rs:1177-1194`, whose comment states the design).
- `kiosk_core::app::state`: `(Online { .. }, IdleExpired)` → `self.go_online(self.home)`
  (`state.rs:306-311`). With `idle_clear` set, it instead routes through
  `Clearing` + `Effect::ClearProfile { full: true }` and re-navigates only on
  `ProfileCleared` (`:296-304`).
- **Every other state is a no-op** — there is no other `IdleExpired` arm, and the wiring
  comment says so explicitly: *"any other state (Offline, ErrorPage, Clearing) is a no-op"*.

So (c) passes only if all three hold, and E states none of them:

1. **The device must be in `Online`.** A fixture that reaches its leaking page any other way
   (offline/error path) gets a silent no-op: no reload, no RSS drop, assertion fails for a
   reason that has nothing to do with the feature.
2. **The leaking page must *be* `home`.** The reload navigates to `self.home`, not "reload the
   current URL". If the fixture navigates to a leaking page that is not the configured home,
   the reload swaps to a *different, lighter* page — the assertion passes while proving
   nothing about a reload resetting webview state, which is exactly parent §10's claim.
3. **`idle_clear` must be off** — otherwise the profile clear also frees memory and the
   assertion cannot attribute the drop to the reload.

**Why it matters.** (c) is one of parent §10's three named assertions and, post-split, the
only run that carries it. Condition 2 in particular makes it a **false-pass** risk, not just a
flake: the gate can go green while measuring the wrong thing (Q3). Fix is spec text on the
fixture, not code.

### NEW-3 — E1's IPC contract and E3's new call site disagree (LOW, E1/E3)

E1 (R1, unrevised on this point) declares
`#[tauri::command] fn media_error(kind: String, at: f64, telem: State<Telemetry>)`, with
`kind` validated against a closed set and *no free-form string across IPC*. E3's R2 code
calls `fallback("stall", { at_loop_boundary: Date.now() - wrapAt < 12000 })` — it **drops
`at`** and **adds a boolean field the command does not accept**. One of the two must move;
neither turn says which.

While that is being reconciled, one substantive remark on the field itself: the boolean
thresholds at 12 000 ms, but the monitor's *minimum* detection latency is two 5 s misses
= ≥10 000 ms after the last `timeupdate`. That is a 2 s margin for a `setInterval` whose
firing the page cannot control (a busy main thread, or WebKit's hidden/occluded timer
throttling, both delay it). Any delay over 2 s silently flips a genuine loop-boundary stall
to `at_loop_boundary: false`, and E6's activation is "mechanical, not judgment" — it would
simply not arm. The lazy fix removes the threshold rather than tuning it: send
`ms_since_wrap` as a number and let the plan reader apply the rule. Same one field, no magic
constant, no margin to get wrong.

---

## On the three replacements the Moderator named, verified

**`set_open_files_limit(0)` — the claim holds exactly, including its precondition.**
`lib.rs:140-168`: `_new_limit` clamps to 0, `max = get_max_nb_fds()` (`system.rs:89-101`,
hard/2), then `remaining_files().fetch_update(|remaining| Some(0.saturating_sub(max - remaining)))`.
Called **before any process refresh**, `remaining == max`, so `diff == 0` and the budget lands
at exactly 0 — the Writer's stated precondition is load-bearing and he states it. Then
`FileCounter::new` (`unix/linux/process.rs:931-944`) returns `None` on a zero budget, so
`_get_stat_data` (`:364-368`) still opens, reads and **drops** the handle rather than
retaining it (`stat_file` stays `None`). Functionality preserved, retention gone. And he is
right that the `setrlimit` is unavoidable: `set_open_files_limit` itself calls
`remaining_files()`, whose `OnceLock` initialiser is the `getrlimit`/`setrlimit` pair
(`unix/linux/system.rs:22-46`). Residual owned; note for the Moderator that the effect is a
**raise** of the soft limit, so the `LimitNOFILE` ask on C/G buys very little and C declining
it costs essentially nothing — it should not be treated as a blocking cross-spec dependency.

**The `timeupdate` predicate — correct on all three axes.** *Loop vs stall:* a wrap emits
`timeupdate`, a hung decode emits none, so the discriminator is engine activity rather than a
float comparison across a wrap — the stuck-at-0.0 case that OB-4 named goes from blind spot to
primary detection path. *Cadence:* the HTML media element fires `timeupdate` on the order of
4–66 Hz during normal playback, against a detection window of two 5 s intervals — tens to
hundreds of events per interval, an enormous margin, and WebKit's hidden-page timer throttling
only makes the *check* less frequent, which is the safe direction (`ticks > 0` resets). *False
fire during degrade:* the guard is `if (degraded || v.paused) { misses = 0; return; }` and
`degraded` is checked first, so the degrade path — which sets `display:none` without pausing
(`offline.html:40-48`) — cannot trip it. The two-miss requirement supplies the confirming
sample R1 said was missing. No objection.

**The sibling asks.** *B — two ACL entries:* legitimate. B is adding its own command; the
`build.rs` and capability entries are B's own work and E is naming a newly-shared file, not
transferring effort. The register correctly upgrades B↔E from one shared file to three.
*C/G — `LimitNOFILE`:* legitimate in form (declared, one line, on the spec that owns the
artefact, with the residual named if declined) but marginal in substance per the paragraph
above. Neither is work-pushing.

---

## Counts

| Status | Count | IDs |
|---|---|---|
| ACCEPTED | 9 | OB-1, OB-4, OB-5, OB-6, OB-7, OB-8, OB-9, OB-10, OB-11 |
| COUNTERED | 2 | OB-2, OB-3 |
| ESCALATED | 0 | — |

New: **NEW-1** (HIGH), **NEW-2** (MED), **NEW-3** (LOW).

Still open: NEW-1; OB-2's missing pin for the shipped-default interlock; OB-3's E↔F fixture
divergence; NEW-2's three unstated 18-W(c) preconditions; NEW-3's signature reconciliation.
Everything else is closed from my side.

**Verdict on the memory cap at the shipped Windows default: not yet safe.** The mechanism is
sound, the exit path is sound, and 18-W(b) now proves breach→restart end to end — but the
*level* is a number the parent never derived, compared against a quantity that over-counts
shared pages worst on Windows, and no gate in P2 observes a healthy Windows working set at
that level. One recorded baseline on 18-W(c), a run that already exists, closes it.
