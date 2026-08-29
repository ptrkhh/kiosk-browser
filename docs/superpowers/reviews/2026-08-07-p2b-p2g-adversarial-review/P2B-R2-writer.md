# P2-B — WRITER, Round 2

No frame dispute. Every claim below was re-verified in-session this round; where I cite a line
I read it this turn.

**Dispositions: REBUT 0 · REVISE 7 (OB-2…OB-7, OB-9) · CONCEDE 2 (OB-1, OB-8).**
I win nothing this round. OB-1 is correct and it takes B1's enforcement half with it.

---

## OB-1 — CONCEDE

**`WebKitWebResource` has no `send-request` signal. B1's cancel half does not exist.**

Re-confirmed in writing, tier 4, read this turn:

```
$R/webkit2gtk-2.0.2/src/auto/web_resource.rs — every signal literal in the file:
  :137  b"failed\0"                   :224  b"received-data\0"
  :175  b"failed-with-tls-errors\0"   :258  b"sent-request\0"
  :197  b"finished\0"                 :281  b"notify::response\0"
                                      :304  b"notify::uri\0"
```

`sent-request` is past tense and its trampoline returns `()` — `web_resource.rs:233-266`,
`fn connect_sent_request<F: Fn(&Self, &URIRequest, &URIResponse) + 'static>`, body ends
`f(…)` with no return value. `(request, redirected_response)` is exactly the argument shape I
attributed to `send-request`; I misread a notification signal as a decision signal.

`grep -rn "send.request\|send_request"` over **both** `webkit2gtk-2.0.2/` and
`webkit2gtk-sys-2.0.2/` → **zero hits** (exit 1). No `web_page.rs` in `src/auto/` (0 matches);
`WebKitWebPage` appears 0 times in `webkit2gtk-sys-2.0.2/src/lib.rs`. So the gboolean
`send-request` is a `WebKitWebPage` (web-process-extension) signal and is unreachable from this
process through these crates, exactly as stated.

I also accept the gir-completeness premise, having checked the control myself:
`download.rs` binds `created-destination` `:188`, `decide-destination` `:221`, `failed` `:247`,
`finished` `:269`, `received-data` `:295` plus four `notify::` — a complete signal set. My R1
inference "unbound ≠ unreachable" was sound *as a general principle about `connect_local`* and
worthless here, because the signal is absent from the type, not merely from the bindings.

**Cost, stated plainly.** B1 as designed leaves SEC-10 on Linux as a `config.warn` plus a log
line — fail-open on a security gate, C5. The boot probe I offered as the pin returns `None` on
every device, so the design degrades to observe-only universally, not in a tail case. Four of
five sub-assertions of my own scenario 8 are unreachable. My R1 Q2/Q3 argument for the
restructure rested entirely on the cancel working; with it gone, R1's B1 is worse than the B2 I
withdrew, on the only axis that matters.

---

## The consequence — what enforces SEC-10 on Linux

**Decision: reinstate B2 as the enforcement authority, scoped to host+scheme+port. Keep
`resource-load-started` as observe-only. Both, not either.**

I take the Critic's "conceded too far" finding. My §A "not expressible" was over-generalised and
I withdraw it in that form. But the correct scoping is narrower than "sound except for paths",
so here is the analysis I should have done in R1 rather than the sweeping one I did.

**Soundness requirement.** Layer 1 is block-all + `ignore-previous-rules` for allowed, so the
ignore-rules define the allow set. The filter is sound iff

> AllowSet(regex) ⊆ AllowSet(URLPattern)

Over-blocking is an availability cost; only a false **allow** is a SEC-10 hole.

**Host dimension — sound, with a delimiter-terminated anchor.** Compiling
`https://app.example.com/*` to `^https://app\.example\.com(:[0-9]+)?/` rather than a bare
prefix:

| Battery case (`allowlist.rs`) | Allowlist | Regex | Direction |
|---|---|---|---|
| `…com@evil.com/` `:197` | BLOCK | BLOCK (`@` ≠ `/`) | exact |
| `…com:pw@evil.com/` `:199` | BLOCK | BLOCK (`:pw` ∉ `[0-9]+`, then `≠/`) | exact |
| `…com\@evil.com` `:502-507` | ALLOW | BLOCK | over-block |
| tab / `%2e` / U+3002 `:517-528` | ALLOW | BLOCK | over-block |
| punycode↔unicode pair `:286-301` | ALLOW | BLOCK unless both forms emitted | over-block |
| Cyrillic homoglyph `:313-323` | BLOCK | BLOCK | exact |
| trailing dot `:567-577` | BLOCK | BLOCK | exact |

