# P2-G — CRITIC, Round 2

No frame dispute. **[C]** = checked by me this turn, command + result inline. I re-ran every
replacement rather than reading the Writer's transcript; where his result and mine agree I
say so, where I found something he did not I say that too.

## Disposition of Round-1 objections

| OB | Writer's move | My response | Status |
|---|---|---|---|
| OB-1 | `[Install] WantedBy=multi-user.target`, G owns it; debhelper autoscripts | Fix reproduced; **and I checked the upgrade question he did not raise** — the canonical snippet does *not* reintroduce enable-on-upgrade | **ACCEPTED** |
| OB-2 | Split: `is-enabled` in the container, `is-active` → H2 | Both halves reproduced; split is honest. **One required addendum** — `verify` no longer checks the launcher path | **ACCEPTED** |
| OB-3 | squeekboard/onboard ruled out on evidence; in-page keyboard default | Protocol-surface claim **verified in full**. Disposition and ownership are not discharged | **ESCALATED** |
| OB-4 | Provisioning helper + C3 asymmetry; partial rebuttal on limb 2 | Limb 1 accepted. **Limb-2 rebuttal is right and I withdraw my sentence** | **ACCEPTED** |
| OB-5 | Zero conffiles; `/usr/share/kiosk/`, not `/usr/share/doc/` | Both reproduced independently, including his `path-exclude` find | **ACCEPTED** |
| OB-6 | `Conflicts:` on idle daemons | Adopted with the honest removal-risk residual | **ACCEPTED** |
| OB-7 | Parent §11 closed negatively; H3 loses `--list`; B relabelled | Verified at source. Cross-spec consequence for B stated below as asked | **ACCEPTED** |
| OB-8 | `[ -z "$2" ]` first-install-only | Accepted — and now closed on both axes (ownership *and* enablement) | **ACCEPTED** |
| OB-9 | C3 divergence paragraph, adopting my scoping note | Exactly what C3 requires | **ACCEPTED** |
| OB-10 | Loudness withdrawn; `RestartSec` 30; persistent journal. Partial rebuttal on evidence loss | **His narrowing is correct and I concede it**; the fixes discharge both readings | **ACCEPTED** |
| OB-11 | Outright concession | Correctly recorded and correctly scoped | **ACCEPTED (concession)** |
| OB-12 | `${shlibs:Depends}` via `dpkg-shlibdeps`; t64 residual withdrawn | Accepted, with one precision on the assembly pipeline | **ACCEPTED** |

**11 ACCEPTED · 1 ESCALATED · 0 COUNTERED.**

---

## OB-1 — ACCEPTED, and the follow-on question is answered

**[C]** systemd 255, the R2 unit written out verbatim under `/tmp/r2`:

```
$ systemctl --root=/tmp/r2 is-enabled kiosk.service     → disabled
$ systemctl --root=/tmp/r2 enable kiosk.service
Created symlink /tmp/r2/etc/systemd/system/multi-user.target.wants/kiosk.service → …
$ systemctl --root=/tmp/r2 is-enabled kiosk.service     → enabled   (rc=0)
```

The ownership argument is sound and consistent with how he took `SuccessExitStatus=86`:
C:80 hands "values **and installation**" to G, and `[Install]` is installation.
`multi-user.target` is right — the runbook installs no display manager.

**The Moderator's follow-on — did fixing enable-on-install reintroduce enable-on-upgrade?
No. [C]** I fetched the canonical `Debian/debhelper` `autoscripts/postinst-systemd-enable`:

```sh
# was-enabled defaults to true, so new installations run enable.
if deb-systemd-helper --quiet was-enabled #UNITFILE#; then
	deb-systemd-helper enable #UNITFILE# >/dev/null || true
else
	deb-systemd-helper update-state #UNITFILE# >/dev/null || true
fi
```

and read the implementation on this host, `/usr/bin/deb-systemd-helper:418-434`:

```perl
sub was_enabled {
    my @entries = state_file_entries(dsh_state_path($scriptname));
    for my $link (@entries) {
        if (! -l $link) { … return 0; }      # operator `systemctl disable` removed it
    }
    return 1;
}
```

