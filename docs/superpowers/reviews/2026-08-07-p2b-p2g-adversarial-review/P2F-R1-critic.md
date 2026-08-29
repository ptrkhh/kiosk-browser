# P2-F — CRITIC, Round 1

No frame dispute. Everything below was checked against the repo at the current tree before
being argued. Where I could not check a claim mechanically I say so and name the tier.

## Objection index

| ID | Change | Objection (one line) | Sev | Evidence tier |
|---|---|---|---|---|
| OB-1 | F1 | The harness hand-off is circular: A (reviewed), B and C all assign CI/harness automation **to F**; F1 assigns the scenario bodies **back to A–E**. A cannot be reassigned, so 5 of F2's 10 per-PR scenarios have no committed producer. | **HIGH** | 2 (A rev 3, binding) + 2 (B, C) |
| OB-2 | F7 | F7's three assertions cannot be observed by the mechanism F cites as making them assertable: E samples **self** RSS (`/proc/self/status`), a leaking *page* leaks in the renderer process, parent §6:671 says **webview RSS**. Plus the config key F7 names does not exist in E's design, and `nightly_reload` has no acceleration knob. | **HIGH** | 3 (code) + 1 (parent §6:671) + 2 (E) |
| OB-3 | F6 | The re-budget's arithmetic is self-defeating: 315 min of soak inside a 330-min **job** budget leaves 15 min — not the claimed 30 — for container pull, apt, rust toolchain, cold build and artifact upload. The job dies at its own timeout with the artifact unwritten, which is the exact failure the explicit timeout was introduced to prevent. | MED | 5 (Actions docs) + 3 |
| OB-4 | F9 | The freshness window certifies "endurance ran recently", not "this tree was soaked". `headSha` is queried and never used; and F's own open decision #3 (alternating nights) permits a green `endurance` run that contains **no** soak job. | MED | own text + 5 |
| OB-5 | F14 | Concession went too far. The paper trail was the control that made retry-once safe; the Writer accepted run-history machinery in F9 and rejected cheaper machinery here (`gh issue`, one line, default token). A `flaky` line on a green run's summary is read by nobody, and "a maintainer" is an unnamed owner. | MED | own text (F9 vs F14) + Q2/Q3 |
| OB-6 | F1/F2 | F1 picks a cargo **integration-test** target without naming a profile. Cargo builds the package's bins in the test's profile, so the per-PR gate validates a **debug** binary, not the release binary `build-linux` ships — and the subset includes timing-sensitive scenarios. | MED | 3 (`ci.yml:56-63`) + 5 (cargo) |
| OB-7 | F8 | "A new tag-triggered workflow that **reuses the existing build job bodies**" names no mechanism. Actions has no cross-workflow job-body reuse absent `workflow_call` or a composite action. Same defect class the Writer conceded at F9, left standing at F8. | MED | 3 (`ci.yml:1-63`) + 5 |
| OB-8 | F10 | One fail-closed `release` workflow means a missing **Windows** signing cert blocks the **Linux `.deb`** — P2's actual deliverable — from day one. Also: `sign.ps1` already runs `signtool verify /pa /all` internally, and mandates an HTTPS `-TimestampUrl`, a fourth input F10 does not name. | MED | 3 (`packaging/windows/sign.ps1`) |
| OB-9 | F12 | F12 asserts "G's `packaging/linux/` provides a documented single-command build producing one `.deb`". G's §1 specifies payload, deps, postinst, conffiles and versioning — and **no build command**. F declares a consequential ask on E (F6) but silently invents one on G. | MED | 2 (G `:25-51`) |
| OB-10 | F4 | "B 12 … per-PR runs the degrade assertion only if it is free" is undecidable as written: B's degrade assertion is predicated on "**the container** has no systemd", and F2's per-PR runner is an `ubuntu-22.04` VM that has systemd. The precondition does not hold on the machine F4 proposes to run it on. | MED | 2 (B `:193-196`) + 3 (`ci.yml:12`) |
| OB-11 | F16 | "Watch the deprecation announcement issue in `actions/runner-images`" is a vigilance promise, not a pinning mechanism under FRAME §4.4 (a smoke scenario / a plan-time check). | LOW | frame §4.4 |
| OB-12 | F15 | The negative proofs consume the real `v*` tag namespace (F10's proof requires an actual tag push), and all three proofs are one-time — nothing standing detects the gate rotting after the landing PR. | LOW | own text |

**Counts: HIGH 2 · MED 8 · LOW 2.**

---

## OB-1 — The harness hand-off is circular; A cannot be assigned work (vs F1, HIGH)

**What breaks.** F1's whole structure is "A–E write the scenario bodies into
`crates/kiosk-main/tests/smoke_linux.rs`; F invokes them." Three of the five specs it
assigns that work to have already assigned it to **F**, and one of them is closed:

- **A rev 3, reviewed and binding** (`p2a…:312-315`): "The smoke is **human-run
  in-session** and is deliberately **not** wired into `ci.yml`; **automating the
  compositor harness is P2-F**." A's deliverable is a human procedure. It contains no
  commitment to produce a runnable test target, and per FRAME §0 A's text is authority,
  not a proposal — F cannot retroactively add a deliverable to it.
- **B** `:34`: "**CI automation of the harness → P2-F**"; again at `:236`
  ("update+CI harness automation (P2-F)").
- **C** `:31`: "**Out:** … update/**CI-harness (P2-F)**".

So the documentary record uniformly routes harness automation to F, and F1 routes the
bodies back. The Writer's bridging sentence — "that hand-off is only executable if A's
scenarios exist as something F can invoke" — is correct and is precisely the problem: it
identifies an obligation and then assigns it to a spec that is closed.

**When.** At F2's first run. F2's fast subset is A 1–3, 5, 7 · B 8–11 · D 16 — **five of
ten** scenarios come from A. A ships a checklist; F2 needs code. Same for F5 ("full
A–D matrix"), which additionally needs A 6's "dedicated harness binary (cargo example)"
that the verifier confirmed does not exist (`crates/kiosk-core/examples/` holds only
`kioskctl.rs` — I re-checked).

**Why it matters.** FRAME §2: an obligation no spec in A–G owns is a HIGH defect against
its natural owner; §6 lists "a gate that cannot run" as HIGH; C9 makes the gate part of
the change. F1 was introduced to close exactly this gap and closes it only for the specs
still open to amendment. Note the asymmetry that shows F1 is not uniformly wrong: **E does
own its harness** ("**In:** … **the soak harness** + pass criteria", `p2e…:25`), and D/B/C
are drafts that could adopt bodies in this review round. A is the one that cannot, and A is
the largest share.

**What would answer it.** Either (a) F owns the conversion of A 1–7 into the shared target
and says so — which is what "automating the compositor harness is P2-F" most plainly reads
as, and which W2's honest replacement already half-concedes ("F does own test-harness
code"); or (b) a named amendment to A, which the frame does not permit inside this debate;
or (c) an explicit ledger entry that A 1–7's automation is unowned. (a) is the only one
that leaves the gate runnable.

**Evidence.** Tier 2: `p2a…:312-315`, `p2b…:34,236`, `p2c…:31`, `p2e…:25`. Tier 3:
`crates/kiosk-core/examples/` = `kioskctl.rs` only.

---

## OB-2 — F7's assertions are unobservable with the mechanism F cites (vs F7, HIGH)

**What breaks.** F7 asserts three things: bounded RSS, that a `max_webview_mem_mb` breach
fires a restart, and that nightly reload resets RSS. F's feasibility argument is entirely
"E declares the cross-platform landing that makes this assertable". Checked, three ways,
and it does not:

1. **Wrong process.** E `:63-64`: "`health.rs` gains an RSS sample per existing poll tick
   (`/proc/self/status` `VmRSS` on Linux; `GetProcessMemoryInfo` on Windows)".
   `/proc/**self**` is kiosk-main. Parent §6 `:671` says the sampled field is "**webview
   RSS**". §10's fixture is "a deliberately **leaking page**" — a leaking page grows the
   *renderer* (`WebKitWebProcess` on Linux, `msedgewebview2.exe` on Windows), which is a
   different process from the one E samples. Under E's stated mechanism the cap never
   breaches, so the assertion "a breach fires a restart" cannot pass, and "bounded RSS"
   measures a quantity the fixture does not move. This is C9 / §6 "a gate that cannot run",
   and it is F's to own because F is the party asserting F7 is buildable.
2. **The key F7 names does not exist in its own dependency.** F7 asserts a
   "`max_webview_mem_mb` breach". `maintenance.max_webview_mem_mb` is real *today*
   (`crates/kiosk-core/src/config/schema.rs:234`, default **1500** at `:343`, range
   `{0} ∪ [256,8192]` enforced at `validate.rs:108-112`, and listed `("maintenance.max_webview_mem_mb", "P2")`
   in `UNIMPLEMENTED` at `:19`) — but E does not implement it. E `:65-66` creates a **new**
   key `memory_max_mb`, default **0 = off**, "schema section placed at plan time". F7's
   pass criterion therefore names a key its dependency is replacing, and the substitute's
   name and location are explicitly unfixed. F declared a consequential ask on E for F6's
   duration; it declares none here, where the ask is larger.
3. **`nightly_reload` has no acceleration knob.** F7 says "accelerated `max_webview_mem_mb`
   **and nightly-reload thresholds**". `maintenance.nightly_reload` is an `Option<String>`
   `"HH:MM"` wall clock (`schema.rs:228`), and `maintenance.rs:1-4,29-36` fires it at most
   once per calendar day via `next_fire`, which "always returns a strictly-future instant".
   There is no threshold to accelerate — only a time to set a few minutes ahead, which F
   does not say. And "nightly reload **resets RSS**" is a *page* reload, which does not
   restart the process E measures, so that assertion is also unobservable at self-RSS.

**When.** The first time F7 is implemented — before that, at plan time, when the
implementer discovers the fixture cannot move the measured number.

**Why it matters.** F7 is F's adoption of an uncovered §10 obligation (coverage matrix I8,
UNOWNED). Adopting it is right — that part I credit. But adopting it with a feasibility
claim that does not survive one grep converts an *openly* unowned row into an *apparently*
owned one, which is worse for the ledger (Q3). The minimum fix is one sentence naming the
measured process and one declared ask on E (name the key; sample the webview process, not
`self`).

**Evidence.** Tier 1: parent `:671`, `:870-873`. Tier 2: `p2e…:63-66`. Tier 3:
`crates/kiosk-core/src/config/schema.rs:228,234,343`,
`crates/kiosk-core/src/config/validate.rs:19,108-112`,
`crates/kiosk-main/src/maintenance.rs:1-4,29-36`, `crates/kiosk-main/src/health.rs:1-5`.

---

## OB-3 — The 5 h 15 m re-budget does not fit inside its own timeout (vs F6, MED)

**What breaks.** F6 sets the soak to **5 h 15 m = 315 min** and the job to
`timeout-minutes: 330`, then justifies 330 by: "330 leaves ~30 min for container pull, apt,
build, teardown and artifact upload inside the hard 360." That 30 min is the gap between
330 and 360 — but setup happens **inside** the job clock, not outside it.
`jobs.<id>.timeout-minutes` is "the maximum number of minutes to let a job run before
GitHub automatically cancels it" — it covers container creation and every step. So the
real headroom is 330 − 315 = **15 minutes**, and it must cover: pulling `debian:12`,
`apt-get install` of `libwebkit2gtk-4.1-dev` + the four GStreamer packages + weston,
installing a Rust toolchain (the official `debian:12` image ships no cargo/rustup, and
`Swatinem/rust-cache` is cold on the first containerised run), a build of `kiosk-main`
(Tauri + wry + webkit2gtk), teardown, and the artifact upload.

If instead the 330 is meant as a *step* timeout on the soak, then job total = setup + 330,
which can exceed the platform's hard 360 — the same failure, reintroduced.

**When.** Every scheduled soak run whose setup exceeds 15 minutes, which on a cold
container is the expected case, not the edge case.

**Why it matters.** The Writer's own stated reason for pinning 330 rather than taking the
default is to guarantee the artifact-upload step runs: "the RSS series and spool would be
lost on the one run where they matter most". With setup inside the clock, the job is
cancelled at 330 with the soak still running and the same artifacts lost. The concession
fixed the 6 h wall and re-created a smaller version of it one layer down. Fix is trivial —
budget the soak from a measured setup cost (e.g. soak ≈ 270 min, `timeout-minutes: 330`) —
but it has to be stated, because 5 h 15 m is now the number E is being asked to adopt.

**Evidence.** Tier 5 (Actions `timeout-minutes` semantics, `debian:12` base image
contents); tier 3 (`ci.yml:15-19,49-56` for the apt + toolchain + cache steps a container
job must reproduce). The arithmetic 330 − 315 = 15 is checkable on F's own text.

*Not objected:* that 5 h 15 m discharges PF-05. The parent's word is "multi-hour"
(`:873-875`), the Writer quoted it correctly, and E's pass criteria (`p2e…:99-103`) are
duration-agnostic with the one duration-sensitive number (the RSS delta bound) already
declared as plan-time-calibrated. The ≥72 h obligation is G H5 (`p2g…:96`), verified. That
half of F6 is correct.

---

## OB-4 — "Latest endurance green" does not establish that the tagged tree was tested (vs F9, MED)

**What breaks.** Two separate holes in the replacement mechanism:

1. **The tree is never compared.** The query is
   `gh run list --workflow=endurance --branch=main --status=success --limit=1 --json headSha,createdAt,conclusion`,
   and the rule is "refuse the tag if there is no success, or if the newest success
   predates a freshness window". `headSha` is fetched and then unused. A tag pushed on a
   commit merged ten minutes ago passes on last night's endurance run, which tested a tree
   without the commit being shipped. The gate's stated purpose — "to stop a leaking build
   from shipping to devices that reboot once a month" — is about *this build*, and a
   time-only window cannot express that. The fix is already in the JSON F requests:
   require the endurance `headSha` to be an ancestor of the tagged commit, and require the
   window on top.
2. **A green `endurance` may contain no soak.** F's open decision #3 asks "whether the soak
   job and full-matrix job share the nightly or **split across alternating nights**". With
   F7 the workflow now has three jobs, making a split more likely, not less. If the split
   is implemented as alternating runs of the same workflow, `--status=success` returns a
   run that legitimately never executed the soak — the gate goes green on a workflow run
   that did not perform the check the gate exists to enforce. F9 and open decision #3 are
   mutually incompatible as written and neither references the other.

**When.** (1) on every tag pushed between nightly runs — i.e. normally. (2) if open
decision #3 resolves to "split", which F leaves open.

**Why it matters.** C5 fail-closed is satisfied in form (query error → refuse) but the
positive branch certifies the wrong proposition. Q3: a release that passes a gate which did
not test it is precisely the silent-failure class this project names.

**Not objected:** fail-closed on an API blip. Releases are rare and re-runnable; C5 makes
this the correct trade. `gh` being preinstalled on hosted runners is accepted (tier 5).
The two scheduling caveats the Writer adopted (default-branch-only, 60-day auto-disable in
public repos) are handled correctly by the freshness window — that is a genuinely good
piece of the replacement.

---

## OB-5 — Withdrawing quarantine gave up the control that made retry-once safe (vs F14, MED)

This is the concession I judge went **too far**.

**What breaks.** F's own draft names the hazard: "silent retry-laundering is how gates
rot." Retry-once is the laundering mechanism; the paper trail was the thing that made it
safe. The replacement keeps the laundering and downgrades the trail to "a `flaky` line in
`$GITHUB_STEP_SUMMARY`" on a **passing** run, plus a human rule ("a scenario seen `flaky`
twice inside a week is moved to `endurance` by a maintainer").

Three problems:

1. **Nobody reads a green run's summary.** The signal exists but is not surfaced anywhere a
   reviewer looks: not on the PR checks line, not in a notification, not aggregated. Under
   Q3 the failure mode (a scenario that flakes persistently and is retried into green every
   time) is now silent in practice.
2. **The machinery argument is applied asymmetrically.** F14 rejects cross-run state as "a
   great deal of machinery"; F9 *adopts* cross-run state — a `gh run list --json` query
   over run history — as the right answer to the same class of gap, in the same turn. If
   run-history querying is acceptable machinery for the release gate, it is acceptable for
   the flake ledger. And the cheap version needs no history at all: on pass-after-retry,
   one `gh issue comment` against a standing "flaky smoke" issue, using the default
   `GITHUB_TOKEN` with `issues: write`. That is one line, durable, loud, and searchable —
   strictly less machinery than F9's accepted mechanism, and it *is* the "paper trail,
   never deletion" property the draft claimed.
3. **"A maintainer" is not a named owner.** FRAME §4.5 requires deferrals to name one. No
   maintainer role is defined anywhere in this spec corpus.

**Why it matters.** Q2 says the simplest design that *meets the requirement* wins — the
requirement here is Q3 observability, and the simplified version does not meet it. The
correct simplification is to drop the *automated demotion* (which genuinely needs an actor
and a rolling window) while keeping the *durable, visible record* (which does not).

---

## OB-6 — The per-PR gate validates a debug binary; F1 never names a profile (vs F1/F2, MED)

**What breaks.** F1's chosen shape is an integration-test target
(`crates/kiosk-main/tests/smoke_linux.rs`), invoked as
`cargo test --test smoke_linux -- --ignored`. Cargo builds the package's binaries in the
same profile as the test target and hands them to the test via `CARGO_BIN_EXE_*`; the
default profile is `dev`. So unless F says `--release`, the per-PR gate exercises a
**debug** build of `kiosk-main` — while `build-linux` (`ci.yml:56-63`) already produces,
and the `.deb` will ship, the **release** build. F never states which.

**When.** Every per-PR run, and it bites hardest on exactly the scenarios F2 selects:
D 16 is a short-threshold idle timer, C's FSM windows and A's crash-recovery path are
timing-sensitive, and debug-vs-release changes wall-clock behaviour by an order of
magnitude in this workspace.

**Why it matters.** C9 — the gate is part of the change, and a gate that validates a
different artifact than the one shipped is a weaker gate than the spec claims. It also
interacts with F3: choosing `--release` roughly doubles the build cost inside the 10-minute
budget, so this is a decision F3's budget depends on and F3 does not know it is making.
One sentence fixes it either way, but it has to be a stated choice with its parity
consequence recorded (C3: divergence stated in both directions).

**Evidence.** Tier 3 `ci.yml:56-63`; tier 5 cargo's integration-test/bin-profile behaviour.

---

## OB-7 — "Reuses the existing build job bodies" is a mechanism-free claim (vs F8, MED)

**What breaks.** The revised F8 text is: "a **new** tag-triggered workflow that **reuses
the existing build job bodies** (`ci.yml:30-43`, `:45-63`)". GitHub Actions has no
mechanism by which one workflow file reuses another workflow's job bodies. The options are
`workflow_call` (which requires refactoring `ci.yml`'s build jobs into a reusable workflow
and changing `ci.yml` itself — F claims to change no existing job), a composite action
(the steps must be extracted into `.github/actions/…`), or copy-paste (two apt lines, two
toolchain pins and two build invocations kept in sync by hand — the drift F16 already
worries about for a single label).

**Why it matters.** This is the *same defect class* the Writer just conceded at F9
(verifier #54: "F stated the gate with no mechanism") and at F12 ("F named the tool while
claiming F only executes it"). Applying the standard to F9 and F12 and not to F8 is an
internal inconsistency in the same turn. Q5: an implementer cannot execute F8 without
re-deriving the design decision.

**Not objected:** the correction itself. "Existing" → "new tag-triggered workflow" is
right, and I re-verified `ci.yml:2-5` has `push: branches: [main]` and `pull_request` only,
with no `tags:` filter and no release/checksum/`gh release` machinery anywhere in the repo.

---

## OB-8 — One fail-closed release workflow lets a missing Windows cert block the Linux `.deb` (vs F10, MED)

**What breaks.** F10: "Missing cert secret on a tag push = **job failure**, not a skip".
F8 puts `.deb` + MSI + checksums + draft release in one `release` workflow. P2's shipped
deliverable is the **Linux** `.deb` (§9 P2 row); the Authenticode requirement is a Windows
artifact property. As specified, a repo without a provisioned code-signing certificate —
which is the repo's state today, since P1-F2 says explicitly "the cert itself is not in the
repo" (`p1f2…:41-44`, re-verified) — cannot cut *any* release, including a Linux-only one.

**Why it matters.** The parent's rule is "**unsigned artifacts** fail the release gate",
which scopes to the artifacts that require signing. F's rendering scopes it to the whole
tag. That is stricter than the requirement in a way C3 requires be stated in both
directions, and it makes F15's third meta-proof ("a tag with the cert secret unset must
fail F10's verify step") the *default* state of the release path rather than a negative
test. The fix is per-artifact: the signing gate fails the Windows artifact set and blocks
the draft release from including it, rather than aborting the workflow.

**Two smaller findings in the same change, from reading `packaging/windows/sign.ps1`:**

- F10 proposes "then **verify** every shipped PE/MSI (`signtool verify /pa`)". `sign.ps1`
  already runs `& $signTool verify /pa /all $target` after every sign, and throws on
  non-zero. F10's separate verify step is only non-redundant for artifacts that were *not*
  passed to `sign.ps1` — which is the right thing to say, and F does not say it (Q2).
- `sign.ps1` declares `[Parameter(Mandatory)] [uri]$TimestampUrl` and throws unless the
  scheme is `https`. So the release job needs egress to a timestamp authority as well.
  F10's revised text names three inputs (Microsoft egress, an out-of-band offline
  installer, the cert secret); this is a fourth.
- Also, per `sign.ps1`'s parameter sets, "a repo-secret cert" must be resolved concretely:
  the `Thumbprint` set requires a cert already in `Cert:\CurrentUser\My` or
  `Cert:\LocalMachine\My` (i.e. a pre-import step F does not name); only the `Pfx` set
  works from a secret, and it needs both a PFX file materialised on disk *and*
  `KIOSK_SIGNING_PFX_PASSWORD` in the environment.

**Evidence.** Tier 3: `packaging/windows/sign.ps1` (whole file, read). Tier 2:
`p1f2…:41-44`. Tier 1: parent `:883-884`.

**Credit where due:** adopting F10 rather than deferring it is correct — it is verbatim in
the same §10 row F quotes for its own mandate, and C5 makes it fail-closed. The fork-secret
dismissal also checks out: forks cannot push tags to the upstream repo.

---

## OB-9 — F12 invents an obligation on G without declaring it (vs F12, MED)

**What breaks.** F12's replacement text: "**G's `packaging/linux/` provides a documented
single-command build producing one `.deb`**; F's release job invokes it and uploads the
result". I read G's §1 in full. It specifies payload paths, `Dependencies:`, state dirs,
secrets discipline, autostart, versioning and `Conflicts`/`Replaces` — and **no build
command, no build tool, and no statement that the build is single-command**. G's Testing
section requires "Lintian clean (or documented overrides)", which constrains the *output*,
not the invocation.

**Why it matters.** F6 handles this situation correctly: it names the change E must make
("Consequential ask on E (declared dependency, not a unilateral edit)") and states what
happens if E declines. F12 does the same kind of thing to G and calls it a concession that
narrows F's scope. It does the opposite: it moves a decision F had made (badly, per the
verifier) into a spec that has not agreed to make it, with no declared ask and no fallback.
Q5 — the release job cannot be written until someone commits to the invocation.

