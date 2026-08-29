# VERIFIER — P2-F (CI Functional Gate, Scheduled Endurance, Release Artifacts)

Target: `docs/superpowers/specs/2026-08-06-p2f-ci-functional-gate-design.md`
Repo: `/home/user/kiosk-browser` @ `1decd59`
Role: fact-check only. No proposals, no opinions.

Counts: **VERIFIED 32 · FALSE 8 · DRIFT 4 · UNVERIFIABLE 4 · UNCOVERED (parent §10) 3**

---

## 1. `ci.yml` current state — F's factual foundation

Only one workflow file exists: `/home/user/kiosk-browser/.github/workflows/ci.yml` (63 lines).
F's "Current state" paragraph reads:

> `ci.yml` today: `lint-test` (ubuntu-22.04: fmt, clippy `-D warnings` workspace, full
> test suite, `cargo check -p kiosk-main`), `build-windows`, `build-linux` (release
> binaries as artifacts).

Checked claim by claim against the file:

| # | Claim | Verdict | Evidence (`ci.yml`) |
|---|---|---|---|
| 1 | `lint-test` exists | **VERIFIED** | `:11 lint-test:` |
| 2 | on `ubuntu-22.04` | **VERIFIED** | `:12 runs-on: ubuntu-22.04` |
| 3 | fmt | **VERIFIED** | `:24 - run: cargo fmt --check` |
| 4 | clippy `-D warnings` workspace-wide | **VERIFIED** | `:25 - run: cargo clippy --workspace --all-targets -- -D warnings` |
| 5 | full test suite | **VERIFIED** | `:26 - run: cargo test --workspace` |
| 6 | `cargo check -p kiosk-main` | **VERIFIED** | `:27-28` step named `Linux compile check (kiosk-main)`, `run: cargo check -p kiosk-main` |
| 7 | `build-windows` exists, release binaries as artifacts | **VERIFIED** | `:30-43`; `windows-latest`; `cargo build --release -p kiosk-main -p kiosk-launcher`; `upload-artifact@v4` with `if-no-files-found: error` |
| 8 | `build-linux` exists, release binaries as artifacts | **VERIFIED** | `:45-63`; `runs-on: ubuntu-22.04`; same release build; artifact `kiosk-linux-x86_64-${{ github.sha }}` |
| 9 | webkit 4.1 dev deps already installed in `lint-test` | **VERIFIED** | `:15-19` — `sudo apt-get install -y libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev libssl-dev` |

**Zero divergence found between F's "Current state" section and the real workflow file.**
This section is accurate in every particular, including the parenthetical in §1
("matching `lint-test`'s image and the webkit 4.1 dev deps already installed there") —
`build-linux` installs the identical apt line (`:49-53`), so the image and dep set claim
holds for both existing Linux jobs.

Two facts F does not state but which bear on it (neither contradicts F):
- The `lint-test` apt line installs **no** `weston`, **no** `cage`, **no** GStreamer.
  F correctly treats those as additions.
- `build-linux` is a full `--release` build of both binaries, i.e. the Linux side is
  already stronger than the parent's "compile check" wording. F's Current-state sentence
  reports this correctly.

---

## 2. `ubuntu-22.04` runner label

| # | Claim | Verdict | Evidence |
|---|---|---|---|
| 10 | `ubuntu-22.04` is a valid GH Actions runner label and is what the workflow pins | **VERIFIED** | `actions/runner-images` `README.md` "Available Images" table lists `Ubuntu 22.04` / YAML label `` `ubuntu-22.04` ``, x64, **not** carrying a `deprecated` badge (macOS 14 rows in the same table do carry one). `ci.yml:12` and `:46` both pin `ubuntu-22.04` literally. |

**Tier-5 risk note, stated factually, no argument attached.** Upstream currently lists
Ubuntu 26.04 and 26.04-Arm64 with a `preview` badge alongside GA 24.04 and 22.04. The same
README states the policy verbatim:

> We support (at maximum) 2 GA images and 1 beta image at a time. We begin the deprecation
> process of the oldest image label once the newest OS image label has been released to GA.

> Images begin the deprecation process of the oldest image label once a new GA OS version
> has been released.

So: `ubuntu-22.04` is **not** deprecated as of this check, but the trigger condition for its
deprecation (26.04 reaching GA) is one step away. F pins `ubuntu-22.04` in `smoke-linux` and
does not list image retirement among its open decisions. Whether retirement lands before or
after P2 ships is **UNVERIFIABLE** here (see §11); the pinning mechanism available is
watching the pinned announcement issue in `actions/runner-images`.

---

## 3. RT-13