The Critic is right: every normalisation divergence lands in the over-block direction. My R1
headline example (`\@`) is a *false block*, not a false allow, and I presented it as if it cut
the other way. Conceded.

**And the tier-5 unknown does not block the decision.** I do not know from any tier 1–4 artifact
whether WebKit matches `url-filter` against the raw markup URL or the canonicalised request URL.
It does not matter: on the raw string these cases over-block (table above); on the canonical
string they do not arise at all, because the canonicaliser has already folded them. Both answers
sit on the safe side of the soundness requirement. That converts the unknown from load-bearing
to informational, which is why I am willing to reinstate B2 on it.

**Path/query dimension — genuinely unsound, so it is not compiled.** Verified false-allow:
pattern `https://app.example.com/kiosk/*`, URL `…/kiosk/%2e%2e/%2e%2e/etc/passwd` — `url::Url`
resolves the path to `/etc/passwd` and the allowlist BLOCKS (`allowlist.rs:628-642`), while a
raw-string prefix regex matches and ALLOWS. My own probe this turn shows two more path features
a prefix regex cannot honour: `https://a.test/{x,y}/*` compiles with pathname `/x,y/*` (the
group became a literal), and an explicit search constraint is pinned as significant at
`allowlist.rs:360-368`.

**So the compiler emits host+scheme+port only, deliberately**, and B2 carries one declared
divergence, in the direction C3 requires me to name:

> **Divergence (looser than Windows).** On Linux, Layer 1 enforces the allowlist at
> **host+scheme+port** granularity for subresources. A subresource fetch to an off-pattern
> *path* on an *already-allowlisted host* is permitted on Linux and blocked on Windows
> (`resource_allowed` → full URLPattern, `nav_policy.rs:131-137`). Main-frame and sub-frame
> **navigations** are unaffected — P2-A's nav guard runs the full URLPattern on every frame.

I claim this is the right cut, not merely the convenient one: SEC-10's stated purpose is
"Closes CSS/JS exfiltration … that never triggers a navigation" (parent §7:700, and
`egress.rs:1-14` names the same threat). Exfiltration is a question about *which host* a page may
talk to. Path scoping on a host the operator already trusts is a least-privilege refinement, not
the exfil boundary. I expect this to be contested and it should be.

**Fail-closed on inexpressible patterns.** Any host component containing `:`, `{`, `(`, or a
modifier the compiler does not handle → **no ignore-rule is emitted**, plus
`config.warn("egress.filter_pattern", pattern)`. Emitting nothing = block = the safe direction.
From my probe, `https://:sub.example.com/*` compiles to host `:sub.example.com` — a live operator
input that hits this branch. Note `https://(.*)\.example\.com/*` normalises to host
`*.example.com`, which *is* expressible.

**Observability — `resource-load-started` keeps a job.** `connect_resource_load_started`
(`web_view.rs:2523`, ungated) fires per resource with `&WebResource`; connect
`WebResource::connect_failed` (`web_resource.rs:118`) on it and, when `!resource_allowed(uri)`,
emit `telem.nav_blocked(REASON_EGRESS, uri)`. Whether a content-blocked load reaches the signal
at all is runtime — **pinned by scenario 8(b), not asserted**. If it does not, host-scoped blocks
are silent on Linux, which is the divergence the original draft declared and which I wrongly
withdrew in R1. It is back.

**Options costed and rejected**, so the option space is on the record rather than assumed:
- *Web-process extension* (`WebKitWebPage::send-request`): the only route to a cancel-capable,
  full-fidelity request hook. A new cdylib, a new IPC hop for the live allowlist, and wry already
  sets the extensions directory (`$R/wry-0.55.1/src/webkitgtk/mod.rs:283` →
  `set_web_extensions_directory`) under the `linux-body` feature tauri-runtime-wry enables. Large
  and unbudgeted; if the Moderator wants full path-fidelity on Linux, this is what it costs, and
  it is a sub-project, not a section.
