# P2-F — CI Functional Gate, Scheduled Endurance, Release Artifacts (Design)

> Sixth sub-project of P2. Parent spec of record:
> `docs/superpowers/specs/2026-07-05-kiosk-browser-design.md` (rev 2), §10 (CI matrix —
> "Linux compile check (P0 → **functional at P2**)" at `:882-884`; the soak/endurance rows at
> `:870-875`; the live token-exchange release gate at `:876-878`). **Builds the smoke harness
> A–E specify** and wires it, plus artifact and release plumbing, into GitHub Actions.
> P2-A hands this over explicitly (`p2a:312-315`: *"the smoke is human-run in-session and is
> deliberately not wired into `ci.yml`; automating the compositor harness is P2-F"*), and
> P2-B (`p2b:34`, `:236`) and P2-C (`p2c:31`) route the same work here.

**Status:** rev 2, 2026-08-07 — adversarial design review; see
`docs/superpowers/reviews/2026-08-07-p2b-p2g-adversarial-review/`.

## Goal

Every PR gets the Linux functional gate §10 promised at P2 — the real release binary under a
real compositor with real signed config, in under ten minutes. Long-running validation (full
matrix, video soak, Windows leak soak) runs nightly on a target-faithful image. Tags produce
signed, lintian-clean, endurance-gated artifacts for both platforms. Merge gates: the per-PR
`smoke-linux` job green on its ten scenarios, one recorded run of a deliberately-broken
fixture reddening it, and one `workflow_dispatch` dry-run showing the release path refusing a
tag against a red endurance.

## Scope

**In:** the smoke harness itself (`crates/kiosk-smoke`) — compositor bring-up, fixture HTTP
server, spool reader, and the scenario **bodies** for A 1–7 · B 8–12 · C 13–15 · D 16–17,
including A 6's harness binary; `.github/workflows/{build,endurance,release}.yml` and the
edit to `ci.yml` that calls the reusable build; the RT-09 live-token test body; the flake
policy.

**Out:** what each scenario *asserts* and its fixture parameters (A, B, C, D, E own those —
see F-CITE); scenario 18 / 18-W1 / 18-W2 bodies and the soak pass criteria (**P2-E**,
`p2e:25`); `.deb` **content**, the build tree, the runbook, image pinning and the hardware
checklist (**P2-G**); Android CI rows (parent defers to P3).

**Withdrawn, recorded — "F writes no product code."** The draft's header claim was doing
rhetorical work it had not earned, and it hid F's entire dependency surface. Four sibling
texts route harness automation to F (`p2a:312-315`, binding and unamendable; `p2b:34,236`;
`p2c:31`), while E alone keeps its own harness (`p2e:25`). The honest statement is: **F
changes no `crates/*/src/`; F owns the smoke-harness code and every scenario body except
E's.** The intermediate softening ("F does own test-harness code") is withdrawn with it.

**Also withdrawn, recorded — "it promotes the harness from human-run-in-session to CI-run."**
There is nothing to promote. Verified at the tree: `grep -rn "weston\|cage"` over `*.rs`,
`*.yml`, `*.sh`, `*.toml` → **zero hits**; no shell scripts, no `Makefile`, no `justfile`
exist anywhere; `crates/kiosk-core/examples/` holds exactly one file, `kioskctl.rs`, so A 6's
"dedicated harness binary (cargo example)" (`p2a:304-306`) does not exist;
`.github/workflows/` holds exactly one file, `ci.yml` (63 lines).

**Change register:** F1–F16. Cross-spec edges are tabulated at the end; every one is declared
in both directions.

## F-CITE — the citation discipline (spec text, not a convention)

> **F-CITE.** F names sibling scenarios, asks and gates **by ID**. Parameters, assertions,
> fixture configuration and tool invocations belong to the owning spec's register and are
> **never restated in F**. Where F needs a value it does not own, F cites; where F needs the
> sibling to state one, F declares an ask.

This is the root-cause fix for a defect class, not a style rule. Three separate defects in
this spec's drafts — F7 restating E's Round-1 memory parameters, F5 restating G's withdrawn
`systemctl is-active` assertion, F12 inferring `dpkg-buildpackage` from G's tree layout — were
all one error: F wrote concurrently with its siblings and copied their content instead of
referencing it, so every sibling revision desynced F silently. Under F-CITE the consequences
are renames, not redesigns: E's `18-W(b)/(c)` → `18-W1/18-W2` rename cost F two tokens
because F carries no parameters to re-sync.

Applied throughout: F7 names **zero** parameters (§3(c)); F5 runs G's G15 assertion set by ID
(§3(a)); F8 runs G's tool chain in G's order (§4); F2's fast subset names scenario numbers
only.

## Current state (what F changes)

`ci.yml` today has exactly three jobs: `lint-test` (`:11-28`, ubuntu-22.04 — `cargo fmt
--check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`,
`cargo check -p kiosk-main`), `build-windows` (`:30-43`) and `build-linux` (`:45-63`), the
latter two producing release binaries as artifacts. Triggers are `push: branches: [main]` and
`pull_request` (`:2-5`) — **no `tags:` filter**, and no release workflow, checksum step,
draft-release step or `gh release` usage exists anywhere in the repo.

**After C lands, `cargo test` already includes RT-13** — the supervise loop becomes a per-PR
gate with zero F work. Verified both halves: `rt13.rs:27` is `#![cfg(windows)]` (the only
`cfg(` in the file, so the target is empty on Linux today), and `ci.yml:26` is
`cargo test --workspace`, unscoped — no `-p`, no `--test` filter — so once C removes the gate
the existing line picks up the auto-discovered target with no `ci.yml` edit. F adds the
webview-level gate on top.

**W9 — withdrawn, recorded:** the draft implied F changes no existing workflow job. It does.
F extracts `ci.yml:30-63` into a reusable workflow and rewrites those two jobs as calls
(§4.1). Everything else in `ci.yml` is untouched.

## Components

### 1. The harness — `crates/kiosk-smoke` (F1)

A **new workspace member**, `crates/kiosk-smoke`, whose only dependency is **`serde_json`**.
Scenario bodies live in `crates/kiosk-smoke/tests/smoke_linux.rs`.

**Why its own member, and why the fallback is dropped.** An integration test under
`crates/kiosk-main/tests/` is a target *of the `kiosk-main` package*: building it compiles
`kiosk-main`'s lib and its whole graph in `dev` — `tauri` → `wry` → `webkit2gtk-sys`,
`reqwest`, `tokio`, `sysinfo`, `kiosk-core` (`crates/kiosk-main/Cargo.toml:9-26`;
`Cargo.lock` has **510** `[[package]]` entries) — in a job whose `Swatinem/rust-cache` key is
its own and therefore cold on first run. Cargo also builds the package's **bin** targets when
running its integration tests, which is exactly what `CARGO_BIN_EXE_kiosk-main` requires: the
variable cannot exist unless cargo built the binary. So **`KIOSK_BIN` and `KIOSKCTL_BIN` are
mandatory with no `CARGO_BIN_EXE_` fallback** — that default is what made the 510-package
graph a per-PR cost, and removing it is what severs the coupling. Verified: the root
`Cargo.toml:3` member list is core/main/launcher, and `serde_json` has no path to
`tauri`/`wry`/`webkit2gtk-sys`.

**Withdrawn, recorded:** the intermediate claim that a `crates/kiosk-main/tests/` harness
"compiles in `dev` (fast; it is assertion code)". False, for the reason above.

What F builds inside it:

- **Compositor bring-up.** weston headless for A/B/D; cage headless for C — see the
  compositor map in §3(a). F owns the bring-up and exports `WAYLAND_DISPLAY`; the tests
  assume it.
- **Fixture HTTP server.** `std::net::TcpListener`, ~40 lines. **Not** `python3 -m
  http.server`: the `debian:12` image ships no python3, and adding one is a container
  dependency for what stdlib does.
- **Spool oracle.** `serde_json` over the on-disk spool, which is A's own design
  (`p2a:291`: telemetry asserted from the on-disk spool, no fake-GCL endpoint needed).
  `rt13.rs:14-16`'s refusal of spool assertions is scoped to that in-process FSM test and is
  not precedent against an out-of-process app run.
- **Binary under test.** `std::process::Command` on `KIOSK_BIN`. In CI that is the downloaded
  **release** artifact — the gate exercises the artifact `build-linux` uploads and the `.deb`
  ships, which is stronger than `cargo test --release` (that would only match the *profile*).
  The harness target itself compiles in `dev`, and now genuinely cheaply.
- **Fixture signing.** Shell out to `kioskctl` via `KIOSKCTL_BIN` —
  `crates/kiosk-core/examples/kioskctl.rs` is real (`keygen`/`sign`/`hash-pin`/`selftest`),
  built once in `build.yml` and uploaded with the binaries. No crypto crate in the harness.
- **Gating.** Scenario tests are `#[ignore]`, the way this repo already gates operator-only
  tests — precedent `crates/kiosk-core/src/config/signature.rs:203-204`, with its invocation
  documented at `:200`. So `ci.yml:26`'s `cargo test --workspace` skips them on a runner with
  no compositor (C8 preserved, no new job shape, no new dependency), while `ci.yml:25`'s
  `cargo clippy --workspace --all-targets` lints the new member for free.

**Local-developer path.** `KIOSK_BIN` mandatory is fine (`cargo build -p kiosk-main`, then
point at it), but that binary must also be built with the harness's key or every
signed-config scenario fails looking like a product bug. The harness runs a **preflight**
that signs a trivial config and asserts acceptance, failing as a **runner/environment** error
distinct from a scenario failure — reusing the classification F's flake policy already has
(§7). No new mechanism.

**Ownership boundary.** A–E own what each scenario asserts, its fixture shape and its pass
criteria — already written, in prose, in their specs. F owns the bodies for A 1–7 · B 8–12 ·
C 13–15 · D 16–17 and A 6's binary; **E retains 18, 18-W1 and 18-W2 plus the soak harness**
(`p2e:25`). F's dependency is therefore on A–E's **specs**, which exist, not on their code —
which is what removes the deadlock the draft created by routing the bodies back to A, a spec
that is reviewed and closed.

### 2. Per-PR job: `smoke-linux` (F2, F3, F4)

`runs-on: ubuntu-22.04` (matching `lint-test`'s image and the webkit 4.1 dev deps installed
at `ci.yml:15-19`), plus `weston` and the four GStreamer packages (P2-E §5) — verified
additions: that apt line installs `libwebkit2gtk-4.1-dev libgtk-3-dev
libayatana-appindicator3-dev librsvg2-dev libssl-dev` and nothing else.
`needs: [smoke-key, build]`; downloads the smoke-keyed Linux release artifact plus `kioskctl`
and sets `KIOSK_BIN` / `KIOSKCTL_BIN` from them.

**Fast subset:** A 1–3, 5, 7 (boot / nav / offline / iframe / safe boot — `p2a:292-311`);
B 8–11 (egress / downloads / dialog / permissions — `p2b:174-194`); D 16 (idle → clear, short-
threshold fixture — `p2d:117-121`). Ten scenarios.

**Wall-clock rule (F3):** **under 10 minutes or the subset shrinks** — a gate developers route
around is worse than a smaller gate. The rule is self-pinning and the first working version
measures it. **Scope, stated so the first measurement measures the right thing:** it bounds
`smoke-linux`'s *own* wall clock — artifact download, apt, weston bring-up, ten scenarios —
starting after `needs: build`. The shared build is not charged against it, because that build
already exists in `ci.yml` today and runs in parallel with `lint-test`; **F adds no new build
to the per-PR path.**

**Excluded from per-PR, by design (F4) — the list is exhaustive.** Union of include and
exclude is exactly scenarios 1–18, no gaps, no double-listing.

| Excluded | Why |
|---|---|
| **A 4** | crash-kill; timing-flaky candidate, scheduled until it proves stable |
| **A 6** | superseded per-PR by D 16 (`p2d:113-114`: *"A's harness-binary scenario stays as the completion unit check; D's smoke 16 supersedes it as the app-path proof"*); A 6 runs in `endurance` as the unit check |
| **B 12** | **scheduled-only.** B's degrade assertion is predicated on *"the container has no systemd"* (`p2b:193-196`); `ci.yml:12`'s `ubuntu-22.04` is a full VM that has systemd, so on that runner `systemd-inhibit` plausibly succeeds and the scenario asserts nothing or asserts a third, unspecified behaviour. It runs in F5's container, where B's precondition holds. Its positive half stays on G's hardware checklist. **Withdrawn, recorded:** "per-PR runs the degrade assertion only if it is free" — undecidable on the runner it named |
| **C 13–15** | cage chain; scheduled. **Declared assumption, narrowed:** RT-13 gives per-PR coverage of hang→restart and exit-86 **at the FSM level** (its four tests, `rt13.rs:291,324,359,384`). It does **not** cover C 13's cage chain, C 15's zombie-reap assertion or C 14's app-path pinpad driver. Residual: a cage-chain regression is caught nightly, not per-PR — accepted; a full cage chain per-PR is exactly what F3 exists to protect against. **Withdrawn, recorded:** the draft's "RT-13 covers the same FSM paths" |
| **D 17** | blocking only if cage exposes wlr virtual input headless (`p2d:122-129`). That question is now **closed negatively on the floor** — see §3(a) — so 17 runs scheduled under the Xwayland driver, with its existing hardware fallback intact |
| **E 18** | soak is never per-PR (`p2e:111` agrees) |

**Failure artifacts:** spool, compositor logs, best-effort screenshots, uploaded on failure.

### 3. Scheduled workflow: `endurance` — one nightly, three jobs

**Open decision closed, not left open:** *one* nightly workflow running *all three* jobs
*every* night. Alternating nights is incompatible with F9's gate — a run-level
`--status=success` would then certify a run that legitimately never executed the soak. This
is affordable because no endurance job builds anything: a shared `build` feeds all three
(§4.1). **Interlock, written down rather than discovered:** *if a future change splits
`endurance` across nights, F9's gate must move to job level (`gh run view <id> --json jobs`)
in the same change.*

#### (a) Full A–D matrix in a `debian:12` container (F5)

Target fidelity: the distro's actual WebKitGTK/GStreamer, not Ubuntu's — the closest CI gets
to the pinned image before hardware, and the C7 platform floor. All A–D scenarios including
the per-PR exclusions. **Runtime packages only** — `libwebkit2gtk-4.1-0`, the four GStreamer
packages, `weston`, **`cage`, `xwayland`, `xdotool`** — no `-dev` set, no Rust toolchain; the
job consumes `build`'s artifacts.

**Compositor map, stated explicitly (INT-3a).** The draft assigned "all A–D scenarios
including the per-PR exclusions" to this container while naming weston as its only
compositor — and C 13–15 *are* those exclusions, and C's smoke 13 requires
`cage -- kiosk-launcher` under `WLR_BACKENDS=headless` (`p2c:152`). The cage requirement
entered the system with C and reached no environment list. Conceded; fixed here:

> **A 1–7, B 8–12 and D 16–17 run under weston headless. C 13–15 run under
> `cage -- kiosk-launcher` with `WLR_BACKENDS=headless`. No scenario runs under an unnamed
> compositor.**

**Scenarios 14 and 17 — the driver on the floor (INT-3b).** D's open plan-time question
(`p2d:127-129`, whether cage exposes wlr virtual input headless) resolves **negatively on the
C7 floor**: cage 0.1.5 exports `wlr_virtual_pointer_manager_v1_create` and
`wlr_virtual_keyboard_manager_v1_create`, but **cage 0.1.4-4 — Debian 12 — exports neither**;
its `*_create(` list is compositor, data_device_manager, export_dmabuf, gamma_control, idle,
idle_inhibit_v1, output_layout, screencopy, server_decoration, xcursor, xdg_decoration,
xdg_output, xdg_shell, **xwayland**. XTEST reaches only Xwayland clients, not a native
Wayland webview. Scenario 17 already declares a hardware fallback and survives; scenario 14 —
the only end-to-end proof of arch-05's exit-86 app path, since `rt13.rs` builds `LauncherSink`
directly and never runs `kiosk-main` — did not.

> **Driver:** run `kiosk-main` inside cage with **`GDK_BACKEND=x11`**, making the webview an
> Xwayland client, and drive it with **`xdotool`**. `wlr_xwayland_create` is in cage 0.1.4's
> own create list, so this works on the floor. This is the route G's runbook already
> documents as the keyboard fallback, used here as a smoke driver rather than for deployment.
>
> **Declared divergence (C3, a stricter statement of what is proved):** the run exercises
> GTK's X11 GDK backend, not the Wayland one. Faithful for what 14 and 17 assert — D's
> mechanism is GTK *widget signals*, not a Wayland protocol, and 14 asserts exit-86
> propagation — and **not** a substitute for the Wayland input path, which stays hardware-gated
> at P2-G H4a.
>
> **Fallback, recorded not silently dropped:** if even that fails, scenario 14's app-path half
> moves to the deferred hardware list against **P2-G H2** (systemd half, already there) and
> **H4a** (touch half). Scenario 17's existing fallback clause is unchanged.

**Q5 trap, named:** an implementer probing locally on a current distro gets cage 0.1.5, where
virtual input works; the job then fails only on the floor image. Every cage claim in C, D, G
and F is stamped with the version it was made against, and G15 asserts the image's `cage -v`
equals the recorded floor.

**Install/remove/upgrade cycle.** This job also runs **G's G15 container-scope assertions, by
reference to G's register** — currently `systemctl is-enabled` → `enabled`, the file-mode
checks (`/etc/kiosk` 0750), the no-secret grep (zero `BEGIN PRIVATE KEY`, no `kioskctl` in the
package), the `${shlibs` literal check, upgrade preservation of the three operator-owned
files, `deb-systemd-helper` not re-enabling a disabled unit, and the orphan-kill assertion
(`pkill -9 kiosk-launcher; sleep 2; ! pgrep kiosk-main`) that closes verification finding V4.

> **Correction, recorded.** An earlier draft asserted *`systemctl is-active` → `active`* here,
> copied from G's Round-1 text. G has since moved `active` to its hardware row **H2**: a
> `debian:12` container has no PID-1 systemd — the same fact that moved B 12 off per-PR — so
> `is-active` there cannot return `active` and the assertion would fail for environmental
> reasons on every nightly run. F asserts `is-enabled`; `active` is G's H2. F names the gate,
> **G owns its content**, and a by-ID reference survives G's future expansions automatically.

#### (b) Offline-video soak (F6)

**The HIGH the draft carried: an 8 h+ soak cannot run.** GitHub's limits reference is
explicit — *"Each job in a workflow can run for up to 6 hours of execution time. If a job
reaches this limit, the job is terminated and fails,"* and **this limit cannot be increased**.
Every runner F names is hosted; F never mentions self-hosted. An 8 h+ job is terminated at
6 h and fails: frame C9, a gate that cannot run.

**Re-budgeted.** `timeout-minutes: 330` on the job — **5 h 30 m inside the platform's hard
6 h**. The explicit value matters because the job-level default *is* 360, i.e. exactly the
cap, so an overrunning job is killed by the platform **before** the artifact-upload step runs
and the RSS series and spool are lost on the one run where they matter most.

**Arithmetic and headroom, stated.** `jobs.<id>.timeout-minutes` bounds the *whole* job,
container creation and every step included — setup is **inside** the clock, not outside it.
So the soak step is **derived, not asserted**:

```
soak_step = 330 − measured_setup − 20        # 20 min reserve: artifact upload + teardown
initial value: 270 min (4 h 30 m), until the probe measures setup
```

with `if: always()` on the artifact-upload step. **Withdrawn, recorded:** the first re-budget
set the soak itself to 315 min and justified `timeout-minutes: 330` as *"~30 min for container
pull, apt, build, teardown and upload"* — that 30 was the gap to the *platform cap*, not to
F's own timeout. The real headroom was 330 − 315 = **15 minutes**, which re-created the
6-hour wall one layer down. The replacement removes most of the setup rather than budgeting
for it: the soak container installs **runtime packages only** and builds nothing, consuming
`build`'s artifacts (§4.1).

**Still discharges PF-05.** The parent's word for *this* soak, verbatim at `:873-875`:
*"**Offline-video soak:** multi-hour loop on the pinned Debian 12 image, assert no
stall/black frame across loop boundaries (PF-05)."* Multi-hour, not eight. E's pass criteria
are duration-agnostic; the one duration-sensitive number (the RSS-delta bound) is E's, picked
at plan time from first-run baseline and pinned. **The ≥72 h obligation does not move and was
never CI's:** the parent puts *"a ≥72 h real-hardware soak is a pre-release gate (RT-05)"* on
real hardware, and **P2-G already owns it as checklist row H5** (`p2g:96`).

**Traceability corrected (W7).** The draft cited §10's *"assert bounded RSS"* against this
job. That phrase belongs to the **Windows-runner** soak sentence at `:870-873`, not the
offline-video sentence at `:873-875`, and the misquote is what hid the missing Windows job.
The phrase now appears only under F7. The RSS series is still retained here as an artifact on
pass as well as failure — justified as **trend data**, not as discharging a §10 assertion.

**Rejected alternatives, recorded.** A self-hosted runner (5-day cap) buys the 8 h but costs a
machine to own, patch and secure plus a Wayland-capable host, for a duration nobody required.
A split/resumable soak across chained jobs needs RSS-series and loop-count state carried
between jobs plus a stitching step — more machinery than the requirement asks for (Q2).

**Consequential ask on E, settled.** E stops pinning a CI duration: *"scheduled CI:
multi-hour, duration set by F within the hosted-runner cap."* E owns the pass criteria, which
are duration-agnostic; F owns the wall clock, which is F's constraint. E accepted.

#### (c) Windows-runner leaking-page soak (F7) — adopts an uncovered §10 obligation

`runs-on: windows-latest`, `strategy.matrix.scenario: [18-W2]`, consuming `build.yml`'s
windows artifacts (E's scenarios drive launcher+main, so both binaries are needed — precedent
`crates/kiosk-launcher/tests/rt13.rs` + its `rt13-mock-main` bin).

**Uncovered, and adopted rather than deferred.** The draft's `endurance` had two jobs, both
Linux, and its defer list named only package contents, fleet update mechanics and Android —
so §10's Windows soak was neither covered nor deferred. It is adopted because (1) it is in the
same §10 block F quotes for its own mandate (`:870-873`); (2) its subject — memory-cap restart
plus health-sampled RSS — is a **§9 P2-row** deliverable and cannot be pushed to P3; (3) §10
specifies **accelerated thresholds**, making it a minutes-scale job nowhere near any cap.

**F names no parameters and no assertions.** Fixture configuration, thresholds and pass
criteria are **E's scenarios 18-W1 and 18-W2**; E's register is the source of record and F
tracks them by ID. F owns the job, the runner, the artifacts and the scheduling. This is
F-CITE, and it is what reduced E's `18-W(b)/(c)` → `18-W1/18-W2` rename to a two-token edit.

> **Correction, recorded.** An earlier draft specified **one** run carrying 18-W(b)'s memory
> cap *and* 18-W(c)'s near-future `nightly_reload` together — the exact combination E has
> since ruled impossible (*"they genuinely cannot share a fixture … a re-tripping cap resets
> the nightly-reload timer"*) — and omitted `healthy_run_s` and the no-safe-mode assertion E
> added for the accelerated cadence. It also asserted a `max_webview_mem_mb` breach against
> E's *draft* mechanism, which sampled `/proc/self/status` — kiosk-main, not the renderer a
> leaking page grows, so the cap could never trip (parent `:671` says **webview RSS**). All of
> it is withdrawn. E's revised design answers each point (webview-process subtree RSS via the
> already-held `sysinfo::System`; the **shipped** `maintenance.max_webview_mem_mb` at
> `schema.rs:234`, default 1500 at `:343`, whose `UNIMPLEMENTED` row at `validate.rs:19` E
> deletes; the invented `memory_max_mb` key withdrawn), and F now cites rather than restates.

**Ships as `matrix.scenario: [18-W2]` (INT-1).** 18-W2 runs at `max_webview_mem_mb = 0` — it
needs E4's sampler and the nightly-reload path and **not** enforcement — whereas 18-W1 asserts
the breach → exit 80 → restart chain, i.e. the enforcement that E5's own merge gate is waiting
on this job to justify. **`18-W1` is added to the matrix by E5's enforcement commit** (one
commit, two files; owner: whoever implements E5). Building the matrix over both on day one
gives a job that cannot go green.

