# P2-F — CRITIC, Round 2

Everything below was re-checked in-session. Where I accept, I say what changed my
assessment. Where I counter, the counter is against the **replacement**, not the original.

## Disposition of Round-1 objections

| OB | Writer's move | My response | Status |
|---|---|---|---|
| OB-1 | F takes option (a): F owns compositor bring-up, fixture server, spool reader, ~15 scenario bodies and A 6's harness binary; W2 superseded | The loop is closed the only way the four texts allow. Dependency inverts from A–E *code* to A–E *specs*, which exist. | **ACCEPTED** |
| OB-2 | F7 rebased onto E's revised design (subtree RSS, shipped key, 18-W), hard-dependency clause adopted | Boundary now agrees **verbatim** with E (E-R2:106-111). But F7 copies E's **Round-1** parameters, and the single-run config it specifies is the exact combination E-R2 has since ruled impossible. | **COUNTERED** |
| OB-3 | Soak stops building: shared `build` job + `download-artifact`; runtime deps only; duration derived (270 min initial), `if: always()` upload | Arithmetic now closes with ~40 min of setup headroom; E accepted the duration ask (E-R2:117-120). | **ACCEPTED** |
| OB-4 | `git merge-base --is-ancestor` **and** freshness; open decision #3 closed (one nightly, three jobs); split-interlock written into spec text | Both holes closed; closing #3 is the right call and OB-3's shared build pays for it. One implementation note below. | **ACCEPTED** |
| OB-5 | `gh issue comment` on a standing flaky issue, `issues: write`, `continue-on-error` on forks; demotion stays dropped; owner = whoever cuts the release | The control is back, and the owner is a checkpoint F already has rather than an invented role. | **ACCEPTED** |
| OB-6 | `KIOSK_BIN` → the downloaded **release** artifact; profile question retired rather than answered | Choosing the artifact over the profile is strictly better than `--release`. The *binary under test* is fixed. The *compile* cost is not — see OB-13. | **ACCEPTED** |
| OB-7 | `.github/workflows/build.yml` with `on: workflow_call`; `ci.yml`/`endurance.yml`/`release.yml` all `uses:` it; W9 withdraws "F touches no existing job" | Mechanism is real and correctly chosen. But the extraction covers `ci.yml:45-63` only — the Windows build (`:30-43`) that F10 must sign is left duplicated. | **COUNTERED** |
| OB-8 | Windows artifacts in their own job; failure excludes them from the draft release, `.deb` and release still complete; redundant verify → signing-**coverage** assertion; timestamp URL named; `Pfx` set + `-NewerThan` | Genuinely decoupled, not reordered, and the gate still fails for the right reason. C3 stated both directions. One `needs:` note below. | **ACCEPTED** |
| OB-9 | Declared ask on G with a fallback, symmetric with F6's on E; G15's two asks accepted | The ask is **placed** in the right form. Its **content** (`dpkg-buildpackage -b -us -uc`) contradicts G's settled register, which is a `dpkg-shlibdeps` → `dpkg-deb` flow. | **COUNTERED** |
| OB-10 | B 12 → scheduled-only, in the container where B's precondition holds | One word, correct one. | **ACCEPTED** |
| OB-11 | Pin = the nightly `debian:12` matrix (C7 floor); issue-watching dropped | This is a mechanism that runs whether or not anyone watches; §4.4 satisfied. | **ACCEPTED** |
| OB-12 | `workflow_dispatch` + `dry_run: true` for the negative proofs; broken-fixture proof becomes a **standing** per-PR scenario | Dry-run retires the tag-namespace problem and makes two of three proofs repeatable. | **ACCEPTED** |

**ACCEPTED 9 · COUNTERED 3 · ESCALATED 0.** Two new objections below, both MED.

---

## OB-2 — COUNTERED: F7 is synchronised to E Round 1, and its single run cannot hold

**What breaks.** F7's replacement text specifies **one** run:
`max_webview_mem_mb: 256`, `health_sample_s: 10`, `nightly_reload` set a few minutes
ahead, asserting (a) rising `webview_rss_mb`, (b) exit 80 + `watchdog.restart{code:80}`,
(c) post-reload RSS below the pre-reload peak.

