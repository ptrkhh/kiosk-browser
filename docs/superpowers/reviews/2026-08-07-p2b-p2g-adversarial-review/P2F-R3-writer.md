# P2-F — WRITER, Round 3

The Critic names one editorial root cause behind three items and he is right: I wrote R2
concurrently with E and G and restated their content instead of citing it. I adopt the rule
as **spec text**, not just as three edits:

> **F-CITE.** F names sibling scenarios, asks and gates **by ID**. Parameters, assertions,
> fixture configuration and tool invocations belong to the owning spec's register and are
> never restated in F. Where F needs a value it does not own, F cites; where F needs the
> sibling to state one, F declares an ask.

F already applied this to `.deb` *content* (F12) and to the fast subset (F2). The three
misses are where I departed from it. F-CITE is now the rule that prevents the fourth.

---

## OB-2 — REVISE (rebase onto E's current table, by reference only)

Verified at `P2E-R2-writer.md:74-83`: E split 18-W and states the reason — "(b) and (c) are
split into two runs — they genuinely cannot share a fixture … a re-tripping cap resets the
nightly-reload timer." E's table gives 18-W(b) `max_webview_mem_mb = 256`,
`health_sample_s = 10`, **`healthy_run_s = 30`**, `nightly_reload` unset, asserting the
climb, exit 80 + `watchdog.restart{code:80}`, and **no `watchdog.safe_mode`**; 18-W(c)
`max_webview_mem_mb = 0`, `nightly_reload` a few minutes ahead, asserting zero restarts and
post-reload RSS below the pre-reload peak.

My R2 F7 specified one run with (b)'s cap *and* (c)'s reload — the combination E has since
ruled impossible — and omitted `healthy_run_s = 30` and the safe-mode assertion. Conceded on
all four counts. I also confirm E adopts the reciprocal clause as E-side text
(`P2E-R2-writer.md:106-111`), so the boundary is settled from both sides and I state it as a
citation:

**Revised F7, complete text:**

> `endurance` job (c): `runs-on: windows-latest`, `strategy.matrix.scenario: [18-W-b,
> 18-W-c]` — two runs of one job. **F names no parameters and no assertions.** Fixture
> configuration, thresholds and pass criteria are E's scenarios **18-W(b)** and **18-W(c)**;
> E's register is the source of record and F tracks it by ID. F owns the job, the runner,
> the artifacts and the scheduling. **Hard dependency, declared:** if E4, E5 or 18-W is
> withdrawn, F7 is unrunnable and parent §10's Windows-soak row returns to UNOWNED in the
> ledger rather than silently passing.

Two runs cost nothing here — the accelerated dwell is ~50 s, so both finish in minutes.

---

## OB-7 — REVISE (extract both builds; one workflow, one build per platform per run)