**Dependency direction, one way only (INT-1).** F7 depends on **E4 and the 18-W1/18-W2
bodies**. It does **not** depend on E5, and it does not own an outbound edge: F7 *produces an
artifact* — the RSS series, retained — which E's floor gate reads, and **an artifact is not a
dependency**. Both the `E5 (in)` and `E5 (out)` register entries are **deleted**. Carrying the
edge "in both directions" was what made it a declared cycle that no merge order satisfies.

**Hard dependency, declared:** if E4 or the 18-W1/18-W2 bodies are withdrawn, F7 is unrunnable
and parent §10's Windows-soak row **returns to UNOWNED in the ledger rather than silently
passing**. E mirrors this clause on its side.

### 4. Release-on-tag: `release`

#### 4.1 `build.yml` — one reusable workflow, two jobs (F8, OB-7)

**Withdrawn, recorded:** *"a new tag-triggered workflow that reuses the existing build job
bodies."* Actions has **no** cross-workflow job-body reuse; the options are `workflow_call`, a
composite action (which cannot carry a job), or copy-paste. Naming the mechanism:

> `.github/workflows/build.yml`, `on: workflow_call`, **two jobs — `linux` and `windows`** —
> both lifted from `ci.yml:30-63` (the apt line, the toolchain pin,
> `cargo build --release -p kiosk-main -p kiosk-launcher`, `upload-artifact` with
> `if-no-files-found: error`), plus building `kioskctl` and uploading it with the binaries.
> `ci.yml`, `endurance.yml` and `release.yml` each `uses: ./.github/workflows/build.yml`.

