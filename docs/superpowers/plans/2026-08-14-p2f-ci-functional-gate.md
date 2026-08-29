# P2-F — CI Functional Gate, Endurance and Release Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **EXECUTION HOST:** Linux for the harness; GitHub Actions for every workflow. Workflow changes must be validated by an actual run, not by inspection.

**Goal:** Every PR gets the Linux functional gate §10 promised at P2 — the real release binary under a real compositor with real signed config, in under ten minutes. Long-running validation runs nightly on a target-faithful image. Tags produce signed, lintian-clean, endurance-gated artifacts for both platforms.

**Architecture:** A new workspace member `crates/kiosk-smoke` owns compositor bring-up, a stdlib fixture HTTP server, a spool oracle and every scenario body except E's. Its only dependency is `serde_json`, and `KIOSK_BIN`/`KIOSKCTL_BIN` are **mandatory with no `CARGO_BIN_EXE_` fallback** — that fallback is what would drag the 510-package `kiosk-main` graph into the per-PR path.

**Tech Stack:** Rust 2021 (`serde_json` only), GitHub Actions (`workflow_call`, `gh` CLI), weston, cage, Xwayland, xdotool, dpkg tooling, lintian.

**Spec:** `docs/superpowers/specs/2026-08-06-p2f-ci-functional-gate-design.md` (rev 2)

**Depends on:** P2-A, P2-B, P2-C, P2-D, P2-E stage 1, P2-G.

**Merge gates:** the per-PR `smoke-linux` job green on its ten scenarios; **one recorded run of a deliberately-broken fixture reddening it**; and one `workflow_dispatch` dry-run showing the release path refusing a tag against a red endurance.

## Global Constraints

- **F-CITE — the citation discipline, spec text and not a style rule:**
  > F names sibling scenarios, asks and gates **by ID**. Parameters, assertions, fixture configuration and tool invocations belong to the owning spec's register and are **never restated in F**. Where F needs a value it does not own, F cites; where F needs the sibling to state one, F declares an ask.

  This is the root-cause fix for a defect class: three separate defects in this spec's drafts were all one error — F copied sibling content instead of referencing it, so every sibling revision desynced F silently.
- **F changes no `crates/*/src/`.** F owns the smoke-harness code and every scenario body **except E's** (18, 18-W1, 18-W2).
- **`KIOSK_BIN` and `KIOSKCTL_BIN` are mandatory. No `CARGO_BIN_EXE_` fallback.** Cargo builds the package's bin targets when running its integration tests, so that fallback re-couples the harness to the full graph.
- **Scenario tests are `#[ignore]`d** — the repo's existing gate for operator-only tests (precedent: `crates/kiosk-core/src/config/signature.rs:203-204`). So `ci.yml:26`'s `cargo test --workspace` skips them on a compositor-less runner, while `ci.yml:25`'s `cargo clippy --workspace --all-targets` lints the new member for free.
- **Compositor map — no scenario runs under an unnamed compositor:**
  > **A 1–7, B 8–12 and D 16–17 run under weston headless. C 13–15 run under `cage -- kiosk-launcher` with `WLR_BACKENDS=headless`.**
- **The 6-hour hosted-runner job cap cannot be increased.** Any soak job must set an explicit `timeout-minutes` **below** it, because the job-level default *is* 360 — an overrunning job is killed **before** the artifact-upload step runs, losing the RSS series and spool on the one run where they matter most.
- **`pubkey_b64` has `required: true` and NO `default:`.** A defaulted input is fail-**open** on a C5 gate and invisible: the binary *looks* keyed because `option_env!` fails closed only when genuinely unset — which a default guarantees it never is.
- **Version-stamp every cage claim.** An implementer probing locally on a current distro gets cage 0.1.5, where virtual input works; the job then fails only on the floor image (cage 0.1.4-4 on Debian 12 exports neither virtual-input manager).

## File Structure