So an operator `systemctl disable` deletes the recorded symlink, `was-enabled` returns
false, and the upgrade takes the `update-state` branch — bookkeeping only, no re-enable.
**[C]** on a unit with no state file it returns rc=0 ("defaults to true"), which is the
first-install path. My OB-8 concern and OB-1's fix do not collide; adopting the shipped
snippet gets both properties at once, which is the Q2 answer.

Note also that `postinst-systemd-enable` is **not** wrapped in `[ -d /run/systemd/system ]`
(only `-start`/`-restart` are). That is what makes OB-2's split possible at all, and it is
load-bearing — if that guard had been on the enable snippet, the container could not assert
`enabled` and the whole split would collapse. It is not. Verified from source.

---

## OB-2 — ACCEPTED, with one required addendum

**Both halves reproduced. [C]** on this non-systemd host (`ps -p 1 -o comm=` →
`process_api`; `/run/systemd/system` absent):

- `systemctl --root=/tmp/r2 is-enabled kiosk.service` → `enabled`, rc=0 — a pure filesystem
  query, assertable with no PID-1 systemd.
- `systemctl --root=/tmp/r2 is-active kiosk.service` → `Verb 'is-active' cannot be used with
  --root=`, rc=1; without `--root`, `System has not been booted with systemd as init system
  (PID 1). Can't operate.` Unassertable either way.

So the line he drew is the real line, and it falls exactly where the OB-1 regression lives.
Moving `active` to H2 (which already owns the systemd half of the exit-86 contract per
`p2c:155-156`) and re-pinning G8's residual there is correct; the R1 pin was false and is
properly withdrawn.

**Is the split honest?** Mostly yes, and I want to be precise about what CI still catches so
the record is not rosier than the mechanism. It catches: (a) the OB-1 class — no `[Install]`,
not enabled, `static`; (b) unknown/misplaced directives, which is how the
`StartLimitIntervalSec` defect was found; (c) file layout, modes, payload, upgrade
preservation. It catches nothing about *starting*: cage obtaining DRM master, seat/session
availability, `RuntimeDirectory` under a non-root uid, library resolution at exec time. All
of those are H1/H2, declared and owned. That is an acceptable frame-§4.5 disposition.

**Required addendum — `systemd-analyze verify` no longer checks the launcher path. [C]**

```
ExecStart=/usr/lib/kiosk/kiosk-launcher --config /etc/kiosk   (file absent)
  → kiosk.service: Command /usr/lib/kiosk/kiosk-launcher is not executable…   rc=1

ExecStart=cage -- /usr/lib/kiosk/kiosk-launcher --config /etc/kiosk  (launcher absent)
  → rc=0
```

Because cage is the command and the launcher is an *argument*, `verify` checks only that
`cage` resolves (bare names are accepted — rc=0 with `cage` not installed on this host, so
it is not even checking that strictly). A wrong install path for the launcher would sail
through the container gate and surface only at H1. The fix is one line in a test that
already installs the `.deb`: `test -x /usr/lib/kiosk/kiosk-launcher && test -x
/usr/lib/kiosk/kiosk-main`. Please add it; it is the cheapest possible cover for the exact
hole G1's path change opens.

---

## OB-3 — ESCALATED (facts accepted, disposition and ownership are not)

**The protocol-surface claim is true, and I verified it myself rather than taking it. [C]**
I fetched Debian 12's cage 0.1.4-4 and enumerated every `*_create(` in `cage.c`:

```
297 wl_display  338 output_layout  345 compositor  352 data_device_manager
365 seat  372 wlr_idle  379 wlr_idle_inhibit_v1  389 xdg_shell
398 xdg_decoration_manager_v1  407 server_decoration_manager
417 export_dmabuf_manager_v1  424 screencopy_manager_v1  431 xdg_output_manager_v1
438 gamma_control_manager_v1  446 xwayland  455 xcursor_manager
```

**[C]** `grep -iE 'layer_shell|input_method|virtual_keyboard|text_input'` over
`cage.c seat.c output.c` → **zero hits**. **[C]** the upstream file list is
`cage.c idle_inhibit_v1.c output.c render.c seat.c util.c view.c xdg_shell.c xwayland.c` —
no `layer_shell.c`, no `input_method*.c`. squeekboard needs `zwlr_layer_shell_v1` to place
itself and `zwp_input_method_v2` to type; neither exists. It cannot run. And the onboard
analysis is right in shape: Xwayland exists (`cage.c:446`), so onboard runs, but XTEST into
Xwayland cannot reach a native Wayland client. **Both of the parent's two named packages are
non-viable under the parent's own mandated compositor.** That is a real finding and it is
G's.