Both jobs, not one: `build-windows` (`ci.yml:30-43`) has **three** consumers — `ci.yml`, the
release job (F10's `sign.ps1 -Stage Binaries` requires exactly `kiosk-main.exe` and
`kiosk-launcher.exe`), and `endurance` job (c) — which is precisely the argument that
justified extracting the Linux one. Leaving it out halves the drift instead of removing it.
Two jobs in one file rather than two files: one place the toolchain pin lives. One build per
platform per run, no increase over what `ci.yml` does today.

#### 4.2 The `pubkey_b64` input and its guard (security-relevant, adopted in full)

`build.yml` takes one input. `crates/kiosk-core/src/config/signature.rs:63` reads the pinned
Ed25519 key through **`option_env!("KIOSK_CONFIG_PUBKEY_B64")` — compile time**, fail-closed
when unset (`boot.rs:93-95` states it: a keyless build rejects every fetched/last-good
config). A signed-config smoke run therefore requires a binary compiled against a key whose
private half the harness holds.

That input selects the **SEC-01 root of trust in a shipped binary**, in a workflow three
others consume, one of which gained `workflow_dispatch`. Three guards, all required:

**1 — `required: true`, and no `default:`.** A defaulted input is fail-**open** on a C5 gate
and invisible: a caller that omits it still builds, still signs, still ships, and the binary
*looks* keyed because `option_env!` fails closed only when genuinely unset — which a default
guarantees it never is.

