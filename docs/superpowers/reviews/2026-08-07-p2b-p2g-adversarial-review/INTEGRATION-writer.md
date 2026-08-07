# INTEGRATION ROUND — WRITER

No frame dispute. Every claim I use below was re-run by me in-session; where I take the
Critic's evidence I say I reproduced it, and where I go past it I give the new command.

**Dispositions: REVISE 9 · CONCEDE 2 · REBUT-in-part 1 · RISK 1 (INT-12, split).**
Zero of his twelve are struck. Four HIGHs are closed by construction, not acknowledgement.

---

## INT-1 — REVISE (adopt the split; one simplification that removes a merge)

**Conceded that it is a cycle, and that neither thread checked the loop.** Reproduced:
`P2E-R3-writer.md:224` gates E5's enforcement on 18-W2's recorded floor; `P2F-R4-writer.md:106`
carries `E4 + E5 + 18-W1/18-W2 (in); **E5 (out)**` in one cell. Both roles wrote "carried in
both directions" and called it done. It is a cycle and no order satisfies it.

**I adopt his break and tighten it.** His three steps are right on the axis (18-W2 needs E4 and
the reload path, *not* enforcement — `P2E-R3:76` runs it at `max_webview_mem_mb = 0`). The one
thing I change is that his step 3 reads as a second P2-F pass; it does not need to be. The
enforcement half and the matrix line are **one commit touching two files**:

1. **E lands whole except E5's enforcement half.** `pub const MEM_CAP_N`, the launcher-side
   default-relation test, E1–E4, E6, and all three scenario bodies (18, 18-W1, 18-W2) land. The
   18-W1 *body* is inert without enforcement — that is fine, nothing runs it yet.
2. **F lands with `strategy.matrix.scenario: [18-W2]`.** First green nightly records the floor.
3. **One commit:** E5's enforcement branch in `kiosk-main` **+** the single line adding `18-W1`
   to `endurance.yml`'s matrix. Owner: whoever implements E5. No second F pass, no re-opened spec.

**Second correction, which is what actually kills the cycle.** F7 does not depend on E5 at all
and never did — it depends on **E4 + the 18-W2 body**. And F7 does not *own* an outbound edge:
it produces an **artifact** (the RSS series, already retained per F7's row) that E's gate reads.
An artifact is not a dependency. So the register cells become:

- **F7 `Depends on`:** `E4 + 18-W1/18-W2 bodies (in). No outbound edge.` — `**E5 (out)**` and
  `E5 (in)` are both **deleted**.
- **E5 `Depends on`:** `E4 (in); the first green nightly F7/18-W2 run (in, merge gate on the
  enforcement half only).`

One edge, one direction, acyclic. Frame Q2: this is strictly fewer moving parts than the
alternative I considered (moving the measurement off CI to a dev box), which I reject because
the 750 MB threshold is only meaningful against a defined machine — `windows-latest` — and a dev
box is not one.

---

## INT-2 — CONCEDE, then REVISE (the five rows are written into G)

**Conceded outright. I reproduced all four greps against `P2G-*`:**

```
grep -rn "H10" P2G-*.md                              → rc=1, zero
grep -rn "wedge\|INVOCATION_ID\|orphan" P2G-*.md     → rc=1, zero
grep -n "kill -9\|cage -v\|0\.1\.4" P2G-R3-writer.md → one hit, line 43, the *analysis* of 0.1.4
```

He is right about the mechanism of the failure, and right that C12 is the worst case: C12 closes
verification finding V4 (`ledger.md:41`) and its Critic accepted the closure with the gate
located in G (`P2C-R1-writer.md:490`, *"gate owned by P2-G"*). A gate that does not exist is
frame §6 HIGH. He is also right about H4's erosion — it was the touch row at
`P2G-R1-writer.md:444` and is the text-entry row at `P2G-R3-writer.md:151`, while D kept routing
touch deferrals into it (`P2D-R3-writer.md:155-156`).

**The five rows, written. G14's checklist becomes H1–H11 and H4 splits.**

| Row | Text G must now carry | Discharges |
|---|---|---|
| **H4a** (new; the old touch row restored) | *"Corner-tap opens the pin pad on the device's own touch panel. Record: taps counted per single-finger tap; taps counted per N-finger tap (Windows counts 1 — over-count is the declared C3 divergence); whether `GDK_TOUCH_CANCEL` is emitted at all on this panel."* | D5/D11's `GDK_TOUCH_CANCEL` + N-finger deadband (`P2D-R3:130-134`, `:155`) |
| **H4b** (the R3 text, kept verbatim) | *"Verify the deployed site's text-entry surfaces on the device class; record whether any input has no usable keyboard."* | I1 discoverability |
| **H10** (new; verbatim from `P2D-R2-writer.md:58`) | *"Pinch-zoom does not scale the page on touch hardware; two-finger pan/scroll still works."* Second clause failing ⇒ D13's recorded `scale_delta()` deadband is the fix. | D13 / PF-04 |
| **H11** (new) | *"Wedged-compositor recovery: with cage `SIGSTOP`ped, confirm the device does not self-recover and that the documented recovery step (power cycle) restores service. Record time-to-detect by an on-site observer."* | C12's wedged-cage residual (`P2C-R2-writer.md:125-127`); also carries INT-4's residual |
| **G15 +2 assertions** | `pkill -9 kiosk-launcher; sleep 2; ! pgrep kiosk-main` — and — `cage -v` emitted and asserted equal to the recorded floor (`0.1.4`) | C12's orphan-kill gate (`P2C-R1-writer.md:486-487`); C10/C15's cage-floor assertion (`P2C-R2-writer.md:230`, `:279`) |

Note the orphan-kill assertion belongs in **G15** (the container job, where a `.deb` is installed
and processes actually run) and not in an H row — H rows are human hardware checks and this one
is mechanical. That is the only place I move his remedy: he offered "G15 / an H row"; G15 is
correct and is also where F5 picks it up by reference.

---

## INT-3 — CONCEDE (a) · REVISE (b), with the driver he did not have

**(a) CONCEDED without qualification.** Reproduced: `grep -n "cage" P2F-*.md` returns only prose
about what is *excluded* per-PR (`P2F-R1-writer.md:135`, `:142`, `:145-147`, `:405`) — no
install, no bring-up, no package. F's declared package sets are weston-only (`p2f:32-34`,
`P2F-R2-writer.md:128-129`), and `p2f:45-48` assigns "all A–D scenarios including the per-PR
exclusions" — i.e. C 13–15 — to that container. C's smoke 13 requires `cage --` under
`WLR_BACKENDS=headless` (`p2c:152`). Frame C9. And he is right that A deliberately left this
open: `p2a:312-315` declares cage-headless **non-blocking** and hands harness automation to F,
so the cage requirement enters with C and never lands anywhere.

