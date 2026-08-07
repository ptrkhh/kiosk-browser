# P2-F — CRITIC, Round 3 (closing)

All five REVISE items verified. F-CITE is the right root-cause fix and it is the reason the
residuals below are renames rather than redesigns.

## Disposition

| Item | Writer's move | Verified | Status |
|---|---|---|---|
| **OB-13** | Harness → `crates/kiosk-smoke`, `serde_json` only; `KIOSK_BIN`/`KIOSKCTL_BIN` mandatory, `CARGO_BIN_EXE_` fallback dropped; R2's "compiles in dev (fast)" retracted | New member reaches nothing: root `Cargo.toml:3` members are core/main/launcher, and `serde_json` has no path to `tauri`/`wry`/`webkit2gtk-sys`. Dropping the fallback is what severs the coupling — with it, cargo must build `kiosk-main` to define the variable. `ci.yml:25` lints the member free; `ci.yml:26` compiles a cheap test target and skips the `#[ignore]`s. | **CLOSED** |
| **OB-7** | `build.yml` = two jobs (`linux` + `windows`), both lifted from `ci.yml:30-63`; `pubkey_b64` input | `ci.yml:30-43` confirmed uncovered before; three consumers confirmed (F2, F10's `-Stage Binaries` two-file requirement, F7's launcher+main on Windows). `signature.rs:63` is `option_env!("KIOSK_CONFIG_PUBKEY_B64")` — compile-time, fail-closed; `boot.rs:93-95` and the test at `:257-259` confirm a keyless build rejects every fetched config. A signed-config smoke run genuinely needs a keyed build. | **CLOSED**, one guard required below |
| **OB-2** | F7 = `strategy.matrix` over E's 18-W(b)/(c), F names zero parameters | Matches E's split and its stated reason (`P2E-R2:74-83`), and E adopts the reciprocal clause its side (`:106-111`). E has since renamed the runs — integration item 1. | **CLOSED** |
| **OB-9** | `dpkg-buildpackage` struck; F runs G's flow in G's order | `grep "dpkg-buildpackage\|debian/rules\|dh_"` over G's register → zero hits, confirmed. G has since named the missing step itself — integration item 2. | **CLOSED** |
| **OB-14** | F5 runs G15's container assertions **by reference** | Correct shape: a by-ID reference survives G's R3 expansion of G15 automatically. This is F-CITE working. | **CLOSED** |

R1 objections 1–12 and R2's 13–14: **14 of 14 dispositioned, none open, no HIGH remaining.**

## Required guard — `pubkey_b64` (MED, not open HIGH)

The input is correct and necessary; the hazard is that nothing yet forbids the two ways it
goes wrong. `pubkey_b64` selects the Ed25519 root of trust for SEC-01 in the **shipped**
binary, and it now lives in a workflow three others consume, one of which (`release.yml`)
gained `workflow_dispatch` at F15.

Three lines of spec text, all cheap, and I would not close without them:

1. **Required input, no default.** A `pubkey_b64` with a default is the failure that would
   actually happen: `release.yml` omits it once and ships a binary trusting a key CI holds —
   fail-**open** on a C5 gate, and invisible, because the binary looks keyed. Missing input
   must fail the workflow. *If this ends up defaulted, it is HIGH.*
2. **`release.yml` never accepts it from `workflow_dispatch`.** The dry-run input set is
   `dry_run` only; the operator key comes from secrets. Otherwise anyone with write access
   can publish a binary trusting a key they chose.
3. **The smoke key is generated per run** (`kioskctl keygen`, already built and uploaded by
   `build.yml` per F1), never committed and never stored. Repo doctrine is explicit on this —
   `crates/kiosk-main/Cargo.toml:48-50`: *"never a committed fixture (a
   `-----BEGIN PRIVATE KEY-----` in the repo is the highest-signal pattern secret scanners
   hunt for)"* — and G15 asserts zero such strings in the `.deb`. Ephemeral keys remove the
   substitutable artefact entirely rather than protecting it.

With (1)–(3) the C3 divergence note at F8 is accurate as written and I accept it: one
compile-time constant, already per-deployment in production, and today's `build-linux` bakes
no key at all.

## Local-developer path — one line, not an objection

`KIOSK_BIN` mandatory is fine (`cargo build -p kiosk-main` + point at it), but the binary
must *also* be built with the harness's key or every signed-config scenario fails looking
like a product bug. Reuse the classification F already has: a preflight that signs a trivial
config and asserts acceptance, failing as a **runner/environment** error distinct from a
scenario failure (F's flake policy, unchanged). No new mechanism.

## Integration items (flagged, not defects)

1. **18-W(b)/(c) → 18-W1/18-W2.** `P2E-R3-writer.md:61-62` renames them explicitly citing
   the F label collision, and `:88` names the re-sync as a hard E→F edge. F carries no
   parameters, so this is a two-token rename at integration — F-CITE is exactly what makes it
   that and not a redesign.
2. **`substvars` is no longer unnamed.** F's flag was true against G-R2 and is **stale
   against G-R3**, which names all three tools: `dpkg-shlibdeps` → `dpkg-gencontrol` →
   `dpkg-deb -b` (`P2G-R3-writer.md:199`), with G15 asserting no literal `${shlibs` survives
   in the package (`:249`). F's release job takes **three** tools, not two. Item closed by G,
   needs one word in F.
3. **New E→F edge, uncarried.** `P2E-R3-writer.md:224` gates E5's *enforcement* half on
   18-W2's recorded steady-state floor — so F7 now produces a measurement that is a merge gate
   on E, not only a consumer of E. F's register shows the dependency in one direction only.

## Consistency confirmation

**Confirmed, independently.** P2-F is internally consistent: its sixteen changes carry no
mutual contradiction, every claim I checked this round holds against the repo, all fourteen
objections are dispositioned, and **no HIGH remains open** — OB-1 (harness ownership) and
OB-2 (F7's unobservable assertions) are both resolved at the design level, not deferred.
The three integration items are cross-spec token drift that F-CITE reduces to renames, and I
accept them as documented risk. The `pubkey_b64` guard is a required addition, not an open
defect: the design as stated routes the keys correctly, and the guard pins that nothing else
can.

## Budget verdict — reversal, stated plainly

I said twice that the per-PR gate was not buildable within its own budget. **That is no
longer true, and I reverse it.** Both causes are gone:

- R1/R2 cause — the gate rebuilt what `build-linux` already builds. Fixed by `needs: build` +
  the downloaded **release** artifact (OB-6).
- R2 cause — the harness's *location* dragged the 510-package dev graph into every PR. Fixed
  by `crates/kiosk-smoke` with `serde_json` alone, and by dropping the `CARGO_BIN_EXE_`
  fallback that made the graph mandatory (OB-13).

What remains inside the ten minutes is artifact download, one apt install (weston + four
GStreamer packages), weston bring-up and ten scenarios. That is a credible budget, F3's scope
is now stated so the first measurement measures the right thing, and the rule was always
self-pinning — *"under 10 minutes or the subset shrinks."* **Buildable.**