```yaml
on:
  workflow_call:
    inputs:
      pubkey_b64:
        description: >-
          SEC-01 root of trust, baked via option_env!(KIOSK_CONFIG_PUBKEY_B64)
          (crates/kiosk-core/src/config/signature.rs:63). No default, by design:
          a default silently ships a binary trusting the wrong key.
        required: true
        type: string
```

**2 — a non-empty check as the first step of both jobs, before checkout.** `required: true`
rejects a *missing* input but not an empty string, so the declaration alone is not the guard:

```yaml
      - name: Guard — SEC-01 key must be non-empty
        shell: bash
        run: '[ -n "${{ inputs.pubkey_b64 }}" ] || { echo "::error::pubkey_b64 empty"; exit 1; }'
```

Placed before checkout so it cannot be reached past a caller mistake.

**3 — three callers, three distinct sources, none reachable by a dispatcher.**

| Caller | Source | Why |
|---|---|---|
| `release.yml` | `pubkey_b64: ${{ vars.KIOSK_CONFIG_PUBKEY_B64 }}` | Repository configuration, **not** the dispatch caller. `release.yml`'s only `workflow_dispatch` input is `dry_run` (F15), so nothing a dispatcher types can reach the root of trust and the dry-run path cannot mint a release-shaped artifact against a substituted key |
| `ci.yml` | a `smoke-key` job's output | ephemeral, see below |
| `endurance.yml` | a `smoke-key` job's output | ephemeral, see below |

