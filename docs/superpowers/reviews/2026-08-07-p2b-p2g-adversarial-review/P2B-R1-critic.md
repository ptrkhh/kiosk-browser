# P2-B — CRITIC, Round 1

No frame dispute.

I verified every mechanically-checkable claim I argue below. Where the Writer conceded, I
attack only the replacement.

## Objection index

| ID | Change | Objection (one line) | Sev | Evidence tier |
|---|---|---|---|---|
| OB-1 | B1 | `send-request` is not a signal on `WebKitWebResource` at all (it is `sent-request`, void); the cancel half is unreachable, the boot probe returns `None` on every device, and SEC-10 lands fail-open | HIGH | 4 (verified) |
| OB-2 | B3 | `Allowlist::origins()` cannot be "looser by construction": two verified allowlist patterns yield **invalid** CSP source expressions (dropped ⇒ tighter), and CSP restricts dimensions the allowlist does not (inline script) | HIGH | 3+4 (verified, incl. executed probe) |
| OB-3 | B1 | Signature mismatch on `connect_local` is a **panic inside signal emission**, not a clean error — and the near-miss name `sent-request` exists and would hit exactly that path | MED | 4 (verified) |
| OB-4 | B3 / B12 | After the violation listener was dropped, no scenario can fail if the CSP belt never takes effect; the injection idiom is still "resolve at plan time" (frame Q5 forbids that for *whether the mechanism works*) | MED | 3 (Writer's text + B12) |
| OB-5 | B9 / B12 | `try_wait()` "on the first health sample" runs before/at the instant the child is spawned (`main.rs:923` vs `:957`, tokio's first tick is immediate) ⇒ the warn never fires; and the check is one-shot | MED | 3+4 (verified) |
| OB-6 | B5 | A `UserMediaPermissionRequest` with neither audio nor video (display/screen capture) downcasts successfully and therefore never reaches `_ => Other`; the stated mapping has no arm for it | MED | 4 (verified) |
| OB-7 | B1 | `resource-load-started`'s coverage over the four request classes SEC-10 names is undeclared and unpinned; B12 8(a) covers `<img>`+`fetch` only, and B never reconciles with P2-A's recorded "WebView-level signals are main-frame-only" assumption | MED | 1+2+3 |
| OB-8 | B8 | The deferral names no owner (frame §4.5); the parent never defers M4 | MED | 1+3 |
| OB-9 | B12 | Scenario 12(a)/(b) assert environment properties, not controls — they cannot fail if the control is broken | LOW | 3 |

**Counts: HIGH 2, MED 6, LOW 1.**
**Clean passes: B2 (with a "conceded too far" finding, below), B4, B6, B7, B10, B11.**

---

## OB-1 — `WebKitWebResource` has no `send-request` signal; B1's cancel half does not exist (vs B1, HIGH)

**What breaks.** B1's entire enforcement claim. The Writer proposes cancelling by connecting
`WebKitWebResource::send-request` via `connect_local` and returning `Some(true.to_value())`.
That signal does not exist on `WebKitWebResource`. The signal with that exact parameter list
is **`sent-request`** — past tense, notification-only, **void return**:

```
$R/webkit2gtk-2.0.2/src/auto/web_resource.rs:234
  fn connect_sent_request<F: Fn(&Self, &URIRequest, &URIResponse) + 'static>(…)
  … b"sent-request\0".as_ptr()          (trampoline returns `()`)
```

`(request, redirected_response)` is precisely the argument shape the Writer attributes to
`send-request`. The gboolean-returning `send-request` is a **`WebKitWebPage`** signal — the
*web-process extension* API, which is a different library loaded into the web process, and
which webkit2gtk-2.0.2 does not bind at all (`src/auto/` has no `web_page.rs`; `ls` verified).

The Writer's inference "unbound ≠ unreachable" needs the premise that the bindings omit
signals. They do not — these are gir-generated and enumerate the complete signal set of every
type they cover. Two checks:
- `web_resource.rs` binds exactly `failed` `:118`, `failed-with-tls-errors` `:149`,
  `finished` `:185`, `received-data` `:208`, `sent-request` `:234`, plus `notify::response`
  `:268` and `notify::uri` `:291` — i.e. `WebKitWebResource`'s full documented signal set.
- Control: `download.rs` binds `created-destination` `:168`, `decide-destination` `:197`,
  `failed` `:230`, `finished` `:256`, `received-data` `:278` — `WebKitDownload`'s complete
  set; `web_view.rs` has 43 `connect_*`. The generator is not dropping signals.
- `grep -rn "send-request\|send_request"` over `webkit2gtk-2.0.2` **and**
  `webkit2gtk-sys-2.0.2`: **zero hits.**

**When.** At boot, on the first device. `SignalId::lookup("send-request",
WebResource::static_type())` returns `None`. Per B1's own fallback that means: do not connect,
`config.warn("egress.cancel_unavailable")`, **observe-only**. Every device, every boot — not a
tail risk. Note also that a `g_signal_lookup` on a class whose `class_init` has not run
returns 0 regardless, so the probe cannot even distinguish "absent" from "not yet loaded" at
boot; but that distinction is moot here because the signal is genuinely absent.

**Why it matters.**
- **C5, fail-closed on security gates.** B1 as designed converts SEC-10 on Linux into a
  `config.warn` and a log line. The spec's Status paragraph — "Closes the P2-A residual: after
  B, a Linux device no longer has the 'do not field before P2-B' egress hole" — is then false,
  and P2-A:42's "**Residual risk: do not field a Linux device before P2-B**" survives P2-B.
- **C9.** B12 scenario 8 (a), (b), (d), (e) are all cancel-dependent and cannot pass. 8(d) is
  the assumption's own pin and it is designed to fail. Four of five sub-assertions of the
  spec's headline gate are unreachable.
- The Writer's Q2/Q3 argument for the restructure ("six moving parts to zero", "the silent
  divergence disappears") rests on the cancel working. With cancel gone, B1 is strictly
  *worse* than the withdrawn B2 on the only axis that matters: B2 blocked, B1 logs.

**Evidence.** Tier 4, all read by me this turn:
`$R/webkit2gtk-2.0.2/src/auto/web_resource.rs:118,149,185,208,234,268,291`;
`$R/webkit2gtk-2.0.2/src/auto/download.rs:168,197,230,256,278`;
`ls $R/webkit2gtk-2.0.2/src/auto/` (no `web_page.rs`);
grep for `send-request` across both crates → empty. Tier 2: P2-A:42.

**What survives.** Only two routes to an actual WebKitGTK cancel remain, and B must pick one
and cost it rather than assume a third:
(i) `WebKitUserContentFilter` — the withdrawn B2 (see "Conceded too far");
(ii) a web-process extension exposing `WebKitWebPage::send-request` — reachable in principle
(`web_context.rs:709 set_web_extensions_directory`, `:721
set_web_extensions_initialization_user_data`, both bound and ungated) but it is a new cdylib,
a new IPC hop for the live allowlist, and wry already claims that directory
(`$R/wry-0.55.1/src/webkitgtk/mod.rs:283`) under the `linux-body` feature that
tauri-runtime-wry enables. That is a large, unbudgeted change, not a footnote.

---

## OB-2 — B3's "looser by construction" is false; the belt can be *tighter* than the allowlist (vs B3, HIGH)

**What breaks.** The single property B3 rests on. The Writer: "a wildcard pattern maps to a
wildcard source and the loss stays in the **permissive** direction by construction", citing
`UrlPattern::protocol()/hostname()/port()`.

I checked what those accessors actually return: the component's **`pattern_string`**
(`$R/urlpattern-0.3.0/src/lib.rs:431,446,451` → `&self.hostname.pattern_string` etc.), i.e.
URLPattern syntax, not a hostname. I then compiled real allowlist entries through
`Allowlist::compile`'s exact path (`process_construct_pattern_input` + `UrlPattern::parse`,
`allowlist.rs:119-122`) and printed the components. Executed output:

```
OK   https://*.example.com/*        proto="https" host="*.example.com"     port=""
OK   https://api-*.example.com/*    proto="https" host="api-*.example.com" port=""
OK   *://example.com/*              proto="*"     host="example.com"       port=""
OK   https://:sub.example.com/*     proto="https" host=":sub.example.com"  port=""
```

All four **compile**, so all four are live allowlist entries an operator can write today.
Two of them produce CSP source expressions that are not valid CSP:
- `https://api-*.example.com` — CSP `host-part` permits a wildcard only as the whole host or
  as a leading `*.` label; a mid-label wildcard is a parse error.
- `*://example.com` — `scheme-part` must be a scheme; `*` is not.
- `https://:sub.example.com` — parses as scheme + empty host + port `sub.example.com`.

An unparseable source expression is **ignored** by the CSP parser; the rest of the list still
applies. So the host is *absent* from the belt while being *present* in the allowlist ⇒ the
belt is **tighter than the authority**, silently. That is verbatim the bug D2b refused to
ship: *"would make a legitimately-allowlisted subresource pass the native filter and then get
silently blocked by this CSP — a real deployment breaking with no clear signal"*
(`nav_policy.rs:169-184`). B3 reintroduces it in a new direction, which is exactly what B3
claimed to have eliminated.

**Second, independent failure of the same property.** CSP constrains dimensions an origin
allowlist does not, so no origin-only derivation can be "looser" in general. The reference
policy B3 inherits is `csp_policy` at `nav_policy.rs:186-197`: `default-src <origins>` with
**no `'unsafe-inline'`** (only `style-src` gets it, `:190`), plus `object-src 'none';
base-uri 'none'; frame-ancestors 'none'`. Shipping that against arbitrary operator content
blocks every inline `<script>`, every inline event handler and every `eval` on an
*allowlisted* page — none of which `resource_allowed` restricts at all. B3's turn text
addresses only origins and `data:`/`blob:`; the inline-script dimension is not mentioned
anywhere in the turn block or the spec.

**When.** First operator config using a mid-label wildcard, a wildcard scheme, or a page with
an inline script — i.e. routinely.

**Why it matters.** Silent breakage of allowlisted content is a named defect class in this
project (frame Q3), and the property being falsified is the *only* justification B3 offers
for reversing D2b's decision.

**Evidence.** Tier 4 executed: `urlpattern-0.3.0` compile+component probe (run this turn,
output above). Tier 4 read: `$R/urlpattern-0.3.0/src/lib.rs:431,446,451` (`pattern_string`).
Tier 3: `crates/kiosk-core/src/nav/allowlist.rs:119-122`;
`crates/kiosk-main/src/nav_policy.rs:169-184` and `:186-197`.

---

## OB-3 — Signature mismatch on `connect_local` is a panic in the GTK main loop, not a clean error (vs B1, MED)

**What breaks.** The Writer's risk framing. He pins only *existence* (`SignalId::lookup`) and
treats the rest as settled. The failure mode of getting the signature wrong is not a
`Result`:

```
$R/glib-0.18.5/src/object.rs:2576-2586   // return_type == Type::UNIT
  panic!("Signal '{signal_name}' of type '{type_}' required no return value but got value of type '{}'")
$R/glib-0.18.5/src/object.rs:2589-2608   // non-unit return
  panic!(… required return value of type '{}' but got None)   // and a type-coercion panic
$R/glib-0.18.5/src/object.rs:2613-2616
  assert!(type_.is_a(signal_query_type), …)
```

**When.** Concretely reachable here: the correctly-spelled neighbour `sent-request` **does**
exist (OB-1), and its return type is `UNIT`. An implementer who "fixes" the probe by
correcting the name — the single most likely repair given the Writer's own text calls it
"send-request" throughout — gets a connection that succeeds, then **panics inside the signal
emission on the first subresource of the first page**. In a kiosk under the launcher that is a
crash-restart loop, not a degraded control.

**Why it matters.** The Writer's declared residual is "SEC-10 enforcement is CSP-only until B2
is built". The actual worst case one typo away is an availability failure of the whole kiosk.
Frame Q3/Q4: unbounded blast radius from a control that was sold as best-effort.

Two sub-claims of B1 I checked and **do not** contest: `connect_local`'s dynamic `&[Value]`
closure can receive `WebKitURIRequest`/`WebKitURIResponse` (they are GObjects, `Value`-carried),
and the non-`Send` closure is fine at this site — `connect_local` wraps it in a `ThreadGuard`
(`object.rs:2510-2521`) and P2-A's threading note puts the `with_webview` closure on the GTK
main thread. Neither rescues OB-1.

---

## OB-4 — The CSP belt now has no gate at all (vs B3 / B12, MED)

**What breaks.** Observability and gating of B3. The Writer drops the
`securitypolicyviolation` listener and the `#[cfg(not(windows))]` Tauri command ("their only
value was observability B1 now provides"). B12 then contains **no assertion that the belt is
in force**. Walk the scenarios: 8(a)/(b) assert loads are blocked/allowed — under OB-1 those
are B1's assertions and, if the belt silently no-ops, 8(b)'s "in-pattern path loads" still
*passes*. 9/10/11/12 do not touch CSP. So no B12 assertion can fail if B3 ships and does
nothing. C9: the declared gate does not gate this change.

**Compounding.** Whether the mechanism works at all is still an open plan-time item in the
spec: "*Meta-CSP injection timing at document-start (documentElement-append idiom)*" and
"*Whether a swapped user script applies to the current document or only subsequent loads*"
(spec "Open decisions to resolve at plan time"). The Writer's turn does not close either.
Frame Q5 is explicit: "resolve at plan time" is legitimate for values and shims, **not** for
whether the mechanism works at all — the latter must be pinned by a gate. A document-start
script has no `document.head` to append to (that is why the spec reaches for
"documentElement-append"), and an `http-equiv` meta that is not a child of `head` is not
processed as a policy — so the most likely outcome of the idiom the spec names is a belt that
is inert and unobservable. I flag the head-child rule as tier-5 (HTML/CSP processing model, no
tier 1–4 artifact on this box); the tier-3 part of the objection stands without it: **an
unpinned, unobservable security control with an open feasibility question is not adoptable.**

**Why it matters.** Q1 keeps B3 alive only because the parent names it. A belt that the merge
gate cannot distinguish from absent discharges the parent's clause on paper only.

---

## OB-5 — B9's liveness check runs before the child exists, and only once (vs B9 / B12, MED)

**What breaks.** The revision the Writer offered in place of the struck scenario 12. He
specifies "an explicit `try_wait()` check on the **first** health sample (P2's health sampler
already runs; no new timer)".

Verified ordering in the real binary: `tokio::spawn(health::run(…))` is at
`crates/kiosk-main/src/main.rs:923`; the keep-awake block is at `main.rs:949-966` (`if
display.keep_awake` at `:957`) — i.e. **after**. And `health::run` uses
`tokio::time::interval` (`health.rs:34`), whose **first tick completes immediately**
(`$R/tokio-1.52.3/src/time/interval.rs:426`, and `:10`). So the first health sample fires
before, or in the same instant as, the `systemd-inhibit` spawn. `try_wait()` at that moment
returns `Ok(None)` ("still running") — or has no child to inspect at all.

**When.** Every boot, on every host, including the smoke container the Writer built the
scenario for.

**Why it matters.** Scenario 12(c) — "exactly one `config.warn{display.keep_awake}` is
spooled" — is the only assertion in the revised scenario that can fail from a code defect
(OB-9), and it fails for a reason unrelated to the control. In the field the same wiring means
a dead inhibitor is never reported, which is the exact silent-failure mode the Writer conceded
FALSE #74 to fix. Secondarily, a *one-shot* check misses an inhibitor that dies later (logind
restart, session teardown); the same sampler ticks every `health_sample_s` (default 60,
`schema.rs` / `health.rs:18`) and checking every tick costs nothing.