| File | Responsibility |
|---|---|
| `crates/kiosk-smoke/` | **new workspace member** — bring-up, fixture server, spool oracle, preflight; dep: `serde_json` only |
| `crates/kiosk-smoke/tests/smoke_linux.rs` | scenario bodies for A 1–7 · B 8–12 · C 13–15 · D 16–17 |
| `crates/kiosk-smoke/src/bin/clear_probe.rs` | A 6's harness binary |
| `.github/workflows/build.yml` | **new** — `on: workflow_call`, two jobs (`linux`, `windows`), lifted from `ci.yml:30-63` + `kioskctl` |
| `.github/workflows/ci.yml` | `build-windows`/`build-linux` rewritten as calls; new `smoke-key` and `smoke-linux` jobs |
| `.github/workflows/endurance.yml` | **new** — nightly, three jobs, one shared build |
| `.github/workflows/release.yml` | **new** — tag `v*` + `workflow_dispatch(dry_run)` |
| `crates/kiosk-core/tests/live_token_exchange.rs` | **new**, `#[ignore]`d — RT-09 |

---

### Task 1: The harness crate (F1)

**Files:**
- Create: `crates/kiosk-smoke/Cargo.toml`, `src/lib.rs`, `src/compositor.rs`, `src/httpd.rs`, `src/spool.rs`, `src/preflight.rs`
- Modify: root `Cargo.toml:3` — add the member

**Interfaces:**
- Produces: `Compositor::weston_headless()`, `Compositor::cage(cmd)`, `FixtureServer::start(dir) -> u16`, `Spool::events(data_dir) -> Vec<Value>`, `preflight(kiosk_bin, kioskctl_bin) -> Result<(), EnvError>`
- Consumes: `KIOSK_BIN`, `KIOSKCTL_BIN` (both **mandatory**)

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn a_missing_kiosk_bin_is_an_environment_error_not_a_scenario_failure() {
    std::env::remove_var("KIOSK_BIN");
    assert!(matches!(binaries_from_env(), Err(EnvError::Missing("KIOSK_BIN"))));
}

/// The fixture server is std::net::TcpListener, ~40 lines. NOT `python3 -m http.server`:
/// the debian:12 image ships no python3, and adding one is a container dependency for what
/// stdlib does.
#[test]
fn the_fixture_server_serves_a_file_from_a_directory() {
    let dir = tempdir_with(&[("home.html", "<h1>home</h1>")]);
    let port = FixtureServer::start(dir.path()).unwrap();
    let body = http_get(&format!("http://127.0.0.1:{port}/home.html")).unwrap();
    assert!(body.contains("<h1>home</h1>"));
}