**Fix, narrow.** The per-PR `ubuntu-22.04` job needs nothing — 13–15 are scheduled-only
(`p2f:37`). Only **F5's `debian:12` nightly container** changes:

> F5's package set gains **`cage`, `xwayland`, `xdotool`**. F's spec states the
> scenario→compositor map explicitly: **A 1–7, B 8–12, D 16–17 run under weston headless;
> C 13–15 run under `cage -- kiosk-launcher` with `WLR_BACKENDS=headless`.** No scenario runs
> under an unnamed compositor.

**(b) The floor has no virtual input — confirmed, and the fix is a driver, not a deletion.**
His 0.1.5 evidence reproduces here exactly:

```
$ strings /usr/bin/cage | grep -E "_v1_create"
  wlr_virtual_keyboard_manager_v1_create      ← present on 0.1.5
  wlr_virtual_pointer_manager_v1_create       ← present on 0.1.5
  wlr_xwayland_create                         ← present on both
$ strings /usr/bin/cage | grep -iE "layer_shell|input_method|text_input"   → rc=1, zero
```

I accept his 0.1.4 list (tier 5, sources.debian.org) as the floor: no virtual pointer, no
virtual keyboard. So D's open plan-time question (`p2d:127-129`) resolves **negatively on the
floor**, and scenario 14 has no driver and — unlike 17 — no declared fallback. That is the
defect and it is real.

**Where I go further than his remedy.** He proposes 14 be given 17's fallback (→ hardware).
Taken, but that alone leaves the *only* end-to-end proof of arch-05's exit-86 app path with no
CI exercise at all, and RT-13 does not cover it (`rt13.rs` builds `LauncherSink` directly and
never runs `kiosk-main`). There is a driver available on the floor, and G already wrote it up
for a different purpose: **cage's Xwayland**. `wlr_xwayland_create` is in cage 0.1.4's own
`*_create(` list (his INT-3 quote) and on 0.1.5 here.

> **Scenarios 14 and 17, CI driver on the floor:** run `kiosk-main` inside cage with
> `GDK_BACKEND=x11`, so the webview is an Xwayland client, and drive it with `xdotool`. This is
> the route G's runbook already documents as the keyboard fallback
> (`P2G-R3-writer.md:52-55`), used here for a smoke driver rather than for deployment.
> **Declared divergence (C3, stricter statement of what is proved):** the run exercises GTK's
> X11 GDK backend, not the Wayland one. That is faithful for what 14 and 17 assert — D's
> mechanism is GTK *widget signals* (`P2D-R3:141`), not a Wayland protocol, and 14 asserts
> exit-86 propagation — and it is **not** a substitute for the Wayland input path, which
> remains hardware-gated at H4a.
>
> **Fallback if even that fails (17's discipline, now 14's too):** scenario 14's app-path half
> moves to the deferred hardware list against **P2-G H2** (systemd half, already there) and
> **H4a** (touch half, new per INT-2) — *recorded, not silently dropped*. Scenario 17's existing
> fallback clause is unchanged.

**(c) Version-scoping, adopted verbatim.** Every cage claim in C, D and G is stamped with the
version it was made against. C10/C15 already record `cage -v` per run (ruling R3); G15 now
asserts the image's `cage -v` equals the recorded floor (INT-2); G's erratum is re-scoped in
INT-5. His Q5 trap — probe locally on 0.1.5, fail only on the floor image — is exactly why.

---

## INT-4 — REVISE. arch-04 gets an owner: **P2-C, new change C17.** Closed inside P2-C…P2-G.

**This is the most serious finding in the review and I am not dispositioning it as a risk.**

**Every link reproduced.** `tokio::spawn(heartbeat::run(...))` at `crates/kiosk-main/src/main.rs:941`,
under `#[tokio::main]` (`:638`); the ping cadence is `tokio::time::interval` at
`heartbeat.rs:110`. Nothing in that task touches the GTK loop, so a wedged UI thread keeps
pinging and the FSM's 3-missed rule never arms. `arch-15` case (c) — *"child alive + healthy
channel + no round-trip ping → genuine hang"* (parent `:142-149`) — has **no producer** on
Linux, which is `verify-COVERAGE.md` R11. D's own words stand
(`P2D-R2-writer.md:164-175`): *"I claim no covering control for a wedged GTK main loop."* G10/G12
remove VT, getty and SSH. He is right: composed, that is parent §3.5:319-320's un-exitable
device, and no single spec is wrong.