**Not objected:** withdrawing `dpkg-deb`. The verifier's #24 is right that G's postinst /
`deb-systemd-invoke` / conffile / lintian content is debhelper-shaped, and F naming the
tool while claiming not to choose it was incoherent.

---

## OB-10 — B 12's per-PR status is undecidable, not merely conditional (vs F4, MED)

**What breaks.** F4's revised exclusion text says B 12 is "degrade-only; B `:193-196`
routes the positive half to the hardware checklist, so per-PR runs the degrade assertion
**only if it is free**, otherwise scheduled". B's degrade assertion is not
environment-neutral — B `:193-196` verbatim: "**the container has no systemd**, so the
smoke asserts only the degrade path (spawn fails → eprintln, kiosk unaffected)". That
precondition is a property of F5's `debian:12` container. F2's per-PR job runs on
`ubuntu-22.04` (`ci.yml:12`), a full VM with systemd present. On that runner
`systemd-inhibit` does not obviously fail, so the degrade path B specifies may not
reproduce at all — the scenario would either pass vacuously or exercise a third,
unspecified behaviour.

**Why it matters.** F4 exists because the draft's exclusion list "reads as a complete
accounting and is not one". The replacement is complete as a *list* — I checked the union
and it is exactly 1–18 with no gaps (A 1–7, B 8–12, C 13–15, D 16–17, E 18) — but it
reintroduces an undetermined membership for B 12 on a criterion ("if it is free") that the
environment does not support. Either B 12 is scheduled-only (container, where its
precondition holds), or F states what the assertion becomes on a systemd-bearing runner.
The former is one word and is what B's own text implies.

