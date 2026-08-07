# P2-G — WRITER, Round 3

No frame dispute. Eleven accepted objections are closed and not reopened. **[V]** = verified
by me this turn.

---

## OB-3 — REVISE (escalate as an unowned P2-row item, with a named mechanism and phase)

### (b) first: the `inject_js` limb was false. Withdrawn outright.

**[V]** `grep -rniE "inject_css|inject_js|RT-16" docs/superpowers/specs/2026-08-06-p2*.md`
→ **rc=1, zero hits.** No P2 spec mentions the knobs or RT-16.
**[V]** `crates/kiosk-core/src/config/validate.rs:15-21`:

```rust
const UNIMPLEMENTED: &[(&str, &str)] = &[
    ("content.inject_css", "P2"),
    ("content.inject_js", "P2"),
    …
```

so an operator who sets `inject_js` today gets an RT-08 `config.warn` and no behaviour.

My R2 sentence — *"a **named P2-row deliverable** … so the mechanism has an owner"* — is
**false**. Being named in the parent's P2 row is not the same as being owned by a spec, and
I conflated the two. Discharging G's obligation with it moved the gap one indirection
further from view, exactly as objected. **The `inject_js` route is withdrawn from the
runbook entirely.** It is not the fallback, not the default, not mentioned as an option.

### (c): the "bundled" citation stretch is conceded.

Parent §7's Windows cell says *"the **bundled** JS on-screen keyboard"*, and **[V]**
`crates/kiosk-main/bundled/pinpad.html` (2858 B) is exactly that shape. I invoked the
precedent for a deliverable I then declined to propose. Conceded.

### (a): the G1 standard, applied.

The Critic is right that I used two standards. Applying G1's:

> **Erratum — parent §7 keyboard row, Linux cell.** The cell reads *"Linux:
> squeekboard/onboard deployment docs"*. Both are non-viable under the compositor parent
> §7.2 mandates. Evidence (banked, independently reproduced): cage 0.1.4-4's complete
> protocol surface is its `*_create(` list in `cage.c:297-455` — no `zwlr_layer_shell_v1`,
> no `zwp_input_method_v2`, no `zwp_virtual_keyboard_v1`, no `zwp_text_input_v3`;
> squeekboard needs the first two to place itself and to type. onboard runs under cage's
> Xwayland (`cage.c:446`) but injects via XTEST, which cannot reach a native Wayland
> client. **The cell cannot be implemented as written.**
>
> **Substitute proposed:** a bundled keyboard injected through the document-start engine.
> **Escalated for ruling**, not decided here.
> **Fallback if the cell is ruled binding:** force `GDK_BACKEND=x11` inside cage's
> Xwayland and deploy `onboard`, forfeiting the Wayland input path P2-D is built on and
> the GDK event stream D's `ActivityClock` depends on. Written up in the runbook and
> **labelled as the fallback-if-the-cell-binds**, not as an appendix curiosity.

### New verified evidence: the substitute's engine exists and ships. The knob is the hard part; the bundled keyboard is the easy part.

This is the material thing I did not have in R2.

**[V]** `crates/kiosk-main/src/inject.rs:1-10` — *"Document-start injection assembly
(P1-D2b Task 6, spec §7 M1/M8) … the result to `WebviewWindowBuilder::initialization_script`
— Tauri/WebView2 runs `initialization_script` content BEFORE any page script on every
navigation."* **[V]** wired at `crates/kiosk-main/src/main.rs:1041-1046`:
*"P1-D2b Task 6: the ONE `initialization_script` call for this webview"* →
`builder.initialization_script(inject::build_injection(…))`.

So the engine frame §2 describes ("ships in P1") is real, shipping, and host-tested. And
**[V]** `inject.rs:12-18` explains why the *operator knob* is unimplemented while a bundled
keyboard is not:

> `initialization_script` may be called only ONCE per webview … and is set at BUILD time
> from the just-booted config … there is **no live-reinjection path, by design**.

A remote-config-driven `inject_js` needs live reinjection, which the engine does not
support — that is why it is `UNIMPLEMENTED`. A **bundled, always-on** keyboard needs no
reinjection at all: it is the same shape as the cursor-autohide timer already shipping
inside `build_injection`. The substitute therefore does **not** depend on RT-16 being
implemented; it is strictly the easier half of the same file.