He is also right that "P3" is not available: parent `:133-141` says the JS-ping *"lands in **P2**"*.

**Owner: P2-C. Reason it is C and not a new spec.** C already owns
`crates/kiosk-main/src/heartbeat.rs`'s Linux body (`p2c:26`, `p2c:124-129`, register row C8), and
the line that must change is the **ping arm of that exact function**. C also already owns the
hang scenario — `p2c:157`, smoke 15, *"`SIGSTOP` main past the heartbeat-miss window"* — which
today proves only *process*-stop detection, i.e. the wrong failure. This is one arm of one
function in a file C is already rewriting. Rung 2 of the ladder: no new module, no new spec, no
new dependency.

**C17 — webview round-trip gate on the heartbeat (arch-04 / RT-02 / OD-1).**

> `#[cfg(not(windows))]`, in `heartbeat::run`'s `tick.tick()` arm: before each `Frame::Ping`
> write, round-trip a no-op through the webview. `AppHandle::run_on_main_thread` →
> `WebviewWindow::with_webview(|w| w.inner().run_javascript("0", None, cb))`; `cb` resolves a
> `tokio::sync::oneshot`; awaited under a **3 s cap** (the parent's own number, `:139`).
> Timeout or error ⇒ **the ping is withheld** — not an error, not a log storm: one WARN on the
> first withheld ping of a run. Three withheld pings = 15 s = the FSM's existing 3-missed rule
> → `watchdog.hang` → restart. `heartbeat::run` gains one parameter (the `WebviewWindow`);
> `main.rs:941` gains one argument.

**Why this detects the failure D named, verified:**

- A **wedged GTK main loop**: `run_on_main_thread` never dispatches — P2-A already states the
  premise, `p2a:75`: *"the `with_webview` closure runs on the GTK main thread."* Cap expires.
- A **wedged renderer** with a live loop: the `run_javascript` callback is delivered on the GTK
  loop by the web process's reply; a wedged web process never replies. Cap expires.

Both of arch-15's uncovered halves, one mechanism.

**Feasibility checked at B10's declared floor.** `webkit2gtk-2.0.2/src/auto/web_view.rs:1466-1472`
— `run_javascript` carries **no `#[cfg(feature = …)]`**, only
`#[cfg_attr(feature = "v2_40", deprecated)]`. So it compiles at `v2_32` *and* at the `v2_40` that
tauri unifies in (INT-12's fact, used honestly): the call site takes `#[allow(deprecated)]` and
depends on no feature above B10's floor. `evaluate_javascript` is `#[cfg(feature = "v2_40")]`
(`:788`) and is **not** used, precisely so the floor stays real.

**Scope narrowing, declared not silent (C8).** The parent calls the ping "cross-platform" but
scopes its landing to *"P2 (WebKitGTK/Android, where no native unresponsive signal exists)"*.
Windows has `ProcessFailed`/`RenderProcessUnresponsive` from P1. C17 is therefore
`cfg(not(windows))` and **Windows behaviour is byte-unchanged**. I declare this as a narrowing
against the word "cross-platform" rather than letting it pass.

**Gate.** C's smoke 15 gains a variant that produces the actual failure instead of `SIGSTOP` on
the whole process: **block the GTK main thread only** (a `run_on_main_thread` closure that sleeps
past the window) → assert `watchdog.hang` is emitted and main is restarted. That is the first
Linux exercise of arch-15 case (c).

**What remains after C17, and who carries it.** A wedged **cage** is still unrecoverable — C's
own declared residual (`P2C-R2-writer.md:125-127`), and C17 cannot reach it because the
compositor holds the DRM device. Two carriers, both new:

- **G H11** (INT-2) — the hardware row that observes it.
- **G runbook, one line, his remedy (b) half, taken:** *"If the screen is frozen and the device
  has not recovered within 60 s, power-cycle. On a conforming image there is no VT, no getty and
  no SSH by design (G10/G12); a power cycle is the supported recovery, not a workaround."*
  Documented rather than discovered.

**Composition after C17:** wedged GTK loop → detected in 15 s → main SIGKILLed and restarted →
device recovers with no site visit. Parent §3.5 satisfied. The un-exitable case narrows from
"any wedged main loop" to "a wedged compositor", which is observed (H11) and documented.

---

## INT-5 — REVISE. And yes, ruling R2's reasoning needs correcting even though R2 stands.

**His check reproduces exactly** (command and output in INT-3(b) above):
`wlr_virtual_keyboard_manager_v1_create` **is** present in cage 0.1.5, and it is the
`zwp_virtual_keyboard_manager_v1` global. G's erratum states all four absences as
*"verified independently by both roles"* without a version, and one of the four is false on the
version C's own discharge was run against (`ledger.md:81`).

**On the ruling.** He declines to reopen R2; I agree the conclusion survives, and I say why in
the form the correction requires: `zwlr_layer_shell_v1` is absent on **both** versions
(`grep -iE "layer_shell|input_method|text_input"` → rc=1 on the 0.1.5 binary here), so no
separate-process OSK can place itself over a fullscreen client on either. Display, not
injection, is what is impossible.

**But a ruling resting on a wrong sub-claim should be fixed, and I say so rather than letting it
ride.** R2 recites the four-protocol absence as its evidence base (`ledger.md:138-140`). One
limb is version-dependent and false at 0.1.5. The correct standing of R2 is:

> **R2's conclusion is unchanged and its evidence base narrows to one limb:** the erratum rests
> on the absence of `zwlr_layer_shell_v1`, verified on cage 0.1.4 and 0.1.5. The
> virtual-keyboard limb is **withdrawn from the ruling's evidence** — present on 0.1.5, absent
> on 0.1.4 — and the input-method / text-input limbs are re-scoped to "0.1.4 (the C7 floor)".

I am asking the Moderator to restate R2's rationale, not to revisit its verdict.

**The derived cross-spec claim is false and I withdraw it.** `P2G-R2-writer.md:170-175` and
`P2G-R3-writer.md:148-149`: *"any separate-process OSK produces no GDK events in our process and
would break D's `ActivityClock`"*. True for XTEST/Xwayland; **false** for a
`zwp_virtual_keyboard_v1` client, which injects at the seat, so the compositor delivers real
`wl_keyboard` events to the focused client and D's handlers see real GDK events. G's runbook
sentence is rewritten to say what is true: *"an XTEST-based OSK (onboard under Xwayland)
produces no GDK events in our process and would break D's `ActivityClock`; a virtual-keyboard
client would not, but has no way to display itself under cage on either version."*

**Consequence for I1, adopted verbatim as his framing:** *display* is blocked by layer-shell
absence on both versions; *injection* is not blocked on 0.1.5. Written into I1's row so whoever
picks it up does not re-derive it.

---

## INT-6 — CONCEDE, and I take his lazy remedy (it is better than mine)

**Conceded. Reproduced:**

```
grep -n "mod tests\|cfg(test)" crates/kiosk-core/src/nav/allowlist.rs
  144:#[cfg(test)]
  145:mod tests {
wc -l → 732
```

Every battery row both roles cited by line (`:387-397`, `:602`, `:641`, `:653`) is inside that
module, which is not compiled into the rlib `kiosk-main` links. My R3 test as written
(`P2B-R3-writer.md:79-90`, *"the corpus is the existing battery's URLs (`allowlist.rs`)"*) cannot
run. Frame §7 applies to me: I asserted a checkable claim and did not check visibility. This
matters more than a broken test because B's Critic closed the SEC-10 HIGH *on that test*
(`P2B-R3-critic.md:94-95`), and the hand-copied fallback is a second source of truth for the
adversarial battery — one layer above the second matcher C1 already strains to permit.

**Remedy (i), his primary, taken.** `compile_filter` **moves into `kiosk-core::nav`**, next to
`allowlist.rs`, and the implication test goes **inside `allowlist.rs`'s own `mod tests`**.

- **C1 is satisfied rather than strained.** The compiler is what I already called it — *"a pure,
  host-tested compiler"* — and `kiosk-core` is where pure decision logic belongs. What stays in
  `kiosk-main` is only the sys-FFI shim that hands the emitted JSON to
  `WKUserContentManager`. That is the observation/enforcement edge, which is the layering rule.
- **The corpus is reached directly.** A new battery row reaches the implication test on the day
  it is added; the divergence he names cannot open.
- **C6 costs less than he assumed.** `regex` is not needed at runtime — WebKit compiles the
  emitted JSON, we never match with it. It is a **`[dev-dependencies]`** entry on `kiosk-core`
  only, used solely to evaluate `re.is_match(u)` in the test. `regex 1.12.4` is already in
  `Cargo.lock` (verified), so no crate joins the graph and none joins the shipped binary.

The test's shape is otherwise the ruling-R3 form (asserts in `H(u)` terms, corpus extended with
`ws://`/`wss://`/`http://` rows) and is unchanged by the move.

---

## INT-7 — CONCEDE the defect, REVISE with his third option

**Reproduced in-session as root (`id -u` → 0):**

```
mkdir -p $d/content-filters; chmod 000 $d/content-filters; echo x > $d/content-filters/probe
→ WRITE SUCCEEDED despite mode 000
```

And he is right about where it lands: P2-C R3 declares a non-root manual run unsupported
(`P2C-R3-writer.md:81`), G16 ships root by default (no `User=`), and GitHub Actions `container:`
jobs are root — so F5's nightly, which sweeps B 8–12, is the one place it runs and the one place
it is a no-op. It works only on F2's `ubuntu-22.04` runner, whose uid the product never has.
Q3 + C9.

**Taken: his third option, one line, uid-independent.** 8(d)'s fixture creates
`data_dir/content-filters` as a **regular file**. `std::fs::create_dir_all` returns `Err` when
the path exists and is not a directory — `mkdir(2)` gives `EEXIST` and std then fails the
`is_dir()` check (reproduced: `os.makedirs(...)` → `OSError 17 File exists` on a regular file).
Root has no capability that makes a file a directory, so the fixture behaves identically for
every uid, in a container, and on a hardware image.

The `chmod 000` mechanism is deleted from the spec. 8(d) still gates all three things it gated
(B3, B2's degrade path, NB-4's `config.error`).

---

## INT-8 — REVISE (his one-liner, taken; the composition is real)

**Composition confirmed.** B ruled the absent-filter state non-boot-blocking and justified it on
loudness (`P2B-R3-writer.md:183-184`); G5's bounded matrix column says the spool uploads
retroactively only *"if the device is provisioned within the spool's retention window …
Otherwise the on-screen `safe.html` and H8's cold-install step are the only signal"*
(`P2G-R3-writer.md:188-191`). A device with a valid signed config and a filter that failed to
compile is **not** in safe mode, so `safe.html` never paints. He is right: the compensating
observability B leaned on is not delivered by G's provisioning model, and the result is a
fail-open SEC-10 gate that is silent in the field.

**Taken verbatim, in G's H8 cold-install step:**

> *After first boot and before sign-off, read the local spool and assert it contains no
> `egress.filter_absent` and no `egress.csp_absent`.*

No new mechanism — G already uses exactly this shape for the keyboard row, and the spool is on
disk at a known path. It converts a field-silent state into a provisioning-time one. I note for
the record that I am **not** reopening his stronger option (refuse remote content when Layer 1
is absent); he did not ask me to, and B's C4-vs-C5 resolution stands.

---

## INT-9 — CONCEDE, one line into C5

**Reproduced.** `grep -n -- "--config"` over all six P2-C thread files and the P2-C and P2-A
specs → **zero**. In the tree: `spawn.rs:121` (Windows) does `cmd.arg("--config").arg(config_dir)`;
the `#[cfg(not(windows))]` stub at `:199-210` takes `_config_dir` and drops it;
`kiosk-main`'s `resolve_config_dir` (`main.rs:423-431`) falls back to `current_exe().parent()`,
which under G1's layout is `/usr/lib/kiosk` — where `kiosk.ini`, `kiosk-credential.json` and
`kiosk-offline.mp4` do not exist. That reproduces V13 one process downstream, and G1 declares
the requirement one-directionally (`P2G-R3-writer.md:235`).

**C5's change list gains one line, and the edge is written in both registers:**

> The Unix `spawn_main` appends `--config <config_dir>` to the child's argv, byte-identical to
> `spawn.rs:121`. Fail-closed: if `config_dir` is not passed, `kiosk-main` resolves the install
> dir and finds no operator files.

---

## INT-10 — CONCEDE, delete the edge

Reproduced: `P2B-R1-writer.md:347-350` drops the `securitypolicyviolation` listener and the
`#[cfg(not(windows))]` Tauri command, and says explicitly *"nothing needs to touch"* `main.rs:990`.
B's final register (B1–B12) has no Tauri command and no ACL entry. So E1's
`Depends on: P2-B (three shared files…)` (`P2E-R3-writer.md:220`) points at a withdrawn change
and fabricates a B-before-E ordering constraint.

**E1's `Depends on` becomes `P2-A` only. E1 owns `main.rs:990`, `build.rs` and
`capabilities/default.json` alone**, and its justification stops citing a sibling that is not
editing them. (This also matters for C17: if arch-04 had needed a Tauri command it would have
collided here. It does not — `run_javascript` needs no IPC and no ACL entry.)

---

## INT-11 — REBUT in part, then REVISE (his fix adds an edge; the correct fix deletes one)

**His tier-4 facts are right and I re-ran them.** `wry-0.55.1/src/lib.rs`'s `pub use` block
(`:363-415`) does **not** re-export `webkit2gtk`; `tauri-2.11.5/src/webview/mod.rs`'s own doc
example tells the consumer `use webkit2gtk::WebViewExt;`. D1's mechanism is stated on
`webkit2gtk::WebView` (`P2D-R3-writer.md:141`) and D10 declares `gtk` only. The omission is real.

**What I rebut is the consequence: "it makes B10 a prerequisite of D".** It does not.
`webkit2gtk` is a *declaration in `kiosk-main/Cargo.toml`*, not a code artifact one spec hands
another. B10 and D10 write the same line. Cargo unions features across a single dependency
declaration edit, and D needs no feature above B10's `["v2_32"]` — `WidgetExt`/signal connects
are ungated. So:

> **D10's register gains `webkit2gtk = { version = "2.0.2" }`, target-gated, no features beyond
> B10's.** The declaration is **shared** with B10: whichever of B or D lands first writes the
> line; the second reconciles by union. **No ordering edge between B and D exists or is
> declared.**

That is one register line and zero merge-order constraints, against his one register line plus a
B→D edge. Same defect closed, one edge fewer. (C17 also uses this declaration — it calls
`run_javascript` on the same `webkit2gtk::WebView` — so C joins the same shared line, again with
no ordering implication.)

---

## INT-12 — REBUT in part (the hand-forward asked for a re-derivation) · ACCEPT-AS-DOCUMENTED-RISK (enforcement)

**His fact is verified.** `tauri-2.11.5/Cargo.toml:339-341`, linux target block:
`webkit2gtk` `version = "2"`, `features = ["v2_40"]`. Cargo unifies, so the compiled crate
carries `v2_40` whatever B10 declares.

**REBUT on what was promised.** P2-A's hand-forward reads (`p2a:71-74`): *"P2-A introduces no
`v2_40`-gated symbol… **Do not reintroduce** `ResponsePolicyDecision::is_main_frame_main_resource`
**without re-deriving that floor**." That asks for a re-derivation of which symbols are called at
which gate, and B10 delivered it and it is correct (chains verified in `P2B-R3-writer.md:214-218`;
I re-checked `run_javascript`'s gate this turn for C17 — ungated, `web_view.rs:1466`). A
build-enforced floor was never asked for and inventing one now is Q2 invention.

**ACCEPT-AS-DOCUMENTED-RISK on enforcement, with the spec wording corrected.** He is right that
what B10 currently *calls* a floor is a review convention.

- **Residual:** nothing in the build stops a future edit from calling a `v2_40`-gated symbol;
  it would compile and would break only on a distro below 2.40.
- **Carrier:** B's divergence list, restated in B10's own text — *"`["v2_32"]` is the **declared
  minimum of symbols this spec calls**, re-derived per P2-A's hand-forward. It is **not enforced
  by the build**: tauri 2.11.5 declares `webkit2gtk` with `features = ["v2_40"]`
  (`tauri-2.11.5/Cargo.toml:341`) and Cargo unifies features across the graph. Enforcement is
  code review against this line."*
- **Why not the `cargo tree` check he offers:** it reports, it does not enforce — it would print
  `v2_40` on a green build, which is the status quo, and would fail nothing. Runtime risk today
  is nil (Debian 12 ships ≥ 2.40, C7). Adding a CI job that cannot fail for the right reason is
  the over-build.

C17 respects the declared minimum deliberately: it uses `run_javascript` (ungated) rather than
`evaluate_javascript` (`#[cfg(feature = "v2_40")]`, `web_view.rs:788`), which is what makes the
declaration mean something in the one place a new call site was added this round.

---

# Register changes by spec

Exactly what each spec's text must now say. Nothing here is optional.

## B — P2-B (2 changes)

1. **B2 — `compile_filter` moves to `kiosk-core::nav`** (INT-6). The compiler and the corpus
   implication test live in `crates/kiosk-core/src/nav/`, the test inside `allowlist.rs`'s
   existing `#[cfg(test)] mod tests`. `regex` becomes a `[dev-dependencies]` entry on
   `kiosk-core` (test-only; already in `Cargo.lock`). `kiosk-main` keeps only the sys-FFI shim
   that installs the emitted JSON. B's C1 paragraph is rewritten: C1 is *satisfied*, not
   strained, for the compiler; it remains strained only for the second matcher itself.
2. **B10 — floor wording** (INT-12). "Called-symbol floor = 2.32, final" becomes "**declared
   minimum**, re-derived per P2-A's hand-forward, **not enforced by the build** — tauri
   2.11.5 declares `["v2_40"]` (`tauri-2.11.5/Cargo.toml:341`) and Cargo unifies features."
   Added to the divergence list as a named residual. The `webkit2gtk` declaration is noted as
   **shared with D10 and C17** (union of features, no ordering edge).
3. **B12 — smoke 8(d) mechanism** (INT-7). `chmod 000` deleted. 8(d) creates
   `data_dir/content-filters` as a **regular file**; `create_dir_all` then fails for every uid
   including root. One line in the fixture.

## C — P2-C (3 changes, one of them new and load-bearing)

1. **C5 — `--config` forwarding** (INT-9). *"The Unix `spawn_main` appends
   `--config <config_dir>`, byte-identical to `spawn.rs:121`."* Edge to G1 declared on C's side.
2. **C17 — NEW: webview round-trip gate on the heartbeat** (INT-4, arch-04 / RT-02 / OD-1).
   Full text in INT-4 above. `#[cfg(not(windows))]`; `run_on_main_thread` → `with_webview` →
   `run_javascript("0", …)` under a 3 s cap; timeout ⇒ ping withheld ⇒ the existing 3-missed
   rule restarts main. Declared scope narrowing against the parent's word "cross-platform",
   justified by Windows' P1 `ProcessFailed`. Uses the `webkit2gtk` declaration shared with
   B10/D10 (`#[allow(deprecated)]`, no feature above `v2_32`).
   **Smoke 15 gains a variant:** block the GTK main thread only (not `SIGSTOP` on the process) →
   assert `watchdog.hang` + restart. First Linux exercise of arch-15 case (c).
3. **C10/C15 — version stamping** (INT-3c). Every cage claim in C's text is stamped
   *"cage 0.1.4 (Debian 12, C7 floor)"* or *"cage 0.1.5 (as run in-session)"*. C15's `cage -v`
   line (ruling R3) is unchanged; G15 now asserts the value.
4. **C12's residual re-pointed** (INT-2). The wedged-cage carrier is named as **G H11**, not
   "a hardware-checklist row"; the orphan-kill gate is named as **G15's `pkill -9` assertion**,
   not "a P2-G row".

## D — P2-D (3 changes, all one-liners)

1. **D10 — dependency register** (INT-11). Gains `webkit2gtk = { version = "2.0.2" }`,
   target-gated, no features beyond B10's; **declaration shared with B10/C17, no ordering edge**.
2. **D13 / D5 / D11 — deferral targets corrected** (INT-2). `H10` → confirmed as a real G row
   (text now in G14). `GDK_TOUCH_CANCEL` and the N-finger deadband route to **H4a**, not H4.
   Smoke 17's cage-headless fallback routes to **H4a**, not H4.
3. **D12 — smoke 17 driver** (INT-3b). Names the `GDK_BACKEND=x11` + Xwayland + `xdotool`
   route as the CI driver on the floor, with the declared C3 divergence (X11 GDK backend, not
   Wayland) and the hardware fallback to H4a. D's open plan-time question (`p2d:127-129`) is
   **closed negatively** for cage 0.1.4 and recorded as closed.

## E — P2-E (3 changes)

1. **E5 — cycle broken** (INT-1). `Depends on` becomes: *"E4 (in); the first green nightly
   F7/18-W2 run (in, merge gate on the enforcement half only). **No outbound edge.**"* Added:
   *"E5's enforcement branch and the single line adding `18-W1` to F7's matrix land as **one
   commit**, after that nightly. Everything else in E — including all three scenario bodies —
   lands in E's first merge."*
2. **E1 — stale B edge deleted** (INT-10). `Depends on` becomes `P2-A` only. E1 owns
   `main.rs:990`, `build.rs`, `capabilities/default.json` alone.
3. **E8 — the E→F boundary sentence** keeps its "F references, never restates" rule, and adds:
   *"F7's artifact (the RSS series) is the evidence E's floor gate reads. An artifact is not a
   dependency; F declares no edge onto E5."*
4. **E9/E10 — proposed, pending Moderator assent** (see the last section): `maintenance.restart_app`
   and `logging.level` as the two remaining pure-config P2-row knobs.

## F — P2-F (3 changes)

1. **F7 — cycle broken** (INT-1). `Depends on` becomes `E4 + the 18-W1/18-W2 bodies (in)`.
   `E5 (in)` and `**E5 (out)**` are both **deleted**. Ships as `strategy.matrix.scenario:
   [18-W2]`; `18-W1` is added by E5's enforcement commit. F's R4 §"Integration items" item 3 is
   rewritten accordingly — the edge is not "recorded in both directions", it is one direction.
2. **F5 — cage in the environment** (INT-3a). Package set gains **`cage`, `xwayland`,
   `xdotool`**. New spec sentence: *"A 1–7, B 8–12, D 16–17 run under weston headless; C 13–15
   run under `cage -- kiosk-launcher` with `WLR_BACKENDS=headless`. No scenario runs under an
   unnamed compositor."*
3. **F4 — scenario 14/17 driver and fallback** (INT-3b). Names the `GDK_BACKEND=x11` route, its
   declared divergence, and the hardware fallback (G H2 + H4a) — *recorded, not silently
   dropped*, matching 17's existing discipline.

## G — P2-G (6 changes)

1. **G14 — checklist becomes H1–H11**, with **H4 split into H4a (touch) and H4b (text-entry)**.
   New rows H4a, H10, H11 with the text tabulated in INT-2 above.
2. **G15 — two assertions added** (INT-2): `pkill -9 kiosk-launcher; sleep 2; ! pgrep kiosk-main`
   (C12's orphan-kill gate, closing V4 for real) and `cage -v` emitted and asserted equal to the
   recorded floor `0.1.4` (C10/C15's floor assertion).
3. **G14/H8 — SEC-10 provisioning assertion** (INT-8): *"After first boot and before sign-off,
   read the local spool and assert it contains no `egress.filter_absent` and no
   `egress.csp_absent`."*
4. **G runbook — keyboard erratum re-scoped** (INT-5). The four-protocol claim is stamped
   **cage 0.1.4 (C7 floor)**; the `zwp_virtual_keyboard_v1` limb is **withdrawn** — present on
   0.1.5. The load-bearing statement becomes: *"`zwlr_layer_shell_v1` is absent on 0.1.4 and
   0.1.5 (verified), so no separate-process OSK can place itself over a fullscreen client on
   either version."* The ActivityClock sentence is narrowed to the XTEST/onboard route only.
5. **G runbook — recovery step** (INT-4): *"If the screen is frozen and the device has not
   recovered within 60 s, power-cycle. On a conforming image there is no VT, no getty and no SSH
   by design (G10/G12); a power cycle is the supported recovery."*
6. **G1 — the `--config` edge** is now mutual: C5 carries it too (INT-9). G1's cell keeps its
   text; C's register gains the matching line.

---

# Merge order (committed)

Every edge stated in both directions. Steps 1a and 1b are order-independent with respect to each
other; everything else is strict.

```
0.   P2-A rev 3                     already reviewed. kiosk-main resolve_data_dir → /var/lib/kiosk
1a.  P2-C  ⊕ C16 ⊕ C17              hard co-land with (0) per X5
1b.  P2-B                           independent of C
2.   P2-D
3.   P2-E  (all but E5's enforcement branch)
4.   P2-G
5.   P2-F  (F7 as matrix [18-W2])
6.   ONE COMMIT: E5's enforcement branch + `18-W1` into F7's matrix
```

| Edge | Out (spec, register row) | In (spec, register row) | Why |
|---|---|---|---|
| A → C | A: `resolve_data_dir` = `/var/lib/kiosk` | C16 | X5 hard co-landing; C's launcher half must equal A's kiosk-main half |
| C → G | C5: argv carries `--config <dir>` | G1: `spawn_main` must carry `--config` | INT-9; without it config/credential/mp4 resolve to `/usr/lib/kiosk` |
| C → G | C11: `[Service]` shape | G8: the installed unit | already declared both ways |
| C → G | C12 residual (wedged cage), C10/C15 (cage floor) | G H11, G15 `cage -v` | INT-2 |
| C → G | C12 orphan-kill | G15 `pkill -9` assertion | INT-2; closes V4 |
| C → F | C14: RT-13 cross-platform | F5/F2 | already declared |
| C → G | C17: recovery premise (main restarts itself) | G runbook recovery step, H11 | INT-4 |
| D → G | D3: chord sentence | G10's reserved slot | already declared |
| D → G | D13: PF-04 intercept | G H10 | INT-2, now real |
| D → G | D5/D11: touch residuals | G H4a | INT-2, now real |
| B ↔ D ↔ C | shared `webkit2gtk` declaration | — | INT-11: **no ordering edge**; union of features, first writer wins |
| E → F | E8: 18-W1/18-W2 bodies + parameter table | F7 (references, never restates) | X4; ruling R3's key-name fixes apply |
| F → E | *(none)* | — | INT-1: F7's RSS series is an **artifact**, not a dependency. `E5 (out)` deleted |
| F ⇒ E5 | F7's first green nightly 18-W2 | E5's enforcement branch (merge gate) | INT-1; satisfied at step 6, after step 5 |
| E → G | E7: mp4 path | G6 | already declared |
| G → F | G15 assertions, `dpkg-shlibdeps → dpkg-gencontrol → dpkg-deb -b`, lintian | F5, F8, F12 | already declared; step 4→5 strict |
| B → G | B9 relabelled inert defence-in-depth | G11 | X2; labelling only, no code |

**Deleted this round:** E1 → P2-B (INT-10, edge onto a withdrawn change); F7 → E5 (INT-1, the
cycle); B10 → D10 (INT-11, replaced by a shared declaration).

**Why D no longer needs B.** The only claimed B→D edge was the `webkit2gtk` declaration
(INT-11). D declares it itself. D at step 2 is a convenience (it lands next to C17's use of the
same crate), not a constraint.

---

# Unowned P2-row obligations — proposed dispositions

Frame §2: an uncovered P2-row item with no identifiable owner is a HIGH integration defect, and
"defer to P3/P4" is inadmissible unless the parent defers, which it does not for any of these.

## Proposed owners (closed inside this review)

| Item | Owner I propose | Change | Why it is that spec, not a new one |
|---|---|---|---|
| **arch-04 / RT-02 / OD-1 — JS-ping webview-hang detection** | **P2-C** | **C17** (new) | C already owns `kiosk-main/src/heartbeat.rs`'s Linux body (`p2c:26`, `:124-129`, row C8) and the hang scenario (smoke 15). The change is one arm of one function C is already rewriting. Mechanism verified feasible at B10's floor (`run_javascript` ungated, `web_view.rs:1466`). Also the fix for INT-4. |
| **`maintenance.restart_app`** | **P2-E** — *requires Moderator assent to widen E's scope* | **E9** (new) | Same config section E already owns (`maintenance.max_webview_mem_mb`, E4/E5), same timer module (`maintenance.rs` is the nightly-reload timer — verified, `maintenance.rs:1`), same mechanism (exit with a code the launcher restarts on — E5 builds exactly that path). `restart_app` = E5's exit path fired by `maintenance.rs`'s clock instead of by a threshold. `validate.rs:20` tags it `"P2"`. |
| **Remote log level** | **P2-E** — *same assent, or falls to the bucket below* | **E10** (new) | `Severity` already exists (`logging/entry.rs:60`); `logging.level` already parses and validates (`schema.rs:247-248`, `validate.rs:11`) with **no consumer** — verified, `grep "\.level\b" crates/kiosk-core/src/logging/*.rs` → zero. The delivery is a `>=` drop before spool in `Telemetry`, ~5 lines, platform-free. It is not naturally E's; I propose it only because bundling it with E9 costs one sentence and closes a second parent P2-row item, versus standing up a sub-project for five lines. **If the Moderator refuses the widening, E9 and E10 both move to the bucket below.** |

## Need an owner-level decision outside this review

| Item | Why I cannot propose an owner | What I can pin |
|---|---|---|
| **I1 — RT-16 `inject_css`/`inject_js` + Linux touch keyboard** | The only viable mechanism is `crates/kiosk-main/src/inject.rs` (verified shipping, `inject.rs:1-19`, wired `main.rs:1041-1046`). **No B–G spec opens that file**, and P2-D disclaims it explicitly (`p2d:26`, `:162`). Inventing the feature inside a packaging or an input spec is the scope error I would object to in a sibling. Needs a new `inject.rs`-scoped P2 sub-project. | The shape is bounded: a **bundled always-on** keyboard needs no live reinjection (`inject.rs:12-18`) and does **not** depend on RT-16 landing. INT-5's correction goes into I1's row: *display* is blocked by layer-shell absence on 0.1.4 **and** 0.1.5; *injection* is not blocked on 0.1.5. Discoverability meanwhile: G's runbook prerequisite + **H4b**. |
| **I2 — M4 / OD-8 PDF default-block, Linux column** | Parent §12 records OD-8 **applied**, not deferred. `scheme_guard::pdf_decision` is `#[allow(dead_code)]` on **both** platforms (verified, `scheme_guard.rs:36`), and `validate.rs:18` mis-tags it `"P1"` so the warning never fires. B verified the WebKitGTK route (`ResponsePolicyDecision`) is outside the declared floor. Symmetric non-delivery is not a discharge, and no P2 spec owns the Windows half either. | Needs a parent amendment or a cross-platform owner. The `validate.rs:18` `"P1"` → `"P2"` mis-tag is a one-character fix that at least makes the knob warn; I flag it, I do not assign it. |
| **H-i — WebKitGTK `print` signal returns TRUE (H1)** | Parent §7 Linux cell. No spec owns it. | Partially compensated and I say so rather than claiming closure: `inject.rs:46-48` overrides `window.print` platform-agnostically and ships in P1. MED, not HIGH — the residual is a native chrome path, not the JS one. |

**Two of the five get owners here (arch-04, `restart_app`), a third conditionally
(remote log level), and two (I1, I2) do not and cannot without an owner-level ruling.** The
Critic's count of five unowned drops to two-or-three depending on one Moderator decision about
E's scope. I am not proposing a P3 deferral for any of them, because the parent defers none.

---

## Termination — my confirmation

All twelve objections dispositioned; none struck; four HIGHs closed by construction (a split, an
edge deletion, six checklist rows, an apt package plus a driver, and one new change with a
verified mechanism). Both remaining HIGH gaps (I1, I2) are ledger items with named shapes and no
spec pretending to own them.