**The smoke key is generated per run and never committed.** A small `smoke-key` job ahead of
`build` builds `kioskctl` (the `kiosk-core` graph only — no tauri), runs `kioskctl keygen`,
emits the public half as a job output and masks the seed. `build.yml` receives it and stays a
dumb consumer with no key policy in it. **This adds no build** — F1 already needs `kioskctl`
to sign fixtures, so the build moves earlier rather than duplicating. Repo doctrine is
explicit that test keys are generated at run time and are *"never a committed fixture (a
`-----BEGIN PRIVATE KEY-----` in the repo is the highest-signal pattern secret scanners hunt
for)"* — `crates/kiosk-main/Cargo.toml:48-51`, mirrored in `kiosk-core/Cargo.toml`; and G15
asserts zero such strings in the `.deb`. Ephemeral keys delete the substitutable artefact
rather than protecting it. Net: the only private key in existence lives for one workflow run.

**C3, both directions — the one declared residual.** The smoke gate no longer runs
byte-for-byte the shipping artifact: it runs the **same commit, same profile, same flags,
differing in exactly one compile-time constant**, `KIOSK_CONFIG_PUBKEY_B64`. That constant is
*already* per-deployment — every operator bakes a different key, so no single shipping binary
exists across fleets — and `ci.yml:30-43`/`:45-63` bake **no** key at all today, i.e. the
current artifact is the fail-closed dev build. This is not a test/prod divergence; it is the
variation production already has. Stated rather than implied.

#### 4.3 The release job (F8, F10, F11, F12)

Triggers: tag push `v*`, plus `workflow_dispatch` with a `dry_run` input (F15). `checkout`
sets **`fetch-depth: 0`** — without history `git merge-base --is-ancestor` errors, and being
fail-closed it would refuse every tag.

**`.deb` (F12).** F specifies only the **invocation contract**: G's `packaging/linux/`
provides the tree; F's release job runs **G's flow, in G's order** —
**`dpkg-shlibdeps` → `dpkg-gencontrol` → `dpkg-deb -b`**, then **`lintian --fail-on error`**
gating the `.deb` before it is attached. Per F-CITE, F names the steps G named and nothing
else.