Conceded. Verified `ci.yml:30-43` (`build-windows`, `windows-latest`, identical
`cargo build --release -p kiosk-main -p kiosk-launcher`, `upload-artifact` with
`if-no-files-found: error`) is not covered by my extraction, and three workflows now want
it: `ci.yml`, `release` (F10 must sign `kiosk-main.exe` **and** `kiosk-launcher.exe` —
`sign.ps1`'s `-Stage Binaries` throws unless it gets exactly those two, which I read in R2),
and `endurance` (F7's 18-W drives launcher+main on Windows). Leaving it out halves the drift
instead of removing it, which was the objection.

**Revised: `build.yml` is one reusable workflow with two jobs, `linux` and `windows`**, both
lifted from `ci.yml:30-63`. `ci.yml`, `endurance.yml` and `release.yml` each
`uses: ./.github/workflows/build.yml`. Two jobs in one file rather than two files — fewer
moving parts, one place the toolchain pin lives.

**One input, which also settles a problem OB-13 surfaced.** `signature.rs:63` reads the
pinned key through `option_env!("KIOSK_CONFIG_PUBKEY_B64")` — **compile time**, fail-closed
when unset (`boot.rs:94`, `:258` both say so). A signed-config smoke run therefore requires
a binary compiled against a key whose private half the harness holds. So `build.yml` takes
`pubkey_b64` as an input:

- `ci.yml` and `endurance.yml` pass the **smoke test key** → the artifact `smoke-linux` and
  the endurance jobs consume.
- `release.yml` passes the **operator key** → the artifact that ships.

One build per platform per run either way — no increase over what `ci.yml` does today.

**C3, both directions, on OB-6's promise.** The smoke gate no longer runs byte-for-byte the
shipping artifact; it runs the same commit, same profile, same flags, differing in one
compile-time constant. That constant is *already* per-deployment (each operator's key is
different, so no single shipping binary exists across fleets), and today's `build-linux`
bakes no key at all — i.e. the current artifact is the fail-closed dev build. So this is not
a test/prod divergence, it is the variation production already has. Stated rather than
implied.

---

## OB-9 — REVISE (strike `dpkg-buildpackage`; conform to G's settled register)

Conceded, and I verified G's register myself:
`grep -n "dpkg-buildpackage\|debian/rules\|dh_" P2G-R2-writer.md` → **zero hits**.
`P2G-R2-writer.md:457`: "Requires F's release job to run **`dpkg-shlibdeps` before
`dpkg-deb`**; declared as a dependency on F alongside the lintian step." G3 (`:479`):
"`${shlibs:Depends}` via `dpkg-shlibdeps` for libraries", with **F's release job** named as
the runner. G7 (`:481`) adopts debhelper's autoscript *bodies* verbatim — not `dh`, not
`debian/rules`. My inference from `debian/source/lintian-overrides` to `dpkg-buildpackage`
was wrong, and worse: it would have run `dh_shlibdeps` itself and made G's declared
dependency on F meaningless, while F was simultaneously accepting G15's asks that presuppose
G's flow. Both halves in one turn.

**Revised F12/F8.** The `dpkg-buildpackage` sentence is struck. F's release job runs G's
flow, in G's order: `dpkg-shlibdeps` → `dpkg-deb` → `lintian --fail-on error`, over the tree
G's `packaging/linux/` provides. Per F-CITE, F names the steps G named and nothing else.
The R2 ask on G is withdrawn as satisfied — G's register answered it before I asked.

**One integration line, flagged not invented.** `dpkg-shlibdeps` writes `${shlibs:Depends}`
into `debian/substvars`; the step that substitutes it into the control file before
`dpkg-deb` (canonically `dpkg-gencontrol`) is named in **neither** register. F does not
choose it — F records it as an open integration item for whoever sequences G's tree and F's
job.

---

## OB-13 — REVISE (the location was the defect; the harness moves to its own member)

Conceded, and the Critic's reasoning holds on F's own text: `CARGO_BIN_EXE_kiosk-main` — my
stated local default — cannot exist unless cargo built the bin, so `KIOSK_BIN` fixed *which*
binary is exercised and not *whether* the graph is compiled. Verified the weight:
`crates/kiosk-main/Cargo.toml:9-26` pulls `tauri` (→ wry → webkit2gtk-sys), `reqwest`,
`tokio`, `sysinfo`, `kiosk-core`; `Cargo.lock` has **510** `[[package]]` entries. An
integration test under `crates/kiosk-main/tests/` compiles all of it in `dev`, in a job whose
`Swatinem/rust-cache` key is its own and therefore cold on first run. My R2 claim "the
harness target itself compiles in `dev` (fast; it is assertion code)" was false.

**Revised F1 location and dependency set.** New workspace member **`crates/kiosk-smoke`**,
`crates/kiosk-smoke/tests/smoke_linux.rs`, dependencies: **`serde_json` only.**

- Binary under test: `std::process::Command` on a **mandatory** `KIOSK_BIN`. Dropping the
  `CARGO_BIN_EXE_` default is what removes the coupling — there is no fallback that
  compiles `kiosk-main`.
- Fixture signing: shell out to `kioskctl` via a mandatory `KIOSKCTL_BIN`. `kioskctl` is a
  real cargo example (`crates/kiosk-core/examples/kioskctl.rs`, `keygen`/`sign`/`hash-pin`/
  `selftest`) built once in `build.yml` and uploaded with the binaries. No crypto crate in
  the harness.
- Fixture server: `std::net::TcpListener`. Spool oracle: `serde_json` over the on-disk spool
  (A `:291`'s design, unchanged).
- Scenario tests stay `#[ignore]`, so `lint-test`'s `cargo test --workspace` (`ci.yml:26`)
  skips them on a runner with no compositor; `cargo clippy --workspace --all-targets`
  (`:25`) lints the new member, which is free and desirable.

Compile cost drops from the 510-package graph to a handful of packages, and F3's budget
measures runtime, which is what F3 claims to be about.

**And F3's scope is restated so it cannot measure the wrong thing again:** the 10-minute
rule bounds `smoke-linux`'s **own** wall-clock — artifact download, weston bring-up, ten
scenarios — starting after `needs: build`. The shared build is not counted against it
because that build already exists in `ci.yml` today and runs in parallel with `lint-test`;
F adds no new build to the per-PR path.

---

## OB-14 — REVISE

Conceded, one word, and it is the same fact that moved B 12 off per-PR at OB-10. Verified
`P2G-R2-writer.md:491`: G15 "Container scope reduced to what a systemd-less runner can
answer, **plus `is-enabled` → `enabled`** (proven runnable); **`active` moved to H2**", and
`:490`: G14 "**H2 gains `is-active` → `active`** (moved from G15)". I had copied G's R1 text.

**Revised F5 addition** (per F-CITE, by reference): F5's nightly `debian:12` job runs **G's
G15 container-scope assertions as G's register defines them** — currently `systemctl
is-enabled` → `enabled`, the file-mode checks, the no-secret grep and the
upgrade-preservation checks. `active` is G's **H2**, on hardware. F names the gate, G owns
its content.

---

## Implementation notes — adopted as spec text

Flagged by the Critic as needing no response; recording adoption so they are not lost:

- **OB-4.** The release job's `actions/checkout` sets `fetch-depth: 0`; without history
  `git merge-base --is-ancestor` errors, and being fail-closed it would refuse every tag.
- **OB-8.** The publish job carries `needs: [build-deb, build-windows]` **and**
  `if: always()`, with an explicit `contains(needs.*.result, 'failure')` branch writing the
  "Windows artifact set absent" line into the release body. Without `if: always()` a failed
  Windows job skips publish and the coupling OB-8 removed returns.
- **OB-5.** The standing flaky issue is named in the spec and the step is create-if-missing,
  so the mechanism does not depend on a manual pre-step.

---

## Final change register — F1…F16

⚑ = changed this round.

| ID | Final state | Depends on |
|---|---|---|
| F1 | ⚑ Harness is **`crates/kiosk-smoke`**, a workspace member with `serde_json` as its only dependency (OB-13). Mandatory `KIOSK_BIN` and `KIOSKCTL_BIN`; no `CARGO_BIN_EXE_` fallback. F owns compositor bring-up, `TcpListener` fixture server, spool reader, and the scenario bodies for A 1–7 · B 8–12 · C 13–15 · D 16–17 incl. A 6's harness binary. Scenario definitions remain A–E's. | A–E specs (definitions only); `kioskctl` |
| F2 | ⚑ `needs: build`; consumes the smoke-key Linux artifact + `kioskctl`; subset unchanged (A 1–3,5,7 · B 8–11 · D 16) | F1, `build.yml` |
| F3 | ⚑ Rule unchanged; **scope restated** — bounds `smoke-linux`'s own wall-clock after `needs: build`, not the shared build | F2 |
| F4 | Unchanged since R2: B 12 scheduled-only; exclusion list exhaustive | F2, F5 |
| F5 | ⚑ Runs **G's G15 container-scope assertions by reference** — `is-enabled` → `enabled`, not `active` (OB-14) | F1, `build.yml`, G (G15) |
| F6 | Unchanged since R2: 270 min initial, derived `330 − setup − 20`; `timeout-minutes: 330`; runtime deps only; `if: always()` upload. E accepted the duration ask (`P2E-R2-writer.md:117-120`) | `build.yml`; E (ask **settled**) |
| F7 | ⚑ Matrix over **E's 18-W(b) and 18-W(c)**, two runs; **F names no parameters** (OB-2, F-CITE). Hard-dependency clause retained, mirrored in E | E4 + E5 + 18-W(b)/(c) (hard); `build.yml` (windows) |
| F8 | ⚑ `build.yml` extended to **two jobs, linux + windows** (OB-7), with a `pubkey_b64` input; `.deb` flow is **`dpkg-shlibdeps` → `dpkg-deb` → `lintian --fail-on error`**, G's order (OB-9); `workflow_dispatch` dry-run; publish `needs: […]` + `if: always()` | `build.yml`, G, P1-F2 |
| F9 | Unchanged since R2: ancestry **and** freshness; one nightly workflow, three jobs; split-interlock in spec text; ⚑ `fetch-depth: 0` | F5, F6, F7, F8 |
| F10 | ⚑ Consumes `build.yml`'s **windows** job rather than a copy-pasted build; otherwise unchanged — per-artifact fail, signing-coverage assertion, four inputs incl. HTTPS timestamp authority, `Pfx` parameter set | `build.yml` (windows); `sign.ps1`; cert + PFX secrets |
| F11 | Unchanged (clean pass R1) | `kiosk-core/src/logging/*`; SA secret |
| F12 | ⚑ `dpkg-buildpackage` **struck**; conforms to G's settled register. R2's ask on G **withdrawn as satisfied**. One integration line flagged: the `substvars` → control substitution step is unnamed in both registers | G (settled) |
| F13 | Unchanged (clean pass R1) | G runbook |
| F14 | ⚑ `gh issue comment` on a standing flaky issue, **create-if-missing**; `issues: write`; `continue-on-error` on forks; demotion stays dropped; owner = whoever cuts the release | F8/F9 |
| F15 | Unchanged since R2: broken-fixture proof is a standing per-PR scenario; negative proofs via `workflow_dispatch` dry-run | F2, F8, F9 |
| F16 | Unchanged since R2: pin = the nightly `debian:12` matrix (C7 floor) | F5, F6 |

**New spec text this round:** F-CITE (the citation discipline), and the
`KIOSK_CONFIG_PUBKEY_B64` divergence note at F8 with its C3 statement in both directions.

**Withdrawn this round:** R2's inference that G's flow is `dpkg-buildpackage`; R2's claim
that a `crates/kiosk-main/tests/` harness "compiles in `dev` (fast)"; R2's single-run F7
configuration; R2's `active` assertion in F5.
