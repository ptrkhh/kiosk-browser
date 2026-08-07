# P2-F — WRITER, Round 2

Every Critic citation below was re-checked by me in-session before I took a position. Where
I take REBUT I say what I ran. Settled items (F3, F5, F11, F13, F4-exhaustiveness, the
verifier-#14 rebuttal, W1/W2/W3/W5/W6/W7/W8) are not re-argued.

---

## OB-1 — REVISE

**The loop is real and I resolve it the Critic's way (a): F owns the harness code.**

Citations re-verified by me, verbatim:
- `p2a…:312-315`: "The smoke is human-run in-session and is deliberately **not** wired into
  `ci.yml`; **automating the compositor harness is P2-F**." A is rev 3 and closed; frame §0
  forbids adding a deliverable to it. My R1 did exactly that.
- `p2b…:34`: "**CI automation of the harness → P2-F**"; `p2b…:236`: "update+CI harness
  automation (P2-F)".
- `p2c…:31`: "**Out:** … update/**CI-harness (P2-F)**".
- `p2e…:25`: "**In:** … **the soak harness** + pass criteria" — E is the one spec that keeps
  its harness.

Four texts, one direction. F1's inversion was wrong.

**Replacement design.** The split runs along *definition vs. implementation*, which is the
only line all four texts support:

| Artefact | Owner |
|---|---|
| What each scenario asserts, its fixture shape, its pass criteria | A (1–7), B (8–12), C (13–15), D (16–17) — **already written**, in prose, in their specs |
| The harness: compositor bring-up, fixture HTTP server, spool reader, and the scenario bodies for A 1–7 · B 8–12 · C 13–15 · D 16–17 | **F** |
| Scenario 18 and 18-W bodies + the soak harness | **E** (`p2e…:25`, unchanged) |
| The workflows that invoke all of it | **F** |

Concretely F builds, in `crates/kiosk-main/tests/smoke_linux.rs`: weston-headless bring-up,
a `std::net::TcpListener` fixture server (no python — `debian:12` ships none), an on-disk
spool reader (A `:291` already specifies spool-as-oracle), and ~15 scenario bodies. A 6's
"dedicated harness binary (cargo example)" (`p2a…:304-306`) is F's to write; I re-confirmed
`crates/kiosk-core/examples/` holds only `kioskctl.rs`.

**What this costs, stated.**
1. **W2 is superseded.** The honest sentence is now: "F changes no `crates/*/src/`. F owns
   the smoke-harness code and every scenario body except E's." Not "F writes no product
   code" and not R1's softer "F does own test-harness code" — F owns essentially all of it.
2. **F's dependency surface shrinks and its implementation cost grows.** F1 no longer
   depends on A–E landing code; it depends only on their *specs*, which exist. That removes
   the deadlock. It also makes F the largest implementation item in P2 after the product
   work, and the plan must sequence it accordingly.
3. **F3 is unaffected** — the 10-minute rule is runtime, not authoring time. F3's real
   exposure is OB-6, answered there.

I do not take option (c) (record A 1–7's automation as unowned): A names an owner and it is
F.

---

## OB-2 — REVISE (F7 survives, on E's *revised* design, with the edge declared)

**Against E's original spec text, the Critic is right on all three sub-claims.** I verified
each myself:
1. Parent `:671`: `health.sample` … "**webview RSS**" … "(P2)". E's draft sampled
   `/proc/self/status` — kiosk-main, not the renderer. A leaking page grows
   `WebKitWebProcess` / `msedgewebview2.exe`. Under that mechanism the cap never trips.
2. `schema.rs:234` `pub max_webview_mem_mb: u64`, `:343` default **1500**,
   `validate.rs:19` `("maintenance.max_webview_mem_mb", "P2")` in `UNIMPLEMENTED`,
   `:107-114` range `{0} ∪ [256,8192]`. E's draft introduced a *different* key,
   `memory_max_mb`.
3. `schema.rs:228` `nightly_reload: Option<String>`; `maintenance.rs:1-4` "local wall-clock
   `HH:MM`", `:12-14` "`next_fire` always returns a strictly-future instant". There is no
   threshold to accelerate. My phrase "accelerated `max_webview_mem_mb` **and nightly-reload
   thresholds**" was wrong on the second half.

**But E's Round-1 turn block has already restructured onto exactly what OB-2 demands**, and
I verified that too, at `P2E-R1-writer.md`:
- **E4** (`:13`, `:110-116`): "**Webview-process** RSS … via the already-held
  `sysinfo::System` (**pid-subtree sum**)", summing "the **descendant subtree** of the
  current pid (excluding self)". Sub-claim 1 answered — the measured quantity is now the
  renderer's, so a leaking page moves it *and* a page reload releases it.
- **E5** (`:13`, `:159-183`): "Memory-cap restart against the **shipped**
  `maintenance.max_webview_mem_mb`"; `memory_max_mb` withdrawn (`:19`, `:468`); the config
  work is a **deletion** of the `UNIMPLEMENTED` row at `validate.rs:19`. Sub-claim 2
  answered — F7 names the key E now implements.
- **E8 / scenario 18-W** (`:313-321`): "**Scenario 18-W = the Windows memory soak**, owned
  by E (E owns the feature), **wired by F**. Fixture: a deliberately leaking local page;
  `max_webview_mem_mb` set to the range floor **256** and `health_sample_s` to **10** …
  dwell shrinks to 50 s. Asserts: (a) `webview_rss_mb` climbs and is reported; (b) the
  breach produces **exit 80** and a launcher restart, with `watchdog.restart{code:80}` in
  the spool; (c) with `maintenance.nightly_reload` **set a few minutes out**,
  `webview_rss_mb` after the reload is below the pre-reload peak." Sub-claim 3 answered —
  and the acceleration for nightly reload is a near-future `HH:MM`, which is precisely the
  thing the Critic says F failed to state. F did fail to state it; E now does.

**Revised F7.** F7 stops asserting feasibility from E's *draft* and states the edge:

> `endurance` job (c) runs **E's scenario 18-W** on `windows-latest`. Parameters and
> assertions are E's (`max_webview_mem_mb: 256`, `health_sample_s: 10`,
> `nightly_reload` set a few minutes ahead; asserts rising `webview_rss_mb`, exit 80 +
> `watchdog.restart{code:80}`, post-reload RSS below the pre-reload peak). **F owns the
> job; E owns the body and the feature.** F7 is executable only if E4 (subtree RSS), E5
> (cap against the shipped key) and E8's 18-W all land. **Declared dependency, hard:** if
> any of the three is withdrawn, F7 becomes unrunnable and the §10 Windows-soak row returns
> to UNOWNED in the ledger rather than silently passing.

That last clause is the point the Critic makes about Q3 and I adopt it verbatim as spec
text. Also note the chain F7 needs: exit-80 → launcher restart means the Windows job runs
launcher+main, not main alone — precedent exists (`crates/kiosk-launcher/tests/rt13.rs`,
`rt13-mock-main` bin).

---

## OB-3 — REVISE

**The arithmetic is the Critic's, and it is right.** `jobs.<id>.timeout-minutes` bounds the
whole job, container creation included. 330 − 315 = **15** minutes for: `debian:12` pull,
apt, toolchain, a cold Tauri/wry/webkit2gtk build, teardown, upload. That re-creates the
failure the explicit timeout existed to prevent. My "~30 min" read the gap to the platform
cap, not to my own timeout.

**Replacement, which removes most of the setup rather than budgeting for it.** The soak job
stops building anything:

- A `build` job (ubuntu-22.04, `Swatinem/rust-cache`, `cargo build --release`) uploads the
  binaries; the soak and matrix jobs `needs:` it and `download-artifact`. Debian 12 ships
  glibc 2.36 against Ubuntu 22.04's 2.35, so a 22.04-built binary runs on `debian:12`
  — **declared assumption**, pinned by the same probe run as open decision #4 (one
  `ldd`/`--version` check inside the container), fallback is to build inside the container
  and re-budget.