/// The spool oracle reads the on-disk spool — A's own design (telemetry asserted from the
/// durable record, no fake-GCL endpoint needed). rt13.rs's refusal of spool assertions is
/// scoped to that in-process FSM test and is not precedent against an out-of-process run.
#[test]
fn the_spool_oracle_counts_events_by_name() {
    let dir = tempdir_with_spool(&[r#"{"event":"nav.blocked"}"#, r#"{"event":"nav.committed"}"#]);
    assert_eq!(count_events(dir.path(), "nav.blocked"), 1);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p kiosk-smoke`
Expected: FAIL — the crate does not exist.

- [ ] **Step 3: Implement the four components**

- **Compositor bring-up** — weston headless for A/B/D; cage headless for C. F owns the bring-up and exports `WAYLAND_DISPLAY`; the tests assume it.
- **Fixture HTTP server** — `std::net::TcpListener`, ~40 lines.
- **Spool oracle** — `serde_json` over the on-disk spool.
- **Binary under test** — `std::process::Command` on `KIOSK_BIN`. In CI that is the downloaded **release** artifact, which is stronger than `cargo test --release` (that would only match the *profile*). The harness target itself compiles in `dev`.
- **Fixture signing** — shell out to `kioskctl` via `KIOSKCTL_BIN`. No crypto crate in the harness.

- [ ] **Step 4: Implement the preflight**

`KIOSK_BIN` mandatory is fine for a local developer, but that binary must also be built with the harness's key or every signed-config scenario fails **looking like a product bug**. The preflight signs a trivial config and asserts acceptance, failing as a **runner/environment** error distinct from a scenario failure — reusing the classification the flake policy already has.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p kiosk-smoke && cargo clippy -p kiosk-smoke --all-targets -- -D warnings`
Expected: PASS, clean.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml crates/kiosk-smoke
git commit -m "feat(smoke): harness crate — compositor, fixture server, spool oracle, preflight"
```

---

### Task 2: Scenario bodies for A 1–7, B 8–12, C 13–15, D 16–17 (F1)

**Files:**
- Create: `crates/kiosk-smoke/tests/smoke_linux.rs`, `crates/kiosk-smoke/src/bin/clear_probe.rs`, `crates/kiosk-smoke/fixtures/**`

**Interfaces:**
- Consumes: Task 1's harness; **each scenario's assertions and fixture parameters come from the owning spec** (A, B, C, D) and are cited by ID, never re-derived here

> **Ownership boundary.** A–E own what each scenario asserts, its fixture shape and its pass criteria — already written, in prose, in their specs. F owns the **bodies**. F's dependency is on A–E's **specs**, which exist, not on their code.
>
> **Where the bodies come from.** A–D each landed a human-run `packaging/smoke/` harness as their own merge gate. This task **re-homes those bodies** into `crates/kiosk-smoke` — a port, not a rewrite. Read each sibling's scenario prose as the source of truth and the shell harness as the working reference; delete `packaging/smoke/` only once the ported scenario passes in the container, so the merge gate is never absent.

- [ ] **Step 1: Port the A 1–7 bodies**

Read `2026-08-06-p2a-linux-bringup-design.md:292-311` and implement each scenario exactly as written. Mark every test `#[ignore]`.

- [ ] **Step 2: Implement A 6's harness binary**

`crates/kiosk-smoke/src/bin/clear_probe.rs` — it does not exist anywhere in the tree today (`crates/kiosk-core/examples/` holds exactly one file, `kioskctl.rs`).

- [ ] **Step 3: Port the B 8–12 bodies**

From `2026-08-06-p2b-linux-hardening-egress-design.md:174-194` (rev 3's §Smoke additions, including 10(d) keyboard and 10(e) print).

- [ ] **Step 4: Port the C 13–15 bodies under cage**

From P2-C §C15. `cage -v`, **not** `cage --version` — the latter exits 1 and would abort under `set -e`. C 13–15 run under `cage -- kiosk-launcher` with `WLR_BACKENDS=headless`.

Expect and tolerate the `("job", …)` breadcrumb in `startup-degraded.txt`: smoke runs outside systemd, so C12's `INVOCATION_ID` guard **correctly fires**. Scenarios 13–15 must not read it as a failure.

- [ ] **Step 5: Port the D 16–17 bodies**

From P2-D §Smoke additions. Scenario 17 and C's 14 use the **`GDK_BACKEND=x11` + `xdotool`** driver on the floor.

- [ ] **Step 6: Verify locally**

Run: `KIOSK_BIN=target/release/kiosk-main KIOSKCTL_BIN=target/debug/examples/kioskctl cargo test -p kiosk-smoke -- --ignored`
Expected: the scenarios your local compositor supports pass; record which ones need the container.

- [ ] **Step 7: Commit**

```bash
git add crates/kiosk-smoke
git commit -m "test(smoke): scenario bodies for A 1-7, B 8-12, C 13-15, D 16-17"
```

---

### Task 3: `build.yml` — one reusable workflow, two jobs (F8)

**Files:**
- Create: `.github/workflows/build.yml`
- Modify: `.github/workflows/ci.yml:30-63` — rewrite both jobs as calls

**Interfaces:**
- Produces: `on: workflow_call` with one input, `pubkey_b64`; two jobs, `linux` and `windows`; artifacts including `kioskctl`

> **Actions has no cross-workflow job-body reuse.** The options are `workflow_call`, a composite action (which cannot carry a job), or copy-paste. **Both jobs, not one:** `build-windows` has three consumers — `ci.yml`, the release job, and endurance job (c) — which is precisely the argument that justified extracting the Linux one.

- [ ] **Step 1: Write `build.yml`**

Lift `ci.yml:30-63` verbatim: the apt line, the toolchain pin, `cargo build --release -p kiosk-main -p kiosk-launcher`, `upload-artifact` with `if-no-files-found: error`. Add building `kioskctl` and uploading it with the binaries. Two jobs in one file rather than two files: **one place the toolchain pin lives.**

- [ ] **Step 2: Add the `pubkey_b64` input with all three guards**

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

**Guard 2 — a non-empty check as the first step of both jobs, before checkout.** `required: true` rejects a *missing* input but not an empty string:

```yaml
      - name: Guard — SEC-01 key must be non-empty
        shell: bash
        run: '[ -n "${{ inputs.pubkey_b64 }}" ] || { echo "::error::pubkey_b64 empty"; exit 1; }'
```

Placed **before checkout** so it cannot be reached past a caller mistake.

**Guard 3 — three callers, three distinct sources, none reachable by a dispatcher:**

| Caller | Source |
|---|---|
| `release.yml` | `pubkey_b64: ${{ vars.KIOSK_CONFIG_PUBKEY_B64 }}` — repository configuration, **not** the dispatch caller |
| `ci.yml` | the `smoke-key` job's output (Task 4) |
| `endurance.yml` | the `smoke-key` job's output |

- [ ] **Step 3: Rewrite `ci.yml`'s two build jobs as calls**

`uses: ./.github/workflows/build.yml`. **Everything else in `ci.yml` is untouched** — `lint-test` keeps its four commands, and after P2-C lands, `cargo test --workspace` picks up RT-13 with **no `ci.yml` edit** (the line is unscoped: no `-p`, no `--test` filter).

- [ ] **Step 4: Verify by an actual run**

Push the branch and confirm `ci` is green with the same artifacts as before. Inspection is not verification for workflow changes.

- [ ] **Step 5: Commit**

```bash
git add .github/workflows/build.yml .github/workflows/ci.yml
git commit -m "ci: extract build into a reusable workflow with a guarded pubkey input"
```

---

### Task 4: The `smoke-key` and `smoke-linux` jobs (F2, F3, F4)

**Files:**
- Modify: `.github/workflows/ci.yml` — add `smoke-key` and `smoke-linux`

**Interfaces:**
- Produces: `smoke-key`'s public-half output; the per-PR functional gate

**The smoke key is generated per run and never committed.** A small `smoke-key` job ahead of `build` builds `kioskctl` (the `kiosk-core` graph only — no tauri), runs `kioskctl keygen`, emits the public half as a job output and **masks the seed**. `build.yml` receives it and stays a dumb consumer with no key policy in it. **This adds no build** — the harness already needs `kioskctl` to sign fixtures, so the build moves earlier rather than duplicating.

> Repo doctrine is explicit that test keys are generated at run time and are "never a committed fixture (a `-----BEGIN PRIVATE KEY-----` in the repo is the highest-signal pattern secret scanners hunt for)". Ephemeral keys **delete** the substitutable artefact rather than protecting it. Net: the only private key in existence lives for one workflow run.

- [ ] **Step 1: Add `smoke-key`**

- [ ] **Step 2: Add `smoke-linux`**

`runs-on: ubuntu-22.04` (matching `lint-test`'s image), plus `weston` and the four GStreamer packages. `needs: [smoke-key, build]`; downloads the smoke-keyed Linux release artifact plus `kioskctl` and sets `KIOSK_BIN` / `KIOSKCTL_BIN`.

**Fast subset — ten scenarios:** A 1–3, 5, 7 · B 8–11 · D 16.

**Exclusions, exhaustive — union of include and exclude is exactly 1–18, no gaps, no double-listing:**

| Excluded | Why |
|---|---|
| A 4 | crash-kill; timing-flaky candidate, scheduled until it proves stable |
| A 6 | superseded per-PR by D 16; A 6 runs in `endurance` as the unit check |
| B 12 | **scheduled-only** — B's degrade assertion is predicated on "the container has no systemd", and `ubuntu-22.04` is a full VM that *has* systemd, so `systemd-inhibit` plausibly succeeds and the scenario asserts nothing |
| C 13–15 | cage chain; scheduled. RT-13 gives per-PR coverage of hang→restart and exit-86 **at the FSM level** but does **not** cover C 13's cage chain, C 15's zombie reap or C 14's pinpad driver |
| D 17 | needs the Xwayland driver; scheduled |
| E 18 | a soak is never per-PR |

**Failure artifacts:** spool, compositor logs, best-effort screenshots, uploaded on failure.

- [ ] **Step 3: Measure the wall clock (F3)**

**Under 10 minutes or the subset shrinks** — a gate developers route around is worse than a smaller gate. **Scope:** it bounds `smoke-linux`'s *own* wall clock — artifact download, apt, weston bring-up, ten scenarios — starting after `needs: build`. The shared build is **not** charged against it: that build already exists in `ci.yml` today and runs in parallel with `lint-test`; **F adds no new build to the per-PR path.**

Record the first measurement in the PR description. If it exceeds 10 minutes, drop scenarios (not the rule).

- [ ] **Step 4: Verify by an actual run, twice**

Once green, and once with a deliberately-broken fixture to confirm it reddens. **The recorded broken-fixture run is a merge gate.**

- [ ] **Step 5: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: per-PR smoke-linux gate on ten scenarios with an ephemeral smoke key"
```

---

### Task 5: The `endurance` nightly workflow (F5, F6, F7)

**Files:**
- Create: `.github/workflows/endurance.yml`

**Interfaces:**
- Produces: three jobs sharing one `build` — the A–D matrix, the offline-video soak, and the Windows leaking-page soak

> **One nightly workflow running all three jobs every night.** Alternating nights is incompatible with F9's gate — a run-level `--status=success` would then certify a run that legitimately never executed the soak. Affordable because no endurance job builds anything.
>
> **Interlock, written down rather than discovered:** *if a future change splits `endurance` across nights, F9's gate must move to job level (`gh run view <id> --json jobs`) in the same change.*

- [ ] **Step 1: Job (a) — the full A–D matrix in a `debian:12` container (F5)**

Target fidelity: the distro's actual WebKitGTK/GStreamer, and the C7 platform floor. **Runtime packages only** — `libwebkit2gtk-4.1-0`, the four GStreamer packages, `weston`, **`cage`, `xwayland`, `xdotool`** — no `-dev` set, no Rust toolchain; the job consumes `build`'s artifacts.

Apply the compositor map. Scenarios 14 and 17 use `GDK_BACKEND=x11` + `xdotool` (`wlr_xwayland_create` **is** in cage 0.1.4's create list, so this works on the floor).

Also run **G's G15 container-scope assertions, by reference to G's register**. Assert `systemctl is-enabled` → `enabled`; **do not assert `is-active`** — a `debian:12` container has no PID-1 systemd, so that assertion would fail for environmental reasons on every nightly run. `active` is G's hardware row **H2**. F names the gate, **G owns its content**.

- [ ] **Step 2: Add the container probe**

One probe pinning three declared assumptions in the same step: an `ldd` check that the 22.04-built binary runs in the `debian:12` container (glibc 2.35 vs 2.36); the weston backend-flag spelling on **both** `ubuntu-22.04` and `debian:12` (`--backend=headless-backend.so` vs `--backend=headless`); and `cage -v` against the recorded floor. Fallback if `ldd` fails: build inside the container and re-budget job (b).

- [ ] **Step 3: Job (b) — the offline-video soak (F6)**

```yaml
    timeout-minutes: 330    # 5h30m inside the platform's hard 6h. The job-level DEFAULT is
                            # 360 — exactly the cap — so an overrunning job is killed BEFORE
                            # the artifact-upload step runs and the RSS series and spool are
                            # lost on the one run where they matter most.
```

The soak step is **derived, not asserted**:

```
soak_step = 330 − measured_setup − 20        # 20 min reserve: artifact upload + teardown
initial value: 270 min (4 h 30 m), until the probe measures setup
```

`if: always()` on the artifact-upload step. The container installs **runtime packages only** and builds nothing.

This still discharges PF-05 — the parent's word is *"multi-hour"*, not eight. E's pass criteria are duration-agnostic. **The ≥72 h obligation does not move and was never CI's:** it is P2-G checklist row **H5**.

The RSS series is retained as an artifact on pass as well as failure — justified as **trend data**, not as discharging a §10 assertion (that phrase belongs to the Windows-runner sentence, under job (c)).

- [ ] **Step 4: Job (c) — the Windows leaking-page soak (F7)**

`runs-on: windows-latest`, `strategy.matrix.scenario: [18-W2]`, consuming `build.yml`'s windows artifacts (both binaries — E's scenarios drive launcher+main).

**F names no parameters and no assertions.** Fixture configuration, thresholds and pass criteria are **E's scenarios 18-W1 and 18-W2**; E's register is the source of record and F tracks them by ID.

**Ships as `[18-W2]` only.** 18-W2 runs at `max_webview_mem_mb = 0` — it needs E4's sampler and the nightly-reload path and **not** enforcement — whereas 18-W1 asserts the breach chain, i.e. the enforcement E5's own merge gate is waiting on **this job** to justify. **`18-W1` is added to the matrix by E5's enforcement commit** (one commit, two files; owner: whoever implements E5). Building the matrix over both on day one gives a job that cannot go green.

**Dependency direction, one way only:** F7 depends on E4 and the 18-W1/18-W2 bodies. It does **not** depend on E5. F7 *produces an artifact* — the RSS series — which E's floor gate reads, and **an artifact is not a dependency**.

**Hard dependency, declared:** if E4 or the 18-W1/18-W2 bodies are withdrawn, F7 is unrunnable and parent §10's Windows-soak row **returns to UNOWNED in the ledger rather than silently passing**.

- [ ] **Step 5: Verify by an actual dispatch run**

Run the workflow manually once end to end and record each job's wall clock, especially job (b)'s measured setup, then set `soak_step` from the formula.

- [ ] **Step 6: Commit**

```bash
git add .github/workflows/endurance.yml
git commit -m "ci: nightly endurance — A-D matrix, offline-video soak, Windows leak soak"
```

---

### Task 6: RT-09 live token exchange (F11)

**Files:**
- Create: `crates/kiosk-core/tests/live_token_exchange.rs`

**Interfaces:**
- Consumes: the existing client at `crates/kiosk-core/src/logging/{auth,client,transport}.rs`

> Verified unowned: `grep -rn "RT-09"` across all specs returns three hits, all inside the parent, and the only `#[ignore]` in the workspace is `signature.rs:204` — so no live-smoke test exists.

- [ ] **Step 1: Write the test**

`#[ignore]`d, same pattern as `signature.rs:203-204`: a real RS256 → oauth2 token exchange plus one `entries:write` against a throwaway service account. Document the invocation in a comment above it, as `signature.rs:200` does.

- [ ] **Step 2: Note what is deliberately not duplicated**

The *other* half of that §10 sentence is already discharged in P1: `crates/kiosk-core/src/logging/auth.rs:531` `jwt_claims_match_googles_server_to_server_contract`.

- [ ] **Step 3: Verify it is skipped by default**

Run: `cargo test -p kiosk-core`
Expected: the new test reports as ignored, not run.

- [ ] **Step 4: Commit**

```bash
git add crates/kiosk-core/tests/live_token_exchange.rs
git commit -m "test(core): RT-09 live token-exchange smoke, ignored by default"
```

---

### Task 7: The release workflow (F8, F9, F10, F12, F15)

**Files:**
- Create: `.github/workflows/release.yml`

**Interfaces:**
- Consumes: `build.yml`, G's `packaging/linux/` tree, `packaging/windows/sign.ps1`, the RT-09 test (Task 6)

Triggers: tag push `v*`, plus `workflow_dispatch` with a `dry_run` input. `checkout` sets **`fetch-depth: 0`** — without history `git merge-base --is-ancestor` errors, and being fail-closed it would refuse every tag.

- [ ] **Step 1: The endurance-green gate, as the tag job's first gate (F9)**

```bash
gh run list --workflow=endurance --branch=main --status=success --limit=1 \
  --json headSha,createdAt,conclusion
```

Refuse the tag unless **both**: `git merge-base --is-ancestor <headSha> <tag_sha>` succeeds, **and** the run falls inside a freshness window (pick the value here and record it). **Both, not either** — ancestry alone admits a year-old soak; recency alone admits a soak of a tree that lacks the tagged commit. **Fail-closed on a query error.**

Two scheduling facts adopted as spec text: scheduled workflows run only on the default branch, and in a public repo they auto-disable after 60 days of inactivity. Both would make the gate silently *unavailable* rather than red — **the freshness window is what converts "endurance stopped running" into a refused tag instead of a green one.**

- [ ] **Step 2: The `.deb` step (F12)**

F specifies only the **invocation contract**: G's `packaging/linux/` provides the tree; the release job runs **G's flow, in G's order** — `dpkg-shlibdeps` → `dpkg-gencontrol` → `dpkg-deb -b`, then `lintian --fail-on error` gating the `.deb` before it is attached. Per F-CITE, name the steps G named and nothing else. **Do not infer `dpkg-buildpackage`, `debian/rules` or `dh_*`** — G's register has zero hits for all of them.

- [ ] **Step 3: Authenticode signing, per-artifact not per-tag (F10)**

> The Windows artifact set is produced by its own job. If the signing cert is absent or `sign.ps1` throws, **that job fails and its artifacts are excluded from the draft release**; the `.deb` job and the draft release still complete, with the release body recording the Windows set as absent. **No unsigned Windows artifact ever reaches a release.**

Mechanically: the publish job carries `needs: [build-deb, build-windows]` **and** `if: always()`, with an explicit `contains(needs.*.result, 'failure')` branch writing the "Windows artifact set absent" line — without `if: always()` a failed Windows job skips publish and the coupling returns.

**Four inputs the release job must be given:**
1. Network egress to Microsoft — the bundle build downloads the Evergreen WebView2 bootstrapper when absent and verifies its Authenticode signature.
2. An out-of-band standalone installer for offline builds (`-p:WebView2InstallerPath=…`, deliberately not committed).
3. The signing cert via the **`Pfx`** parameter set — the `Thumbprint` set requires a certificate already in a cert store and a fresh hosted runner has none: materialise a base64 repo secret to a temp `.pfx`, set `KIOSK_SIGNING_PFX_PASSWORD`, pass `-PfxPath`, and let the script's own `finally` block remove the imported cert and key.
4. **HTTPS egress to a timestamp authority** — `sign.ps1` declares `[Parameter(Mandatory)] [uri]$TimestampUrl` and throws unless the scheme is `https`.

Note: `-Stage Installers` requires `-NewerThan` dependency paths, so the MSI sign call must pass the built PE paths.

**Do not add a separate `signtool verify /pa` step** — `sign.ps1` already runs it after every sign and throws on non-zero. Add instead the assertion `sign.ps1` **cannot** make: a **coverage** check — enumerate the release set, diff against the set passed to `sign.ps1`, fail on non-empty.

- [ ] **Step 4: The RT-09 step**

Run the ignored test with `-- --ignored` when the throwaway-SA secret is present and **skip when absent** — the parent's own word, so absence is a **skip** here, unlike F10 where the parent says fail.

- [ ] **Step 5: Prove the two negative paths via `dry_run` (F15)**

Both **refused tag on a red endurance** and **a Windows artifact set refused for a missing cert** run via `workflow_dispatch` + `dry_run: true`: every gate runs (endurance-green + ancestry, lintian, signing coverage) and the publish step is skipped. **No real `v*` tag is consumed and there is no draft release to clean up**, and both proofs are re-runnable on demand by anyone instead of once.

**The dry-run showing a refused tag against a red endurance is a merge gate.**

- [ ] **Step 6: Commit**

```bash
git add .github/workflows/release.yml
git commit -m "ci: release-on-tag with endurance gate, lintian gate and per-artifact signing"
```

---

### Task 8: The meta-test and the flake policy (F14, F15)

**Files:**
- Modify: `crates/kiosk-smoke/tests/smoke_linux.rs` — the standing known-bad scenario
- Modify: `.github/workflows/ci.yml` — retry + flaky reporting
- Create: the standing flaky-smoke issue (create-if-missing)

- [ ] **Step 1: Add the standing known-bad scenario**

~10 lines: run a known-bad fixture and assert the harness reports it as a failure. **Standing, per-PR** — the draft made this a one-time run documented in the landing PR, after which nothing detected the smoke job ceasing to fail on a broken scenario. It cannot rot silently.

- [ ] **Step 2: Implement the flake policy**

A failed smoke scenario retries **once** within the job (compositor startup races are the realistic flake class). A pass-on-retry is reported as pass **with a `flaky` line in `$GITHUB_STEP_SUMMARY`** *and* a durable comment:

```bash
gh issue comment "$FLAKY_ISSUE" --body "flaky: <scenario> · <run-url> · <sha>"
```

on a **standing flaky-smoke issue named in this workflow, create-if-missing** so the mechanism does not depend on a manual pre-step. `GITHUB_TOKEN` with `permissions: issues: write`; fork PRs get a read-only token, so the step is `continue-on-error: true` and the summary line is the fallback there. No cross-run state, no rolling window, no bot, no external store.

> Retry-once *is* the laundering mechanism; the paper trail is what makes it safe. Dropping the trail to a `flaky` line on a **green** run's summary — read by nobody — keeps the laundering and loses the control.

**Automated quarantine stays withdrawn:** it assumes durable cross-run state over a rolling window plus an actor to perform the move; Actions provides neither, and it automates a judgement call made a few times a year.

- [ ] **Step 3: Add the release-step owner check**

**Whoever cuts the release** reads the standing flaky issue as a release step; a scenario with two or more comments since the previous tag is fixed or moved to `endurance` before the tag is pushed. Write this into `release.yml` as a checklist line in the draft-release body.

- [ ] **Step 4: Keep runner-environment failures distinct**

apt mirror down, compositor won't start at all, the Task 1 preflight failing → fail the job **distinctly** from scenario failures, so the signal stays clean.

- [ ] **Step 5: Commit**

```bash
git add crates/kiosk-smoke .github/workflows/ci.yml .github/workflows/release.yml
git commit -m "ci: standing known-bad scenario, retry-once with a durable flaky trail"
```

---

### Task 9: The update-path parity statement (F13)

**Files:**
- Modify: `README.md` or `docs/` — one short section

- [ ] **Step 1: Write it**

Windows P1/P2 ships no auto-updater: update = install the new MSI. Linux matches: update = install the new `.deb` (`dpkg -i` / operator tooling). Devices tolerate the restart by construction (spool durability, launcher exit/restart semantics).

Recorded ponytails, deliberately **not** P2 scope: an apt repository, `unattended-upgrades` policy, delta/A-B updates. **The `unattended-upgrades` fact lives in G's runbook; cite it and do not restate it**, so the citation loop between the two specs runs one way.

- [ ] **Step 2: Commit**

```bash
git add README.md
git commit -m "docs: update-path parity — .deb install matches the MSI model"
```

---

## Self-Review

**Spec coverage:** F1 → T1, T2; F2/F3/F4 → T4; F5/F6/F7 → T5; F8 → T3, T7; F9 → T7 Step 1; F10 → T7 Step 3; F11 → T6; F12 → T7 Step 2; F13 → T9; F14 → T8; F15 → T4 Step 4 + T7 Step 5 + T8 Step 1; F16 → T5 Step 2 (the container probe is the pinning mechanism; the platform F *certifies* is `debian:12`, so an `ubuntu-22.04` retirement cannot silently change what P2 is certified against).

**Residual risks, each with a named carrier:** the smoke gate exercises a binary differing from a shipping one in exactly `KIOSK_CONFIG_PUBKEY_B64` — a variation production already has, since every operator bakes a different key and today's `build-linux` bakes none at all; C-chain regressions caught nightly rather than per-PR; scenario 14/17's CI driver proves the **X11** GDK backend, with the Wayland path hardware-gated at P2-G H4a.

**Not F's:** what each scenario asserts and its fixture parameters (A–E); scenario 18/18-W1/18-W2 bodies and soak pass criteria (P2-E); `.deb` content, the build tree, the runbook, image pinning and the hardware checklist (P2-G); Android CI rows (P3).