- *In-process allowlisting proxy* via `WebsiteDataManager::set_network_proxy_settings`
  (`website_data_manager.rs:589`, verified bound). Rejected on Q2: for HTTPS a proxy sees only the
  CONNECT target, i.e. **the same host granularity the content filter already gives**, for an
  entire new HTTP/CONNECT component.
- *Declaring SEC-10 undischargeable.* Rejected, and I state the cost the Moderator asked for:
  P2-A:42's "**Residual risk: do not field a Linux device before P2-B**" would survive P2-B, and
  P2-B's Status paragraph claiming to close it would have to be deleted. P2-B would then discharge
  no part of its headline requirement. Host-level enforcement with a named path divergence is
  strictly better than that.

B2's costs stand and are written into the spec, not glossed: contained `unsafe` sys-FFI shim, a
new direct `webkit2gtk-sys` dependency with `ffi/v2_24`, async save, filter-id lifecycle across
`ConfigApplied`. C6 justification is that the alternatives above are larger and the do-nothing
option fails C5.

---

## OB-2 — REVISE

**Probe re-run this turn**, same path as `Allowlist::compile` (`process_construct_pattern_input`
+ `UrlPattern::parse`, `allowlist.rs:119-122`), against `urlpattern` 0.3:

```
OK   https://app.example.com/*       proto="https" host="app.example.com"     port=""     path="/*"
OK   https://*.example.com/*         proto="https" host="*.example.com"       port=""     path="/*"
OK   https://api-*.example.com/*     proto="https" host="api-*.example.com"   port=""     path="/*"
OK   *://example.com/*               proto="*"     host="example.com"         port=""     path="/*"
OK   https://:sub.example.com/*      proto="https" host=":sub.example.com"    port=""     path="/*"
OK   https://app.example.com:8443/*  proto="https" host="app.example.com"     port="8443" path="/*"
OK   https://a.test/{x,y}/*          proto="https" host="a.test"              port=""     path="/x,y/*"
OK   https://(.*)\.example\.com/*    proto="https" host="*.example.com"       port=""     path="/*"
```

Confirmed: the accessors return `pattern_string`, all of these are live allowlist entries, and
`api-*.example.com`, `*`, and `:sub.example.com` are not valid CSP source expressions. An invalid
source is dropped and the rest of the list applies ⇒ **the belt would be tighter than the
authority, silently** — verbatim the bug `nav_policy.rs:169-184` refused to ship. **I withdraw
"looser by construction."** I also concede the second, independent half without reservation: a
`default-src` with no `'unsafe-inline'` blocks inline script, inline handlers and `eval` on
allowlisted pages, which `resource_allowed` does not restrict at all. My R1 text addressed only
origins and `data:`/`blob:` and never mentioned the inline dimension.

**Replacement mechanism.** The derivation returns `Option<String>`, and the whole property is
carried by one branch:

1. **Expressibility gate.** If **any** allowlist pattern is not CSP-expressible — mid-label host
   wildcard, non-literal scheme, named/regex group in host, non-numeric port — return `None`:
   **inject no CSP at all**, plus `config.warn("egress.csp_skipped", pattern)`. Absent ⇒ blocks
   nothing ⇒ trivially never tighter. One branch, loud, no partial policies.
2. **Origin sources** otherwise: allowlist origins ∪ {content, app, asset origins} ∪ `data:`,
   `blob:`.
3. **Non-origin dimensions opened**, because the authority does not restrict them:
   `'unsafe-inline'` and `'unsafe-eval'` on `script-src`/`style-src`.
4. **Three restrictions kept and reclassified**: `object-src 'none'; base-uri 'none';
   frame-ancestors 'none'`. These *are* tighter than the allowlist. They are not derivation
   output — they are a deliberate hardening decision, declared in the spec's divergence list under
   C3. Not silent, which is the whole of D2b's complaint.