**What I do not accept is the disposition, on two grounds.**

**1. Consistency with G1.** I banked G1 as a clean pass specifically *because* the Writer
did not decide the parent question himself: he named the exact cells, gave the evidence,
offered a survivable fallback, and escalated the ruling to the Moderator. OB-3's replacement
does none of that. It discovers an equally hard parent-internal contradiction — §7's
keyboard row (squeekboard/onboard) against §7.2's mandated cage session — and resolves it
*inside a runbook section*. If the tier-1 rule was worth honouring at G1 it is worth
honouring here. The §7 keyboard row's Linux cell needs the same treatment as the §4 table:
named as an erratum, with the substitute stated, escalated for ruling, and with a fallback
if the ruling goes the other way (the `GDK_BACKEND=x11` + onboard route already written is
that fallback — it just has to be labelled as the fallback-if-the-cell-binds, not as an
appendix curiosity).

**2. The substitute leans on an UNOWNED P2 row, so the requirement moves rather than
discharges.** The runbook's default is "in-page keyboard", deployed either by the site's own
web app or by the operator's `inject_js`. The Writer calls `inject_js` *"a **named P2-row
deliverable** … so the mechanism has an owner"*. It does not. **[C]**
`grep -rniE "inject_css|inject_js|RT-16"` over all seven `2026-08-06-p2*.md` returns **zero
hits** — the coverage matrix's R12 finding, unchanged this round. **[C]**
`crates/kiosk-core/src/config/validate.rs:16-17` still carries
`("content.inject_css","P2"), ("content.inject_js","P2")` in `UNIMPLEMENTED`, so in the
shipped build an operator who sets `inject_js` gets an RT-08 `config.warn` and **no
behaviour**. Discharging G's keyboard obligation by pointing at a knob that no P2 spec
implements converts one unowned row into a dependency on another unowned row. Frame §4.5
requires a named owner; there is none.

**3. A smaller citation stretch, stated for completeness.** Parent §7's keyboard row does
sanction the mechanism by name — but as *"the **bundled** JS on-screen keyboard"* (Windows
cell, read directly). Bundled means shipped in `bundled/`, and the in-repo precedent he
cites, **[C]** `crates/kiosk-main/bundled/pinpad.html` (2858 B), is exactly that: a
bundled, in-process, grid-of-buttons input surface. The Writer explicitly declines to ship
one ("G is not inventing a code deliverable inside a packaging spec"). So the precedent he
invokes is for a deliverable he is not proposing.

**What discharges this, any one of three — I am not prescribing which:**

- (a) Ship it: `bundled/keyboard.html` on the `pinpad.html` pattern, invoked on input focus.
  The pattern already exists in-repo, it is same-process (so it preserves D's `ActivityClock`
  premise — his cross-spec note there is correct and worth keeping either way), and it makes
  the parent's *bundled* wording literally true on Linux. This is the only option that
  leaves nothing unowned.
- (b) Rest solely on "the site web app renders its own input UI", state that P2 ships no
  keyboard, and give the residual an explicit owner (an H-row that records the gap per
  device class, or a P3 line). No `inject_js` dependency at all.
- (c) Keep `inject_js` as the route, but declare the dependency on RT-16/R12 — which is
  currently ownerless — so the Moderator can route it at integration.

What is not acceptable is the current text, which reads as though the requirement is
discharged. **Severity unchanged: HIGH**, because a touch device with any text input still
has no keyboard, and now the gap is one indirection further from view.

Everything else in the OB-3 replacement I accept and want kept: squeekboard documented as
ruled out with the evidence (so nobody re-litigates it), the X11 fallback with its stated
cost, "no `Depends:`, stated explicitly", and the D-premise note.

---

## OB-4 — ACCEPTED, and I withdraw my limb-2 sentence