- The soak container then installs **runtime** packages only (`libwebkit2gtk-4.1-0`,
  the four GStreamer packages, `weston`) — not the `-dev` set `ci.yml:15-19` needs, and no
  Rust toolchain.
- Soak duration is **derived, not asserted**: `timeout-minutes: 330`; soak step =
  `330 − measured_setup − 20 min` reserve for artifact upload and teardown. Initial value
  **270 min (4 h 30 m)** until the probe measures setup; the artifact-upload step carries
  `if: always()`.

Still "multi-hour" per parent `:873-875`, which is the only duration word the parent uses.

**Consequential ask on E, restated** (replacing R1's "≥5 h"): E should stop pinning a CI
duration at all. `P2E-R1-writer.md:297` still reads "scheduled CI **8 h+** in `debian:12`
(F)". Ask: "scheduled CI: multi-hour, duration set by F within the hosted-runner cap." E
owns the pass criteria, which are duration-agnostic (`p2e…:99-103`); F owns the wall clock,
which is F's constraint. If E declines, F runs 270 min and records the divergence.

---

## OB-4 — REVISE (both holes)

1. **Ancestry, not just recency.** Conceded: `headSha` was fetched and unused. Revised
   gate: take the newest successful `endurance` run's `headSha`, and refuse the tag unless
   `git merge-base --is-ancestor <headSha> <tag_sha>` **and** the run is inside the
   freshness window. Both, not either — ancestry alone admits a year-old soak, recency
   alone admits a soak of a tree that lacks the tagged commit.
2. **Open decision #3 is closed, not left open.** F9 and "alternating nights" are
   incompatible; I close it in favour of **one nightly workflow, all three jobs, every
   night**, so run-level `--status=success` implies the soak ran. This is affordable
   because OB-3 removed the per-job build: one shared `build` job feeds all three. Spec
   text carries the interlock: "if a future change splits `endurance` across nights, this
   gate must move to job-level (`gh run view <id> --json jobs`) in the same change" — so
   the coupling is written down rather than discovered.

Not contested: fail-closed on API error, and the freshness window as the answer to
default-branch-only and 60-day auto-disable.

---

## OB-5 — REVISE (I over-conceded; restoring the trail, keeping the demotion dropped)

The Critic is right about the asymmetry, and it is in the same turn: F9 accepted a
`gh run list --json` query as the mechanism for one gap and F14 rejected cheaper machinery
for another. Q3 is the requirement here, and a `flaky` line on a green run's summary does
not meet it.

**Restored, with the mechanism the Critic names.** On pass-after-retry the job runs one
step:

```
gh issue comment "$FLAKY_ISSUE" --body "flaky: <scenario> · <run-url> · <sha>"
```

`GITHUB_TOKEN` with `permissions: issues: write`. No cross-run state, no rolling window, no
bot, no external store — strictly less machinery than F9's accepted query. The `flaky`
summary line stays (it is free and it is where the person looking at *that* run sees it).
Fork PRs get a read-only token, so the step is `continue-on-error: true` and the summary
line remains the fallback there.

**Automated demotion stays dropped** — that is the part that genuinely needs a rolling
window and an actor, and W4's reasoning survives for it.

**Named owner, without inventing a role.** The Critic is right that "a maintainer" is not
one. The review point is moved to a checkpoint that already exists in F: **whoever cuts the
release (F8/F9)** reviews the standing flaky issue as a release step; a scenario with two
or more comments since the previous tag is fixed or moved to `endurance` before the tag is
pushed. That is an owner F's own workflow already presupposes, and it puts the decision at
the moment it matters.

---

## OB-6 — REVISE (and the fix removes the question rather than answering it)

Conceded: `cargo test` builds the package's bins in the test's profile, default `dev`, and
hands them over as `CARGO_BIN_EXE_*`. F1 named no profile, so as written the per-PR gate
would have validated a debug `kiosk-main` while `build-linux` (`ci.yml:56-63`, re-verified)
ships release — on a subset containing a short-threshold idle timer (D 16) and
crash-recovery timing (A 4, scheduled). A gate that proves a different artifact than the
one shipped is C9-weak, and the Critic is right that F3's budget was silently depending on
this unmade choice.

**Replacement.** Do not choose a profile — choose the artifact. The harness locates the
binary under test via `KIOSK_BIN`, defaulting to `CARGO_BIN_EXE_kiosk-main` for local runs;
in CI, `smoke-linux` `needs:` the release `build` job and sets `KIOSK_BIN` to the
downloaded **release** binary. Then:

- The gate exercises **byte-for-byte the artifact `build-linux` uploads and the `.deb`
  ships** — stronger than `cargo test --release`, which would only match the profile.
- The harness target itself compiles in `dev` (fast; it is assertion code).
- No second release build inside the 10-minute budget — the build is shared, which is also
  OB-3's answer and resolves open decision #1 toward "stands alone, consumes the build
  artifact" rather than "shares `lint-test`'s job".

One line in F1, and it retires the profile question instead of deciding it.

---

## OB-7 — REVISE

Conceded, and it is the same defect class I conceded at F9 and F12; applying the standard
unevenly in one turn is the fair charge. Actions has no cross-workflow job-body reuse.
Naming the mechanism:

- Extract the release build into `.github/workflows/build.yml` with `on: workflow_call`
  (inputs: none; outputs: the artifact name), containing the apt line and
  `cargo build --release -p kiosk-main -p kiosk-launcher` that `ci.yml:45-63` has today.
- `ci.yml`, `endurance.yml` and `release.yml` each `uses: ./.github/workflows/build.yml`.

**This edits `ci.yml`**, so I withdraw the implication in R1's F8 that F touches no existing
job. The alternatives are copy-paste (three apt lines and three toolchain pins kept in sync
by hand — the drift F16 worries about, tripled) or a composite action (also an edit, and it
cannot carry a job). `workflow_call` is the least-drift option and it is the one OB-3 and
OB-6 both already need, since three workflows now want the same release binaries.

---

## OB-8 — REVISE (all four points; I read `sign.ps1` in full)

**Per-artifact, not per-tag.** Conceded. A repo with no provisioned cert — today's state,
`p1f2…:41-44` "the cert itself is not in the repo", re-verified — could not cut a
Linux-only release under my rendering, and P2's shipped deliverable is the `.deb`. Revised:

> The Windows artifact set is produced by its own job. If the signing cert is absent or
> `sign.ps1` throws, **that job fails and its artifacts are excluded from the draft
> release**; the `.deb` job and the draft release still complete, with the release body
> recording the Windows set as absent. No unsigned Windows artifact ever reaches a release.

That is the parent's rule as written — "**unsigned artifacts** fail the release gate"
(`:883-884`), scoped to the artifacts that require signing. **C3, both directions:** this is
*looser* than my R1 rendering (a tag now succeeds without a cert) and *exactly as strict as*
the parent (no unsigned artifact ships). Both stated.

**The redundant verify — dropped.** Verified in `sign.ps1`: after each sign it runs
`& $signTool verify /pa /all $target` and throws on non-zero. My separate verify step was
duplicate work (Q2). Replaced by a **coverage** assertion instead, which is the thing
`sign.ps1` cannot check: every `.exe`/`.msi` in the release set must appear in a `sign.ps1`
invocation — enumerate the release set, diff against the signed set, fail on non-empty.

**Fourth input named.** `[Parameter(Mandatory)] [uri]$TimestampUrl` and
`if ($TimestampUrl.Scheme -ne 'https') { throw 'TimestampUrl must use HTTPS.' }` — the job
needs HTTPS egress to a timestamp authority. Added to F10's input list, which now reads:
Microsoft egress for the Evergreen bootstrapper, an out-of-band standalone installer for
offline builds, the cert, and the timestamp authority.

**Cert delivery made concrete.** `sign.ps1`'s `Thumbprint` parameter set requires a
certificate already present in `Cert:\CurrentUser\My` or `Cert:\LocalMachine\My` with a
usable private key — a fresh hosted runner has none. So the release job uses the **`Pfx`**
set: materialise a base64 repo secret to a temp `.pfx`, set
`KIOSK_SIGNING_PFX_PASSWORD` (the script's own default env-var name), pass `-PfxPath`, and
let the script's `finally` block do its own cleanup — I read it, it removes the imported
cert and its key. Also noted for the implementer: `-Stage Installers` requires `-NewerThan`
dependency paths, so the MSI sign call must pass the built PE paths.

---

## OB-9 — REVISE

Conceded: I did to G exactly what I refused to do to E. G's §1 (`p2g…:25-51`) specifies
payload, `Dependencies:`, state dirs, secrets discipline, autostart, versioning,
`Conflicts`/`Replaces` — **no build command, no tool, no single-command claim**. I
re-read it.

**Declared ask on G, symmetric with F6's on E:**

> **Ask:** G names the `.deb` build invocation and where it lives. G's Round-1 turn already
> commits to `debian/source/lintian-overrides` (`P2G-R1-writer.md:468`), which is a
> `debian/` source-tree layout, so the canonical invocation for what G has already chosen
> is `dpkg-buildpackage -b -us -uc`. F asks G to state it.
> **Fallback if G declines:** F specifies the invocation itself and G owns content only —
> i.e. R1's situation, but declared rather than silent.

**And I accept G's two asks on F**, which arrived in the same round
(`P2G-R1-writer.md:455-470`, G15) and which I had not seen when writing R1:
- Install/remove/upgrade cycle → added to F5's nightly `debian:12` job (unit enabled and
  active; `/etc/kiosk` 0750; credential and mp4 absent; zero `BEGIN PRIVATE KEY` and no
  `kioskctl` in the package; upgrade preserves the three operator-owned files;
  `deb-systemd-helper` does not re-enable a disabled unit).
- `lintian --fail-on error` → added to F8's release job, gating the `.deb` before it is
  attached.

Both are recorded as new F↔G dependency edges in the register.

---

## OB-10 — REVISE

Conceded, one word. Verified: `p2b…:193-196` "**the container has no systemd**, so the
smoke asserts only the degrade path (spawn fails → eprintln, kiosk unaffected)";
`ci.yml:12` is `ubuntu-22.04`, a full VM with systemd running. My "only if it is free" makes
membership depend on a condition the per-PR runner does not satisfy, and on that runner
`systemd-inhibit` plausibly *succeeds*, so the scenario would assert nothing or assert a
third behaviour nobody specified.

Revised: **B 12 is scheduled-only**, running in F5's `debian:12` container where B's stated
precondition holds. Its positive half remains on G's hardware checklist per B `:193-196`.

---

## OB-11 — REVISE

Conceded that "watch an upstream issue" is vigilance, not a frame §4.4 pinning mechanism —
no owner, no in-repo trigger. Replaced with the mechanism that already exists in F:

> The platform F actually certifies is `debian:12` (C7 floor), exercised nightly by F5's
> full matrix and F6's soak. `ubuntu-22.04` is a convenience runner for the per-PR subset
> only. A forced image migration therefore cannot silently change what P2 is certified
> against; its cost is bounded to re-running open decision #4's probe on the new image.

Pin = the nightly `debian:12` job, which runs whether or not anyone is watching an issue.
Issue-watching dropped.

---

## OB-12 — REVISE (both)

1. **Tag namespace.** Conceded. Rather than reserve and clean up `v0.0.0-gatecheck`, the
   `release` workflow also accepts `workflow_dispatch` with a `dry_run: true` input: every
   gate runs (endurance-green + ancestry, lintian, signing coverage), and the publish step
   is skipped. No tag consumed, nothing to clean up, and the negative proof is re-runnable
   on demand instead of once.
2. **Nothing standing.** Conceded, and it is the same rot F14 worries about one level up.
   The deliberately-broken fixture becomes a **standing** scenario in the harness: one test
   that runs a known-bad fixture and asserts the harness reports it as a failure. Per-PR,
   ~10 lines, cannot rot silently. The other two proofs (refused tag on red endurance,
   refused unsigned Windows set) stay one-time in the landing PR, but the `dry_run` path
   above makes both repeatable by anyone at any time, which is most of what a standing
   check buys.

---

## Change register after Round 2

Ⓓ = dependency edges moved this round.

| ID | State after R2 | Depends on |
|---|---|---|
| F1 | **Rewritten (OB-1, OB-6).** F **builds** the harness — compositor bring-up, `TcpListener` fixture server, spool reader, scenario bodies A 1–7 · B 8–12 · C 13–15 · D 16–17, plus A 6's harness binary. Binary under test located via `KIOSK_BIN` (release artifact in CI). | Ⓓ **A–E specs for scenario definitions only — no longer on A–E code.** E retains 18/18-W (`p2e…:25`) |
| F2 | Revised: `needs:` the shared release build, sets `KIOSK_BIN`; subset unchanged | Ⓓ F1, `build.yml` |
| F3 | Unchanged (clean pass); its cost model now assumes a downloaded release binary, not an in-job build | F2 |
| F4 | Revised (OB-10): **B 12 scheduled-only**; rest as R1 | F2, F5 |
| F5 | Revised (OB-9): gains G15's install/remove/upgrade cycle assertions | Ⓓ F1, `build.yml`, **G (G15 ask accepted)** |
| F6 | Revised (OB-3): soak **270 min** initial, derived as `330 − setup − 20`; `timeout-minutes: 330`; runtime deps only, no toolchain, no in-container build; upload `if: always()` | Ⓓ `build.yml`; **ask on E changed: E stops pinning a CI duration** |
| F7 | Revised (OB-2): runs **E's scenario 18-W** with E's parameters; F owns the job, E owns the body; unrunnable-if-E-withdraws stated in spec text | Ⓓ **E4 + E5 + E8/18-W (hard, declared)**; `build.yml` |
| F8 | Revised (OB-7, OB-8, OB-9): `build.yml` `workflow_call` — **F now edits `ci.yml`**; per-artifact signing gate; lintian gate on the `.deb`; `workflow_dispatch` dry-run | Ⓓ **`build.yml` (new, edits `ci.yml`)**, G, P1-F2 |
| F9 | Revised (OB-4): ancestry **and** freshness; **open decision #3 closed** — one nightly workflow, three jobs; split-interlock written down | F5, F6, F7, F8 |
| F10 | Revised (OB-8): per-artifact fail; redundant verify dropped, replaced by signing-**coverage** assertion; four inputs named incl. HTTPS timestamp authority; `Pfx` parameter set | P1-F2 `sign.ps1`; cert + PFX password secrets |
| F11 | Unchanged (clean pass) | `kiosk-core/src/logging/*`; SA secret |
| F12 | Revised (OB-9): **declared ask on G** for the build invocation, with fallback | Ⓓ **G (declared ask, not assumed)** |
| F13 | Unchanged (clean pass) | G runbook |
| F14 | Revised (OB-5): **trail restored** — `gh issue comment` on a standing flaky issue, `issues: write`, `continue-on-error` on forks; automated demotion stays dropped; owner = whoever cuts the release | Ⓓ **F8/F9 (review checkpoint)** |
| F15 | Revised (OB-12): broken-fixture proof becomes a **standing** per-PR scenario; negative proofs run via `workflow_dispatch` dry-run, no tag consumed | F2, F9, F8 |
| F16 | Revised (OB-11): pin = the nightly `debian:12` matrix (C7 floor); issue-watching dropped | F5, F6 |

**New in F's own scope this round:** the harness itself (OB-1) and `.github/workflows/build.yml`
(OB-7). **New withdrawal — W9:** R1's F8 implication that F changes no existing workflow job;
F edits `ci.yml` to call the reusable build workflow.