**Evidence.** Tier 2 `p2b…:193-196`; tier 3 `ci.yml:12`.

---

## OB-11 — "Watch an upstream issue" is not a pinning mechanism (vs F16, LOW)

FRAME §4.4 admits an assumption when it has "a named pinning mechanism (a smoke scenario,
a plan-time check)". F16 offers "watch the deprecation announcement issue in
`actions/runner-images`" — a standing human vigilance commitment with no owner and no
trigger inside the repo. The available in-repo mechanism is trivial: the `endurance` job
already runs on `debian:12`, which is the platform floor (C7), so per-PR image drift is
bounded by construction — that is the real mitigation and F16 half-states it ("the
migration cost is a re-probe, not a redesign"). Say that, drop the issue-watching.

The declaration itself is otherwise correct; I re-verified `ci.yml:12` and `:46` both pin
`ubuntu-22.04` literally.

---

## OB-12 — Meta-verification consumes the real tag namespace and leaves nothing standing (vs F15, LOW)

Two small things in the added third proof. (1) F10's negative proof ("a tag with the cert
secret unset must fail F10's verify step") requires pushing a real `v*` tag, since that is
the only trigger; F does not reserve a namespace (`v0.0.0-gatecheck`) or say how the
resulting draft release is cleaned up. (2) All three proofs are explicitly "one-time,
documented in the landing PR, not permanent fixtures", so after the landing PR nothing
detects that the smoke job has stopped failing on a broken scenario — which is the same
rot F14 worries about, one level up. Cheapest standing check: keep the deliberately-broken
fixture as a scenario that asserts its own red, rather than as a one-time run.

---

## Clean passes

- **F3 — the 10-minute rule.** No material objection. The rule is self-pinning ("under 10
  minutes **or the subset shrinks**") and open decision #1 measures it on the first working
  version; the verifier graded it adequately pinned and I agree. My cost concern lives at
  OB-6 (profile choice), which is F1's to answer, not F3's.
- **F5 — `debian:12` full matrix.** No material objection. `jobs.<id>.container` is
  standard, the real risk (seat/device nodes for weston in a container) is declared as open
  decision #4 with a probe run named — the model the frame asks for. One free addition for
  the probe list, offered not as an objection: `debian:12` ships no Rust toolchain, so the
  probe should cover toolchain install and cache-cold cost, which is also OB-3's input.
- **F11 — RT-09 live token exchange.** No material objection, and it is better-founded than
  the Writer claims. I verified: `grep -rn "RT-09" crates/` returns **zero** hits, and the
  only `#[ignore]` in the workspace is `crates/kiosk-core/src/config/signature.rs:204`, so
  the live half genuinely does not exist and the `#[ignore]` + `-- --ignored` precedent is
  real. I also checked the *other* half of §10's RT-09 sentence and it is already
  discharged in P1 — `crates/kiosk-core/src/logging/auth.rs:531`
  `jwt_claims_match_googles_server_to_server_contract`, asserting `claims.scope ==
  LOGGING_WRITE_SCOPE` (`:542-546`) and `aud` = `token_uri` (`:345`) — so F11 correctly
  adopts only the live half and does not duplicate existing coverage. "Skips when creds
  absent" is the parent's own word (`:876-878`). Adopting rather than deferring is right.
- **F13 — update-path parity.** No material objection; the citation-direction fix is
  correct and the fact does live in G's runbook (`p2g…:72`).
- **F4's exhaustiveness** (as distinct from OB-10). I checked the union of the include and
  exclude lists against the coverage verifier's registry: A 1–7 · B 8–12 · C 13–15 ·
  D 16–17 · E 18, eighteen scenarios, no gaps, no double-listing. The concession does what
  it claims.
- **The rebuttal of verifier #14.** Upheld. F's sentence is conditional ("After C lands"),
  `rt13.rs:27` is the only `cfg(` in the file, and `ci.yml:26` is an unscoped
  `cargo test --workspace`, so the existing line picks up the un-gated target with no
  `ci.yml` edit. The verifier's #14 attaches to a present-tense implication F does not make.
- **W1, W2, W3, W5, W6, W7, W8.** All correct withdrawals. W2 in particular — the honest
  version ("F changes no `crates/*/src/`; F does own test-harness code") is what made OB-1
  visible at all, and I credit it even though I am arguing the replacement does not go far
  enough. **W4 is the one I contest** (OB-5).