**Not contested:** the reaping question. The `Child` is held for process life and never
dropped, so no zombie accumulates; on clean exit or crash the pipe EOFs and `cat` exits, and
`cat`'s own reparenting is init's problem. And PF-07 traceability (Q1) holds — P2-G does own
the primary: `IdleAction=ignore`, masked sleep targets, `consoleblank=0`, no idle daemon, and
hardware row H3 (`p2g…design.md:64-67, 94`). B9 is correctly scoped as the secondary.

---

## OB-6 — B5's classifier has an undefined arm for non-audio, non-video media requests (vs B5, MED)

**What breaks.** The default-deny guarantee for `UserMediaPermissionRequest`. B5 maps
"`UserMediaPermissionRequest` → `Camera`/`Microphone` by
`is_for_video_device`/`is_for_audio_device`". The 2.0.2 binding exposes exactly those two
predicates and nothing else (`user_media_permission_request.rs:37` `is_for_audio_device`,
`:44` `is_for_video_device`, both `v2_8`; there is no `is_for_display_device` in this crate —
grep confirms only those two getters plus their notifies).

WebKitGTK ≥2.34 raises a `WebKitUserMediaPermissionRequest` for **display/screen capture**
(`getDisplayMedia`), for which both predicates are false. Such a request downcasts to
`UserMediaPermissionRequest` **successfully**, so it never reaches the `_ => Other` catch-all
the Writer relies on ("`Other` → deny is already the reviewed arm"). The stated mapping has no
arm for it. An implementer resolving that at code time can plausibly land on
`Camera`/`permissions.camera` — a fail-open on a kiosk that has `camera=true` for a legitimate
video-call page.