E-R2 (`P2E-R2-writer.md:73-83`) splits 18-W into **two runs** and states why, in the same
words I would have used: *"(b) and (c) are split into two runs — they genuinely cannot
share a fixture … a re-tripping cap resets the nightly-reload timer."*

| Run | Config (E-R2:80-81) | Asserts |
|---|---|---|
| 18-W(b) | cap **256**, `health_sample_s 10`, **`healthy_run_s = 30`**, `nightly_reload` **unset** | RSS climbs; breach → exit 80 → restart + `watchdog.restart{code:80}`; **no `watchdog.safe_mode`** |
| 18-W(c) | cap **0 (off)**, `nightly_reload` a few minutes ahead | zero restarts; post-reload RSS below pre-reload peak |

F7's config is (b)'s cap plus (c)'s reload in one process — with dwell ≈50 s the cap trips
and restarts long before the reload fires, so assertion (c) measures a fresh process, not a
reload. F7 also omits `healthy_run_s = 30` and the `no watchdog.safe_mode` assertion, both
of which E added because without them 18-W(b) escalates into safe mode at the accelerated
cadence (E-R2:64-71).

**Why it matters.** C9 — as written, F7's job cannot produce assertion (c). And the failure
is structural, not clerical: F7 **restates E's parameters**, so every E revision desyncs F
silently. The boundary statement itself is fine and I accept it — E-R2:106-111 confirms
F's table verbatim in both directions, and E adopts F's unrunnable-if-withdrawn clause as
E-side text, which is exactly what I asked for at Q3.

**What answers it.** One edit: F7 runs **E's scenarios 18-W(b) and 18-W(c)** — two invocations
of the same `windows-latest` job — and F names **no parameters at all**, citing E's table as
the source of record. That is the same discipline F applied to `.deb` content at F12 and to
the fast subset at F2.

**Evidence.** `P2E-R2-writer.md:64-83, 106-111`; `P2F-R2-writer.md:95-102`.

---

## OB-7 — COUNTERED: the reusable build covers one of the two builds it must cover