**Limb 1.** The replacement is the right shape: the package now *ships a thing that sets the
mode* (`/usr/lib/kiosk/kiosk-provision-credential` doing `install -m 0600 -o root -g root`)
instead of a sentence asking the operator to remember flags, the traversal barrier is
described as a traversal barrier, and the C3 asymmetry against F2's `util:PermissionEx` is
declared with its consequence ("a mis-provisioned Linux device sits in safe mode rather than
never having been mis-provisioned"). Accepted as written.

**Limb 2 — his partial rebuttal is correct and my R1 sentence was wrong. [C]** I traced it
end to end:

- `crates/kiosk-main/src/main.rs:815` — `if let Some(reason) = boot_fault_reason { telemetry::spool_boot_config_error(…) }`, with the comment naming exactly this case
  (*"this needs no GCL client and no credential"*).
- `crates/kiosk-main/src/telemetry.rs:291-320` — opens `<data>/spool/main` directly and
  appends a `LogEvent::ConfigError` entry. No transport, no credential.
- `crates/kiosk-main/src/boot.rs:81-90` `into_parts()` — `Ready` → `(booted, None, None)`;
  `RenderSafe{reason}` passes `reason` through. So **row 2** (empty-`0600` → parse failure,
  `reason: None`) and **row 3** (F2 placeholder → `Ready`) both yield
  `boot_fault_reason == None` and spool nothing about the credential; only **row 1** spools.
- Durability is tested: `telemetry.rs:697`
  `spool_boot_config_error_writes_a_durable_config_error_entry`.

So "to the fleet, all three rows are equally invisible" is **false** and is withdrawn. Row 1
is deferred-visible with a named draining mechanism; rows 2 and 3 are permanently invisible.
The ranking argument for shipping nothing is stronger than I allowed, and the relabelled
matrix column ("spooled locally; uploaded retroactively on the first provisioned boot") is
accurate.

**One precision on that label, since he is now relying on it.** The spool is bounded —
`Spool::open(…, SpoolConfig::from_logging(logging))`, and the project already emits
`spool.dropped_expired` (parent §6). A `config.error` sitting unspooled on a device that is
never provisioned, or provisioned months later, can age out. "Uploaded retroactively" should
read "…if the device is provisioned within the spool's retention window; otherwise the
on-screen `safe.html` and the runbook's cold-install step (H8) are the only signal." Not an
objection — a one-clause accuracy fix on a sentence that is now doing real work.

**The Moderator's judgement question — does the remaining invisibility need an owner?** No.
Rows 2/3's permanent invisibility is a property of the designs G *rejected*, so there is
nothing to own. Row 1's residual (never-provisioned device stays silent to the fleet) is
already owned by H8's cold run and by `safe.html` being visible on the physical screen,
which is the one place a fresh install is guaranteed to be looked at.

---

## OB-5 — ACCEPTED, both halves reproduced, including the trap I did not have

**[C]** Zero-conffile upgrade, `DEBIAN_FRONTEND=noninteractive`, `</dev/null`:

```
Setting up zc (2) ...            rc=0
/etc/kiosk/kiosk.ini  → mode 640, content OPERATOR-WRITTEN-INI   (untouched)
```

against my R1 reproduction of the conffile path, which aborted with
`end of file on stdin at conffile prompt`. Applying the credential/mp4 rule to `kiosk.ini`
is the consistent version of the rule and it removes the failure entirely rather than hiding
it behind a `--force-confold` an operator must remember.

**The `/usr/share/doc` trap is real and I reproduced it. [C]** built one package shipping the
example at both paths:

```
/etc/dpkg/dpkg.cfg.d/…  →  path-exclude=/usr/share/doc/*
ls root/usr/share/doc/kiosk/     → (empty — silently dropped)
ls root/usr/share/kiosk/         → kiosk.ini.example
dpkg -L zc                       → lists BOTH, including the file that is not there
```

That is a genuinely useful find — `dpkg -L` lying is exactly the kind of thing a packaging
test would assert on and be fooled by. `/usr/share/kiosk/` is the right home. One scoping
caveat for the record: I verified the `path-exclude` on **this** host (a minimized image); I
did not verify that the `debian:12` container image carries the same
`/etc/dpkg/dpkg.cfg.d/` config, so that half stays tier 5. Nothing load-bearing rests on it
— `/usr/share/kiosk/` is correct under either configuration.

**One consequence worth stating, checked and clean.** With zero conffiles, a fresh install
has *no* `/etc/kiosk/kiosk.ini` at all until the runbook writes one. That is fine and
fail-closed: **[C]** `crates/kiosk-launcher/src/main.rs:64-80` — a missing/invalid ini is
deliberately non-fatal in the launcher (defaults + `startup-degraded.txt` breadcrumb), and
kiosk-main's `boot::load` renders `safe.html`. All three operator files now behave
identically on a fresh device, which is the property G5/G6/G9 were reaching for.

Also flag for consistency: G15's R1 assertion list said "`kiosk.ini` present"; it must now
say **absent**, alongside the credential and mp4. The R2 register no longer states it either
way.

---

## OB-6 — ACCEPTED

`Conflicts:` adopted, the Policy §7.4 quote is accurate, and the scoping is right (idle
consumers and screensavers only; correctly declining to conflict with
`xdg-desktop-portal`, which is a dependency of too much and does not itself blank). The
declared residual — `apt install <thing pulling swayidle>` proposes **removing** `kiosk`,
and `apt -y` would proceed — is the honest statement of what `Conflicts` buys and does not
buy, and it is loud rather than silent, which is the point. Retaining the provisioning grep
as the verify line for hand-started binaries is right: `Conflicts:` binds packages, not
processes. Nothing further from me.

---

## OB-7 — ACCEPTED, and the cross-spec consequence stated plainly, as asked

**Verified at source. [C]** `idle_inhibit_v1.c:34-35`:

```c
bool inhibited = !wl_list_empty(&server->inhibitors);
wlr_idle_set_enabled(server->idle, NULL, !inhibited);
```

Cage's `zwp_idle_inhibit_v1` support does exactly one thing: it toggles **cage's own
`wlr_idle` notifier** — the notifier that, per G11's verified finding, no client on the
device consumes. It has no relationship to logind inhibitor locks. So parent §11's
precondition, *"confirm cage honours idle-inhibit **before** relying on it"*, is answered
**no**, definitively, and the answer is not "cage ignores it" but "there is nothing for it
to inhibit". G produced that evidence and now closes the row with it. Correct.

**Cross-spec consequence for P2-B, stated here because B's Writer rounds are closed.**

B's `display.keep_awake` Linux body spawns
`systemd-inhibit --what=idle:sleep --mode=block cat` and holds the child for process life.
On a device built by G's runbook that child has **no effect on either axis**:

- `sleep` — G12 masks `sleep.target suspend.target hibernate.target hybrid-sleep.target`
  (**[C]** all four exist and are `static` on systemd 255). A masked target cannot be
  entered, lock or no lock.
- `idle` — a `what=idle` lock blocks logind's `IdleAction`; G11 asserts `IdleAction` is at
  its stock `ignore`, so the blocked action is already a no-op, and cage never raises the
  idle hint that would reach logind anyway.

Three things follow, and none of them is "delete B's code":

1. **Keep the child.** It costs one `cat`, and it is the only mechanism still acting if an
   operator unmasks a sleep target. G's label — *defence-in-depth with no current effect* —
   is the Q2-honest one.
2. **B's framing is inverted relative to the parent and should be corrected at integration.**
   B calls its inhibitor the mechanism and G's image config *"the suspenders"*; parent §7
   and §11 name the compositor configuration **PRIMARY**. The coverage matrix already
   recorded that inversion (R3 / §2.3). G's R2 evidence settles which way round it goes.
   B's smoke 12 asserts only the degrade path, so nothing in B ever tested a positive
   effect — there is no test to update, only a sentence.
3. **H3's `--list` assertion is correctly demoted** from keep-awake proof to a regression
   check that B's spawn path ran. Keep-awake evidence is now the 24 h observation plus OB-6's
   `Conflicts:` gate, which is the first time that row has had a mechanism behind it.

Owner for (2): integration, since B cannot answer. It is a labelling change, not a design
change, and the parent §11 risk row closes with it.

---

## OB-8 — ACCEPTED (closed on both axes)

`[ -z "$2" ]` for directory creation and `chown`, mode-only on every configure, is the fix,
and mode-only is uid-agnostic so G16's `chown -R kiosk:kiosk` survives. The flip-list line
("postinst never re-asserts ownership after first install") is the durable part. Combined
with the `was-enabled` guard verified under OB-1, the whole "upgrade silently reverts
operator state" class is now closed on both the ownership axis and the enablement axis —
which is more than my objection asked for.

---

## OB-9 — ACCEPTED

The added paragraph is what C3 requires, in both directions, with the justification and the
"this is not the project's normal posture" sentence that keeps it from reading as a
decision. He adopts my scoping note verbatim rather than paraphrasing around it, which makes
the residual narrower and more accurate than either of our previous statements: the non-root
variant does not close the renderer→credential path, and the mitigation list addresses
escape, not privilege. Nothing further.

---

## OB-10 — ACCEPTED, and his narrowing is conceded

**Concession first, since he is right.** For a *permanently* broken install the Nth failure
is byte-identical to the first, so journald rotation discards duplicates and
`systemctl status` shows the same failure it always showed. My R1 phrasing implied the cause
is erased in the general case; it is erased only in the transient-then-persistent case. I
concede the narrowing and do not press it.

I hold only that the surviving case is not a small one — a device that failed once for
reason X and now fails persistently for reason Y is the hardest field diagnosis there is —
and he evidently agrees, because he fixed it anyway: `RestartSec` 5→30 (2,880 starts/day
instead of ~17k, still an order of magnitude looser than the FSM's `WINDOW_S = 600` /
60 s backoff ceiling, so the FSM remains the authority that gives up first),
`Storage=persistent`, and a `SystemMaxUse` stated as a computed ≥7-day floor rather than a
round number. Those three discharge both readings.

The loudness concession is correct and verified: **[C]** `sink.rs:163-179` — the breadcrumb
is `File::create` + one `writeln!`, and its own doc says *"Presence therefore means 'the LAST
boot was degraded', not 'some boot, once, was degraded'"*. Replacing G8's loudness sentence
with "pre-launcher failures (cage/DRM/library/path) are journal-only" is the accurate
statement. `StartLimitIntervalSec=0` stands — I never contested the directive, and a
systemd-enforced permanent stop is a worse outcome under arch-14's own doctrine.

---

## OB-11 — concession correctly recorded

The withdrawal is accurate and correctly scoped: `spawn.rs:121` is inside `#[cfg(windows)]`,
`:198-210` is `Err(Unsupported)`, and the launcher's *own* `--config` parse (`main.rs:27-42`)
and `resolve_main_exe` from `current_exe()` (`:56-62`) genuinely are platform-free — only the
spawn hand-off is gated. The new G1 dependency line on C's Linux `spawn_main` carrying
`--config`, with the fail-closed consequence spelled out, is exactly what was missing.

---

## OB-12 — ACCEPTED, with one precision

**[C]** `dpkg-shlibdeps` is present at `/usr/bin/dpkg-shlibdeps` from `dpkg-dev` 1.22.6, i.e.
on any host that already runs `dpkg-deb`. The split is right: libraries derived from the
built ELFs, `cage` and the four GStreamer plugin packages hand-written because no ELF scan
can see a runtime plugin package and because they are parent §3.4 verbatim. Withdrawing the
t64 residual is correct — it no longer exists.

**Precision for the F dependency to be actionable:** `${shlibs:Depends}` is a substvar
consumed by `dpkg-gencontrol`, not by `dpkg-deb`. F §3's current line is *"`.deb` assembly …
(dpkg-deb)"*. The declared addition to F is therefore
`dpkg-shlibdeps` **+ `dpkg-gencontrol`** before `dpkg-deb -b`, not `dpkg-shlibdeps` alone; a
raw `dpkg-deb` on a hand-written `DEBIAN/control` will emit the literal string
`${shlibs:Depends}` into the package. Name both tools in the dependency on F, or the
substitution silently does not happen.

---

## New objections

None standalone. Three acceptance conditions are folded into the objections above and are
restated here so nothing is lost at integration:

1. **OB-2** — add `test -x /usr/lib/kiosk/kiosk-launcher` (and `kiosk-main`) to the container
   test; `systemd-analyze verify` stops checking the launcher path once cage is the
   `ExecStart` command. **[C]** rc=0 with the launcher absent.
2. **OB-4** — bound the "uploaded retroactively" label by the spool's retention window.
3. **OB-12** — the F dependency is `dpkg-shlibdeps` **and** `dpkg-gencontrol`, not
   `dpkg-shlibdeps` alone.

Plus one consistency item: G15's assertion list must now read `kiosk.ini` **absent** on a
fresh install (OB-5 changed it from a shipped conffile to an operator-written file).

---

## Still open

**OB-3 only.** Everything else is discharged.
