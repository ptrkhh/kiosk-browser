# P2-F — WRITER, Round 4 (final)

## The `pubkey_b64` guard — ADOPT, all three lines

He is right that I introduced a C5 surface with my own OB-7 fix, and right about which
failure actually happens. A `default:` on a `workflow_call` input is silent: a caller that
forgets the input still builds, still signs, still ships — fail-**open** on the SEC-01 root
of trust (`signature.rs:63`, `option_env!`, fail-closed only when *unset*, which a default
guarantees it never is). Adopted verbatim.

**1 — Required, no default.**

```yaml
# .github/workflows/build.yml
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

**Where the guard sits:** first step of *both* jobs in `build.yml`, before checkout, so it
cannot be reached past a caller mistake — `required: true` rejects a missing input but not
an empty string:

```yaml
      - name: Guard — SEC-01 key must be non-empty
        shell: bash
        run: '[ -n "${{ inputs.pubkey_b64 }}" ] || { echo "::error::pubkey_b64 empty"; exit 1; }'
```

**2 — `release.yml` never takes it from dispatch.** `release.yml`'s only
`workflow_dispatch` input is `dry_run` (F15/OB-12). The key comes from repository
configuration, not from the caller of the dispatch:
`uses: ./.github/workflows/build.yml` with `pubkey_b64: ${{ vars.KIOSK_CONFIG_PUBKEY_B64 }}`.
Nothing a dispatcher types can reach it, so the dry-run path cannot mint a release-shaped
artifact against a substituted root of trust.

**3 — Ephemeral smoke key, never committed.** Adopted, and it is the better fix because it
deletes the thing rather than protecting it. Repo doctrine is explicit at
`crates/kiosk-main/Cargo.toml:44-53` — the RSA keypair for the telemetry tests is
"generated at run time, mirroring kiosk-core's own auth.rs tests — **never a committed
fixture**"; `kiosk-core/Cargo.toml` carries the same comment. A committed smoke key would be
the one private key in the tree.

Mechanism: a small `smoke-key` job in `ci.yml` and `endurance.yml`, ahead of `build`. It
builds `kioskctl` (kiosk-core graph only — no tauri), runs `kioskctl keygen`, emits the
public half as a job output and masks the seed. `build.yml` receives it as `pubkey_b64` and
stays a dumb consumer with no key policy in it. This is not an added build: F1 already needs
`kioskctl` to sign fixtures, so the build moves earlier rather than duplicating.

Net: three callers, three distinct key sources, no default on any path, and the only private
key in existence lives for one workflow run.

## Integration items — all three confirmed

1. **18-W(b)/(c) → 18-W1/18-W2.** Confirmed at `P2E-R3-writer.md:61-62` ("E renames:
   **`18-W1`** … and **`18-W2`** … One word, permanent") — the collision is with F's own
   `endurance` job letters (a)/(b)/(c), so E's rename is correct and F's was the ambiguity.
   F7's matrix reads `strategy.matrix.scenario: [18-W1, 18-W2]`. Two tokens, exactly because
   F-CITE means F carries no parameters to re-sync.
2. **`substvars` flag — stale, withdrawn.** `P2G-R3-writer.md:199` names all three:
   "`dpkg-shlibdeps` → `dpkg-gencontrol` → `dpkg-deb -b`", with the reason (`${shlibs:Depends}`
   is a `dpkg-gencontrol` substvar; `dpkg-deb -b` over a hand-written `DEBIAN/control` emits
   the literal). F8 consumes **three** tools in that order. My R3 "unnamed in both registers"
   line is deleted — G named it. G's `grep -q '\${shlibs' && FAIL` assertion rides along in
   F5's G15 set by reference.
3. **E5 ← 18-W2 floor, recorded in both directions.** Confirmed at
   `P2E-R3-writer.md:224`: E5's enforcement half is gated on 18-W2's recorded floor
   (≥ 750 MB ⇒ defect against parent §5.2 rather than shipping). F7 therefore produces a
   **merge-gating measurement for E**, not only a test result. Recorded on F's side: F7's
   register row now carries the outbound edge, and F7's artifacts (the RSS series) are
   retained as the evidence E's gate reads.

## Termination criteria — my confirmation

**Confirmed.** P2-F is internally consistent; all 14 objections are dispositioned; no open
HIGH. Specifically: both HIGH findings closed by redesign, not by acceptance — OB-1 by F
taking ownership of the harness (the only resolution the four sibling texts permit), OB-2 by
rebasing F7 onto E's register by reference. Every §10 obligation is covered or has a named
owner; the three that were uncovered at verification (Authenticode gate, Windows soak,
RT-09) are adopted, none deferred. Every cross-spec edge is declared in both registers: F↔E
(F6 duration ask settled, F7 ↔ 18-W1/18-W2, F7 → E5's floor gate), F↔G (G15 container
assertions, `dpkg-shlibdeps` → `dpkg-gencontrol` → `dpkg-deb -b`, lintian), F→C (RT-13
de-gating). The one residual I carry is declared, not silent: the C3 note that the smoke
gate exercises a binary differing from a shipping one in exactly the `KIOSK_CONFIG_PUBKEY_B64`
constant — a variation production already has, since each operator bakes a different key.

## Final register — F1…F16

⚑ = changed in Round 4.

| ID | Final state | Depends on |
|---|---|---|
| F1 | `crates/kiosk-smoke`, `serde_json` only; mandatory `KIOSK_BIN` / `KIOSKCTL_BIN`; F owns bring-up, fixture server, spool reader, scenario bodies A 1–7 · B 8–12 · C 13–15 · D 16–17 incl. A 6 | A–E specs (definitions); `kioskctl` |
| F2 | ⚑ `needs: [smoke-key, build]`; consumes the ephemeral-key artifact + `kioskctl` | F1, `build.yml`, `smoke-key` |
| F3 | Bounds `smoke-linux`'s own wall-clock after `needs: build`; "under 10 min or the subset shrinks" | F2 |
| F4 | Exclusion list exhaustive; B 12 scheduled-only | F2, F5 |
| F5 | G's G15 container-scope assertions **by reference** (`is-enabled` → `enabled`; `active` is G's H2); ⚑ includes G's `${shlibs` literal-check | F1, `build.yml`, G (G15) |
| F6 | Soak 270 min initial, derived `330 − setup − 20`; `timeout-minutes: 330`; runtime deps only; `if: always()` upload | `build.yml`; E (ask settled) |
| F7 | ⚑ `matrix.scenario: [18-W1, 18-W2]`; F names no parameters; hard-dependency clause mirrored in E; ⚑ **outbound edge — F7's floor measurement merge-gates E5's enforcement** (`P2E-R3:224`) | E4 + E5 + 18-W1/18-W2 (in); **E5 (out)**; `build.yml` (windows) |
| F8 | ⚑ `build.yml` = two jobs (linux + windows) with `pubkey_b64` **required, no default**; ⚑ `.deb` flow is **`dpkg-shlibdeps` → `dpkg-gencontrol` → `dpkg-deb -b`** → `lintian --fail-on error`; `workflow_dispatch` dry-run takes `dry_run` only; publish `needs: […]` + `if: always()` | `build.yml`, G (settled), P1-F2 |
| F9 | Ancestry **and** freshness; one nightly workflow, three jobs; split-interlock in spec text; `fetch-depth: 0` | F5, F6, F7, F8 |
| F10 | Consumes `build.yml`'s windows job; per-artifact fail; signing-coverage assertion; four inputs incl. HTTPS timestamp authority; `Pfx` parameter set | `build.yml`; `sign.ps1`; cert + PFX secrets |
| F11 | RT-09 `#[ignore]` test + creds-present step, skips when absent | `kiosk-core/src/logging/*`; SA secret |
| F12 | `dpkg-buildpackage` struck; conforms to G's register; ⚑ `substvars` flag **withdrawn — G named `dpkg-gencontrol`** | G (settled) |
| F13 | Update path = install-the-new-`.deb` | G runbook |
| F14 | `gh issue comment` on a standing, create-if-missing flaky issue; `continue-on-error` on forks; demotion dropped; owner = whoever cuts the release | F8/F9 |
| F15 | Broken-fixture proof standing per-PR; negative proofs via dry-run | F2, F8, F9 |
| F16 | Pin = the nightly `debian:12` matrix (C7 floor) | F5, F6 |

**New this round:** the `pubkey_b64` guard (declaration, placement, per-caller sourcing) and
the `smoke-key` ephemeral-keygen job. **Withdrawn this round:** R3's stale `substvars`
integration flag.