**What B3 is for, stated honestly.** With Layer 1 healthy the belt restricts almost nothing Layer 1
does not. It earns its place in exactly one state — Layer 1's compile or save failed and
degraded to `config.warn` (B's own best-effort path), where the belt is the only origin control
left — plus Q1: the parent names it in the same clause as the filter (§7:700). If the Moderator
judges Q2 to outrank Q1 here, B3 is the change to cut, and I would not fight hard for it.

---

## OB-3 — REVISE

Panic sites re-verified this turn: `$R/glib-0.18.5/src/object.rs` carries three `panic!` and one
`assert!` in the `connect_local` return-value marshalling path (~`:2580`, `:2590`, `:2605`,
`:2613`). The objection is right that this is a panic inside signal emission, not a `Result`.

Under the OB-1 concession the `connect_local` call is deleted along with the cancel design, so the
surface is gone. But the near-miss trap is real and survives the deletion: `sent-request` exists,
has a `UNIT` return, and is one character from what my R1 text called the mechanism throughout.
An implementer "fixing" the probe by correcting the spelling gets a successful connection and then
a panic on the first subresource of the first page — a crash-restart loop under the launcher, not
a degraded control.

**Replacement:** the spec states, in the `egress.rs` section, "**`WebKitWebResource` exposes no
cancel-capable signal. `sent-request` (`web_resource.rs:233`) is a void-return notification — it
cannot cancel, and must not be connected in an attempt to.**" Plus a blanket rule for P2-B: **no
dynamic signal connection (`connect_local` / `connect_closure`) anywhere** — every signal P2-B
uses is a typed, generated binding. That is a stronger and cheaper guarantee than probing.

I accept the two sub-claims the Critic explicitly did not contest; they are moot now.

---

## OB-4 — REVISE

Accepted: after I dropped the violation listener, no B12 assertion could fail if the belt shipped
inert — 8(b)'s "in-pattern path loads" passes either way, and 9–12 never touch CSP. C9 says the
declared gate must gate the change; it did not.

**Replacement, in two parts.**

1. **A falsifiable assertion, with no product code.** New scenario **8(d)**: the harness starts
   kiosk-main with the content filter deliberately not installed (a harness-only flag), loads a
   fixture page from the allowlisted httpd, and asserts an off-list `fetch()` is **still blocked**.
   The fixture page carries its own `securitypolicyviolation` listener writing to a DOM node the
   harness reads — the assertion lives in the fixture, not in the product, so no listener and no
   `#[cfg(not(windows))]` Tauri command come back. 8(d) fails if the belt is inert. This is also
   the scenario that exercises the one state where B3 does real work (OB-2 §"what B3 is for"), so
   the gate and the justification are the same thing.
2. **Q5: the feasibility question stops being a plan-time item.** I concede the Critic's reading of
   Q5 — "whether the mechanism works at all" cannot sit in Open Decisions. Decided here: the belt
   is a document-start `UserScript` that appends the `<meta http-equiv>` to `document.head`,
   creating `head` if absent, and re-checking on `readystatechange`. I mark the Critic's
   head-child point as plausible and tier-5, and I am not resolving it by argument: **8(d) is what
   settles which idiom takes effect**, and it is blocking. The two Open Decisions entries
   ("Meta-CSP injection timing", "whether a swapped user script applies to the current document")
   are deleted from the spec and replaced by 8(d) plus a config-apply-then-navigate ordering note.

---

## OB-5 — REVISE

Verified this turn, and the ordering is exactly as reported: `tokio::spawn(health::run(…))` at
`crates/kiosk-main/src/main.rs:923`; `if display.keep_awake` at `main.rs:957` — health first.
`health::run` builds `interval(Duration::from_secs(period_s.clamp(10,3600)))`
(`health.rs:34`) with `MissedTickBehavior::Delay`, and tokio's first tick completes immediately.
So "`try_wait()` on the first health sample" inspects a child that does not exist yet. The warn
never fires — on every boot, including the container I wrote the scenario for. And the one-shot
check would miss a later death regardless.

**Replacement — drop the sampler from this entirely.** After spawning the inhibitor, take the pipe
handle out of the child and keep *it* alive, then hand the `Child` to a dedicated thread that
blocks in `wait()`:

```rust
let mut child = Command::new("systemd-inhibit").args([...]).stdin(Stdio::piped()).spawn()?;
let _inhibit_pipe = child.stdin.take();          // THIS is what holds the inhibitor open
std::thread::spawn(move || {                      // Telemetry is Send + Clone (P2-A:77-78)
    let status = child.wait();
    telem.config_warn("display.keep_awake", &format!("inhibitor exited: {status:?}"));
});
```

Why this shape and not a polled check: `Child::wait` closes stdin before waiting, which would kill
the very inhibitor we are holding — taking the pipe first is load-bearing, not stylistic. The
thread reports at the instant of death rather than up to `health_sample_s` later, catches a death
at *any* time rather than one-shot, needs no reordering of `main.rs`, and adds no argument to a
function already carrying `#[allow(clippy::too_many_arguments)]` (`health.rs:23`). `config.warn`
replaces R1's `eprintln`, which is invisible under systemd. Spawn failure keeps its own `Err` arm.

Not contested and gratefully noted: the reaping analysis and the PF-07/Q1 traceability check.

---

## OB-6 — REVISE

Verified: `user_media_permission_request.rs` exposes exactly `is_for_audio_device` `:37` and
`is_for_video_device` `:44` (plus their two notifies) — no display-capture predicate. So
`(audio=false, video=false)` is reachable from the bindings alone, it downcasts to
`UserMediaPermissionRequest` successfully, and it never reaches `_ => Other`. My R1 defence of
omitting `InstallMissingMediaPlugins` was "the catch-all covers it" — here the catch-all does not,
and the objection is right that this is the one classifier in B deciding a security default.

**Replacement — the pure classifier takes the tuple, so the arm is host-testable:**

```rust
fn classify_user_media(audio: bool, video: bool) -> Verdict {
    match (audio, video) {
        (false, false) => Verdict::Deny,                          // display/screen capture, unknown
        (true,  false) => Verdict::Kind(PermissionKind::Microphone),
        (false, true ) => Verdict::Kind(PermissionKind::Camera),
        (true,  true ) => Verdict::Both,   // require camera && microphone
    }
}
```

Two decisions I am making explicitly rather than leaving to code time: `(false,false)` denies
unconditionally — it is not `Camera`, and a kiosk with `camera=true` for a video-call page must not
thereby grant screen capture; and `(true,true)` requires **both** `permissions.camera` and
`permissions.microphone`, because `PermissionKind` is one-of and silently picking either is a
fail-open. Host test covers all four arms.

---

## OB-7 — REVISE

Accepted: B1 declared one assumption and inherited several by silence. Under the reinstated B2 the
question moves — request-class coverage is now a property of the **content blocker**, not of
`resource-load-started` — but it is still a runtime property no tier 1–4 artifact settles, so it
gets declared and pinned rather than assumed.

**Declared assumption:** WebKit's content-rule engine applies to all four request classes the
parent enumerates. **Pinning:** scenario 8(a) is extended from `<img>` + `fetch()` to all four
named in `egress.rs:1-14` and parent §7:700 — `<img src>`, CSS `url()`, `fetch()`, and
`navigator.sendBeacon` — each asserted individually, all blocking. Two of the four shipped
unverified under R1's B12; that was my own "pinned by smoke, not assumed" standard applied to half
the surface.

**Reconciliation with P2-A, written into the spec** rather than left silent: P2-A's recorded
assumption (P2-A:227-232) is that `load-changed`/`load-failed` are `WebKitWebView`-level signals
tracking the **main frame's** load only. `resource-load-started` is a different signal with a
different subject — its parameters are `&WebResource, &URIRequest` (`web_view.rs:2523`), i.e. per
resource, not per frame load — so nothing about A's assumption transfers either way, and P2-B does
not weaken it. Blast radius is bounded by the role change: `resource-load-started` is now
**observe-only**, so if it turns out to be main-frame-scoped, the cost is missing telemetry, not
missing enforcement. Enforcement is the filter.

---

## OB-8 — CONCEDE

Verified: parent §12 line 930 records OD-8 as **applied**, not deferred — "*block by default,
`content.pdf_view=true` opt-in (applied)*", with the §7 PDF row (`:699`) naming the per-platform
interceptors to confirm. The parent nowhere defers M4. My B8 text asserted "a recorded ponytail
with an owner" and named none; the record it points at (`scheme_guard.rs:161-175`) ends "left for a
follow-up once one of those is confirmed available" and is equally ownerless.

I accept the substance of the objection *and* the Critic's acceptance of B8's technical position —
parity is C3-correct, wiring Linux alone would be an undeclared stricter divergence, and
`ResponsePolicyDecision` is fenced by P2-A:71-74. The defect is ownership, and I will not
manufacture an owner I have no authority to assign.

**Revision:** the false claim is struck. B8's text becomes: "*M4 is undischarged on **both**
platforms. P2-B does not close it and asserts no owner. This is an integration defect against P2 as
a whole, not a P2-B design decision, and it needs an owner assigned outside this spec.*" I ask the
Moderator to record it as an unowned P2-row item at the severity the frame assigns (§2: a P2-row
item with no identifiable owner is a HIGH integration defect). Cost of leaving it: a parent
requirement with an applied decision ships unimplemented on every platform, silently.

---

## OB-9 — REVISE

Accepted. Scenario 12(a) "spawn succeeds" and (b) "child exits non-zero" are properties of the
bus-less container and hold identically if the keep-awake block is deleted. They are preconditions
and the scenario will say so.

Under the OB-5 revision the scenario has a real assertion: **exactly one
`config.warn{display.keep_awake}` carrying the child's exit status is spooled, emitted by the
watcher thread** — which fails if the thread is missing, if the pipe was not taken before `wait()`,
or if the warn is wired to `eprintln`. (d) stays as a labelled non-regression check.

---

## Updated register — post-round state

| ID | Change | State after R2 | Dependencies moved |
|---|---|---|---|
| B1 | `resource-load-started` | **DEMOTED to observe-only.** Cancel half deleted (OB-1). Telemetry via per-resource `connect_failed`; whether blocked loads reach the signal is smoke-pinned (8b) | No longer discharges SEC-10 alone; now depends on **B2** for enforcement. `connect_local` dependency deleted |
| B2 | Content filter, sys-FFI shim | **REINSTATED as the SEC-10 enforcement authority**, compiler scoped to host+scheme+port; inexpressible patterns fail closed and loud | Regains: `webkit2gtk-sys` direct dep + `ffi/v2_24`. New declared divergence (looser than Windows on subresource paths) |
| B3 | CSP belt | **REVISED.** "Looser by construction" withdrawn; `Option<String>` with an expressibility gate (`None` ⇒ no CSP + `config.warn`); `'unsafe-inline'`/`'unsafe-eval'` retained; three non-origin restrictions reclassified as declared hardening | Gains a real gate (8d). Q2-marginal — flagged as the change to cut if the Moderator ranks Q2 over Q1 |
| B4 | hardening mapping | Clean pass, unchanged | — |
| B5 | permission classifier | **REVISED** — `classify_user_media(audio, video)` with `(false,false) ⇒ deny` and `(true,true) ⇒ camera && microphone` | — |
| B6 | clipboard-read divergence | Clean pass, unchanged | — |
| B7 | downloads via `on_download` | Clean pass, unchanged | — |
| B8 | PDF parity | **Ownership claim struck.** Technically unchanged; escalated as an unowned P2-row item | Owner: **none — escalated to the Moderator** |
| B9 | keep-awake | **REVISED** — pipe taken before `wait()`, blocking watcher thread, `config.warn` not `eprintln`; sampler route abandoned | No longer depends on `health::run`; no `main.rs` reordering |
| B10 | feature/floor accounting | Clean pass — **but the floor must be re-derived.** The 2.32 floor rested on `remove_script` i.e. on B3; B2's reinstatement puts `remove_filter_by_id` (`v2_26`) back in and B3 may change. Floor still 2.32 while B3 ships; **P2-A:71-74 is not finally discharged until B3's final shape is fixed** | Now depends on B2 **and** B3 |
| B11 | A-filter re-derivation | Clean pass. The added `send-request` claim is **deleted** with OB-1 | — |
| B12 | smoke 8–12 | **REVISED.** 8(a) → all four SEC-10 request classes; 8(b) pins block-observability; 8(d) new, gates the belt with filter-disabled; scenario 12 preconditions relabelled, real assertion added | Depends on a new harness flag (disable filter) for 8(d) |

**Withdrawn:** B1's cancel half, the `connect_local` route, the `SignalId::lookup` probe, the
"looser by construction" property, B8's ownership claim, and two Open Decisions entries (CSP
injection timing / user-script currency — replaced by gate 8d).
**Reinstated:** B2, as the enforcement authority.
