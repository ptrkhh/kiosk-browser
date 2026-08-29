# INTEGRATION ROUND — CRITIC

No frame dispute. Every mechanically-checkable claim below was run in-session; the command
or file:line is given per objection. Where a thread's settled position survives my probe I
say so in **Clean passes** rather than manufacturing an objection.

## Objection index

| ID | Scope (specs touched) | Objection (one line) | Sev | Evidence tier |
|---|---|---|---|---|
| INT-1 | E, F | E5's enforcement is gated on F7's measurement and F7 declares E5 as an inbound dependency — a self-declared cycle; no merge order satisfies both edges as written | **HIGH** | 1 (spec registers) |
| INT-2 | C, D, G | Five cross-spec deferrals name P2-G rows that do not exist in G's final register (C12's orphan-kill gate, C10/C15's cage-floor assertion, C12's wedged-cage row, D13's H10, D's touch content in H4) | **HIGH** | 1 + grep |
| INT-3 | C, D, F, G | Scenarios 13–15 have no runner: F installs weston, never cage; and on the C7 floor (Debian 12 → cage **0.1.4**) no virtual-input protocol exists at all, so scenario 14's pinpad driver cannot run and, unlike 17, has no declared fallback | **HIGH** | 3, 5 (cage source) |
| INT-4 | D, G, +I3 | A wedged GTK main loop is invisible to the launcher (heartbeat is a tokio task), has no exit route (D withdrew leg 3, G10 removes VT/getty/SSH), and its covering control is the *unowned* arch-04 JS-ping. Composed: silently un-exitable device, parent §3.5 | **HIGH** | 3, 1 |
| INT-5 | G, D, ruling R2 | G's keyboard erratum evidence is version-split and one limb is false on cage 0.1.5 (`wlr_virtual_keyboard_manager_v1_create` present); the derived "separate-process OSK breaks D's ActivityClock" claim is false for the virtual-keyboard route | **MED** | 5 (binary + source) |
| INT-6 | B | B's SEC-10 soundness test cannot read the corpus it names — `allowlist.rs`'s battery is entirely inside `#[cfg(test)] mod tests` (`:144-732`), unreachable from `kiosk-main`. The sole carrier of SEC-10 soundness becomes a hand-copied second source of truth (C1) | **MED** | 3 |
| INT-7 | B, C, F | Smoke 8(d) makes a directory unwritable; root ignores that (CAP_DAC_OVERRIDE, reproduced). Root is the only supported principal (C R3) and the uid of F5's container job, so the gate proves the degrade path only on the one runner whose uid the product never has | **MED** | 3 (reproduced) |
| INT-8 | B, G | B's fail-open-with-`config.error` resolution of C4-vs-C5 is justified by that event being loud; G ships no credential and bounds retroactive upload to the spool retention window, so on an unprovisioned device the only SEC-10 degrade signal is inaudible and nothing shows on screen | **MED** | 1, 3 |
| INT-9 | C, G | G1 declares "C's Linux `spawn_main` must carry `--config`"; `--config` occurs **zero** times in P2-C's spec and its whole thread. Without it `resolve_config_dir` falls back to the exe dir → `/usr/lib/kiosk`, and config/credential/mp4 are read from the wrong tree | **MED** | 1, 3 |
| INT-10 | B, E | E1's declared P2-B shared-file edge is stale — B dropped the `#[cfg(not(windows))]` Tauri command and the `main.rs:990` edit in Round 1 | **LOW** | 1 |
| INT-11 | B, D | D10 declares `gtk` only; D1's mechanism is stated on `webkit2gtk::WebView`, which wry does not re-export and tauri's own docs tell consumers to declare. The direct dep arrives only with B10 → undeclared D→B edge | **LOW** | 4 |
| INT-12 | B | The declared WebKitGTK feature floor is not mechanically enforced: tauri 2.11.5 declares `webkit2gtk` with `features = ["v2_40"]` and Cargo unifies features, so a `v2_40` symbol compiles despite B10's `["v2_32"]` | **LOW** | 4 |

Counts: **HIGH 4 · MED 5 · LOW 3** (12 total).

---

## INT-1 — E5 ↔ F7 is a declared dependency cycle (HIGH)

**What breaks.** No merge order satisfies both specs' final registers.