**What breaks.** F: *"Extract the release build into `.github/workflows/build.yml` … containing
the apt line and `cargo build --release -p kiosk-main -p kiosk-launcher` that **`ci.yml:45-63`**
has today."* That is `build-linux`. `build-windows` (`ci.yml:30-43`, re-verified: `windows-latest`,
the identical `cargo build --release`, `upload-artifact` with `if-no-files-found: error`) is not
extracted — yet F10 must sign `kiosk-main.exe` and `kiosk-launcher.exe`, and `sign.ps1`'s
`-Stage Binaries` **requires exactly those two files** (`sign.ps1`: *"Binaries stage requires
exactly kiosk-main.exe and kiosk-launcher.exe"*), so the release workflow needs a Windows
release build. As written it gets one by copy-paste — the drift OB-7 objected to, halved
rather than removed.

Also worth stating since F7 now exists: `endurance` job (c) runs on `windows-latest` and needs
the same Windows binaries (E's 18-W drives launcher+main, per F's own note at
`P2F-R2-writer.md:105-107`). So **three** workflows want the Windows build, which is the same
argument F used to justify extracting the Linux one.

**Why it matters.** Q2/Q5 — the fix is to make `build.yml` a two-job reusable workflow (or two
reusable workflows), which is a one-line extension of a decision already taken, not a new
design. Low cost, and leaving it out reintroduces exactly the divergence W9 was withdrawn to
prevent.

**Not contested:** `workflow_call` as the mechanism, the W9 withdrawal, or the coupling. I
checked the coupling the Moderator flagged: `build.yml` is called by three workflows, so a
broken build reds all three — which is correct, they all consume one artifact. The caller-ref
semantics are also right (release builds the tag; endurance builds the default branch). No
failure-mode coupling defect.

---

## OB-9 — COUNTERED: the ask is placed correctly, but its content contradicts G's settled register

**Form: accepted.** F12 now reads as a declared ask with a named fallback ("F specifies the
invocation itself and G owns content only — R1's situation, but declared rather than silent").
That is symmetric with F6's ask on E and it is what OB-9 demanded. The `debian/source/lintian-overrides`
citation checks out (`P2G-R1-writer.md:468`, verbatim).

**Content: the inference does not survive G's Round 2.** F infers "the canonical invocation for
what G has already chosen is `dpkg-buildpackage -b -us -uc`". G's settled register says
otherwise:

- `P2G-R2-writer.md:458` (OB-12): *"Requires F's release job to run `dpkg-shlibdeps` **before
  `dpkg-deb`**; declared as a dependency on F"* — G names `dpkg-deb` as the assembler, in the
  round that settled.
- G3 (register, `:478`): *"`${shlibs:Depends}` via `dpkg-shlibdeps` for libraries"*, with F's
  release job named as the runner.
- G7 adopts debhelper's **autoscripts** (the maintscript *bodies*) verbatim — not `dh`, not
  `debian/rules`. `grep -n "dpkg-buildpackage\|debian/rules\|dh_"` over `P2G-R2-writer.md`
  returns **zero hits**.

So the two specs now name different toolchains for the same artifact: G = hand-built tree +
`dpkg-shlibdeps` + `dpkg-gencontrol`-shaped substitution + `dpkg-deb`; F = `dpkg-buildpackage`,
which would run `dh_shlibdeps` itself and make G's "F runs `dpkg-shlibdeps`" dependency
meaningless. F's own R2 contains both halves: it accepts G15's asks (which presuppose G's flow)
while asking G to adopt the other.

**Why it matters.** This is now an integration item, not a within-spec defect — both registers
have settled and they disagree. It needs one line in the ledger, not another round: F's
**fallback** already resolves it correctly (F specifies the invocation; G owns content), and
the invocation consistent with G's settled text is `dpkg-shlibdeps` → `dpkg-gencontrol` →
`dpkg-deb`, not `dpkg-buildpackage`. I would strike F's `dpkg-buildpackage` sentence and keep
the fallback.

---

## New objections

### OB-13 — The harness's *location* drags the whole dev dependency graph into the per-PR job (MED)

**What breaks.** F1 places the harness at `crates/kiosk-main/tests/smoke_linux.rs` and F's OB-6
answer claims *"The harness target itself compiles in `dev` (fast; it is assertion code)"* and
*"No second release build inside the 10-minute budget."* The first half is false and the second
is true only of the *release* build.

An integration test under `crates/kiosk-main/tests/` is a target **of the `kiosk-main` package**.
Building it requires compiling `kiosk-main`'s lib and its entire dependency graph in `dev` —
`tauri` (→ `wry` → `webkit2gtk-sys`), `reqwest`, `tokio`, `sysinfo`, `kiosk-core`
(`crates/kiosk-main/Cargo.toml:9-26`; the workspace lock has **510** packages). And cargo builds
the package's **bin** targets when running its integration tests — which F's own design depends
on, since `CARGO_BIN_EXE_kiosk-main` (F's stated local default) does not exist unless cargo built
the bin. So the smoke job still performs a full debug build of the same graph the shared release
job just built, and `KIOSK_BIN` changes only *which* binary is exercised, not whether one is
compiled.

**When.** Every per-PR run; worst on the first run of the new job, where `Swatinem/rust-cache`
is cold (its default key includes the job, so `smoke-linux` does not inherit `lint-test`'s
cache).

**Why it matters.** F3 was a clean pass in R1 on the basis that the rule is self-pinning. It
still is — but F's R2 cost model ("assumes a downloaded release binary, not an in-job build",
register row F3) is wrong about where the cost went, so the first measurement against the
10-minute rule will be taken with a mistaken idea of what is being measured. Under C9 the
budget is part of the gate.

**What answers it, cheaply.** Put the harness in its own workspace member — a `smoke` crate whose
only job is to spawn a binary path, talk HTTP over `std::net::TcpListener`, and read spool files
(`serde_json`, or `kiosk-core` if the spool types are wanted). No `tauri`, no `wry`. `KIOSK_BIN`
becomes mandatory instead of defaulted, which removes the `CARGO_BIN_EXE_` coupling that caused
this. Compile cost drops to seconds and F3's budget becomes about *runtime*, which is what F says
it is about.

**Evidence.** Tier 3: `crates/kiosk-main/Cargo.toml:9-26`, `Cargo.lock` (510 `[[package]]`),
`ci.yml:20-23, 54-56`. Tier 5: cargo's package-scoped compilation of integration tests and its
`CARGO_BIN_EXE_` contract — which F's own text relies on, so the point stands on F's text alone.

### OB-14 — F5 adopts a G15 assertion that G has since withdrawn as unrunnable (MED)

**What breaks.** F's OB-9 answer accepts G15's install/remove/upgrade cycle into F5's nightly
`debian:12` job and lists it as asserting *"unit enabled **and active**"*. That is G's **Round-1**
text (`P2G-R1-writer.md:459-463`). G's Round 2 register withdraws exactly that:

> G15 | ⚑ **Container scope reduced to what a systemd-less runner can answer**, plus
> `is-enabled` → `enabled` (proven runnable); **`active` moved to H2** (`P2G-R2-writer.md:491`)

and G14 confirms the destination: *"**H2 gains `is-active` → `active`** (moved from G15)"* (`:490`).

A `debian:12` container has no running systemd — the same fact that put B 12 on the scheduled
side one objection ago (OB-10, accepted). `systemctl is-active` there cannot return `active`, so
F5 would specify an assertion that fails for environmental reasons on every nightly run.

**Why it matters.** C9, and it is the third instance this round of F synchronising against a
sibling's R1 text (F7 vs E-R2, F12 vs G-R2, F5 vs G-R2). None of the three is a reasoning error
— F wrote concurrently and says so — but all three land in the same place: **F should cite
sibling scenarios and asks by name and let the sibling's register carry the content.** One
editorial rule fixes all three and prevents the fourth.

**What answers it.** F5 asserts `systemctl is-enabled` → `enabled`, plus the file-mode /
no-secret / upgrade-preservation checks G kept in the container, and takes G's `dpkg-shlibdeps`
addition to F8. `active` belongs to G's H2.

**Evidence.** `P2G-R2-writer.md:490-491`; `P2G-R1-writer.md:459-463`; `P2F-R2-writer.md:302-306`.

---

## Implementation notes (not objections — one line each, no response needed)

- **OB-4.** `git merge-base --is-ancestor` needs history: `actions/checkout` defaults to
  `fetch-depth: 1`, under which the check errors and — being fail-closed — refuses every tag.
  Set `fetch-depth: 0` on the release job's checkout.
- **OB-8.** For the draft release to complete while the Windows job failed, the publish job
  needs `needs: [build-deb, build-windows]` **plus** `if: always()` (and an explicit
  `contains(needs.*.result, 'failure')` branch for the "Windows set absent" release-body line);
  without `if: always()` a failed dependency skips the publish job and the coupling returns.
- **OB-5.** The standing flaky issue needs to exist before the first comment; name it in the
  spec (or have the step create-if-missing) so the mechanism does not depend on a manual
  pre-step.

---

## Still open

1. **OB-2** — F7 must run 18-W(b) and 18-W(c) as two runs and stop restating E's parameters.
2. **OB-7** — extract the Windows release build too; three workflows now consume it.
3. **OB-9** — strike `dpkg-buildpackage`; G's settled flow is `dpkg-shlibdeps` → `dpkg-deb`.
   Integration item, F's declared fallback already resolves it.
4. **OB-13** — move the harness out of `crates/kiosk-main/tests/` into its own member crate.
5. **OB-14** — F5 asserts `is-enabled`, not `active`.

Items 1, 3 and 5 are all "cite the sibling, do not copy it" and can be closed by one editorial
rule plus three edits.