| # | Claim | Verdict | Evidence |
|---|---|---|---|
| 12 | `crates/kiosk-launcher/tests/rt13.rs` exists | **VERIFIED** | 16,331 bytes; plus `tests/bin/mock_main.rs` and a `[[bin]] name = "rt13-mock-main"` target in `crates/kiosk-launcher/Cargo.toml:9-11` |
| 13 | it is `#[cfg(windows)]`-gated | **VERIFIED** | `rt13.rs:27` — `#![cfg(windows)]` (inner attribute, whole file). Only `cfg(` in the file. `mock_main.rs:27-31` prints `"rt13-mock-main is Windows-only (kiosk-launcher's IPC is a named pipe)"` on non-Windows. |
| 14 | *(F's implication)* `cargo test` on ubuntu currently runs it | **FALSE** | The file-level `#![cfg(windows)]` empties the whole integration-test target on Linux. `cargo test --workspace` on ubuntu compiles the `rt13` target and runs **0 tests** from it today. |
| 15 | "After C lands, `cargo test` already includes RT-13 — the supervise loop is a per-PR gate with zero F work" | **VERIFIED (dependency direction correct; conditional on C)** | See below. |

Claim 15 checks out on both halves:

- **Dependency direction.** P2-C owns the de-gating, not F. C's spec has a section
  `### RT-13 — cross-platform, becomes the CI gate` (`p2c…:140`) reading: "`tests/rt13.rs`
  + the `rt13-mock-main` bin are `#[cfg(windows)]`-gated today … C makes the …" and C's
  Testing section lists "**RT-13 on Linux CI** (gate)". C's Scope also names
  "update/CI-harness (P2-F)" as **out** of C. No circularity: C de-gates, F consumes.
- **Would the existing invocation pick it up?** Yes. `ci.yml:26` is
  `cargo test --workspace` — workspace-scoped, no `-p` narrowing, no `--lib`/`--test`
  filter. `kiosk-launcher` is a workspace member (`Cargo.toml` `members = [...,
  "crates/kiosk-launcher"]`), and `tests/rt13.rs` is an auto-discovered integration
  target. Once the `cfg` gate goes, the existing command runs it with no `ci.yml` edit.
  F's "zero F work" is literally true.

Cross-reference finding (a **C** defect, recorded here because it surfaced during this
check, not an F claim): C cites the gate at `rt13.rs:32,101-112`. The actual `#![cfg(windows)]`
is at **`rt13.rs:27`**. The pipe-name template citation `rt13.rs:107` is correct
(`r"\\.\pipe\kiosk-heartbeat-rt13-{tag}-{}-{}"`). **DRIFT** (#16), five lines off.

RT-13's actual test inventory, for the C13–15 exclusion rationale in §6:
`healthy_child_arms_the_watchdog_and_is_never_restarted` (`:291`),
`a_hung_child_is_detected_and_restarted_within_the_miss_window` (`:324`),
`a_crashing_child_is_restarted_with_its_real_exit_code` (`:359`),
`exit_86_stops_the_launcher_without_a_restart` (`:384`).

---

## 4. Release / tag machinery, and what F2 actually delivered

| # | Claim | Verdict | Evidence |
|---|---|---|---|
| 17 | *(F §3 premise)* tag-push release plumbing is new work | **DRIFT — but in F's favour, with one wording snag** | `ci.yml:2-5` triggers are `push: branches: [main]` and `pull_request`. **No `tags:` filter, no `release` workflow, no checksum step, no draft-release step, no `softprops`/`gh release` usage anywhere.** F §3's phrase "Tag push `v*`: **existing** release builds" is loose — the build *jobs* exist, but nothing about them is tag-triggered, so "existing" describes reusable job bodies, not existing release machinery. |
| 18 | `packaging/windows/` contains a WiX build | **VERIFIED** | `kiosk.wxs`, `kiosk.wixproj`, `bundle.wxs`, `bundle.wixproj`, plus `sign.ps1`, `verify-webview2.ps1`, `install-task.ps1`, `KioskLauncher.xml`, `lockdown.md`, `README.md` |
| 19 | it is **not** CI-wired (F2 "built it locally") | **VERIFIED** | No `wix`, `dotnet`, `msbuild`, `signtool` or `.wixproj` reference anywhere in `ci.yml`. `packaging/windows/README.md` documents the build as a local PowerShell invocation: `dotnet build packaging/windows/bundle.wixproj -c Release`, with an "Equivalent WiX CLI command" block. |
| 22 | F2 shipped the signing invocation | **VERIFIED** | `packaging/windows/sign.ps1` exists; F2 spec `:41-44` — "a build/CI step `signtool sign`s both PE binaries **and** the MSI (and, if used, the Burn bundle) … F2 provides the invocation + docs; the cert itself is not in the repo." |

**F2 delivered vs. what F claims F2 delivered.** F §3 says: "the Windows MSI if/when F2's
WiX build is CI-wired (F2 built it locally; wiring is the same one-job shape — included
here for symmetry, not new design)."

"F2 built it locally" — **VERIFIED**. "Wiring is the same one-job shape" — **undeclared
assumption (#20)**, and the repo contains three specific reasons it is not shape-identical
to `build-windows`:

1. `packaging/windows/README.md`: the bundle build "downloads Microsoft's Evergreen
   WebView2 bootstrapper to `obj` when absent, **verifies its Authenticode signature is
   valid and issued to Microsoft Corporation**, then embeds both." That is a network fetch
   plus a signature-verification gate inside the build — not present in any existing job.
2. Offline releases require `-p:WebView2InstallerPath=...` pointing at a manually
   downloaded standalone installer, and "Neither Microsoft installer is committed to this
   repository." A CI job must source that artifact from somewhere F does not name.
3. F2 §Authenticode requires an "operator/CI-supplied code-signing cert" — i.e. a repo
   secret, a `signtool` invocation, and a decision about signing on forked PRs. F's §3
   mentions none of these.

**UNCOVERED (#21):** parent §10's CI row ends "**Authenticode signing step (unsigned
artifacts fail the release gate)**." F's `release` job (§3) enumerates: existing release
builds, `.deb` assembly, the Windows MSI conditionally, checksums, draft GitHub release,
and exactly one gate — "release requires the latest `endurance` run green." The
signing step and the unsigned-artifacts-fail-the-release-gate rule appear nowhere in F.
F is the release-workflow spec and is the natural owner. See §9.

---

## 5. `packaging/linux/`

| # | Claim | Verdict | Evidence |
|---|---|---|---|
| 23 | `packaging/linux/` does not exist yet (F consumes G's) | **VERIFIED** | `packaging/` contains exactly one subdirectory, `windows/`, with 10 files. No `linux/`. F correctly attributes it to G (`p2g…:25` — "### 1. `.deb` — `packaging/linux/`"). No defect. |
| 24 | ".deb assembly … (dpkg-deb; the package *content* is G's spec, F only executes it)" | **DRIFT** | G's `packaging/linux/` design specifies `postinst` scripts (`:44`, `:48`), conffile semantics ("marked conffile-adjacent so upgrades don't clobber a replaced video", `:29`), `deb-systemd-invoke` conventions (`:48`), `Conflicts`/`Replaces` handling (`:50`), and a Testing bullet requiring "**Lintian clean** (or documented overrides)" (`:108`). Those are debhelper / `dpkg-buildpackage` deliverables; `dpkg-deb --build` over a hand-built tree is a different toolchain. F names the tool while asserting "F only executes it" — F is in fact making a build-system choice that G's stated content does not obviously support. |

Circular-citation note (LOW, both directions): F §4 asserts "The G runbook pins
`unattended-upgrades` **off**." G `:72` does say `- Updates: `unattended-upgrades` off`
— but its parenthetical is "**(F §4** — update timing is operator-owned)". So F cites G
and G cites F for the same fact; neither derives it. The fact **is** present in G's
runbook section, so F's claim is **VERIFIED** as written; only the provenance is circular.

---

## 6. Scenario inventory cross-check

Source of record for each number is the owning spec's own smoke section.

### Fast subset — F says "A scenarios 1–3, 5, 7 (boot/nav/offline/iframe/safe)"

| # | F's label | A's actual scenario text (`p2a…:292-311`) | Verdict |
|---|---|---|---|
| 25a | 1 = boot | "boot → splash → remote home `nav.committed`" | **VERIFIED** |
| 25b | 2 = nav | "off-list navigation → exactly one `nav.blocked`, page unchanged" | **VERIFIED** |
| 25c | 3 = offline | "config/network down → offline fallback page loads from app origin" | **VERIFIED** |
| 25d | 5 = iframe | "**iframe:** an in-allowlist iframe loads; an off-allowlist iframe is blocked…" | **VERIFIED** |
| 25e | 7 = safe | "**safe boot:** fixture is a **malformed `kiosk.ini`**…" | **VERIFIED** |

### F says "B 8–11 (egress/downloads/dialog/permissions)"

| # | F's label | B's actual text (`p2b…:174-194`) | Verdict |
|---|---|---|---|
| 26a | 8 = egress | "8. **egress:**" | **VERIFIED** |
| 26b | 9 = downloads | "9. **downloads:**" | **VERIFIED** |
| 26c | 10 = dialog | "10. **dialog/chrome:**" | **VERIFIED** |
| 26d | 11 = permissions | "11. **permissions:**" | **VERIFIED** |

B's own header is "extend A's harness; **8–11 blocking, 12 degrade-only**" — F's fast
subset is exactly B's blocking set. Consistent.

### F says "D 16 with a seconds-scale idle threshold"

| # | Claim | Verdict | Evidence |
|---|---|---|---|
| 27 | 16 = idle→clear, short threshold | **VERIFIED** | `p2d…:117-121` — "16. **idle → clear (blocking; needs no input injection…)**: **short-threshold fixture** → `IdleExpired` observed → profile clear runs → `ProfileCleared` → session cookie gone…" F's "seconds-scale idle threshold" matches D's "short-threshold fixture". |

### Exclusion list — F says "A 4 (crash-kill…), C 13–15 (…), E 18 (…)"

| # | Claim | Verdict | Evidence |
|---|---|---|---|
| 28 | A 4 = crash-kill | **VERIFIED** | `p2a…:296` — "kill the `WebKitWebProcess` → `webview.crash` spooled + recovery navigate-home" |
| 29 | C 13–15 | **VERIFIED (numbers)** | `p2c…:152-158` — 13 full chain (`cage -- kiosk-launcher` headless, `kill -9`), 14 technician exit 86, 15 hang path (`SIGSTOP`/`SIGCONT`, zombie reap) |
| 30 | E 18 = soak | **VERIFIED** | `p2e…:94` — "## Soak protocol (scenario 18)"; E `:111` independently agrees "18 (short form per-PR is **NOT** run…)" |

F's C-exclusion **rationale** ("RT-13 covers the same FSM paths per-PR; the cage-chain
variants are scheduled") is **partially supported**: RT-13's four tests do cover
hang→restart (C15's FSM half) and exit-86 (C14's launcher half). Not covered by RT-13 and
silently riding on "the cage-chain variants are scheduled": C13's cage chain, C15's
`SIGCONT`ed-corpse zombie-reap assertion, and C14's app-path pinpad driver (which D `:137`
assigns — "C's scenario 14 (technician exit 86) gains its app-path driver from D's chord
and is re-run"). No verdict of FALSE; the umbrella clause covers them, thinly.

### #31 — **FALSE: the exclusion list is presented as exhaustive and is not**

F §1 states the fast subset, then: "**Excluded from per-PR, by design:** A 4 …, C 13–15 …,
E 18 …". Union of F's include-list and F's exclude-list = A 1,2,3,4,5,7 · B 8,9,10,11 ·
C 13,14,15 · D 16 · E 18.

Three numbered scenarios that exist in the source specs appear in **neither** list:

- **A 6** — "profile clear: no app-path producer for `ClearProfile` until P2-D, so a
  dedicated harness binary (cargo example) creates a webview under the compositor, drives
  `clear::clear` directly…" (`p2a…:304-306`). A's gate line says "scenarios 1–7 …
  **all blocking**". D `:114` says "A's harness-binary scenario (A smoke 6) stays as the
  completion unit check; D's smoke 16 supersedes it as the app-path proof" — so A 6 is
  *superseded but retained*, and F never says which bucket it lands in.
- **B 12** — keep-awake, which B marks "degrade-only" and whose positive half B routes to
  the hardware checklist (`p2b…:193-196`).
- **D 17** — "gesture + chord + activity-reset (blocking under cage-headless IF virtual
  input is available, else hardware-checklist)" (`p2d…:122-129`).

F §2(a) sweeps these up implicitly ("**full matrix** — all A–D scenarios including the
per-PR exclusions"), so nothing is lost operationally. But the §1 sentence reads as a
complete accounting of per-PR exclusions and is not one — three scenarios' per-PR status is
determined only by inference from §2. Recorded as FALSE-as-stated.

---

## 7. Does the harness F "promotes" exist?

F's header: "**Builds on the A–E smoke harness** (weston headless + signed-config fixtures
+ spool assertions) and C's cross-platform RT-13. F writes no product code: **it promotes
the harness from human-run-in-session to CI-run**."

Searched the whole repo (excluding `target/`).

| # | Assumed component | Verdict | Evidence |
|---|---|---|---|
| 32 | `kioskctl` signing harness | **VERIFIED — exists** | `crates/kiosk-core/examples/kioskctl.rs` (`keygen` / `sign` / `hash-pin` / `selftest`), referenced by `README.md:88` and `docs/testing/p1d2-signed-config-smoke.md`. The one A-harness prerequisite that is real code. |
| 33 | local HTTP fixture server | **FALSE — no code** | Only a manual runbook line: `docs/testing/p1d2-signed-config-smoke.md:212` — `python3 -m http.server 8000 # serves lobby-01.json at http://<host>:8000/lobby-01.json`; and `:75` "signed configs served from a local `127.0.0.1:8000` static server". Prose in a human checklist, not a harness. |
| 34 | weston invocation | **FALSE — zero occurrences** | `grep -rn "weston\|cage"` across all `*.rs`, `*.yml`, `*.sh` returns **nothing**. The repo contains **no shell scripts at all**: `find` for `*.sh`, `*.py`, `Makefile`, `justfile`, `*.service` returns empty. |
| 35 | spool assertion helpers | **FALSE — none as harness** | `spool` appears only in product code (`kiosk-main/src/{health,boot,main,telemetry}.rs`, `kiosk-launcher/src/{spawn,sink,main}.rs`, `kiosk-core/src/config/{validate,mod}.rs`) and in `rt13.rs`'s doc comment — which explicitly declines the approach: "The alternative (asserting on delivered spool entries) would need a service account, a transport and the whole telemetry stack…" (`rt13.rs:14-16`). No out-of-process spool assertion helper exists. |
| 36 | A 6's "dedicated harness binary (cargo example)" | **FALSE — does not exist** | `crates/kiosk-core/examples/` contains exactly one file: `kioskctl.rs`. No compositor/webview harness example in any crate. |

### #37 — **FALSE (headline): there is no harness to promote.**

A, B, C, D and E are all design specs. A is `rev 3` and reviewed; B–E are `draft,
2026-08-06 (awaiting review)`. `git log` shows the last five commits are all
`docs(spec): P2-{A..G}` — **no P2 implementation has landed**. A's own text confirms the
harness is unwritten and in-session-only:

> **Gate:** scenarios 1–7 under weston headless, **all blocking**. … The smoke is
> **human-run in-session** and is deliberately **not** wired into `ci.yml`; automating the
> compositor harness is **P2-F**. (`p2a…:312-315`)

So A's spec and F's spec agree on the hand-off *direction* — F is correctly the automation
owner. What F states as fact and is not: that a harness exists in a "human-run-in-session"
form that CI can "promote". Today the only artefacts are `kioskctl.rs` and a
`python3 -m http.server` line in a P1 markdown checklist. Every scenario body F schedules
(A 1–7, B 8–12, C 13–15, D 16–17, E 18) is prose in an unreviewed spec.

Consequence for F's own scoping sentence "**F writes no product code**": true in the narrow
sense (no `crates/*/src/` change), but F's §1 requires "the fixture tooling" to exist, and
none of it does. The work is deferred to A–E without F naming which spec owns the CI-runnable
form of each fixture. Recorded as an undeclared assumption in §12.

---

## 8. Environment presence check (this container)

Factual, no interpretation. Host is `Ubuntu 24.04.4 LTS` — neither the `ubuntu-22.04`
CI image nor the Debian 12 floor.

| Tool | Present | Detail |
|---|---|---|
| `weston` | **absent** | `apt-cache policy weston` → `Installed: (none)`, `Candidate: 13.0.0-4build3` (noble/universe) |
| `cage` | **absent** | `Installed: (none)`, `Candidate: 0.1.5+20240127-2build1` (noble/universe) |
| `dpkg-deb` | **present** | `/usr/bin/dpkg-deb` (and `/usr/bin/dpkg`) |
| GStreamer (`gst-launch-1.0`) | **absent** | not on `PATH` |
| `wlrctl` (D 17's injector) | **absent** | not on `PATH` |

Nothing in F depends on this container; recorded because item 8 asked. Note the candidate
versions are Ubuntu 24.04's, not 22.04's or Debian 12's — this host cannot stand in for
either target image.

---

## 9. Parent-spec traceability

### §10 CI row — verbatim (`2026-07-05-kiosk-browser-design.md`, §10 final bullet)

> - CI: `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test`, release build Windows
>   (P0) + Linux compile check (P0 → functional at P2), Android build (P3), Authenticode
>   signing step (unsigned artifacts fail the release gate).

**VERIFIED (#44)** — F's header quotes "Linux compile check (P0 → functional at P2)"
correctly, and that clause is the mandate F discharges.

### §10 soak/endurance rows — verbatim

> - **Soak/endurance (scheduled CI, not per-PR):** a Windows-runner job drives looped
>   navigation + a deliberately leaking page with accelerated thresholds; asserts bounded
>   RSS, that a `max_webview_mem_mb` breach fires a restart, and that nightly reload resets
>   RSS. A ≥72 h real-hardware soak is a pre-release gate (RT-05). **Offline-video soak:**
>   multi-hour loop on the pinned Debian 12 image, assert no stall/black frame across loop
>   boundaries (PF-05).

> - **Live token-exchange smoke (gated/opt-in, release gate):** a real RS256 → oauth2 token
>   exchange + one `entries:write` against a throwaway service account; skipped when creds
>   absent (RT-09).

**VERIFIED (#45)** as quotations.

### §9 P2 row — verbatim

> | **P2** | Linux + robustness | WebKitGTK parity (incl. pinch-gesture intercept, keep-awake
> at compositor), .deb + systemd + cage docs + §7.2 Linux hardening, idle reset (native),
> **memory cap restart + health-sampled RSS**, cross-platform webview-hang detection (JS
> ping), config-driven `inject_css`/`inject_js` knobs (behind signed config), remote log
> level, restart_app |

**VERIFIED (#46)** — matches FRAME §2 exactly. Nothing in this row is CI work; F correctly
claims none of it.

### #47 — Parent items assigned to P2 CI that F does **not** cover

Three, all from §10, all in F's declared domain (per-PR CI, scheduled CI, release job):

1. **Authenticode signing step / "unsigned artifacts fail the release gate".** §10 CI row,
   verbatim. F §3's release job lists checksums and a draft release but no signing step,
   and its only gate is endurance-green. F2 supplied the invocation (`sign.ps1`, F2 §41-44)
   and explicitly called it "a build/**CI** step" — the CI half is unclaimed by F.
2. **The Windows-runner soak job.** §10 specifies "a **Windows-runner** job … looped
   navigation + a deliberately leaking page with accelerated thresholds; asserts bounded
   RSS, that a `max_webview_mem_mb` breach fires a restart, and that nightly reload resets
   RSS." F's `endurance` workflow (§2) has exactly two jobs, both Linux, both in a
   `debian:12` container. No Windows job. F's §5 "Scope / defer" defers only "Package
   contents… → P2-G", "Fleet update mechanics", and "Android CI rows (§10) → P3" — the
   Windows soak row is neither covered nor deferred.
3. **Live token-exchange smoke (RT-09), named a release gate.** Absent from F's release
   job, absent from F's defer list.

### #48 — **DRIFT: misattributed §10 citation**

F §2(b): "RSS series retained as an artifact even on pass (trend data is the point, per
§10's '**assert bounded RSS**')." The phrase "asserts bounded RSS" occurs in §10 **only**
in the Windows-runner soak sentence. F attaches it to its Linux `debian:12` soak. The
offline-video soak sentence F is actually implementing says something different: "multi-hour
loop on the pinned Debian 12 image, assert **no stall/black frame across loop boundaries**
(PF-05)". The misattribution is what makes finding #47.2 easy to miss.

Correctly deferred, for the record: "Android CI rows (§10) → P3" matches parent §10
("Android build (P3)") and §9 (Android = P3). **VERIFIED.**

---

## 10. GitHub Actions capability claims (tier 5 documentary)

Sources: `github/docs` `content/actions/reference/limits.md` and
`content/actions/reference/workflows-and-actions/events-that-trigger-workflows.md`,
fetched live.

| # | Claim | Verdict | Note |
|---|---|---|---|
| 49 | run the full matrix "in a **`debian:12` container**" | **VERIFIED (capability)** | `jobs.<id>.container` is standard. F additionally declares the real risk itself as open decision #4 ("confirm no seat/device-node surprises … pin with a probe run"), which is the correct handling. |
| 50 | "Nightly cron" | **VERIFIED (capability)** | `schedule:` exists. Two unstated caveats, neither wrong in F: "Scheduled workflows will only run on the default branch" and "In a public repository, scheduled workflows are automatically disabled when no repository activity has occurred in 60 days." Neither breaks F; both bear on "a red nightly … is a release blocker" if the nightly silently stops running. |
| **51** | **"(b) soak — E's protocol at 8 h+, same container"** | **FALSE — infeasible as specified** | See below. |
| 52 | "Artifact retention windows … likely defaults suffice" | **VERIFIED (capability)** | `actions/upload-artifact` `retention-days` is configurable; F correctly files it as a plan-time measurement. |
| 53 | "a `flaky` line in the job summary" | **VERIFIED (capability)** | `$GITHUB_STEP_SUMMARY` exists. |
| 54 | "Gate: release requires the latest `endurance` run green" | **capability gap, undeclared** | Actions has **no** native cross-workflow status gate. `workflow_run` triggers on the *dependent* workflow, not on a tag push, so this requires the tag job to query run history (`gh run list --workflow=endurance --status=success`) and decide. Implementable; F states it as a plain gate with no mechanism named. |
| 55 | "Two flakes of the same scenario in **seven days** → the scenario moves to `endurance` and a tracking issue is opened" | **capability gap, undeclared** | Requires cross-run state over a rolling 7-day window. Actions provides no such store; this needs run-history/artifact scraping or an external store, plus an actor to perform the "moves to `endurance`" edit. Stated as policy-as-fact with no mechanism and no owner. |
| 56 | "Scheduled failures notify via the repo's normal mechanisms" | **VERIFIED (hedged)** | Deliberately non-specific; nothing to falsify. |

### #51 in full — the 6-hour job cap

`github/docs` `content/actions/reference/limits.md`, verbatim rows:

> | All {% data variables.product.github %}-hosted runners | **Job execution time** |
> **6 hours** | Each job in a workflow can run for up to 6 hours of execution time.
> **If a job reaches this limit, the job is terminated and fails.** | (not increasable) |

> | Self-hosted | Job execution time | 5 days | … |

> | Workflow execution limit | Workflow run time | 35 days / workflow run | … |

F §2 specifies the soak as "**8 h+**, same container". "Same container" refers back to
§2(a)'s `debian:12` container, and every runner F names anywhere in the spec is
GitHub-hosted (`ubuntu-22.04` in §1). F never mentions self-hosted runners — the only
runner class where a 5-day cap would make 8 h legal. **A single 8 h+ job on a GitHub-hosted
runner is terminated and fails at 6 h**, regardless of budget.

Two aggravating details, stated factually:

- The 6-hour cap is marked **not increasable** (no support-ticket escalation), unlike job
  concurrency and re-run limits in the same table.
- F's open-decisions list does raise runner economics — "Cron time + whether the soak job
  and full-matrix job share the nightly or split across alternating nights (**runner-minutes
  budget** question — measure first)" — but that is a *cost* question. The hard wall-clock
  cap is not named anywhere in F, including in the open decisions, so nothing in the spec
  routes an implementer into discovering it.

E's spec asserts the same figure ("**scheduled CI** 8 h+ (wired by F, run in a `debian:12`
container for target fidelity)", `p2e…:98-99`) and explicitly assigns the wiring to F, so F
is the owner of the feasibility question. Under FRAME §3 C9 ("a gate that cannot actually
run in the stated environment is a feasibility defect") and §6 HIGH ("a gate that cannot
run"), this is the one finding here that meets the HIGH bar on its own evidence.

---

## 11. UNVERIFIABLE (with the pinning mechanism each would need)

| Claim | Why unverifiable here | Pinning mechanism |
|---|---|---|
| "Wall-clock budget: **under 10 minutes**" (§1) | Requires building `kiosk-main` (Tauri + wry + webkit2gtk) on an `ubuntu-22.04` runner and executing ten compositor scenarios. Cannot be measured in this container: no `weston`, no GStreamer, wrong OS (24.04), and the cache state of a real runner is unknown. Note the fast subset shares a runner with whatever build produces the binary; F's open decision #1 (share `lint-test`'s job vs. stand alone) is unresolved, and the two options differ by a full cold `cargo build`. | **F already declares the knob**: "under 10 minutes **or the subset shrinks**", plus open decision #1 "decide on measured wall-clock of the first working version". Adequately pinned. |
| Artifact retention "likely defaults suffice" (open decision #2) | Depends on real spool/RSS-series sizes, which no run has produced. | F declares it: "decide with real sizes". Adequately pinned. |
| "`debian:12` container + weston headless inside GH Actions: confirm no seat/device-node surprises" (open decision #4) | Not reproducible here. | F declares it and names the mechanism: "**pin with a probe run before building the whole job on it**". Adequately pinned — this is the model the other assumption-class findings lack. |
| Whether `ubuntu-22.04` is retired before P2 ships | Depends on Ubuntu 26.04's GA date, unannounced. | Watch the deprecation announcement issue in `actions/runner-images` (their documented process: "Deprecation process begins with an announcement that sets a date"). **Not currently declared in F.** |

---

## 12. Claims stated as fact that are actually undeclared assumptions

Listed separately per the task. Each is asserted in F's prose without hedging and without a
named pinning mechanism, and each is checkable-in-principle but unchecked in the spec.

1. **"it promotes the harness from human-run-in-session to CI-run"** (header). Presupposes a
   human-run harness exists. It does not: A–E are specs, no P2 code has landed, and the only
   real fixture tooling in the repo is `crates/kiosk-core/examples/kioskctl.rs` plus a
   `python3 -m http.server` line in a P1 markdown checklist. F's per-PR job depends on
   "the fixture tooling" (§1) that no spec has yet committed to producing in CI-runnable
   form, and F does not name which of A–E owns that form. (Evidence: §7 above.)

2. **"E's protocol at 8 h+, same container"** (§2b). Assumes a GitHub-hosted job may exceed
   6 hours. It may not. Presented as a schedule parameter, not as an assumption. (§10, #51.)

3. **"wiring is the same one-job shape — included here for symmetry, not new design"** (§3).
   Assumes the MSI/bundle build is shape-equivalent to `build-windows`. `packaging/windows/README.md`
   documents a network fetch of Microsoft's bootstrapper, an Authenticode verification of it,
   an out-of-band standalone-installer input for offline releases, and F2 requires an
   operator-supplied signing cert — none present in any existing job. (§4, #20.)

4. **"release requires the latest `endurance` run green"** (§3). Assumes a cross-workflow
   gate. Actions has none natively; the mechanism (run-history query) is unnamed. (#54.)

5. **"Two flakes of the same scenario in seven days → the scenario moves to `endurance` and
   a tracking issue is opened"** (§Error handling). Assumes durable cross-run state over a
   rolling window and an actor to perform the move. Neither exists nor is named. (#55.)

6. **"dpkg-deb"** (§3), asserted alongside "F only executes it". Assumes raw `dpkg-deb`
   satisfies G's `postinst` / conffile / `deb-systemd-invoke` / lintian-clean requirements.
   (§5, #24.)

7. **"C 13–15 (RT-13 covers the same FSM paths per-PR)"** (§1). True for hang→restart and
   exit-86 against RT-13's four tests; assumes without saying so that C15's zombie-reap
   assertion and C14's app-path pinpad driver (assigned by D `:137`) are adequately served
   by the "cage-chain variants are scheduled" clause. (§6, #29.)

8. **`ubuntu-22.04` as the pinned per-PR image** (§1). Assumes label longevity across P2.
   Currently valid and undeprecated, but 26.04 is in preview and upstream policy begins
   deprecating the oldest GA label when a new one ships. Not listed among F's open
   decisions. (§2.)

---

## 13. Verdict summary

**VERIFIED — 32.** The entire "Current state" section (9/9 claims, exact); the
`ubuntu-22.04` label's current validity; RT-13's existence, its `#![cfg(windows)]` gate,
and the soundness of the C→F dependency including the `--workspace` scoping that makes
"zero F work" literally true; the WiX build's existence and its local-only status; the
absence of `packaging/linux/`; **every one of the 14 scenario numbers F cites**, in A, B, C,
D and E; `kioskctl`'s existence; all three parent-spec quotations; and five Actions
capabilities.

**FALSE — 8.** #14 (`cargo test` does not run RT-13 on ubuntu today), #31 (exclusion list
incomplete: A 6, B 12, D 17 unaccounted), #33/#34/#35/#36 (fixture server, weston
invocation, spool assertion helpers, A 6's harness binary — none exist as code), #37 (there
is no harness to promote), #51 (8 h+ soak exceeds the 6-hour GitHub-hosted job cap).

**DRIFT — 4.** #16 (C cites `rt13.rs:32`; actual is `:27`), #17 ("existing release builds"
when no tag trigger exists at all), #24 (`dpkg-deb` vs. G's debhelper-shaped requirements),
#48 (§10's "assert bounded RSS" belongs to the Windows-runner soak row, not the Linux one).

**UNVERIFIABLE — 4**, three of which F itself pins adequately (10-minute budget, retention
sizes, container/weston probe run); the fourth (ubuntu-22.04 longevity) is unpinned.

**UNCOVERED parent §10 items — 3.** Authenticode signing step / unsigned-fails-release-gate;
the Windows-runner soak job; the live token-exchange smoke (RT-09) release gate. None appear
in F's §5 defer list.

### Headline FALSE findings

1. **The 8 h+ soak cannot run.** GitHub-hosted runners terminate any job at **6 hours**, and
   the limit is documented as not increasable. F specifies "8 h+, same container" on hosted
   runners and never mentions self-hosted (5-day cap). F's open decisions name the
   *runner-minutes budget* but not the wall-clock cap, so nothing routes an implementer into
   the problem.
2. **F promotes a harness that does not exist.** `grep` for `weston`/`cage` across every
   `.rs`/`.yml`/`.sh` returns zero hits; the repo contains no shell scripts at all; the
   "local HTTP fixture server" is a `python3 -m http.server` line in a P1 manual checklist;
   there are no spool assertion helpers (RT-13 explicitly declines that approach at
   `rt13.rs:14-16`); A 6's "dedicated harness binary" is absent (`examples/` holds only
   `kioskctl.rs`). A–E are unimplemented specs — `git log` shows the last five commits are
   all `docs(spec)`.
3. **Three parent §10 CI obligations are neither covered nor deferred**: the Authenticode
   signing gate ("unsigned artifacts fail the release gate" — verbatim in the same §10 CI
   row F quotes for its own mandate), the Windows-runner soak job, and RT-09's live
   token-exchange release gate. F's misquote of "assert bounded RSS" onto its Linux soak is
   what obscures the second one.
4. **The per-PR exclusion list is presented as exhaustive and omits A 6, B 12 and D 17.**
   Only §2's "all A–D scenarios" catches them, by inference.

Not a defect, worth stating because it is the spec's factual foundation: **F's description
of the existing `ci.yml` is accurate in every checkable particular** — job names, runner
label, all four `lint-test` commands, both build jobs, artifact production, and the
pre-installed webkit 4.1 dev deps.
