# P2-E — WRITER, Round 3

Every citation re-run by me at HEAD `1decd59`. The nine accepted objections are closed and
not re-argued. Five items open; five positions.

---

## OB-2 (COUNTERED) — REVISE: I take the Critic's pin, it is better than mine

**My (iv) was wrong and I withdraw it.** "That is the only place the relation is observable at
all" generalised from a true premise (`kiosk-main` never reads `healthy_run_s`) to a false
conclusion. 18-W(b) at dwell 50 / `healthy_run_s` 30 pins `50 > 30`, a generic FSM property
already covered by `watchdog.rs`'s own tests — not the shipped `300 > 120` relation. After R2
that relation had zero pins. Conceded.

**The counterexample is real — I verified both reach points:**

- `crates/kiosk-launcher/Cargo.toml:14` — `kiosk-core.workspace = true`.
- `watchdog_config(None)` — `crates/kiosk-launcher/src/main.rs:110-124`, returning
  `healthy_run_s: 120` from `BootstrapConfig::parse`'s own defaults, already asserted at
  `:285-290` (`a_missing_ini_falls_back_to_the_spec_default_timings`).
- `RemoteConfig::default().logging.health_sample_s` — `RemoteConfig` is `pub`
  (`schema.rs:272`), `health_sample_s` is `pub` (`:250`), `d_health_sample() = 60` (`:44-46`),
  pinned at `:345`.

**Taken, as specified.** `kiosk-core` exports `pub const MEM_CAP_N: u32 = 5` (E5 needs it
public for the latch regardless), and `kiosk-launcher` gains one test:

```rust
#[test]
fn mem_cap_dwell_exceeds_the_launchers_healthy_run_window() {
    let dwell = MEM_CAP_N as u64 * RemoteConfig::default().logging.health_sample_s;
    assert!(dwell > watchdog_config(None).healthy_run_s,
        "a memory-cap exit must land after the crash-loop window has been cleared");
}
```

No hardcoded copy of either number; it fails if `d_health_sample`, `MEM_CAP_N` or the
launcher's default `healthy_run_s` moves. One `pub`, one test — cheaper than what it replaces.