> **Withdrawn, recorded — `dpkg-deb` named while claiming "F only executes it," then
> `dpkg-buildpackage` inferred from G's `debian/source/lintian-overrides`.** Both wrong. The
> first was incoherent; the second contradicted G's settled register
> (`grep "dpkg-buildpackage\|debian/rules\|dh_"` over G's thread → **zero hits**) and would
> have run `dh_shlibdeps` itself, making G's declared dependency on F meaningless — while F
> was simultaneously accepting G's asks that presuppose G's flow. F's Round-2 ask on G is
> **withdrawn as satisfied**: G's register answered it before F asked. A flag that the
> `substvars` → control substitution step was unnamed in both registers is also withdrawn —
> G names `dpkg-gencontrol`, so the chain is **three** tools, not two.

**Authenticode signing (F10) — adopts an uncovered §10 obligation.** §10's CI row ends,
verbatim at `:883-884`: *"Android build (P3), **Authenticode signing step (unsigned artifacts
fail the release gate)**."* The draft's release job listed exactly one gate. Adopted, not
deferred: it is in F's own quoted row, C5 makes it fail-closed, and P1-F2 already supplied the
invocation (`packaging/windows/sign.ps1` exists; F2 spec `:41-44`: *"a build/CI step `signtool
sign`s both PE binaries and the MSI … the cert itself is not in the repo"*).

**Per-artifact, not per-tag.** A repo with no provisioned cert — today's state — must still be
able to cut a Linux release, because P2's shipped deliverable *is* the `.deb`:

> The Windows artifact set is produced by its own job. If the signing cert is absent or
> `sign.ps1` throws, **that job fails and its artifacts are excluded from the draft release**;
> the `.deb` job and the draft release still complete, with the release body recording the
> Windows set as absent. **No unsigned Windows artifact ever reaches a release.**

Mechanically: the publish job carries `needs: [build-deb, build-windows]` **and**
`if: always()`, with an explicit `contains(needs.*.result, 'failure')` branch writing the
"Windows artifact set absent" line — without `if: always()` a failed Windows job skips publish
and the coupling returns. **C3, both directions:** this is *looser* than the draft's rendering
(a tag now succeeds without a cert) and *exactly as strict as* the parent, whose rule is
"**unsigned artifacts** fail the release gate", scoped to the artifacts that require signing.

**Four inputs the release job must be given**, all read out of `sign.ps1` and
`packaging/windows/README.md`: (i) network egress to Microsoft — the bundle build downloads
the Evergreen WebView2 bootstrapper when absent and verifies its Authenticode signature is
valid and issued to Microsoft Corporation; (ii) an out-of-band standalone installer for
offline builds (`-p:WebView2InstallerPath=…`, deliberately not committed); (iii) the signing
cert — via the **`Pfx`** parameter set, since the `Thumbprint` set requires a certificate
already in `Cert:\CurrentUser\My` or `Cert:\LocalMachine\My` and a fresh hosted runner has
none: materialise a base64 repo secret to a temp `.pfx`, set `KIOSK_SIGNING_PFX_PASSWORD`,
pass `-PfxPath`, and let the script's own `finally` block remove the imported cert and key;
(iv) **HTTPS egress to a timestamp authority** — `sign.ps1` declares
`[Parameter(Mandatory)] [uri]$TimestampUrl` and throws unless the scheme is `https`. Note for
the implementer: `-Stage Installers` requires `-NewerThan` dependency paths, so the MSI sign
call must pass the built PE paths.

> **Withdrawn, recorded — a separate `signtool verify /pa` step.** `sign.ps1` already runs
> `& $signTool verify /pa /all $target` after every sign and throws on non-zero, so the step
> was duplicate work (Q2). Replaced by the assertion `sign.ps1` **cannot** make: a **coverage**
> check — enumerate the release set, diff against the set passed to `sign.ps1`, fail on
> non-empty. Also withdrawn: *"wiring is the same one-job shape … included here for symmetry,
> not new design"*; three of the four inputs above are things no existing job does.

Forked PRs never reach this workflow (tag-triggered, default branch), so the fork-secret
problem does not arise: forks cannot push tags upstream.

**RT-09 live token exchange (F11) — adopts an uncovered §10 obligation.** Verbatim at
`:876-878`: *"**Live token-exchange smoke (gated/opt-in, release gate):** a real RS256 →
oauth2 token exchange + one `entries:write` against a throwaway service account; skipped when
creds absent (RT-09)."* Verified unowned: `grep -rn "RT-09"` across all specs returns three
hits, all inside the parent, and the only `#[ignore]` in the workspace is
`signature.rs:204` — so no live-smoke test exists. F adopts both halves rather than leave a
second §10 row unowned:

> `crates/kiosk-core/tests/live_token_exchange.rs`, `#[ignore]`d (same pattern as
> `signature.rs:203-204`), against the existing client at
> `crates/kiosk-core/src/logging/{auth,client,transport}.rs`; a release-job step runs it with
> `-- --ignored` when the throwaway-SA secret is present and **skips when absent** — the
> parent's own word, so absence is a skip here, unlike F10 where the parent says fail.

The *other* half of that §10 sentence is already discharged in P1 and is deliberately not
duplicated: `crates/kiosk-core/src/logging/auth.rs:531`
`jwt_claims_match_googles_server_to_server_contract`.

#### 4.4 The endurance-green gate (F9)

**Withdrawn, recorded:** *"release requires the latest `endurance` run green"*, stated with no
mechanism. Actions has no native cross-workflow status gate — `workflow_run` fires on the
dependent workflow, not on a tag push. The mechanism, as the tag job's first gate:

```
gh run list --workflow=endurance --branch=main --status=success --limit=1 \
  --json headSha,createdAt,conclusion
```

then refuse the tag unless **both**: `git merge-base --is-ancestor <headSha> <tag_sha>`
succeeds, **and** the run falls inside a freshness window (value at plan time). Both, not
either — ancestry alone admits a year-old soak; recency alone admits a soak of a tree that
lacks the tagged commit. **Fail-closed on a query error** (C5); releases are rare and
re-runnable. `gh` is preinstalled on hosted runners: no new dependency.

> **Correction, recorded.** The first version of this query fetched `headSha` and never used
> it, so a tag pushed on a commit merged ten minutes ago passed on last night's run — a gate
> whose stated purpose is *this* build certifying a different one.

Two scheduling facts adopted as spec text rather than argued with: scheduled workflows run
only on the default branch, and in a public repo they auto-disable after 60 days of
inactivity. Both would make the gate silently *unavailable* rather than red — the freshness
window is what converts "endurance stopped running" into a refused tag instead of a green one.

### 5. Update path — the parity statement (F13)

Windows P1/P2 ships no auto-updater: update = install the new MSI. Linux matches: update =
install the new `.deb` (`dpkg -i` / operator tooling). Devices tolerate the restart by
construction (spool durability, launcher exit/restart semantics). Recorded ponytails,
deliberately not P2 scope: an apt repository, `unattended-upgrades` policy, delta/A-B updates.
The `unattended-upgrades` fact lives in **G's runbook**; F cites it and does not restate it,
so the citation loop between the two specs runs one way.

## Testing — F's own verification is meta (F15)

F's product *is* test infrastructure, so C9 demands F prove its gates can fail.

1. **Standing, per-PR:** a scenario in `crates/kiosk-smoke` that runs a known-bad fixture and
   asserts the harness reports it as a failure. ~10 lines, cannot rot silently. **Corrected:**
   the draft made this a one-time run documented in the landing PR, after which nothing
   detected the smoke job ceasing to fail on a broken scenario — the same rot the flake policy
   worries about, one level up.
2. **Refused tag on a red endurance**, and **3. a Windows artifact set refused for a missing
   cert** — both run via `release.yml`'s `workflow_dispatch` + `dry_run: true` path: every
   gate runs (endurance-green + ancestry, lintian, signing coverage) and the publish step is
   skipped. **No real `v*` tag is consumed and there is no draft release to clean up**, and
   both proofs are re-runnable on demand by anyone instead of once. **Corrected:** the draft
   required an actual tag push to prove F10's negative path, consuming the real tag namespace.

## Error handling / flake policy (F14)

A failed smoke scenario retries **once** within the job (compositor startup races are the
realistic flake class). A pass-on-retry is reported as pass **with a `flaky` line in
`$GITHUB_STEP_SUMMARY`** *and* a durable comment:

```
gh issue comment "$FLAKY_ISSUE" --body "flaky: <scenario> · <run-url> · <sha>"
```

on a **standing flaky-smoke issue named in this spec, create-if-missing** so the mechanism
does not depend on a manual pre-step. `GITHUB_TOKEN` with `permissions: issues: write`; fork
PRs get a read-only token, so the step is `continue-on-error: true` and the summary line is
the fallback there. No cross-run state, no rolling window, no bot, no external store.

> **Over-concession corrected, recorded.** Retry-once *is* the laundering mechanism; the paper
> trail is what makes it safe. Dropping the trail to a `flaky` line on a **green** run's
> summary — read by nobody, not on the PR checks line, not aggregated — kept the laundering and
> lost the control (Q3). It was also asymmetric: F9 accepts a `gh run list --json` query over
> run history as the right answer to one gap in the same breath as rejecting cheaper machinery
> here. One `gh issue comment` is strictly less machinery than F9's accepted mechanism.

**Automated quarantine stays withdrawn (W4).** "Two flakes of the same scenario in seven days
→ the scenario moves to `endurance` and a tracking issue is opened" assumes durable cross-run
state over a rolling window plus an actor to perform the move; Actions provides neither, and
it automates a judgement call made a few times a year (Q2).

**Named owner, without inventing a role.** "A maintainer" is not an owner. The review point is
a checkpoint F already has: **whoever cuts the release (F8/F9)** reads the standing flaky
issue as a release step, and a scenario with two or more comments since the previous tag is
fixed or moved to `endurance` before the tag is pushed.

**Runner-environment failures** (apt mirror down, compositor won't start at all, the F1
preflight failing) fail the job **distinctly** from scenario failures, so the signal stays
clean.

## Residual risks — each with a named carrier

| Risk | Carrier |
|---|---|
| The smoke gate exercises a binary differing from a shipping one in exactly the `KIOSK_CONFIG_PUBKEY_B64` constant | **Declared C3 residual** (§4.2) — a variation production already has, since every operator bakes a different key; today's `build-linux` bakes none at all |
| C-chain regressions (cage chain, zombie reap, pinpad driver) are caught nightly, not per-PR | **Accepted**, F4; RT-13 covers hang→restart and exit-86 at the FSM level per-PR |
| Scenario 14/17 CI driver proves the **X11** GDK backend, not the Wayland one | **Declared divergence** (§3(a)); the Wayland input path stays hardware-gated at **P2-G H4a**, with 14's app-path half falling back to **H2 + H4a** if the driver fails |
| `ubuntu-22.04` runner-image retirement (F16) | **Pinned by the nightly `debian:12` matrix.** The platform F *certifies* is `debian:12` (C7 floor), exercised nightly by F5 and F6; `ubuntu-22.04` is a convenience runner for the per-PR subset only, so a forced migration cannot silently change what P2 is certified against — its cost is bounded to re-running the container probe. **Withdrawn, recorded:** "watch the deprecation announcement issue in `actions/runner-images`" — vigilance, not a frame §4.4 pinning mechanism |
| A 22.04-built binary running in the `debian:12` container (glibc 2.35 vs 2.36) | **Declared assumption**, pinned by the container probe (one `ldd` check inside the container). Fallback: build inside the container and re-budget F6 |
| weston backend-flag spelling differs across generations (`--backend=headless-backend.so` vs `--backend=headless`) | **Declared assumption**, pinned by the same probe, on both `ubuntu-22.04` and `debian:12` |
| `endurance` split across nights would silently green F9 | **Interlock in spec text** (§3): a split must move the gate to job level in the same change |
| F7 unrunnable if E4 or the 18-W1/18-W2 bodies are withdrawn | **Declared hard dependency** — §10's Windows-soak row returns to **UNOWNED in the ledger**, not silently passing |

## Open decisions to resolve at plan time

Values and shims only; no mechanism is left unpinned.

- **The container probe**, which answers four things at once before the jobs are built on it:
  seat/device-node availability for weston **and cage** headless inside a GH Actions
  `container:`; the weston backend-flag spelling; the glibc forward-compatibility check; and
  **measured setup cost**, which is F6's `soak_step` derivation input.
- F9's freshness-window value.
- Artifact retention windows (spools are small, RSS series smaller — likely defaults suffice;
  decide with real sizes).
- Cron time for the nightly. *(Whether the jobs share the nightly is **closed**: one workflow,
  three jobs, every night — see §3.)*

## Change register and cross-spec edges

| ID | Change | Discharges | Depends on |
|---|---|---|---|
| F1 | Harness = **`crates/kiosk-smoke`**, workspace member, `serde_json` only; mandatory `KIOSK_BIN`/`KIOSKCTL_BIN`, **no `CARGO_BIN_EXE_` fallback**; F owns compositor bring-up, `TcpListener` fixture server, spool reader, bodies for A 1–7 · B 8–12 · C 13–15 · D 16–17 incl. A 6's binary | §10 "functional at P2" (`:882-883`) | A–E **specs** (definitions only); `kioskctl` |
| F2 | Per-PR `smoke-linux`, `needs: [smoke-key, build]`, `KIOSK_BIN` = downloaded **release** artifact; subset A 1–3,5,7 · B 8–11 · D 16 | §10 functional-at-P2 | F1, `build.yml`, `smoke-key` |
| F3 | Under 10 min or the subset shrinks; bounds `smoke-linux`'s own wall clock after `needs: build` | §10 per-PR/scheduled split; Q5 | F2 |
| F4 | Exclusion list **exhaustive** (A 4, A 6, B 12, C 13–15, D 17, E 18); **B 12 scheduled-only**; 14/17 Xwayland driver + fallback | §10 "soak/endurance scheduled, not per-PR" | F2, F5 |
| F5 | `endurance` (a): full A–D matrix in `debian:12`; set gains **`cage`, `xwayland`, `xdotool`**; scenario→compositor map; **G's G15 assertions by reference** (`is-enabled`, not `active`) | §10 scheduled rows; C7 floor | F1, `build.yml`, **G (G15)**, **C (C15)** |
| F6 | `endurance` (b): offline-video soak, **8 h+ → derived `330 − setup − 20`, initial 270 min, `timeout-minutes: 330`**; runtime deps only, no in-container build; `if: always()` upload | §10 `:873-875` offline-video soak / **PF-05** | `build.yml`; **E** (duration ask settled) |
| F7 | `endurance` (c): **Windows leaking-page soak**, `windows-latest`, `matrix.scenario: [18-W2]`; **F names no parameters** | §10 `:870-873` "asserts bounded RSS…"; §9 P2 row | **E4 + 18-W1/18-W2 bodies (in). No outbound edge**; `build.yml` (windows) |
| F8 | **`build.yml`** = `workflow_call`, two jobs (linux + windows), `pubkey_b64` **required, no default** + empty-string guard before checkout; `release.yml` on tag `v*` + `workflow_dispatch(dry_run)`; publish `needs: […]` + `if: always()`; `fetch-depth: 0` | §9 P1/P2 deployable rows; §10 release gate; **SEC-01** | `build.yml`, G, P1-F2 |
| F9 | Endurance-green gate: `gh run list --json` + **ancestry `git merge-base --is-ancestor` AND freshness**, fail-closed; one nightly, three jobs; split-interlock in spec text | §10 soak-as-pre-release-gate | F5, F6, F7, F8 |
| F10 | **Authenticode signing gate, per-artifact** — Windows set excluded from the draft release on failure, `.deb` unaffected; signing-**coverage** assertion; four named inputs incl. HTTPS timestamp authority; `Pfx` parameter set | §10 `:883-884` verbatim; **C5** fail-closed | `build.yml` (windows); `sign.ps1`; cert + PFX secrets |
| F11 | **RT-09 live token exchange** — `#[ignore]`d test + creds-present step, **skips when absent** | §10 `:876-878` (RT-09) | `kiosk-core/src/logging/*`; SA secret |
| F12 | `.deb` = **invocation contract**; G's flow in G's order: **`dpkg-shlibdeps` → `dpkg-gencontrol` → `dpkg-deb -b` → `lintian --fail-on error`**; `dpkg-buildpackage` struck | §9 P2 ".deb"; C1/Q1 ownership | **G (settled)** |
| F13 | Update path = install-the-new-`.deb`, parity with install-the-new-MSI | §9 P1/P2 (no auto-updater either platform) | G runbook |
| F14 | Flake policy: retry-once + `flaky` summary line + **`gh issue comment` on a standing create-if-missing issue**; automated quarantine withdrawn; owner = **whoever cuts the release** | Q3 observability-of-failure | F8/F9 |
| F15 | Meta-verification: broken-fixture proof **standing per-PR**; refused-tag and refused-unsigned proofs via **`workflow_dispatch` dry-run**, no tag consumed | C9 "merge gates are real" | F2, F8, F9 |
| F16 | Runner-image assumption **declared and pinned by the nightly `debian:12` matrix** (C7 floor) | §4.4 evidenced/pinned | F5, F6 |

**Edges, both directions.**
**A → F1** (`p2a:312-315` hands compositor-harness automation to F; A is reviewed and cannot
be assigned work, which is why F owns the bodies). **B → F1/F2** (`p2b:34,236`; scenario
definitions 8–12). **C → F** — C14 de-gates RT-13 into per-PR Linux CI, and **C15 → F5**
(cage/xwayland/xdotool in the nightly container and the scenario→compositor map, declared on
C's side too). **E → F6** (duration ask, settled: E stops pinning a CI duration).
**E → F7** (18-W1/18-W2 bodies and the parameter table; **F references, never restates**).
**F ⇒ E5** — F7's first green nightly 18-W2 run records the floor that merge-gates E5's
*enforcement half only*; this is an **artifact E's gate reads, not a dependency F declares**,
and E5's enforcement branch plus the single line adding `18-W1` to F7's matrix land as **one
commit**, after F. **G → F5/F8/F12** (G15 container assertions by ID; the three-tool `.deb`
chain; `lintian --fail-on error`). **P1-F2 → F10** (`packaging/windows/sign.ps1` + the WiX
build).

**Merge position:** F lands **after G** (F executes G's package flow and references G15 by
ID) and after C, D and E-part-1. The final step of P2 is one commit carrying E5's enforcement
branch plus `18-W1` into F7's matrix.

**Deleted this round:** the `E5 (in)` and `E5 (out)` entries on F7 — carrying that edge "in
both directions" was a declared cycle no merge order satisfies.

## Scope / defer

Package contents, the `.deb` build tree, the lockdown runbook, image pinning, the hardware
checklist and the **≥72 h hardware soak (H5)** → **P2-G**. Scenario 18/18-W1/18-W2 bodies, the
soak harness and all memory/RSS parameters → **P2-E**. Fleet update mechanics (apt repository,
`unattended-upgrades` policy, delta/A-B updates) → recorded ponytail in §5. Android CI rows
(§10) → **P3**, the one deferral the parent itself makes in this block. Nothing else in §10 is
deferred: the three obligations that were uncovered at verification — the Authenticode signing
gate (F10), the Windows-runner leak soak (F7) and RT-09's live token exchange (F11) — are
**adopted here**, because none has a parent deferral to lean on and none has a candidate owner
elsewhere in A–G. F is the release-and-CI spec; they are F's.