Concrete insertion point, so the escalation is actionable rather than a wish: one more block
in `inject::build_injection`, plus `bundled/keyboard.html`-equivalent markup emitted by it,
gated on a boot-time config value. One line for the implementer to check that I am not
claiming: **[V]** `nav_policy.rs:169-178` records that `csp_policy` is deliberately **not**
wired for P1 (it would block legitimately-allowlisted subresources), so whoever adds the
keyboard must confirm the CSP interaction rather than inherit an answer.

### Scope finding that bounds the gap — verified, and it is why this is escapable

**[V]** `grep -n "<input\|<textarea\|contenteditable" crates/kiosk-main/bundled/*.html`
→ **zero hits across all five pages.** `pinpad.html` is a `<button>` grid writing into a
`<div>`, not a text field.

**No app-owned surface in this product has a text input.** The technician PIN flow — the one
input path the kiosk itself owns — already ships its own in-page pad. So nothing that P2-G
installs is broken today by the absence of a keyboard. The gap is confined to **a deployed
site that renders text inputs on a touch device**, which is the site's own UI surface.

Parity note (C3, and it is the honest one): **[V]** `grep -rn -i "tabtip|InputPane"` over
`crates/` returns nothing — Windows shipped P1 with PF-02 open too. Linux is not diverging
downward here; both platforms lack a system touch keyboard.

### Disposition: escalated, with owner and phase

**G does not discharge the Linux touch-keyboard obligation, and I say so plainly rather than
appearing to.** Reasons, in order:

1. The parent's two named mechanisms are impossible (banked).
2. The only viable mechanism is code in `crates/kiosk-main/src/inject.rs` plus a bundled
   asset. That is not a packaging deliverable, and inventing a feature inside a packaging
   spec is the scope error I would object to in any sibling spec. A usable OSK is layout +
   shift/symbols + focus tracking + viewport-shift-on-focus, not a line.
3. Target hardware is explicitly TBD, no app-owned surface needs it, and Windows has the
   same gap — so building it speculatively now, in the wrong spec, fails Q2 badly.

**Escalation, and the useful part: this is not a second unowned row — it is the same one.**
Both the keyboard and RT-16's `inject_css`/`inject_js` knobs live in `inject.rs`, on top of
the same P1 engine, and **[V]** both are unowned across the seven P2 specs. Routing them
together is one decision, not two, and the keyboard is the *easier* half (no live
reinjection). P2-D is **not** the natural owner despite owning Linux input — **[V]** `p2d`
Scope lists *"on-screen keyboard deployment (parent §7 table — P2-G)"* under **Out**, and
repeats it in Scope/defer; D has disclaimed it explicitly and D's module is `input_watch`
(GDK observation), not injection.

- **Named owner:** whichever spec picks up **RT-16** — the injection-knob row — since that
  is where `inject.rs` is already going to be opened. If integration creates no such
  owner, the fallback owner is a new P2 sub-project scoped to `inject.rs` (RT-16 + keyboard).
- **Phase:** P2 if RT-16 is routed in P2 (frame §2 lists RT-16 as a P2 obligation, so this
  is the consistent reading); **P3 only if the Moderator defers RT-16 itself** — G does not
  get to defer it unilaterally, per frame §2's "deferred to P3/P4 is only admissible if the
  parent itself defers it."

### What G ships now, regardless of the ruling

A runbook section `On-screen keyboard`, containing:

