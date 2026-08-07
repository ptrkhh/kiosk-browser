# P2-F — WRITER, Round 1 (opening)

No frame dispute.

Everything below was re-verified by me against the repo at `1decd59` and, for the one
tier-5 claim that decides a HIGH finding, against upstream docs directly. Citations are
mine, not the verifier's, except where I say "verifier's cite, confirmed".

## Change register

| ID | Change | Requirement discharged | Depends on |
|---|---|---|---|
| F1 | **Harness invocation contract** — one env-gated cargo test target per platform; A–E write scenario bodies into it, F invokes it. NEW. | parent §10 "Linux compile check (P0 → **functional at P2**)" — the thing that makes "functional" invocable | A (fixture server, weston launch, scen. 1–7), B (8–12), C (13–15), D (16–17), E (18 + leaking page). **No F change is executable without this.** |
| F2 | Per-PR job `smoke-linux` on `ubuntu-22.04`, fast subset A 1–3,5,7 · B 8–11 · D 16 | §10 functional-at-P2 | F1; A, B, D |
| F3 | Wall-clock rule: **under 10 min or the subset shrinks** | §10 per-PR/scheduled split (Q5 reviewability) | F2 |
| F4 | Per-PR exclusion list, made **exhaustive**: A 4, **A 6**, C 13–15, **B 12**, **D 17**, E 18 | §10 "soak/endurance scheduled, not per-PR" | F2, F5 |
| F5 | Scheduled `endurance` (a): full A–D matrix in a `debian:12` container | §10 scheduled rows; C7 platform floor | F1; A–D |
| F6 | Scheduled `endurance` (b): offline-video soak, **re-budgeted 8 h+ → 5 h 15 m** with `timeout-minutes: 330` | §10 "**multi-hour** loop on the pinned Debian 12 image … no stall/black frame across loop boundaries (PF-05)" | F1, E (E must amend its "8 h+"); ≥72 h stays G H5 / RT-05 |
| F7 | Scheduled `endurance` (c): **Windows-runner leaking-page soak** on `windows-latest`, accelerated thresholds. NEW — adopts §10. | §10 "asserts bounded RSS, that a `max_webview_mem_mb` breach fires a restart, and that nightly reload resets RSS"; §9 P2 row "memory cap restart + health-sampled RSS" | E §3 (memory-cap + RSS land cross-platform, E's own declared exception); P1 nightly reload; F1 |
| F8 | `release` on tag `v*`: **new** tag-triggered workflow reusing the existing build job bodies; `.deb` + MSI + checksums + draft release | §9 P1/P2 deployable rows; §10 release gate | G `packaging/linux/`; P1-F2 `packaging/windows/` |
| F9 | Endurance-green gate, **with its mechanism**: `gh run list --workflow=endurance --branch=main --status=success` in the tag job | §10 soak-as-pre-release-gate intent | F5, F6, F7, F8 |
| F10 | **Authenticode signing + verify step; unsigned artifacts fail the release gate.** NEW — adopts §10. | §10 CI row verbatim; C5 fail-closed | P1-F2 `sign.ps1` + WiX; operator cert secret |
| F11 | **RT-09 live token-exchange release gate**, `--ignored` test + creds-present job. NEW — adopts §10. | §10 "Live token-exchange smoke (gated/opt-in, **release gate**) … skipped when creds absent (RT-09)" | `kiosk-core/src/logging/{auth,client,transport}.rs` (exist); throwaway-SA secret |
| F12 | `.deb` assembly = **invocation contract**, build tool is G's choice (was: `dpkg-deb`) | §9 P2 ".deb"; C1/Q1 ownership | G |
| F13 | Update path = install-the-new-`.deb`, parity with Windows' install-the-new-MSI | §9 P1/P2 (no auto-updater on either platform) | G runbook |
| F14 | Flake policy: retry-once + `flaky` line in `$GITHUB_STEP_SUMMARY`; **automated 7-day quarantine withdrawn**, quarantine becomes a human decision | Q3 observability-of-failure | F2 |
| F15 | F's meta-verification: deliberately-broken fixture must red the smoke job; a red endurance must refuse a tag | C9 "merge gates are real" | F2, F9 |
| F16 | Runner-image pin declared as an assumption + retirement watch | §4.4 evidenced/pinned | — |

---

## F1 — Harness invocation contract (new, and the load-bearing one)

**Proposal.** F specifies *the shape and the invocation*, A–E supply the bodies:

- One target per platform: `crates/kiosk-main/tests/smoke_linux.rs`, and for F7
  `tests/soak_windows.rs`. Integration-test targets, not shell scripts, not a bespoke
  runner.
- Gated the way this repo already gates operator-only tests: `#[ignore]` + run with
  `-- --ignored`. Precedent in-tree: `crates/kiosk-core/src/config/signature.rs:203-204`
  (`#[test] #[ignore] fn print_signing_keypair`, with the invocation documented at
  `:200` as `cargo test -p kiosk-core print_signing_keypair -- --ignored --nocapture`).
  So `cargo test --workspace` on a normal PR is unaffected — C8 preserved, no new job
  shape, no new dependency (C6).
- Fixture HTTP server: `std::net::TcpListener` inside the harness target. **Not**
  `python3 -m http.server` — the `debian:12` container has no python3, and adding one is
  a container dep for ~40 lines of stdlib. Owner: A (it is A's fixture; A `:290` already
  says "local HTTP server serving a **signed** config via the P1 `kioskctl` signing
  harness", and `crates/kiosk-core/examples/kioskctl.rs` is real).
- Spool assertions: read the on-disk spool directly, as A `:291` already specifies
  ("telemetry asserted from the **on-disk spool** … no fake-GCL endpoint needed"). No
  helper crate. `rt13.rs:14-16`'s refusal of spool assertions is scoped to *that* test
  ("would need a service account, a transport and the whole telemetry stack for no extra
  coverage of the supervise behaviour") — an in-process FSM test, not an out-of-process
  app run. It is not precedent against A's on-disk approach.
- Compositor launch: F owns the workflow step that starts weston headless and exports
  `WAYLAND_DISPLAY`; the tests assume it. Backend-flag spelling
  (`--backend=headless-backend.so` vs `--backend=headless`) differs across weston
  generations — **declared assumption**, pinned by the probe run already in F's open
  decisions, which I extend to cover the flag spelling on both `ubuntu-22.04` and
  `debian:12`.

**Requirement.** parent §10 "Linux compile check (P0 → **functional at P2**)". Verified
verbatim at `docs/superpowers/specs/2026-07-05-kiosk-browser-design.md:883`.

**Evidence.** Tier 3, verified by me:
- `grep -rn "weston\|cage" --include=*.rs --include=*.yml --include=*.sh --include=*.toml`
  across the repo → **zero hits**.
- `find` for `*.sh`, `*.py`, `Makefile`, `justfile`, `*.service` (excluding `target/`) →
  **empty**. The repo contains no shell scripts.
- `crates/kiosk-core/examples/` contains exactly one file, `kioskctl.rs`. A 6's
  "dedicated harness binary (cargo example)" (`p2a…:304-306`) does not exist.
- `.github/workflows/` contains exactly one file, `ci.yml` (63 lines).
- `git log --oneline -8`: the last seven commits are all `docs(spec):`. No P2
  implementation has landed.

**Position on ownership.** Building the harness is **not** F, and it is **not** an
unowned gap. It is A–E's, per scenario, and the reason it looked unowned is that F never
named the artifact they must produce. A's gate line (`p2a…:312-315`) says the smoke is
"human-run in-session and is deliberately **not** wired into `ci.yml`; automating the
compositor harness is P2-F" — that hand-off is only executable if A's scenarios exist as
something F can invoke. F1 closes it by naming the artifact. The dependency edge is:

> A–E each land their scenarios as `#[ignore]`d tests in the shared target → F's
> workflow runs `cargo test --test smoke_linux -- --ignored`.

F remains true to "F writes no **product** code" only in the narrowed sense stated in
the withdrawals below.

**Dependencies.** A (blocking, largest share), B, C, D, E. F2/F4/F5/F6/F7/F15 all depend
on F1. This is F's whole dependency risk and it is now on one line.

---

## F2 — Per-PR `smoke-linux`

**Proposal.** Unchanged from the draft: `ubuntu-22.04`, plus `weston`, the four GStreamer
packages (E §5), on top of the webkit 4.1 dev deps that job already installs. Fast subset
A 1–3, 5, 7 · B 8–11 · D 16. Failure artifacts (spool, weston log, best-effort
screenshots) uploaded on failure.

**Requirement.** §10 functional-at-P2 (`:883`).

**Evidence.** Tier 3, verified by me against `.github/workflows/ci.yml`: `lint-test` at
`:11`, `runs-on: ubuntu-22.04` at `:12`, apt line at `:15-19` installing
`libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev libssl-dev`
— no weston, no cage, no GStreamer, so those are genuinely additions. Scenario numbers
re-checked by me against each owning spec: A `:292-311` (1 boot, 2 nav, 3 offline, 5
iframe, 7 safe boot), B `:174-194` (8 egress, 9 downloads, 10 dialog, 11 permissions),
D `:117-121` (16 idle→clear, "short-threshold fixture"). All match.

**Dependencies.** F1; A, B, D.

---

## F3 — The 10-minute rule

Unchanged, and I accept the verifier's finding that it is UNVERIFIABLE-but-adequately-pinned:
the rule is self-pinning ("under 10 minutes **or the subset shrinks**") and open decision
#1 measures it on the first working version. Nothing to concede.

---

## F4 — Exclusion list, made exhaustive (concession)

**Conceded**: the draft's exclusion sentence reads as a complete accounting and is not
one. Revised text:

> **Excluded from per-PR, by design:** A 4 (crash-kill, timing-flaky candidate);
> **A 6** (superseded per-PR by D 16 — D `:113-114` verbatim: "A's harness-binary
> scenario (A smoke 6) stays as the completion unit check; D's smoke 16 supersedes it as
> the app-path proof" — A 6 runs in `endurance` as the unit check); **B 12**
> (degrade-only; B `:193-196` routes the positive half to the hardware checklist, so
> per-PR runs the degrade assertion only if it is free, otherwise scheduled);
> C 13–15; **D 17** (blocking *only if* cage exposes wlr virtual input headless — D
> `:122-129`; until the plan-time probe answers that, scheduled, and if the probe says no
> it moves to G's H4, not to per-PR); E 18 (soak is never per-PR — E `:111` agrees).

**On the C 13–15 rationale (verifier's undeclared-assumption #7).** I do not rebut. I
**declare it as an assumption and narrow the claim**: RT-13 covers the hang→restart and
exit-86 FSM paths per-PR (its four tests, verified by me at `rt13.rs:291,324,359,384`).
It does **not** cover C13's cage chain, C15's zombie-reap assertion, or C14's app-path
pinpad driver (assigned by D `:137`). Revised wording drops "covers the same FSM paths"
and says: "RT-13 gives per-PR coverage of hang→restart and exit-86 at the FSM level; the
cage-chain, zombie-reap and pinpad-driver assertions are scheduled-only and are named as
such." Residual risk: a cage-chain regression is caught nightly, not per-PR. Accepted —
per-PR cost of a full cage chain is exactly what F3 exists to protect.

---

## F5 — `endurance` (a): full matrix in `debian:12`

Unchanged. Capability confirmed (`jobs.<id>.container` is standard); the real risk —
seat/device nodes for weston inside a container — is already F's open decision #4 with
"pin with a probe run before building the whole job on it", which I keep and extend per
F1 (weston flag spelling).

---

## F6 — The soak, re-budgeted (concession, the HIGH one)

**Conceded in full.** I verified the cap myself against
`https://docs.github.com/en/actions/reference/limits`:

> GitHub-hosted runners: "Each job in a workflow can run for up to 6 hours of execution
> time. If a job reaches this limit, the job is terminated and fails." **This limit
> cannot be increased.**
> Self-hosted runners: 5 days, also not increasable.

Every runner F names is hosted; F never mentions self-hosted. An "8 h+" job on
`debian:12`-in-`ubuntu-22.04` is terminated at 6 h and fails. That is FRAME §3 C9 /
§6 HIGH, and it is F's to own — E `:98-99` explicitly assigns the wiring to F.

**Revision.** Scheduled soak runs **5 h 15 m**, with `timeout-minutes: 330` set
explicitly on the job. Two reasons for the explicit value: the job-level default is 360
minutes, i.e. exactly the platform cap, so a job that overruns is killed by the platform
*before* the artifact-upload step runs — the RSS series and spool would be lost on the
one run where they matter most. 330 leaves ~30 min for container pull, apt, build,
teardown and artifact upload inside the hard 360.

**Rejected alternatives, briefly.** Self-hosted runner (5-day cap) buys the 8 h but costs
a machine to own, patch and secure, plus a Wayland-capable host — for a duration nobody
required. Split/resumable soak across chained jobs needs RSS-series and loop-count state
carried between jobs and a stitching step: more machinery than the requirement asks for
(Q2).

**This still satisfies §10.** The parent's own words for *this* soak, verified verbatim
at `:873-875`:

> **Offline-video soak:** multi-hour loop on the pinned Debian 12 image, assert no
> stall/black frame across loop boundaries (PF-05).

"Multi-hour" — not eight. 5 h 15 m is multi-hour. E's pass criteria (`p2e…:99-103`: zero
`media.error`, process alive, no launcher restarts, RSS delta under a declared bound,
loop count consistent with wall-clock) are all duration-agnostic; only the RSS-delta
bound is calibrated against the window, and E already picks that number "at plan time
from first-run baseline, then pinned".

**Obligation that legitimately moves.** The long-duration half of the intent —
RT-05's "**A ≥72 h real-hardware soak is a pre-release gate**" (`:873`) — was never CI's
in the first place: the parent puts it on real hardware, and G already owns it as
checklist row **H5**, verified at `p2g…:96`: "≥72 h offline-video soak, RSS trend, loop
count; visual black-frame check | E / RT-05". Nothing moves that wasn't already there.
The only thing I am moving is E's self-chosen "8 h+", which has no parent authority.

**Consequential ask on E** (declared dependency, not a unilateral edit): E `:98-99`
should read "**scheduled CI** ≥5 h (wired by F …)". If E declines, F's job is still
capped at 5 h 15 m and the divergence is F's to document — but then it is a stated
divergence, not a broken gate.

---

## F7 — Windows-runner leaking-page soak (new — adopts an uncovered §10 obligation)

**Conceded that it was uncovered.** F's `endurance` had two jobs, both Linux, and F's
defer list named only package contents, fleet update mechanics and Android. The Windows
soak was neither covered nor deferred. **I adopt it rather than defer it**, for three
reasons: (1) it is in the same §10 block F quotes for its mandate; (2) its subject —
memory-cap restart + health-sampled RSS — is a **§9 P2-row** deliverable, verified
verbatim at `:837`-adjacent P2 row, so it cannot be pushed to P3; (3) it is cheap, because
§10 specifies **accelerated thresholds**, which makes it a minutes-scale job, nowhere
near any cap.

**Proposal.** Third job in `endurance`, `runs-on: windows-latest`: drives looped
navigation plus a deliberately leaking page with accelerated `max_webview_mem_mb` and
nightly-reload thresholds; asserts bounded RSS, that the breach fires a restart, and that
nightly reload resets RSS.

**Evidence / feasibility.** E declares the cross-platform landing that makes this
assertable, verified by me at `p2e…:33-36`: "**Two deliberate cross-platform changes,
flagged as such** … the `media.error` bridge and the RSS/memory-cap feature land on
Windows too", and `p2e…:61-71` (`GetProcessMemoryInfo` on Windows, default `0 = off`).
No compositor is involved, so F1's harness cost on this side is small.

**Ownership edge.** F owns the job; **E owns the leaking-page fixture and the three
assertions** — they are assertions about E's feature, and E already owns the equivalent
Linux ones. If E declines, this becomes an unowned §10 item again and I would rather that
be visible in the ledger than buried.

---

## F8 — `release` on tag (wording concession)

**Conceded (verifier #17).** "Tag push `v*`: **existing** release builds" is wrong as
written. I verified: `ci.yml:2-5` triggers are `push: branches: [main]` and
`pull_request` — no `tags:` filter; and no `release` workflow, checksum step, draft-release
step, or `gh release`/`softprops` usage exists anywhere in the repo. Revised: "a **new**
tag-triggered workflow that **reuses the existing build job bodies** (`ci.yml:30-43`,
`:45-63`) — the job bodies are existing, the release machinery is not."

---

## F9 — Endurance-green gate, with a mechanism (concession)

**Conceded (verifier #54).** Actions has no native cross-workflow status gate;
`workflow_run` fires on the dependent workflow, not on a tag push. F stated the gate with
no mechanism. Revised: the tag job's first step queries run history and fails closed —

```
gh run list --workflow=endurance --branch=main --status=success --limit=1 \
  --json headSha,createdAt,conclusion
```

— and refuses the tag if there is no success, or if the newest success predates a
freshness window (window value at plan time). `gh` is preinstalled on GitHub-hosted
runners; no new dependency. Fail-closed on a query error, per C5.

Two scheduling caveats the verifier raised that I adopt into the spec text rather than
argue with: scheduled workflows run only on the default branch, and in a public repo
scheduled workflows auto-disable after 60 days of inactivity. Both would make the gate
silently *unavailable* rather than red, so the freshness window above is what converts
"endurance stopped running" into a refused tag instead of a green one.

---

## F10 — Authenticode signing gate (new — adopts an uncovered §10 obligation)

**Conceded that it was uncovered.** The §10 CI row F quotes for its own mandate ends,
verified verbatim at `:883-884`: "Android build (P3), **Authenticode signing step
(unsigned artifacts fail the release gate)**." F's release job listed exactly one gate
(endurance-green). Adopting, not deferring: it is in F's own quoted row, C5 makes it
fail-closed, and P1-F2 already supplied the invocation — verified at
`packaging/windows/sign.ps1` (exists) and F2 spec `:41-44`: "a build/**CI** step
`signtool sign`s both PE binaries **and** the MSI … F2 provides the invocation + docs;
the cert itself is not in the repo."

**Proposal.** In `release`: sign both PEs and the MSI via `sign.ps1` with a repo-secret
cert; then **verify** every shipped PE/MSI (`signtool verify /pa`) and fail the job on
any unsigned artifact. Missing cert secret on a tag push = job failure, not a skip —
that is what "unsigned artifacts fail the release gate" means. Forked PRs never reach
this workflow (tag-triggered, default branch), so the fork-secret problem does not arise.

**Also conceded (verifier #20, undeclared assumption #3).** "Wiring is the same one-job
shape … included here for symmetry, not new design" is wrong. I verified
`packaging/windows/README.md`: the bundle build downloads Microsoft's Evergreen WebView2
bootstrapper when absent and **verifies its Authenticode signature is valid and issued to
Microsoft Corporation**; offline releases need `-p:WebView2InstallerPath=…` pointing at a
manually-downloaded standalone installer that is deliberately not committed; and F2
requires an operator-supplied signing cert. Three things no existing job does. Revised
text drops "same one-job shape" and names all three as inputs the release job must be
given (network egress to Microsoft, an out-of-band installer source for offline builds,
and the cert secret).

---

## F11 — RT-09 live token-exchange release gate (new — adopts an uncovered §10 obligation)

**Conceded that it was uncovered.** Verified verbatim at `:876-878`: "**Live
token-exchange smoke (gated/opt-in, release gate):** a real RS256 → oauth2 token exchange
+ one `entries:write` against a throwaway service account; skipped when creds absent
(RT-09)."

I also checked whether a test body exists to wire: `grep -rn '#\[ignore'` across
`crates/` returns exactly one hit, `signature.rs:204` (the keypair printer) — so **no
live-smoke test exists**, and no P1 or P2 spec claims one: `grep -rn "RT-09"` across all
specs returns three hits, all inside the parent. It is unowned parent debt sitting in a
row F is the natural owner of.

**Proposal.** F adopts both halves rather than leave it unowned: a
`crates/kiosk-core/tests/live_token_exchange.rs` `#[ignore]`d test (same pattern as
`signature.rs:203-204`) against the existing client at
`crates/kiosk-core/src/logging/{auth,client,transport}.rs`, plus a release-job step that
runs it with `-- --ignored` when the throwaway-SA secret is present and **skips when
absent** — the parent's own wording, so absence is a skip here, unlike F10 where the
parent says fail. If the Moderator rules that a test body is out of F's scope, the
alternative is a named owner, and there is no candidate in A–G; I would rather F carry
~40 lines than record a second unowned §10 row.

---

## F12 — `.deb` assembly (concession)

**Conceded (verifier #24).** F named `dpkg-deb` while claiming "F only executes it" —
those are inconsistent, and G's stated content is debhelper-shaped: verified at
`p2g…:44` and `:48` (postinst creating `/var/lib/kiosk`; `systemctl enable` +
`deb-systemd-invoke` conventions), `:29` (conffile-adjacent video), `:50`
(`Conflicts`/`Replaces`), `:108-112` ("**Lintian clean** (or documented overrides)").
Revised: F specifies only the **invocation contract** — "G's `packaging/linux/` provides
a documented single-command build producing one `.deb`; F's release job invokes it and
uploads the result" — and the build tool is G's decision, alongside the postinst and
lintian requirements that drive it.

---

## F13 — Update path parity

Unchanged. The circular-citation note (F §4 cites G, G `:72` cites "F §4") is real but
the fact lives in G's runbook, so it is provenance polish, LOW. I will drop F's "The G
runbook pins `unattended-upgrades` off" to a plain cross-reference so the loop has one
direction.

---

## F14 — Flake policy, quarantine de-automated (concession)

**Conceded (verifier #55).** "Two flakes of the same scenario in seven days → the
scenario moves to `endurance` and a tracking issue is opened" assumes durable cross-run
state over a rolling window plus an actor to perform the move. Actions provides neither.
Building it means run-history scraping or an external store, plus a bot with write
access — a great deal of machinery to automate a judgement call made a few times a year
(Q2).

**Revised.** Keep the two mechanisms that are native and cheap: retry-once within the
job, and a `flaky` line in `$GITHUB_STEP_SUMMARY` on pass-after-retry (verified
capability). Quarantine becomes an explicit **human** rule in the spec: "a scenario
seen `flaky` twice inside a week is moved to `endurance` by a maintainer, with a tracking
issue; the `flaky` summary lines are the evidence." Runner-environment failures still
fail distinctly from scenario failures. This keeps the anti-retry-laundering property
(Q3) — the flake is loud in every run's summary — and drops only the automation.

---

## F15 — Meta-verification

Unchanged, and it is what C9 demands of a spec whose product is gates: the landing PR
records one deliberately-broken-fixture run that reds `smoke-linux`, and one refused tag
against a red endurance. With F9 now naming a mechanism, the second proof is executable
rather than aspirational. Add a third, cheap one: a tag with the cert secret unset must
fail F10's verify step.

---

## F16 — Runner-image assumption (declared)

**Declared as an assumption** (verifier's fourth UNVERIFIABLE, the unpinned one).
`ubuntu-22.04` is a valid, currently-undeprecated label — I confirmed both `ci.yml:12`
and `:46` pin it literally. Whether it survives P2 depends on Ubuntu 26.04's GA date,
which is unannounced. **Pinning mechanism:** watch the deprecation announcement issue in
`actions/runner-images`, added to F's open decisions. **Residual risk:** a forced move to
`ubuntu-24.04` changes the weston and GStreamer versions under the smoke job — which is
the same probe F1/open-decision-#4 already runs, so the migration cost is a re-probe, not
a redesign.

---

## Response to the verification record

**8 FALSE** — 7 conceded, 1 rebutted.

| # | Finding | Disposition |
|---|---|---|
| 14 | `cargo test` does not run RT-13 on ubuntu today | **REBUT.** F does not claim it does. F's sentence, verbatim: "**After C lands**, `cargo test` already includes RT-13". I verified both halves: `rt13.rs:27` is `#![cfg(windows)]` (the only `cfg(` in the file, so the whole target is empty on Linux today), and `ci.yml:26` is `cargo test --workspace` — unscoped, no `-p`, no `--test` filter — so once C removes the gate the existing line picks up the auto-discovered target with no `ci.yml` edit. The verifier's own #15 grades exactly this VERIFIED. #14 attaches to a present-tense implication F's text does not make. |
| 31 | Exclusion list not exhaustive (A 6, B 12, D 17) | **CONCEDE** → F4 |
| 33 | No fixture HTTP server | **CONCEDE** → F1 (std `TcpListener`, owner A, no python) |
| 34 | No weston/cage invocation anywhere | **CONCEDE** → F1. Re-verified: zero grep hits, and no shell scripts exist at all. |
| 35 | No spool-assertion helpers | **CONCEDE** → F1, with the narrowing that `rt13.rs:14-16` declines spool assertions for an in-process FSM test, which is not precedent against A's on-disk approach for out-of-process app runs. |
| 36 | A 6's harness binary absent | **CONCEDE** → F1. Verified: `crates/kiosk-core/examples/` holds only `kioskctl.rs`. |
| 37 | There is no harness to promote | **CONCEDE, headline** → F1 + withdrawal W1/W2. |
| 51 | 8 h+ soak exceeds the 6 h hosted cap | **CONCEDE, HIGH** → F6. Cap re-verified by me against GitHub's limits reference, including "cannot be increased". |

**4 DRIFT** — 3 conceded, 1 not mine.

| # | Finding | Disposition |
|---|---|---|
| 16 | C cites `rt13.rs:32`; actual `:27` | **Not an F claim.** I confirm `:27` is correct. Recorded for C. |
| 17 | "existing release builds" | **CONCEDE** → F8 |
| 24 | `dpkg-deb` vs G's debhelper-shaped content | **CONCEDE** → F12 |
| 48 | "assert bounded RSS" misattributed | **CONCEDE, and it matters.** Verified at `:870-873`: "asserts bounded RSS" belongs to the **Windows-runner** soak sentence. The sentence F is actually implementing is "**Offline-video soak:** multi-hour loop on the pinned Debian 12 image, assert **no stall/black frame across loop boundaries** (PF-05)" (`:873-875`). Corrected traceability: F6 cites the offline-video sentence and PF-05; the "asserts bounded RSS" phrase moves to **F7**, where it belongs. The verifier is right that the misquote is what hid the missing Windows job — F6's RSS-series artifact is still worth keeping as trend data, but it is now justified as trend data, not as discharging a §10 assertion. |

**4 UNVERIFIABLE** — three (10-min budget, retention sizes, container/weston probe) are
already self-pinned by F and I keep them as-is; the fourth (runner-image longevity) is
now declared and pinned at F16.

**8 undeclared assumptions** — dispositions: #1 harness → conceded, F1. #2 8 h → conceded,
F6. #3 MSI one-job shape → conceded, F10. #4 cross-workflow gate → conceded with
mechanism, F9. #5 flake window → conceded and de-automated, F14. #6 `dpkg-deb` →
conceded, F12. #7 C 13–15 rationale → **declared as assumption**, claim narrowed, residual
risk stated, F4. #8 `ubuntu-22.04` → **declared as assumption**, pinned, F16.

**3 UNCOVERED §10 items** — all three **adopted**, none deferred: Authenticode gate (F10),
Windows-runner soak (F7), RT-09 live token exchange (F11). None of the three has a parent
deferral to lean on — I checked; the parent defers Android to P3 and nothing else in this
block — so deferring them would have needed a named owner, and there is no candidate in
A–G for any of them. F is the release-and-CI spec; they are F's.

---

## Withdrawals / restructuring

- **W1 — "it promotes the harness from human-run-in-session to CI-run" (header).**
  Withdrawn. No such harness exists in any form. Replaced by F1's invocation contract:
  F *specifies and invokes*; A–E *write*.
- **W2 — "F writes no product code."** Withdrawn as stated, because it was doing rhetorical
  work it had not earned. Replaced with: "F changes no `crates/*/src/`. F does own test-harness
  code — the invocation contract (F1), and RT-09's gated test (F11) — and names A–E as the
  owners of every scenario body." This is the honest version; the original sentence is what
  let F's dependency surface stay invisible.
- **W3 — "8 h+".** Withdrawn (F6). 5 h 15 m with an explicit `timeout-minutes: 330`.
- **W4 — the automated two-flakes-in-seven-days quarantine.** Withdrawn (F14). Retry-once
  and the `flaky` summary line survive; the quarantine trigger becomes a documented human
  rule.
- **W5 — "dpkg-deb".** Withdrawn (F12). Tool choice is G's; F keeps only the invocation
  contract.
- **W6 — "existing release builds".** Withdrawn (F8): the job bodies are existing, the
  release machinery is entirely new.
- **W7 — the §10 "assert bounded RSS" citation on the Linux soak.** Withdrawn (F6/F48
  correction); the phrase now appears only under F7.
- **W8 — "wiring is the same one-job shape … not new design" (Windows MSI).** Withdrawn
  (F10); three concrete inputs named instead.

**Restructuring.** `endurance` goes from two jobs to three (Linux full matrix, Linux
video soak, Windows leaking-page soak). `release` goes from one gate to three
(endurance-green with a named query mechanism, Authenticode-verified, RT-09 when creds
present). F gains one section it did not have — F1, the harness invocation contract —
and that section is now the first dependency every other F change declares.

**Net.** 16 changes: 11 substantially as drafted (7 unchanged, 4 with corrections),
4 new adoptions of uncovered §10 obligations (F1 counts as the fifth structural
addition), 8 withdrawals.