**Scope stated honestly:** this pins the relation *at shipped defaults*. Both sides remain
operator-settable (`health_sample_s` by remote config, `healthy_run_s` by `kiosk.ini` with no
range validation — `bootstrap.rs:74-89`'s `number()` applies no bounds). That residual is
already declared and unchanged.

**18-W(b)'s `no watchdog.safe_mode` assertion is kept as well**, not replaced: it is the only
end-to-end exercise of exit → restart → window behaviour across both processes. Two pins for
two different properties.

## OB-3 (COUNTERED) — REVISE: one authoritative parameter set, one owner, edge declared

**Conceded.** I re-read `P2F-R2-writer.md:93-101`. F7 as adopted restates my **Round-1**
parameters — one run, cap 256 *and* `nightly_reload` set, no `healthy_run_s` override, no
safe-mode assertion. That is the fixture I retracted four sections later in the same turn: the
cap re-trips every 50 s, every restart resets the reload timer so "post-reload RSS" is
unreachable, and at `healthy_run_s = 120` the run escalates to `watchdog.safe_mode` on the
Critic's R1 timeline, which I accepted. An implementer reading F builds the broken fixture.
The boundary form was right and the content drifted, exactly as charged.

**Label collision removed on E's side.** F's `endurance` jobs are (a)/(b)/(c); my scenario
sub-labels (b)/(c) collide with them. E renames: **`18-W1`** (breach → restart) and
**`18-W2`** (nightly reload resets RSS). One word, permanent.

**The authoritative parameter set — E owns it; F references, never restates:**

| | **18-W1** | **18-W2** |
|---|---|---|
| Runner | `windows-latest` | `windows-latest` |
| Page | deliberately leaking, **is `content.home`** | deliberately leaking, **is `content.home`** |
| `maintenance.max_webview_mem_mb` | **256** | **0** (off) |
| `logging.health_sample_s` | **10** (dwell = 50 s) | default |
| `kiosk.healthy_run_s` (ini) | **30** | default |
| `maintenance.nightly_reload` | **unset** | **a few minutes ahead** |
| `input.idle_clear` | off | **off** (NEW-2) |
| `--safe` | no | **no** (NEW-2) |
| Asserts | `webview_rss_mb` climbs; breach → **exit 80** → `watchdog.restart{code:80}`; **no `watchdog.safe_mode`** | zero restarts; post-reload `webview_rss_mb` < pre-reload peak; post-reload URL == the leaking page; **steady-state `webview_rss_mb` recorded** (NEW-1) |

**Spec text for the boundary, both sides:**

> Scenario 18-W1/18-W2's parameters and assertions are defined **only** in P2-E §E8. P2-F's
> `endurance` Windows job **references them by ID and must not restate them**; restating is
> what produced the R2 drift. F owns the job, the runner, scheduling and artifacts; E owns the
> body, the parameters and the feature.

**Declared dependency edge, for the Moderator to carry into integration:**

> **E → F (hard, blocking):** F7's spec text currently encodes E's retracted single-run
> fixture. F7 must be re-synced to E8's two-run table (18-W1, 18-W2) **in the same integration
> pass**, replacing its inline parameter list with a reference. Until re-synced, F7 is not
> implementable as written. F's own clause stands unchanged: if E4/E5/E8 are withdrawn, F7
> becomes unrunnable and parent §10's Windows-soak row returns to UNOWNED rather than silently
> passing.

## NEW-1 (HIGH) — REVISE on the gate · CONCEDE the justification · ACCEPT-AS-DOCUMENTED-RISK on the remainder

**The facts are conceded; I checked each.** `grep -n "1500" docs/superpowers/specs/*.md`
returns **exactly one line** across every spec — parent §5.2:538 — with no derivation and no
stated measurand. No observed RSS number exists anywhere in the repo. E4 binds that number to
a descendant sum that over-counts shared pages, worst on Windows. 18-W1 runs a leaking page at
cap 256, 18-W2 runs cap-off, scenario 18 is Debian offline-video, P2-G H5 is Linux hardware —
no gate observes a healthy Windows working set at 1500. The Critic's verdict — *mechanism
sound, level unsafe* — is correct as stated.

**(a) The justification correction — CONCEDED, do not bank the OOM argument.** "The machine
dies on total footprint" is wrong twice: the sum is strictly *above* total footprint by the
shared-page over-count, and the quantity that does track machine pressure already ships —
`mem_used_mb` / `mem_total_mb` in `HealthSample` (`metrics.rs:8-9`, from `sys.used_memory()`).
The sum's justification is **Q1 traceability to parent §6:671's literal "webview RSS"**, and
that alone. E4's rationale text is rewritten accordingly.

**(b) The cheap fix — TAKEN, on the run that already exists.** 18-W2 records, as a first-class
artifact number, the **steady-state `webview_rss_mb` of the fixture at rest** on
`windows-latest`, plus a gate rule with a derived margin:

> **Enforcement gate (E5).** If the observed steady-state Windows sum is **≥ 750 MB (half of
> 1500)**, E5's enforcement half does not ship: a defect is raised against parent §5.2's
> default instead. Rationale for the margin: a leak must be distinguishable from a working
> set, which needs at least 2× headroom between healthy steady state and the cap; below that
> the cap fires on normal variance rather than on a leak.

This converts "carried by the deployment" into "carried by a gate E owns", using the
escalation path E5 already names, at zero new runs — exactly the shape the Critic specifies.

**(c) What the gate does *not* cover — ACCEPT-AS-DOCUMENTED-RISK, named precisely.** 18-W2's
fixture is a leaking test page, not fleet content, so the number it records is a **floor** on
the healthy Windows working set: engine + helpers + a trivial page. Real content adds on top.
The floor check catches the disqualifying case (even the floor is near 1500) and cannot
certify the general one.

- **Residual:** a Windows fleet whose real site drives the summed tree between the recorded
  floor and 1500 gets a clean, well-logged, permanent restart cycle every 300 s that it did
  not have in P1.
- **Carried by:** the operator, informed rather than surprised — see (d).
- **Not** carried by G's checklist, which is Linux; my R2 sentence pointing there was wrong for
  Windows and is withdrawn.

**(d) What P2-E commits to measuring, and what an operator should set.** The R1 ordering claim
is restored in a form the evidence supports, because it is now an ordering of *shipping*, not
of *measurement quality*:

1. **E4 ships first, unconditionally** — sampler only, no enforcement. Every fleet gets
   `webview_rss_mb` in `health.sample` from that build.
2. **E5's enforcement ships only after 18-W2's floor check passes** (the gate in (b)).
3. **Operator guidance in the release note, concrete:** *after upgrading to the E4 build, read
   your fleet's `health.sample.webview_rss_mb` p99 over one week; set
   `maintenance.max_webview_mem_mb` to roughly **2× that value** within `[256, 8192]`, or to
   **0** to disable. Until you have done so the shipped default of 1500 applies.* That
   guidance is actionable only because E4 precedes E5, which is the whole reason for the
   ordering.

I do **not** propose changing the default: parent §5.2:538 pins 1500 and parent §10:872
requires a breach to fire a restart. E cannot unilaterally overturn either. What E can do —
and now does — is gate its own enforcement on a measurement and hand the operator a derived
number.

**(e) PID-recycle inflation guard — TAKEN.** Verified `Process::start_time()` exists at
`sysinfo-0.32.1/src/common/system.rs:1384`, cross-platform. The subtree helper rejects any
candidate child whose `start_time()` **precedes** its claimed parent's — one comparison in a
helper E is writing anyway, closing the stale-`InheritedFromUniqueProcessId` graft the Critic
names. Covered by the existing synthetic-pid-map host test with one recycled-pid case added.

## NEW-2 (MED) — REVISE: three preconditions become fixture spec text, plus one the Critic did not name

**Conceded; every step verified.** `main.rs:1177-1194` — the nightly timer sends
`AppEvent::IdleExpired` **into the FSM**, and its own comment enumerates the outcomes.
`state.rs:296-304` — `Online` + `idle_clear` → `Clearing` + `Effect::ClearProfile{full:true}`,
re-navigating only on `ProfileCleared`. `state.rs:306-311` — `Online` without `idle_clear` →
`go_online(self.home)`. `grep -n "IdleExpired" state.rs` returns exactly those two transition
arms; every other state is a no-op, pinned by `:979-993`
(`IdleExpired from Boot is a no-op`, `… from Offline is a no-op`).

**18-W2's fixture preconditions, now spec text:**

1. **The device must be in `Online`** when the timer fires — any other state is a silent
   no-op, and the assertion would fail for a reason unrelated to the feature.
2. **The leaking page must be `content.home`.** The reload navigates to `self.home`, not "the
   current URL". This is the Critic's false-pass case and it gets a *second* assertion rather
   than only a precondition: **18-W2 asserts the post-reload URL equals the leaking page**, so
   a swap to a different, lighter page cannot go green while proving nothing.
3. **`input.idle_clear` off** — otherwise `ClearProfile{full:true}` also frees memory and the
   drop cannot be attributed to the reload.
4. **Added, not raised:** the run must not use `--safe`. `main.rs:1185-1186` — *"`--safe` never
   spawns this (same `if safe {} else {}` split as the FSM driver above)"* — so a safe-mode run
   has no reload timer at all. Same failure class, one line.

## NEW-3 (LOW) — REVISE: E1's signature moves, and the constant is deleted rather than tuned

**Conceded — E3's R2 call site and E1's R1 signature disagree, and E1 is the one that moves.**
Also conceded on the constant: 12 000 ms against a minimum detection latency of two 5 s misses
(≥10 000 ms) leaves 2 s of margin on a `setInterval` the page cannot control, and any overrun
silently flips a genuine loop-boundary stall to `false`, leaving E6's "mechanical, not
judgment" trigger unarmed. Tuning the constant would only move where it is wrong.

**Reconciled contract, single source of truth:**

```rust
#[tauri::command]
fn media_error(kind: String, at: f64, ms_since_wrap: Option<f64>, telem: State<Telemetry>)
```

- `kind` — validated against the closed set, unchanged (the trust boundary).
- `at` — restored; E3's R2 snippet dropped it, which was my error.
- `ms_since_wrap` — a **number**, `None` when no wrap has been observed. No boolean, no
  threshold in the page. E6's trigger rule reads the number in the plan; the constant lives in
  one place (the contingency's activation rule) instead of two.
- Numeric hygiene at the boundary: non-finite or negative `at` / `ms_since_wrap` are recorded
  as `null` rather than logged, alongside the existing `kind` rejection.

E3's call becomes `fallback("stall", v.currentTime, Date.now() - wrapAt)`, and the `12000`
literal is deleted from the page.

---

## Change register after Round 3

Ⓓ = dependency edges moved this round.

| ID | State after R3 | Depends on |
|---|---|---|
| **E1** | **Revised (NEW-3).** Signature is `media_error(kind, at, ms_since_wrap: Option<f64>)`; numeric hygiene at the boundary. ACL entries + `.manage(telem.clone())` unchanged from R2. | P2-B (three shared files: `main.rs:990`, `build.rs`, `capabilities/default.json`); P2-A |
| **E2** | Unchanged (clean pass R1) | E1 |
| **E3** | **Revised (NEW-3).** `timeupdate`-counter monitor unchanged; call site sends `currentTime` + raw `ms_since_wrap`; the `12000` literal deleted | E1, P2-A |
| **E4** | **Revised (NEW-1a, NEW-1e).** OOM justification **withdrawn** — rationale is Q1 traceability to parent §6:671 alone; subtree helper gains a `start_time()` PID-recycle guard. Sum, C3 declaration, t=0 baseline, `set_open_files_limit(0)` unchanged. **E4 ships before E5, unconditionally.** | Ⓓ **E5 now depends on E4 shipping first (ordering is spec, not intent)**; ask on C/G for `LimitNOFILE` downgraded to non-blocking |
| **E5** | **Revised (OB-2, NEW-1b/c/d).** `pub const MEM_CAP_N = 5` exported; the launcher-side default-relation test added. **Enforcement gated on 18-W2's floor check (≥ 750 MB ⇒ defect against parent §5.2 instead of shipping).** Operator guidance (p99 × 2, or 0) in the release note. Residual named and carried by the operator, not by G's Linux checklist. | Ⓓ **18-W2's recorded floor is now a merge gate on E5's enforcement half**; E4; P2-C (code space) |
| **E6** | **Revised (NEW-3).** Trigger rule reads `ms_since_wrap` as a number; the threshold lives here, once. `loop` removal / `load()` reset / `canplaythrough`-before-swap unchanged | E3 |
| **E7** | Unchanged (accepted R2) | P2-G, P2-F |
| **E8** | **Revised (OB-3, NEW-1b, NEW-2).** 18-W(b)/(c) renamed **18-W1 / 18-W2** (F label collision); one authoritative parameter table owned by E; 18-W2 gains the four fixture preconditions, the post-reload-URL assertion, and the recorded steady-state floor | Ⓓ **E → F hard edge: F7 must re-sync to the two-run table and reference, not restate**; E1–E6; P2-G H5 (Linux only — no longer load-bearing for the Windows level) |

**New this round:** the launcher-side default-relation test (OB-2); the 18-W2 floor gate on
E5's enforcement (NEW-1); the `start_time()` recycle guard (NEW-1e); four 18-W2 preconditions +
a post-reload-URL assertion (NEW-2).
**Withdrawn this round:** "18-W(b) is the only place the relation is observable" (OB-2); the
OOM justification for the subtree sum (NEW-1a); "carried by G's hardware-checklist sign-off"
for the Windows residual (NEW-1c); the `12000` threshold in the page (NEW-3).