**Why it matters.** C5 / M9: this is the one classifier in B that decides a security default,
and it has a reachable input with no defined output. One line fixes it (`(false, false) =>
Other`), but it has to be *in the design*, because the Writer's own argument for omitting
`InstallMissingMediaPlugins` is that the catch-all covers it — and here the catch-all does not.

**Evidence.** Tier 4: `$R/webkit2gtk-2.0.2/src/auto/user_media_permission_request.rs:34-44`
(complete getter set), `permission_request.rs:27,34` (`allow`/`deny`). Tier 3:
`nav_policy.rs:219-228` (`Other` → `false`). The ≥2.34 display-capture request type is tier 5
and I mark it as such; the objection does not need it — `(audio=false, video=false)` is
reachable from the bindings alone and the mapping is silent on it either way.

**Not contested:** dropping `InstallMissingMediaPluginsPermissionRequest` from the named list.
`clippy -D warnings` on `ubuntu-22.04` (`.github/workflows/ci.yml:24`, verified) plus the
deprecation makes that correct.

---

## OB-7 — `resource-load-started`'s request-class coverage is undeclared and unpinned (vs B1, MED)

**What breaks.** Evidence discipline (frame §4.4). B1 substitutes one mechanism for another
and declares exactly **one** assumption (`send-request`). But SEC-10's scope is enumerated:
"an `<img src=…>`, a CSS `url(…)`, a `fetch()`, or a beacon" (`egress.rs:1-14`, and parent §7
names the same set). Whether the `WebKitWebView`-level `resource-load-started` fires for all
four — plus redirects, preloads, `data:`/`blob:`, and anything issued before the handler is
connected — is a runtime property of the installed WebKitGTK that no tier 1–4 artifact on this
box settles, and B1 asserts it implicitly by silence.