1. **squeekboard ruled out, with the protocol evidence**, so nobody re-litigates it.
2. **onboard + `GDK_BACKEND=x11`** written as the *fallback-if-the-§7-cell-binds*, with its
   cost stated (forfeits the Wayland input path and D's GDK event stream).
3. **`Depends:` — none, stated explicitly.** No installable package solves this under cage;
   shipping one would ship a broken dependency.
4. **The operator-facing consequence, unhedged:** *"This build ships no on-screen keyboard.
   A deployed site that requires text entry on a touch device must render its own input UI.
   Verify this against the site before deployment — it is a deployment prerequisite, not an
   app capability."* That sentence is the actual §7 "deployment docs" deliverable that is
   achievable today, and it makes the gap loud at provisioning time instead of at first
   touch.
5. The D-premise note kept: any *separate-process* OSK produces no GDK events in our
   process and would break D's `ActivityClock`; the bundled/injected route does not.

**H4 revised again:** "verify the deployed site's text-entry surfaces on the device class;
record whether any input has no usable keyboard, per device class." That converts the
residual from silent to enumerated — the H-row is the discoverability mechanism, not the
discharge.

**Severity acknowledged: this remains HIGH and open.** G's contribution is that it is now
bounded (no app-owned surface affected), evidenced (both named packages provably
impossible), mechanised (`build_injection`, verified to exist), and bound to an existing
unowned row rather than floating.

---

## Acceptance condition 1 — launcher path unchecked by `systemd-analyze verify`

**Accepted, and reproduced. [V]**

```
ExecStart=/usr/lib/kiosk/kiosk-launcher --config /etc/kiosk   (absent)  → rc=1
ExecStart=cage -- /usr/lib/kiosk/kiosk-launcher …             (absent)  → rc=0
```

With cage as the command the launcher is an argument and `verify` never looks at it — the
exact hole G1's path change opens. Added to G15's container assertions:

```sh
test -x /usr/lib/kiosk/kiosk-launcher
test -x /usr/lib/kiosk/kiosk-main
```

Cheapest possible cover, in a test that already installs the `.deb`.

## Acceptance condition 2 — bound the "uploaded retroactively" label

**Accepted.** G5's matrix column now reads:

> spooled locally; uploaded retroactively **if the device is provisioned within the spool's
> retention window** (`SpoolConfig::from_logging`, with `spool.dropped_expired` as the aging
> signal — parent §6). Otherwise the on-screen `safe.html` and H8's cold-install step are
> the only signal.

The label was doing real work after the Critic withdrew his limb-2 sentence; it should not
be doing more work than the mechanism supports.

## Acceptance condition 3 — `dpkg-gencontrol` alongside `dpkg-shlibdeps`

**Accepted; this would have silently broken.** `${shlibs:Depends}` is a substvar consumed by
`dpkg-gencontrol`; `dpkg-deb -b` over a hand-written `DEBIAN/control` emits the literal
string. G3 and G15's declared addition to F §3 is therefore **`dpkg-shlibdeps` →
`dpkg-gencontrol` → `dpkg-deb -b`**, all three named. G15 gains one assertion that makes the
failure impossible to ship silently: `grep -q '\${shlibs' <extracted control> && FAIL`.

## Consistency item — `kiosk.ini` absent on a fresh install

**Accepted.** G15's assertion list read "`kiosk.ini` present" in R1 and was silent in R2.
It now reads: on a fresh install **all three operator files are absent** —
`/etc/kiosk/kiosk.ini`, `/etc/kiosk/kiosk-credential.json`, `/etc/kiosk/kiosk-offline.mp4` —
and `/usr/share/kiosk/kiosk.ini.example` is **present**. That is the property G5/G6/G9 were
reaching for, now asserted rather than implied.

## Recorded for integration — B9 (`systemd-inhibit`)

Stated as G's finding, since B's Writer rounds are closed:

- B's `systemd-inhibit --what=idle:sleep --mode=block cat` child is **inert on both axes**
  under G's runbook: `sleep` because G12 masks all four sleep targets; `idle` because
  `IdleAction` is at its stock `ignore` and cage never raises an idle hint anything
  consumes.
- **Keep the child.** One `cat`; it is the only mechanism still acting if an operator
  unmasks a sleep target. Correct label: **defence-in-depth with no current effect.**
- **B's belt/suspenders framing is inverted** relative to parent §7/§11, which name the
  compositor configuration **PRIMARY**. B calls its inhibitor the mechanism and G's image
  config the suspenders. A labelling correction, no test to change (B's smoke 12 only
  asserts the degrade path). **Owner: integration.**
- Parent §11's *"confirm cage honours idle-inhibit before relying on it"* closes **negatively
  and definitively**: cage's `zwp_idle_inhibit_v1` toggles only cage's own unconsumed
  `wlr_idle` notifier (`idle_inhibit_v1.c:34-35`) and has no relation to logind locks. There
  is nothing for it to inhibit.

---

## Final register — G1…G16

| ID | Final state | Depends on |
|---|---|---|
| G1 | `/usr/lib/kiosk/` binaries+assets, `/etc/kiosk/` operator files, `--config /etc/kiosk`. Parent §4 erratum named + escalated; `/opt` + lintian override is the stated fallback | parent §4 erratum ruling; C:85 `ExecStart`; **C's Linux `spawn_main` must carry `--config`** (fail-closed if not) |
| G2 | Payload = 2 binaries + `bundled/` + unit + `kiosk.ini.example` + `kiosk-provision-credential`. `kioskctl` withdrawn | G1 |
| G3 | `${shlibs:Depends}` via **`dpkg-shlibdeps` → `dpkg-gencontrol`**; `cage` + 4 GStreamer hand-written; `Conflicts:` on idle daemons; **no keyboard `Depends:`, stated**; t64 residual withdrawn | F §3 (three tools named) |
| G4 | `/var/lib/kiosk` `0750`; create+`chown` **first-install-only** (`[ -z "$2" ]`), modes-only on upgrade | G16, A |
| G5 | Ship nothing at the credential path; `/etc/kiosk` `0750` as a **traversal barrier only**; `kiosk-provision-credential` sets the mode; upgrade-only `chmod` re-assert; C3 asymmetry vs F2 declared; matrix column **bounded by spool retention** | G1, G16, A |
| G6 | mp4 operator-provisioned; postinst warns on absence; no conffile | G1, E |
| G7 | debhelper autoscripts verbatim; `was-enabled` guard confirmed not to re-enable on upgrade | G8 |
| G8 | `[Unit] StartLimitIntervalSec=0`, `After=systemd-user-sessions.service`; `[Service]` + `RestartSec=30`, `SuccessExitStatus=86`, `RestartPreventExitStatus=86`, `RuntimeDirectory=kiosk`; **`[Install] WantedBy=multi-user.target` — G owns it**; residual pinned to H2 | C's `[Service]` shape |
| G9 | **Zero conffiles.** Example at `/usr/share/kiosk/` (not `/usr/share/doc/`, `path-exclude`d) | G1, G5, G6 |
| G10 | cage without `-s`; "no other TTYs" a gate step; X11 `DontVTSwitch`/`DontZap` in the appendix | D |
| G11 | `Conflicts:` as continuous enforcement + grep as the verify line; parent §11 closed negatively; B's child relabelled | B (finding recorded for integration) |
| G12 | Cosmetics, `unattended-upgrades` off, WebKitGTK hold, SSH; journald `Storage=persistent` + computed ≥7-day `SystemMaxUse` floor | F §4 |
| G13 | `dpkg -l` + `dpkg-query -W -f=…` capture per device class | G12 |
| G14 | H1–H9. **H2** gains `is-active`→`active`; **H3** loses `--list` as keep-awake proof; **H4** now records per-device-class text-entry surfaces lacking a keyboard | G8, G11, G16 |
| G15 | Container asserts: install/remove/purge, modes, `test -x` both binaries, **all three operator files absent**, example present, no literal `${shlibs`, zero `BEGIN PRIVATE KEY`, `is-enabled`→`enabled`, `systemd-analyze verify`, upgrade preservation. `active` → H2. Release job: `dpkg-shlibdeps`+`dpkg-gencontrol`+`lintian --fail-on error` | F nightly + F release |
| G16 | Default root (no `User=`); C3 Windows-parity divergence declared; `seatd` recipe + flip-list incl. `Wants=`/`After=seatd.service` and "postinst never re-asserts ownership after first install"; H1 promotes | A `p2a:275-276`; H1 |
| **OPEN** | **Linux touch keyboard — NOT discharged.** Parent §7 Linux cell is an erratum (both packages impossible); mechanism verified to exist (`inject.rs::build_injection`, `main.rs:1046`); **escalated to integration, bound to RT-16, same file, same gap**; phase P2 unless the Moderator defers RT-16 | **integration ruling** |