**Evidence (tier 1, both final registers, verbatim).**
- `P2E-R3-writer.md:224` (E5's row): *"Ⓓ **18-W2's recorded floor is now a merge gate on E5's enforcement half**"*.
- `P2F-R4-writer.md:106` (F7's row, `Depends on` column): *"E4 + E5 + 18-W1/18-W2 (in); **E5 (out)**"*.

F7 declares E5 as both an inbound and an outbound dependency, in one cell. Both roles
recorded this as "the edge is now carried in both directions" (`P2F-R4:74-78`,
`P2E-R3-critic.md:95-99`) — carrying an edge in both directions is precisely what makes it
a cycle, and neither thread checked whether the loop closes.

**When.** At the first merge attempt. E5's enforcement half cannot land before F7 has run
18-W2; F7 as specified is one `strategy.matrix` job over `[18-W1, 18-W2]` (`P2F-R4:106`)
whose 18-W1 leg asserts *"breach → **exit 80** → `watchdog.restart{code:80}`"*
(`P2E-R3:76`) — i.e. 18-W1 requires the enforcement that 18-W2 is supposed to gate.

**Why it matters.** Frame §4.2 (feasible) and §3 C9. This is not a naming drift F-CITE
absorbs; it is the one place the two specs have a *real* ordering constraint and it is
stated in a form that cannot be executed.

**The resolution neither spec states.** 18-W2 runs at `max_webview_mem_mb = 0`
(`P2E-R3:76`) — it needs E4's sampler and the nightly-reload path, and **not** enforcement.
So the cycle breaks by splitting on the axis E already found:

1. E4 + E1/E2/E3/E6 + the 18-W2 body land (no enforcement).
2. F7 lands with `matrix.scenario: [18-W2]`; first green nightly records the floor.
3. E5's enforcement half + 18-W1 land together; F7's matrix gains `18-W1`.

That must be written into both registers, because as written an implementer building F7
from F's text builds a matrix job that cannot go green on step 2.

---

## INT-2 — Five deferrals land on P2-G rows that do not exist (HIGH)

**What breaks.** Frame §4.5 requires a named owner; §6 makes a gate that cannot run HIGH.
Five items were closed inside their own threads *on the strength of* a P2-G gate, and P2-G's
final register carries none of them.

**Evidence.** `grep -rn "H10" scratchpad/debate/` → hits in `ledger.md`, `P2D-*` only; **zero
in any `P2G-*` file**. `grep -n "wedge\|INVOCATION_ID\|orphan" P2G-R{1,2,3}-writer.md
P2G-R{2,3}-critic.md` → **zero output**. `grep -n "kill -9\|cage -v\|0\.1\.4" P2G-R3-writer.md`
→ only the *analysis* uses of 0.1.4; no assertion.

| Deferral | Declared at | Named heir | Present in G? |
|---|---|---|---|
| PF-04 pinch intercept validated on touch hardware | `P2D-R3-writer.md:153` | "new P2-G **H10**" | **No.** `P2G-R3-writer.md:248` G14 reads *"H1–H9"* |
| Smoke 17 fallback (cage-headless virtual input) | `P2D-R3-writer.md:156` | H4 | **Ambiguous.** H4's final text (`P2G-R3-writer.md:151`) is *"verify the deployed site's text-entry surfaces… record whether any input has no usable keyboard"* — no touch, no corner-tap |
| `GDK_TOUCH_CANCEL` emission; N-finger over-count deadband | `P2D-R3-writer.md:155`, `:130-134` | H4 | **No.** Same sentence; G14's summary is *"H4 now records per-device-class text-entry surfaces"* |
| Orphan-kill: *"P2-G image validation asserts `kill -9` of the launcher leaves no `kiosk-main`"* | `P2C-R1-writer.md:486-487` | G15 / an H row | **No.** G15's assertion list (`P2G-R3-writer.md:249`) has no process assertion |
| cage floor-version assertion (Debian 12 = 0.1.4) | `P2C-R2-writer.md:230`, `:279` | "P2-G image validation" | **No** |
| Wedged-cage residual | `P2C-R2-writer.md:125-127` | "P2-G — image validation plus a hardware-checklist row" | **No** |

**Why it matters most for C12.** C12 is the change that closes verification finding V4
(`ledger.md:41`: a Linux launcher death today orphans `kiosk-main`). Its Critic accepted the
closure with the gate explicitly located in G (`P2C-R1-writer.md:490`: *"gate owned by
P2-G"*). The gate does not exist, so V4 is closed on paper only. H4's rewrite is the same
failure by erosion: it started as the *touch* row (`P2G-R1-writer.md:444`) and after two
revisions is the *text-entry* row, while a sibling kept routing touch deferrals to it.

**Remedy (cheap, all one-liners in G14/G15).** Add H10 verbatim from `P2D-R2-writer.md:58`;
split H4 into H4a (touch: corner-tap, `GDK_TOUCH_CANCEL`, N-finger count) and H4b
(text-entry surfaces); add to G15 `pkill -9 kiosk-launcher; sleep 2; ! pgrep kiosk-main` and
`cage -v` recorded against the image.

---

## INT-3 — Scenarios 13–15 have no runner, and 14 is unrunnable on the floor (HIGH)

**Two independent defects on the same rows.**

**(a) cage is not in F's environment.** F's spec §2(a) assigns *"all A–D scenarios including
the per-PR exclusions"* to the `debian:12` container (`p2f:45-48`), and C 13–15 are exactly
those exclusions (`p2f:37`). F's declared compositor is weston throughout — F1's harness is
*"weston-headless bring-up"* (`P2F-R2-writer.md:35`), the per-PR package set is *"`weston`,
the four GStreamer packages"* (`p2f:32-34`), the soak container installs
*"`libwebkit2gtk-4.1-0`, the four GStreamer packages, `weston`"* (`P2F-R2-writer.md:128-129`).
`grep -n "cage" P2F-R{1,2,3,4}-writer.md P2F-R{1,2}-critic.md` returns **no** install,
bring-up or package mention — only prose about what is excluded per-PR. C's smoke 13 requires
`cage -- kiosk-launcher` under `WLR_BACKENDS=headless` (`p2c:152`). Frame C9: a gate assigned
to a runner that cannot run it.

This is not cosmetic. It is the *only* CI exercise of C10 (cage exit-code propagation),
C12's chain and the whole `kiosk.service` → cage → launcher → main shape, and A's own gate
deliberately declares cage-headless **non-blocking** (`p2a:312`), so the cage requirement
enters the system with C and never reaches F's environment list.

**(b) On the platform floor there is no input injection at all.** I resolved D's open
plan-time question (`p2d:127-129`, *"whether cage exposes the wlr virtual-input protocols
headless is pinned at plan time"*) from source, and the answer differs by version:

```
cage 0.1.5 (installed here):  strings /usr/bin/cage | grep _create
  wlr_virtual_pointer_manager_v1_create      ← present
  wlr_virtual_keyboard_manager_v1_create     ← present

cage 0.1.4-4 (Debian 12 = C7 floor), sources.debian.org cage.c full *_create( list:
  compositor, data_device_manager, export_dmabuf, gamma_control, idle, idle_inhibit_v1,
  output_layout, screencopy, server_decoration, xcursor, xdg_decoration, xdg_output,
  xdg_shell, xwayland
  → NO virtual_pointer, NO virtual_keyboard   (seat.c/server.h/output.c: also zero)
```

So on Debian 12 there is no `wlrctl`/virtual-pointer route, and XTEST reaches only Xwayland
clients, not our native Wayland webview (G's own argument, `P2G-R3-writer.md:47-48`).

Scenario 17 declares this fallback and survives (`p2d:127-129`). **Scenario 14 does not.**
`p2d:137`: *"C's scenario 14 (technician exit 86) gains its app-path driver from D's chord
and is re-run."* Scenario 14 is the only end-to-end proof of arch-05's exit-86 chain on the
app path (its systemd half is G's H2). It is scheduled-only, assigned to F5's `debian:12`
container, and on that image it cannot be driven.

**Q5 trap worth naming.** An implementer probing locally on a current distro gets cage 0.1.5
and virtual input works; the job then fails only on the floor image. The version split is
already latent in the debate — G's protocol analysis is 0.1.4, C's behavioural discharge of
C10 was run on 0.1.5 (`ledger.md:81`) — and no spec states which version any claim holds for
except C15's version-recording line.

**Remedy.** (i) Name `cage` in F's package sets and state which scenarios run under cage vs
weston. (ii) Give scenario 14 the same declared fallback 17 has (→ H2, which already asserts
the systemd half on hardware). (iii) Record cage 0.1.4 as the version every cage claim is
made against, and re-derive any claim taken on 0.1.5.

---

## INT-4 — A wedged GTK main loop composes into a silently un-exitable device (HIGH)

Each step is locally justified and every one was accepted. Composed they remove the last
exit route and the last detector.

**The chain, all verified.**

1. **D declares the failure and claims no covering control.** `P2D-R2-writer.md:164-175`:
   *"A wedged main iteration disables observation on both at once"* … *"I claim no covering
   control for a wedged GTK main loop — I have not verified that any existing watchdog
   detects it, so I am not asserting one."*
2. **The launcher cannot see it.** `heartbeat::run` is a `tokio::spawn`ed task
   (`heartbeat.rs:222`, `tokio::time::interval` at `:110`), not driven by the GTK loop. A
   wedged UI thread keeps pinging, so the FSM never reaches the 3-missed-ping restart.
3. **The control that would see it is unowned.** `verify-COVERAGE.md` R11: `watchdog.hang`
   has no Linux producer, arch-15 case (c) unreachable. Ledger I3 carries arch-04/RT-02
   (JS-ping) as OPEN with no owner — and the parent puts it in P2 verbatim (frame §2).
4. **D withdrew the third leg.** `P2D-R2-writer.md:180-186`: leg 3 (OS-lockdown escape)
   withdrawn on the parent's *"and/or"*.
5. **G removes what leg 3 would have used.** G10 (cage without `-s`, `NAutoVTs=0`/
   `ReserveVT=0`, `systemctl mask getty@.service`, "no other TTYs" as a gate step); G12
   (*"SSH keyed-only if present, absent by default"*).
6. **Legs 1+2 both ride the wedged loop**, and `open_pin_pad` ends in `window.navigate`
   (`gesture.rs:167`) which needs the same thread anyway.

**Result.** On a conforming P2-G image, a wedged WebKitGTK/GTK main loop produces a device
that is up, supervised, reporting healthy heartbeats, with no technician exit, no VT, no
getty, no SSH, and no telemetry event naming the condition. That is parent §3.5's
un-exitable device, reached without any single spec being wrong.

**Why this is an integration objection and not a re-litigation of D.** D is right that leg 3
was its invention; G is right that the image should have no VT; I3 is right that JS-ping is
unowned. The composition is nobody's ledger row. Frame §2 makes an uncovered P2-row item with
no identifiable owner a **HIGH integration defect**, and this is the concrete harm arch-04
was scheduled into P2 to prevent.

**Remedy.** Either (a) give arch-04/RT-02 an owner in P2 — it is the parent's own answer and
it closes both the detection and the restart path — or (b) if the integration round defers
it, the deferral must be recorded against parent §3.1 as an amendment, and G must add a
recovery step to the runbook (the only remaining route on a conforming image is a power
cycle, which should be *documented* rather than discovered).

---

## INT-5 — The keyboard erratum's evidence is version-split; one limb is false, and a derived cross-spec claim with it (MED)

**What I checked.** G's erratum (`P2G-R3-writer.md:43-48`, and Moderator ruling R2,
`ledger.md:135-144`) rests on: *"cage 0.1.4-4's complete protocol surface … no
`zwlr_layer_shell_v1`, no `zwp_input_method_v2`, no `zwp_virtual_keyboard_v1`, no
`zwp_text_input_v3`"*, stated as *"verified independently by both roles"*.

Against cage **0.1.4** (`sources.debian.org` `cage.c`, full `*_create(` list reproduced in
INT-3) G is **correct on all four**. Against cage **0.1.5** — the version C's Critic actually
ran to discharge C10, and the version C15's smoke line will record — one is false:

```
strings /usr/bin/cage | grep virtual
  wlr_virtual_keyboard_manager_v1_create
  wlr_virtual_pointer_manager_v1_create
```

`wlr_virtual_keyboard_manager_v1` *is* the `zwp_virtual_keyboard_manager_v1` global.

**What survives and what does not.**
- **Ruling R2's conclusion survives** on layer-shell alone: without `zwlr_layer_shell_v1` an
  OSK has no way to place itself over a fullscreen client on either version. I am not
  reopening the ruling.
- **The stated evidence must be corrected**, and scoped to a version. As written it will be
  re-checked by the next reader on a modern distro and found wrong.
- **One derived claim is false and it crosses specs.** `P2G-R2-writer.md:170-175` and
  `P2G-R3-writer.md:148-149`: *"any separate-process OSK produces no GDK events in our
  process and would break D's `ActivityClock`"*. That holds for the XTEST/Xwayland route. It
  is **false** for a `zwp_virtual_keyboard_v1` client, which injects into the seat, so the
  compositor delivers real `wl_keyboard` events to the focused client — i.e. real GDK events
  to D's handlers. On 0.1.5 that route exists. The claim is used to justify preferring the
  in-page route, so it should not be load-bearing while false.
- **Consequence for I1.** I1's disposition rests on "no viable mechanism exists other than
  `inject.rs`". The honest statement is narrower: *display* is blocked by layer-shell absence
  on both versions; *injection* is not blocked on 0.1.5. Whoever picks up I1 should be told
  that, or they will re-derive it.

---

## INT-6 — B's SEC-10 soundness test cannot read the corpus it names (MED, C1)

**What breaks.** B's Critic closed the SEC-10 HIGH explicitly on the test, not the prose:
*"the soundness is carried by a corpus test over the existing adversarial battery rather than
by a table — which is the part that makes it hold up after this debate ends"*
(`P2B-R3-critic.md:94-95`). The test is specified as *"the corpus is the existing battery's
URLs (`allowlist.rs`)"* (`P2B-R3-writer.md:79`), and both roles cited battery rows by line
(`:602`, `:653`, `:641`, `:387-397`) as if reachable.

**Evidence (tier 3, checked).** `grep -n "mod tests\|cfg(test)" crates/kiosk-core/src/nav/allowlist.rs`
→ `144:#[cfg(test)]`, `145:mod tests {`, file is 732 lines. Every cited row lives inside that
module. `#[cfg(test)] mod tests` is not compiled into the `kiosk-core` rlib that `kiosk-main`
links, so a host test in `kiosk-main` cannot iterate it. Frame §7: an unchecked checkable
claim is struck — this one was asserted by both roles and neither checked visibility.

**Why it matters.** The remedy that survives is a **hand-copied** corpus in `kiosk-main`.
That is a second source of truth for the adversarial battery, one layer above the second
matcher C1 already tolerates: a future row added to `allowlist.rs`'s battery (a new
URLPattern edge case) does not reach the implication test, and the divergence is silent.
The Critic's own framing — *"it re-proves itself on every future pattern change"* — is then
false.

**Remedy, lazy version.** Put `compile_filter` in `kiosk-core::nav` next to `allowlist.rs`
and the implication test inside that same `mod tests`. The compiler is pure and
platform-free by B's own description (*"a pure, host-tested compiler"*), so C1 is satisfied
rather than strained, the corpus is reached directly, and `regex` is already in `Cargo.lock`
(1.12.4) so C6 costs nothing. Only the sys-FFI shim that installs the filter stays in
`kiosk-main`. Failing that: export the corpus as a `pub const` and cite the export, not the
test module.

---

## INT-7 — Smoke 8(d)'s mechanism is a no-op for the only supported principal (MED)

**What breaks.** 8(d) is B's replacement for the rejected `--no-egress-filter` flag and now
gates three things at once — B3, B2's degrade path and NB-4's `config.error`
(`P2B-R3-writer.md:150-156`). Its mechanism is *"make `data_dir/content-filters/`
unwritable"*.

**Evidence (reproduced in-session).**
```
id -u → 0
mkdir -p $d/content-filters; chmod 000 $d/content-filters; echo x > $d/content-filters/probe
→ WRITE SUCCEEDED despite mode 000 (root/CAP_DAC_OVERRIDE)
```

**Where it lands.** P2-C R3 declares *"a non-root manual run is not a supported
configuration"* and G16 ships **root by default (no `User=`)** — so the product's only
supported uid ignores the fixture. GitHub Actions `container:` jobs run as root, so F5's
nightly `debian:12` matrix (which sweeps all of B 8–12) also ignores it; the scenario there
fails for the wrong reason, or passes for the wrong reason if the assertion is written
loosely. It works only on F2's `ubuntu-22.04` runner, whose `runner` uid the product never
has.

**Why it matters.** Q3/C9. The one gate covering the SEC-10 degrade path exercises a
permission model the shipping configuration does not obey, so it proves the path on a
configuration nobody deploys and produces nightly noise on the one that matters.

**Remedy.** Use a mechanism root respects: a read-only bind mount / `mount -o remount,ro`
over `data_dir`, or `chattr +i` on `content-filters`, or make `content-filters` a regular
file so `create_dir_all` fails with `ENOTDIR`. The last is one line in the fixture and works
for every uid.

---

## INT-8 — B's fail-open resolution is justified by loudness that G's provisioning model does not deliver (MED)

**What breaks.** B resolved the C4-vs-C5 collision by ruling the absent-filter state
non-boot-blocking, and justified it on observability: *"It is distinguishable from healthy at
`error` level, which is the whole point of the escalation"* (`P2B-R3-writer.md:183-184`).

**The other half, from G.** G5 ships **nothing** at the credential path — the credential is
operator-provisioned — and G's own matrix column, as bounded in R3, reads: *"spooled locally;
uploaded retroactively **if the device is provisioned within the spool's retention window**
… Otherwise the on-screen `safe.html` and H8's cold-install step are the only signal"*
(`P2G-R3-writer.md:188-191`).

**Composition.** A device that boots with a valid signed config but a filter that fails to
compile/save is **not** in safe mode, so `safe.html` never paints. It looks correct on
screen, serves the operator's site, and enforces navigation policy (A's nav guard) while
enforcing **no subresource egress at all**. `config.error("egress.filter_absent")` goes to
the spool; on a device never provisioned, or provisioned after the retention window, it is
never seen. Two independently-accepted decisions produce a SEC-10 gate that is fail-open and
silent — the exact defect class the parent names (frame Q3).

**Remedy, one line, no new mechanism.** Add to H8's cold-install step: *after first boot,
assert the local spool contains no `egress.filter_absent` / `egress.csp_absent` before
sign-off*. That converts the field-silent state into a provisioning-time one, using the
mechanism G already built for exactly this in the keyboard row. (A stronger option — refuse
remote content when Layer 1 is absent — is a C5 reading B explicitly rejected; I am not
reopening it, only asking that the compensating control be real.)

---

## INT-9 — `--config` is required by G and absent from C (MED)

**Evidence.** `grep -n -- "--config" P2C-R{1,2,3}-writer.md P2C-R{1,2,3}-critic.md
docs/superpowers/specs/2026-08-06-p2c-*.md docs/superpowers/specs/2026-08-06-p2a-*.md` →
**zero hits in all ten files.** G1's `Depends on` column (`P2G-R3-writer.md:235`) reads:
*"**C's Linux `spawn_main` must carry `--config`** (fail-closed if not)"*, and G's Critic
carries it as residual #8 (`P2G-R3-critic.md:97-98`). One direction only.

**Why it matters (tier 3).** `crates/kiosk-launcher/src/spawn.rs:199-210` — the
`#[cfg(not(windows))]` stub takes `_config_dir` and ignores it; the Windows body does
`cmd.arg("--config").arg(config_dir)` at `:121`, and the module doc at `main.rs:22-26` states
that contract. C5 writes the real Unix body and never restates it. `kiosk-main`'s
`resolve_config_dir` (`main.rs:423-431`) falls back to `current_exe().parent()`. Under G1's
layout that is `/usr/lib/kiosk`, so a launcher that forgets the flag hands `kiosk-main` a
config dir where `kiosk.ini`, `kiosk-credential.json` and `kiosk-offline.mp4` do not exist —
which reproduces verification finding V13 one process downstream.

**Remedy.** One line in C5's change list: *the Unix `spawn_main` appends `--config
<config_dir>`, byte-identical to `spawn.rs:121`* — plus G15's `systemd-analyze verify` hole
is already covered by the `test -x` additions, so nothing else moves.

---

## INT-10 — E1's P2-B edge is stale (LOW)

`P2E-R3-writer.md:220` (E1's `Depends on`): *"P2-B (three shared files: `main.rs:990`,
`build.rs`, `capabilities/default.json`)"*. B dropped that in Round 1:
`P2B-R1-writer.md:347-350` — *"Its `securitypolicyviolation` listener and the
`#[cfg(not(windows))]` Tauri command are **dropped** … That also disposes of the verifier's
note about `tauri::generate_handler!` at `main.rs:990` being unconditional — **nothing needs
to touch it**."* B's final register (B1–B12) contains no Tauri command and no ACL entry.

**Effect.** The only declared B↔E edge is an edge onto a withdrawn change. It is harmless to
the code but it fabricates a B-before-E ordering constraint that does not exist, and E's
justification for editing `capabilities/default.json` cites a sibling that is not editing it.
Delete the edge; E1 owns those three files alone.

---

## INT-11 — D's dependency register omits `webkit2gtk` (LOW)

D1's mechanism is *"pointer/touch/motion/scroll on `webkit2gtk::WebView`"*
(`P2D-R3-writer.md:141`); D10 declares *"One direct target-gated dep: `gtk = "0.18"`"*
(`:150`).

**Checked (tier 4).** `wry-0.55.1/src/lib.rs` re-export list (`pub use` lines 363–415) does
**not** include `webkit2gtk`; `WebViewExtUnix::webview()` returns `webkit2gtk::WebView`
(`lib.rs:2305`). `tauri-2.11.5/src/webview/mod.rs:173` — `PlatformWebview::inner()` returns
`webkit2gtk::WebView`, and tauri's own doc example at `:1629-1631` is `use
webkit2gtk::WebViewExt;`, i.e. the consumer declares the crate.

D can *probably* compile against `gtk`'s blanket `WidgetExt` impl without naming the type,
which is why this is LOW rather than MED — but it is fragile, undeclared, and it makes
**B10 a prerequisite of D**. Versions are compatible (`Cargo.lock`: `webkit2gtk 2.0.2`,
`gtk 0.18.2`, matching B10 and D10), so this is a register line and a merge-order note, not
a redesign.

---

## INT-12 — the declared feature floor is not enforced by the build (LOW)

B10 discharges P2-A's hand-forward with `webkit2gtk = { features = ["v2_32"] }` +
`webkit2gtk-sys = { features = ["v2_24"] }`, floor 2.32 (`P2B-R3-writer.md:206-218`).

**Checked (tier 4).** `tauri-2.11.5/Cargo.toml`, linux target block: `webkit2gtk` is declared
with `features = ["v2_40"]`. Cargo unifies features across the graph, so the compiled crate
carries `v2_40` regardless of B10's declaration — the Writer noted this in passing
(*"below the v2_40 already in the build"*) without drawing the consequence: **nothing in the
build stops a future edit from calling a `v2_40`-gated symbol.** The declaration is a review
convention and a documentation artifact, not a mechanism.

The runtime consequence is nil today (Debian 12 ships WebKitGTK ≥ 2.40), so this is LOW. But
A's hand-forward asked for a *floor*, and what is delivered is a comment. Say so in the spec,
or add the one-line `cargo tree -e features -i webkit2gtk` check to F5 if the floor is meant
to bind.

---

## Merge-order finding

Every declared edge except INT-1's is acyclic. The order below satisfies all of them,
including the four that are currently one-directional (INT-2, INT-9) once those are made
mutual.

```
0.  P2-A rev 3                    (already reviewed; kiosk-main resolve_data_dir → /var/lib/kiosk,
                                   p2a:29,:113 — verified)
1.  P2-C  ⊕ C16                   hard co-landing with (0) per X5. Also delivers RT-13 de-gating
                                   (F depends on it) and the [Service] shape (G8 depends on it).
                                   MUST gain --config forwarding (INT-9).
1'. P2-B                          independent of C; adds webkit2gtk/-sys direct deps.
2.  P2-D                          needs B10's webkit2gtk (INT-11); contributes G10's chord
                                   sentence and C14's pinpad driver.
3.  P2-E, part 1                  E1/E2/E3/E6 + E4 (sampler, unconditional) + the 18/18-W1/18-W2
                                   scenario bodies. NO enforcement.
4.  P2-G                          consumes C's --config + [Service] shape, D's chord note, B9's
                                   relabel (X2), E's mp4 path. MUST gain H10, H4a/H4b,
                                   orphan-kill + cage-version assertions (INT-2).
5.  P2-F                          consumes G's .deb flow + G15 by reference, C's RT-13, E's
                                   bodies. MUST gain cage in the environment (INT-3) and
                                   ship F7 as matrix [18-W2] only.
6.  P2-E, part 2                  E5's enforcement half + 18-W1, after the first green nightly
                                   F7/18-W2 records the floor and it clears 750 MB.
```

B and C are order-independent with respect to each other. Steps 4→5 are strict (F executes
G's package flow and references G15 by ID). Step 6 is strictly after step 5 and is the split
that breaks INT-1.

**Ordering constraints that are currently unstated and must be written into the registers:**
B→D (INT-11), C→G on `--config` (INT-9), and the E5 split (INT-1).

---

## Clean passes

Stated because a clean pass is a result, and because several of these are where I expected to
find something and did not.

- **C16 ↔ P2-A co-landing (X5) is coherent, not a conflict.** `p2a:29` and `:113` put
  *kiosk-main's* `resolve_data_dir()` at `/var/lib/kiosk/`; C16 does the *launcher's*. Two
  crates, two functions, one value, correctly split. No duplication, no contradiction.
- **F1's harness ownership does not contradict A.** `p2a:314-315` verbatim: *"smoke is
  human-run in-session and is deliberately **not** wired into `ci.yml`; automating the
  compositor harness is P2-F."* F's R2 takeover is what A asked for.
- **B12 → F5's `debian:12` container is correctly placed.** B12's precondition is *no
  systemd*; `ubuntu-22.04` is a full VM with systemd, the container is not. F moved it for
  the right reason.
- **Scenario numbering.** 1–18 contiguous, no collisions; the 18-W(b)/(c) → 18-W1/18-W2
  rename resolves the only clash with F's `endurance` (a)/(b)/(c) letters. `H1`–`H9` is a
  disjoint namespace. The only orphan is H10 (INT-2).
- **C6 (no unjustified dependencies) — clean, with the tally.** New *direct* deps across all
  six: `webkit2gtk` + `webkit2gtk-sys` (B10, target-gated), `gtk 0.18` (D10, target-gated),
  a new workspace member `crates/kiosk-smoke` with `serde_json` only (F1). Every one is
  already in `Cargo.lock` transitively (`gtk 0.18.2`, `gdk 0.18.2`, `webkit2gtk 2.0.2`,
  `webkit2gtk-sys 2.0.2`, `regex 1.12.4`, `libc`), so none adds a crate to the graph and
  none can duplicate a version. New `unsafe` surface: four bounded FFI sites (B2's filter-store
  shim, C3's `SO_PEERCRED`, C5/C6's `syscall(434)`/`syscall(424)`, `kill(2)`), each with a
  stated reason no safe binding exists (C2). C's `syscall(2)`-not-extern rule (glibc 2.35 on
  the Ubuntu 22.04 floor) is the correct call and is verifiable. **No C6 drift.**
- **C1 (no reimplemented decision logic) — strained but not violated.** B2 *is* a second
  matcher, and there is no alternative: WebKit's content blocker is declarative and runs in
  the network process, so `Allowlist::allows` cannot be in the path (which is also why B1 is
  observe-only). B narrowed it to `H(u) = (scheme, host, port)`, made refusal the default for
  anything it cannot prove, and tied it to the single source of truth by test. That is the
  right shape. The defect is in the *reachability* of that test, not the design — INT-6.
- **C8 (Windows stays green) — holds.** Total Windows-touching surface across the six:
  E1 (`media_error` command; declared cross-platform #1), E4/E5 (webview-RSS sum + cap;
  declared cross-platform #2, and the only *behaviour* change — mitigated by E4-before-E5,
  the 750 MB floor gate, and the `0` lever), C5's `ChildHandle` alias (declared, zero Windows
  behaviour diff — `ChildHandle` *is* `std::process::Child` there), C14's RT-13 un-gating
  (test-only, and the Windows path keeps its existing transport-name template), F8's
  `pubkey_b64` (build-pipeline; `ci.yml:30-43` bakes no key today, so this is new but
  guarded — required, no default, ephemeral smoke key), F10's Authenticode gate (new, additive).
  B ships nothing on Windows; D withdrew its shared-code edit and puts the `should_swallow`
  guard at the Linux call site only. **Two declared cross-platform changes, both E's, both
  justified — C8 holds as written.**
- **E5's default-relation pin.** `MEM_CAP_N × health_sample_s = 300 > healthy_run_s = 120`,
  observed from `RemoteConfig::default()` and `watchdog_config(None)` with no hardcoded copy.
  I re-checked both reach points; the test does what both roles say it does.
- **Keep-awake (R3/H-o) is better than the matrix records.** With B9 relabelled as inert
  defence-in-depth (X2) the real mechanism is G's — cage 0.1.4 has no blanking to configure,
  `Conflicts:` on idle daemons, no `wlr_idle` consumer, H3's 24 h observation. I confirmed
  cage 0.1.4's `*_create(` list has `wlr_idle_create` + `wlr_idle_inhibit_v1_create` and no
  timeout logic, consistent with G11. The matrix row should move PARTIAL → COVERED (G), with
  B9 recorded as a no-op belt — **not** left reading as B-owned.
- **F's budget reversal is sound.** 6 h hosted cap verified upstream by F; 270 min derived
  soak inside `timeout-minutes: 330` fits, and `crates/kiosk-smoke` with `serde_json` alone
  genuinely severs the dev graph (root `Cargo.toml` members are core/main/launcher).

---

## Verdict on P2 coverage

**What A–G, as revised, now delivers that the verification round said it did not:** PF-04
(D13, needs H10 to exist), SEC-10 soundly at (scheme, host, port) with a fail-closed refusal
path (B2, needs INT-6), orphan-kill and single-instance parity (C12/C13, needs INT-2's gate),
the `/var/lib/kiosk` split (C16 + A), `StartLimitIntervalSec` in `[Unit]` (G8), the Linux
install layout with the parent §4 erratum flagged (G1, ruling R1), memory-cap restart with a
measured floor gate (E4/E5), and all three previously-uncovered §10 obligations —
Authenticode gate, Windows-runner leak soak, RT-09 (F10/F7/F11).

**What P2 still does not deliver, definitively:**

| Gap | Parent status | Owner after this review | Phase |
|---|---|---|---|
| **arch-04 / RT-02 / OD-1** — JS-ping webview-hang detection | Parent §3.1: *"lands in **P2**"* | **None.** I3. Compounded by INT-4 | Unassigned |
| **R13 — remote log level** | Parent §9 P2 row | **None.** `grep -rniE "log.?level\|remote log"` over all seven P2 specs → 0 hits; no severity filter exists in `kiosk-core/logging` | Unassigned |
| **R14 — `restart_app`** | Parent §9 P2 row; `validate.rs:20` tags it `"P2"` | **None.** 0 hits across all seven specs | Unassigned |
| **RT-16 `inject_css`/`inject_js` + Linux touch keyboard** (one row) | Parent §9 P2 row + §7; `validate.rs:16-17` tags both `"P2"` | **Fallback only** — "whoever picks up RT-16", and nobody does. I1 | Nominally P2, no owner |
| **M4 / OD-8 PDF default-block** | Parent §12 records OD-8 **applied**, not deferred | **None.** I2. `scheme_guard::pdf_decision` is `#[allow(dead_code)]` on both platforms; `validate.rs:18` mis-tags it `"P1"`, so the default value warns nobody | Unassigned |
| **H-i — WebKitGTK `print` signal returns TRUE** | Parent §7 Linux cell (H1) | **None.** Partially compensated: `inject.rs:46-48` overrides `window.print` platform-agnostically and ships in P1 | MED, not HIGH |
| **H-f — native drop-target disable** | Parent §7 | **None** on either platform; injected half ships in P1 | Symmetric non-delivery |
| **Memory-cap restart, conditionally** | Parent §9 P2 row | E, **but** E5's enforcement half does not ship if 18-W2's floor ≥ 750 MB — in which branch the P2 row reverts to a defect against parent §5.2 | Conditional |

Five of these have no owner and no phase, which frame §2 makes a HIGH integration defect
each. Three of them (R13, R14, arch-04) are named **verbatim in the parent's P2 row** — the
completion contract frame §2 quotes — and "deferred to P3/P4" is inadmissible unless the
parent itself defers, which it does not. The integration round must therefore either **create
owners** (the natural shapes: one `inject.rs`-scoped sub-project for I1 + R16; one
`logging`/`maintenance` sub-project for R13 + R14 + the PDF wiring; arch-04 to whoever owns
`heartbeat.rs`) or **obtain an owner-level amendment to the parent** deferring them, on the
same standard as rulings R1 and R2. Silence closes P2 with its headline row unmet.

**Bottom line.** The six specs are individually in good shape — the threads did real work and
most of what I probed held. They are **not jointly executable as written**: one declared
cycle (INT-1), five deferrals to gates that do not exist (INT-2), one scenario set with no
runner and one with no fallback on the platform floor (INT-3), and one composed failure mode
that no single spec owns (INT-4). All four are cheap to fix — a split, five checklist lines,
an apt package plus a fallback clause, and one ownership decision — but none of them fixes
itself at plan time.