**Compounding — it sits in tension with a recorded P2-A assumption.** P2-A:~229 records:
"*`load-changed`/`load-failed` are `WebKitWebView`-level signals tracking the **main frame's**
load only, so sub-frame loads never drive them*", pinned by A's scenario 5. B1 now leans on a
different `WebKitWebView`-level signal being emphatically *not* main-frame-scoped, and never
says so or reconciles the two. Whatever the truth, the design is silent where A was explicit.

**When.** Any request class B12 does not exercise. B12 scenario 8 covers `<img>` + `fetch`
(8a) and a service worker (8c). CSS `url()` and beacon — both named by the parent — have no
assertion. So even in a world where OB-1 is fixed, half the enumerated SEC-10 surface ships
unverified.

**Why it matters.** The Writer's own standard: "*pinned by smoke, not assumed*". Two of the
four named classes are assumed.

---

## OB-8 — B8 defers a live parent requirement to nobody (vs B8, MED)

**What breaks.** Frame §4.5 ("anything it defers has a named owner") and §2 ("'deferred to
P3/P4' is only admissible if the parent itself defers it"). B8's text: "Wiring it on both
platforms in one design is a recorded ponytail with an owner." No owner is named — not a
sub-project, not a hardware-checklist row, not a P3 row. The existing record it points at
(`scheme_guard.rs:161-175`) is a P1 descope note that ends "left for a follow-up once one of
those is confirmed available" — also ownerless.

I **accept** the substance: parity with what Windows actually enforces is C3-correct, wiring
Linux alone would be an undeclared stricter divergence, and `ResponsePolicyDecision` is
fenced by P2-A:71-74. The defect is purely that M4 leaves P2 with no owner and the turn block
asserts one that does not exist. Cheap to fix (name the sub-project or a P3 row); not cheap to
leave, because M4 is a parent requirement the parent nowhere defers.

**Evidence.** Tier 3: `crates/kiosk-main/src/scheme_guard.rs:36-40`, `:161-175`. Tier 1:
parent §7 PDF row. Tier 3: the Writer's B8 text.

---

## OB-9 — Two of scenario 12's four assertions cannot fail from a code defect (vs B12, LOW)

Revised scenario 12 asserts (a) spawn succeeds, (b) the child exits non-zero on this bus-less
container, (c) exactly one `config.warn`, (d) kiosk unaffected. (a) and (b) are properties of
the *environment* (`/run/systemd/system` absent), not of B9's code — they hold identically if
the keep-awake block is deleted entirely. Only (c) gates the control, and OB-5 says (c) cannot
pass as wired. (d) is a non-regression check, which is fine but is not the gate the Writer
claims ("a gate that … actually exercises the code"). LOW; I do not veto a fast-track, but the
scenario should say plainly that (a)/(b) are preconditions, not assertions.

---

## Conceded too far — B2's withdrawal as primary

The Writer's §A concludes that a URLPattern → `url-filter` compiler "is not expressible" and
withdraws B2 as primary on that ground plus Q1. The Q1 half is right and I do not re-litigate
it. The expressibility half is **over-generalised**, and under OB-1 that matters because B2 is
now the only surviving in-process enforcement route.

Every divergence §A enumerates lands in the **block** direction for host-scoped patterns, not
the allow direction. Take the battery's own headline case (`allowlist.rs:497-564`):
`https://app.example.com\@evil.com` normalises to host `app.example.com` and the allowlist
**allows** it; a host-anchored raw-string regex (`^https://app\.example\.com[:/]`) requires
`:` or `/` after the host and sees `\`, so it **blocks**. Same for the tab/`%2e`/U+3002 folding
(`:517-528`) and the punycode pair (`:286-301`) — the regex fails to match, so the filter is
*tighter* than the authority. Tighter is a deployment-availability risk, not a SEC-10 hole.
The only false-**allow** cases §A names are path-scoped (`%2e%2e` dot-segments, `:628-642`),
i.e. exactly the class B originally declared as a divergence and the Writer has now withdrawn
from the divergence list.

So the honest statement is narrower than the concession: a host-level content-rules filter is
**sound for the exfiltration boundary** and unfaithful only (a) toward over-blocking on
normalisation quirks and (b) for path scoping. That is a design with a declarable divergence,
which is what C3 exists for. Withdrawing it wholesale, then discovering the replacement cannot
cancel, leaves P2-B with **zero** enforcement of SEC-10 — a strictly worse outcome than the
design that was withdrawn.

---

## Clean passes

- **B4 — hardening control mapping.** Every cite checks out at tier 4: `set_zoom_level`
  `web_view.rs:1980`; `set_zoom_text_only` `settings.rs:1953` (ungated — deciding it here
  rather than deferring is right); `connect_context_menu` `:2074` (ungated, `-> bool`);
  `set_enable_developer_extras` `settings.rs:1475`; `connect_script_dialog` `:2649`
  (`v2_24`, `-> bool`); `confirm_set_confirmed` `script_dialog.rs:28`. Dropping the
  script-dialog budget mirror is correct — `hardening.rs:275-295` is a verified no-op with its
  own ponytail saying so, and porting it would be dead code. Autofill-as-documented-no-op is
  the honest discharge; `settings.rs` exposes no such setter. Suppression semantics are
  declared as assumption #8 and pinned by scenario 10, which can fail. No material objection.
- **B6 — clipboard-read unsatisfiable.** Verified: the nine `*permission_request.rs` files
  contain no clipboard type; `Permissions::clipboard_read` at
  `crates/kiosk-core/src/config/schema.rs:89` has no doc comment today, so adding one is a
  change, correctly declared as such. Stricter divergence, declared in both directions per C3.
- **B7 — downloads via `on_download`.** `WebviewWindowBuilder::on_download` at
  `$R/tauri-2.11.5/src/webview/webview_window.rs:384`; `DownloadEvent::Requested` at
  `$R/tauri-2.11.5/src/webview/mod.rs:77` (the Writer's correction from `:75` is right — `:73`
  is the enum); `false` really cancels on Linux —
  `$R/wry-0.55.1/src/webkitgtk/web_context.rs:355-358`, `else { download.cancel(); }`. The
  cancel lands at `decide-destination`, i.e. after response headers, which is the same point
  Windows' `DownloadStarting` fires — so the parity claim is sound, not merely asserted.
  Rewriting the architecture table's "Not this" row is a correct fix to a real
  self-contradiction. No objection.
- **B10 — feature/floor accounting.** Verified: `v2_32 = ["v2_30", "ffi/v2_32"]` chaining down
  to `v2_2 = []` in `$R/webkit2gtk-2.0.2/Cargo.toml:54-125`, so `["v2_32"]` subsumes A's
  `["v2_16"]`; `remove_script` is `v2_32` (`user_content_manager.rs:163-173`), `add_script`
  ungated (`:57`), `connect_script_dialog` `v2_24`, `remove_filter_by_id` `v2_26` (`:151-154`).
  B1 introduces no gated symbol (`connect_resource_load_started` `web_view.rs:2522` ungated;
  `URIRequest::uri` `uri_request.rs:103`). `crates/kiosk-main/Cargo.toml` has no `webkit2gtk`
  and no Linux target block today, so the declaration is a genuine addition. The wry/`v2_40`
  correction is right and is the strongest part of the turn. **Caveat, not an objection:** the
  2.32 floor is now carried by `remove_script` alone, i.e. by B3 — if OB-2/OB-4 force B3's
  redesign, the floor must be re-derived again, and P2-A:71-74's hand-forward is not finally
  discharged until it is.
- **B11 — A-filter re-derivation.** Verified that B adds no `decide-policy` RESPONSE
  subscription and that wry's download route is `WebContext::connect_download_started` →
  `Download::connect_decide_destination`
  (`$R/wry-0.55.1/src/webkitgtk/web_context.rs:307-320`), never `decide-policy` — so no new
  producer class of `FrameLoadInterruptedByPolicyChange`, and the stateless drop stands. The
  rev 2 → rev 3 citation fix is correct. The added claim about a `send-request` cancel not
  raising that error is moot under OB-1 and should simply be deleted with it.
- **B2 — withdrawal as primary.** No objection to the withdrawal *as a Q1 matter*; see
  "Conceded too far" for the part of the reasoning I dispute and why it now matters.
