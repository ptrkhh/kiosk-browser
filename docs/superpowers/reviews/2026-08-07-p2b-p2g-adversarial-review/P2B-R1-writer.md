# P2-B — WRITER, Round 1 (opening)

No frame dispute.

Two findings in `verify-P2B.md` are structural and I take them head-on before the register,
because the register's shape depends on the answer: **#35 (URLPattern, not globs)** and
**#33/#33b (`resource-load-started` is bound; the parent names it)**. I concede both, and the
concession collapses the spec's Layer-1 mechanism. What replaces it is the parent's own named
mechanism, which is cheaper, and which discharges the divergence B was apologising for. Details
in "Response to the verification record" §A and §B; the register below is already restated
against the corrected design.

## Change register

| ID | Change | Requirement discharged | Depends on |
|---|---|---|---|
| B1 | Egress Layer 1 — `resource-load-started` → existing `NavPolicy::resource_allowed` → cancel via `send-request`, `nav.blocked{egress}` per block **(restructured; was the content filter)** | SEC-10 (parent §7:700), C1, C5 | P2-A (`with_webview`, `SharedNavPolicy`); B10 |
| B2 | Content-filter sys-FFI shim | — | **WITHDRAWN as primary**; recorded contingency under B1 |
| B3 | Egress Layer 2 — CSP belt derived allowlist→origin, lossy toward *permissive* | SEC-10 "plus an injected restrictive CSP" (parent §7:700) | B1 (authority); new `Allowlist::origins()` in kiosk-core |
| B4 | `hardening.rs` control mapping — zoom, context menu, devtools, script dialogs | M5, M9-adjacent, parent §7 hardening matrix | P2-A (webkit2gtk dep); B10 |
| B5 | Permission classifier by GObject runtime type → existing `permission_allowed` | M9 (default-deny), C1 | B4; B10 |
| B6 | Clipboard-read declared unsatisfiable on Linux (stricter divergence) | M9, C3 | B5 |
| B7 | Downloads denied at `WebviewWindowBuilder::on_download` | parent §7 "Downloads / popups / file pickers — blocked", C2 | P2-A (builder site) |
| B8 | PDF stays unwired on Linux, matching Windows | M4 parity under C3; deferral owned by a recorded ponytail | B7 |
| B9 | Keep-awake — `systemd-inhibit --mode=block` child held for process life | PF-07 / M8 / H5 (secondary half; PRIMARY is P2-G's compositor config) | P2-G (image contract); B12 |
| B10 | Feature/floor accounting — `features = ["v2_32"]`, called-symbol floor 2.32 | P2-A's explicit floor-re-derivation hand-forward | P2-A:71-74 |
| B11 | A-filter re-derivation for `FrameLoadInterruptedByPolicyChange` | P2-A:223-226 explicit hand-forward | B1, B7 |
| B12 | Smoke scenarios 8–12 (8–11 blocking, 12 revised) | C9 (the merge gate), §10 | B1, B4, B5, B7, B9 |

---

## B1 — Egress Layer 1: the enforcement boundary (restructured)

**Proposal.** In the `with_webview` closure (P2-A's existing route), connect
`WebViewExt::connect_resource_load_started`. Per resource: read `URIRequest::uri()`, ask the
already-shipped, already-reviewed `NavPolicy::resource_allowed(&uri)`
(`crates/kiosk-main/src/nav_policy.rs:131-137`) — the *same function Windows calls*
(`egress.rs:104`). On deny, cancel by connecting `WebKitWebResource::send-request` on the
handed `&WebResource` via `glib::ObjectExt::connect_local` and returning `Some(true.to_value())`,
then `telem.nav_blocked(REASON_EGRESS, &uri)`. No new decision logic, no second pattern
language, no `unsafe`, no new crate.

**Requirement.** SEC-10, parent §7 line 700 verbatim: "every resource request (not just
navigations) checked against `content.allowlist` and cancelled if off-list — WebView2
`WebResourceRequested`, **WebKitGTK `resource-load-started`**, Android `shouldInterceptRequest`".
This is the parent's named mechanism, not a substitute. C1 (no reimplementation) is satisfied
*by construction*: the decision is one call into the existing pure function.

**Evidence.**
- Tier 4, verified by me: `$R/webkit2gtk-2.0.2/src/auto/web_view.rs:2523` —
  `fn connect_resource_load_started<F: Fn(&Self, &WebResource, &URIRequest) + 'static>(…)`,
  **ungated** (no `#[cfg(feature=…)]` above it).
- Tier 4: `$R/webkit2gtk-2.0.2/src/auto/uri_request.rs:103-105` `fn uri(&self) -> Option<GString>`;
  `:113-114` `fn set_uri(&self, uri: &str)` — both ungated.
- Tier 4: `$R/glib-0.18.5/src/object.rs:1824` —
  `fn connect_local<F>(&self, signal_name: &str, after: bool, callback: F) -> SignalHandlerId
  where F: Fn(&[Value]) -> Option<Value> + 'static`. Signals are connected **by name**, so an
  unbound signal needs no sys binding and no `unsafe`. `glib` is re-exported at
  `$R/webkit2gtk-2.0.2/src/lib.rs:9` — the route P2-A:69 already declared, zero new deps (C6).
- Tier 3: `crates/kiosk-main/src/nav_policy.rs:131-137` `pub fn resource_allowed(&self, url:&str) -> bool`;
  Windows' sole use of it at `egress.rs:104`.
- Tier 3: `REASON_EGRESS` at `egress.rs:22`, and its telemetry-flood doctrine at `egress.rs:112-118`
  ("we deliberately do NOT add a second limiter … coalesces at 20/burst") — unchanged, reused.

**Why this beats the withdrawn content filter, on the frame's own rungs.**
- **Q1** — parent named it; the filter was invention. Traceability wins.
- **Q2** — one signal + one existing pure fn, versus: a URLPattern→`url-filter`-regex compiler, a
  contained `unsafe` sys-FFI shim, a new direct `webkit2gtk-sys` dependency with its own
  `ffi/v2_24` feature, an async save-callback, a `data_dir/content-filters/` store, and
  add/remove filter-id lifecycle across `ConfigApplied`. Six moving parts to zero.
- **Q3** — the block is now emitted *where the decision is made*, per request. The spec's own
  headline divergence ("path-scoped blocks are enforced but **silent** on Linux") **disappears**.
  So does the "operator dashboards see egress blocks identically" claim being false — under B1 it
  is true, because it is literally the same function emitting the same label.
- **C1** — the filter route re-derived the allowlist in a foreign matcher. See §A: a *faithful*
  re-derivation is not merely hard, it is not expressible. B1 has no such surface.

**Dependencies.** P2-A's `with_webview` route and `SharedNavPolicy` plumbing (already threaded to
`egress::install`, `egress.rs:25-33`). B10 for the feature declaration. Pinned by B12 scenario 8.

**Declared assumption, with pinning (see §B).** That the WebKit 4.1 runtime defines the
`send-request` signal with a cancel-capable `gboolean` return is a **tier-5** fact — no gir, no
headers on this box (`pkg-config --exists webkit2gtk-4.1` → no; `find / -name 'WebKit2*.gir'` →
empty), and it is unbound in webkit2gtk-2.0.2 (verified: `web_resource.rs` binds only
`connect_failed` `:118`, `connect_failed_with_tls_errors` `:149`, `connect_finished` `:185`,
`connect_received_data` `:208`, `connect_sent_request` `:234`, and two notifies). `connect_local`
**panics if the signal does not exist** (`object.rs:1824` doc). Pinning mechanism, both halves:
1. **Boot probe, not a panic:** `glib::subclass::signal::SignalId::lookup("send-request",
   WebResource::static_type())` — verified to exist at `$R/glib-0.18.5/src/subclass/signal.rs:221`
   `pub fn lookup(name: &str, type_: Type) -> Option<Self>`. `None` ⇒ do not connect; emit
   `config.warn("egress.cancel_unavailable")` and fall back to observe-only.
2. **Smoke scenario 8 is blocking on the *cancel*, not just the event.** If cancel does not take,
   the residual is a fail-open on a security gate (C5) and B1 does not merge as designed — the
   recorded fallback is B2, paid for only then.
**Residual risk:** if `send-request` proves non-cancel-capable, SEC-10 enforcement on Linux is
CSP-only until B2 is built. That is strictly better than the shipped state (P2-A:42, no
enforcement at all) but it is not "closed", and scenario 8 is what tells us before merge rather
than in the field.

## B2 — Content-filter sys-FFI shim — WITHDRAWN as primary

Retained in the spec as a **recorded contingency** with its cost stated, activated only if B12
scenario 8 shows `send-request` cannot cancel. Its citations survive verification
(`webkit2gtk-sys-2.0.2/src/lib.rs:5411`, `:5467`, `:5511`; `remove_filter_by_id` at
`user_content_manager.rs:154`, `v2_26`), and its two undeclared prerequisites are now written
into the contingency: a direct `webkit2gtk-sys` dependency with `ffi/v2_24` (none exists in
`crates/kiosk-main/Cargo.toml` today — checked), and the URLPattern→regex expressibility problem
of §A, which is the reason it is a contingency and not the plan.

## B3 — Egress Layer 2: the CSP belt, lossy toward permissive

**Proposal.** Keep the belt; drop its "observability layer" job (B1 owns observability now) and
drop the violation-report Tauri command with it. It becomes: a pure derivation
`allowlist origins + content origin + app origins + data: + blob:` → a document-start user script
that appends the `<meta http-equiv>`, swapped on `ConfigApplied` via a kept `UserScript` handle
(`add_script` `user_content_manager.rs:58` ungated / `remove_script` `:166`, `v2_32`), **never**
`remove_all_scripts` (`:131`).

**Requirement.** Parent §7:700 ends "— **plus an injected restrictive CSP**." Q1 keeps this change
even though B1 now enforces alone: the parent names both, not either.

**Evidence.** Tier 3: `nav_policy.rs:169-184` records exactly why P1 refused the tighter belt
("would make a legitimately-allowlisted subresource pass the native filter and then get silently
blocked by this CSP"). The direction inversion is the whole change and it survives the URLPattern
correction. Tier 4: `remove_all_scripts` at `:131`, and wry's own bootstrap living in the same
manager (`$R/wry-0.55.1/src/webkitgtk/mod.rs:721-738`) — I accept the verifier's #37 refinement
that the IPC *handler* is a `register_script_message_handler`, not a `UserScript`; the wording is
corrected to "wry's own initialization script", and the prohibition stands on that alone.

**Dependencies.** B1 is the authority; B3 must never be tighter than it. Needs the pattern-origin
accessor — see §C, assumption 4.

## B4 — `hardening.rs` control mapping

**Proposal.** Unchanged from the spec: `set_zoom_level` (`web_view.rs:1980`, ungated) with
`set_zoom_text_only(false)` asserted explicitly (`settings.rs:1953`, ungated — the spec left this
"plan-time"; it is a one-line setter, so it is decided here, not deferred);
`connect_context_menu` → `true` (`web_view.rs:2074`, ungated); `set_enable_developer_extras(false)`
(`settings.rs:1475`, ungated); `connect_script_dialog` → `true` (`web_view.rs:2649`, `v2_24`) with
`BeforeUnloadConfirm` → `confirm_set_confirmed(true)` (`script_dialog.rs:28`).

**Requirement.** Parent §7 hardening matrix rows (autofill M5, dialogs, chrome); C3 honest parity
with `windows_impl`.

**Evidence.** All line cites above verified tier 4. Autofill: documented **no-op**, and I stand on
it — the parent's own §7 row says "WebKitGTK disable form persistence", but the bindings expose no
password/autofill store to disable (`settings.rs` has no such setter); the honest discharge is a
documented no-op with the reason, not an invented setting.

**Revision from the verification record.** The Windows script-dialog budget is a verified no-op
(`hardening.rs:283-295`, "*ponytail: this counter is a NO-OP today*"). "Same budget semantics
mirrored" would port dead code — struck. Linux suppresses unconditionally; the divergence is
nil because Windows' counter has no observable effect either.

**Dependencies.** P2-A's webkit2gtk dependency; B10's `v2_32`; B12 scenario 10.

## B5 — Permission classifier by GObject runtime type

**Proposal.** `connect_permission_request` (`web_view.rs:2428`, ungated) → `glib::Cast::downcast_ref`
against a small local enum, mapping onto the **existing** `PermissionKind` and the **existing**
`permission_allowed` (`nav_policy.rs:219-228`). Downcasts confined to the handler; the mapping is a
pure host-tested `fn`, exactly like `classify_permission_kind` (`hardening.rs:72`).

**Requirement.** M9, default-deny. C1 — no second policy; `Other` → deny is already the reviewed
arm (`nav_policy.rs:226`).

**Evidence.** Tier 4, all verified: `permission_request.rs:9` (interface),
`user_media_permission_request.rs:15` (`@implements`), `:37`/`:44` `is_for_audio_device` /
`is_for_video_device`, `permission_request.rs:27`/`:34` `allow()`/`deny()`. Gates all ≤ `v2_32`.

**Revision.** Drop `InstallMissingMediaPluginsPermissionRequest` from the named downcast list. It
carries `deprecated = "Since 2.40"` under the build's actual feature set, and CI is
`cargo clippy --workspace --all-targets -- -D warnings` (`.github/workflows/ci.yml:24`) — naming it
is a hard CI failure. It needs no arm: the `_ => Other` catch-all already denies it, which is the
required behaviour.

## B6 — Clipboard-read declared unsatisfiable (stricter divergence)

**Proposal/Evidence.** `Permissions::clipboard_read` (`crates/kiosk-core/src/config/schema.rs:89`)
has no WebKit counterpart — the nine `*permission_request.rs` files are the complete set and none
is clipboard. Declared as a **stricter** divergence per C3, documented in the spec and in a
**new** doc comment on `schema.rs:89` (the field has none today — this is a change I am making,
not a citation I am making).

## B7 — Downloads via `on_download`

**Proposal/Evidence.** `WebviewWindowBuilder::on_download`
(`$R/tauri-2.11.5/src/webview/webview_window.rs:384`), `DownloadEvent::Requested` (variant at
`webview/mod.rs:77` — the spec cited `:75`, the enum declaration; corrected). Returning `false`
really cancels on Linux: `$R/wry-0.55.1/src/webkitgtk/web_context.rs:355-358` — `else { download.cancel(); }`.
`REASON_DOWNLOAD` (`scheme_guard.rs:46`) becomes `pub(crate)`.

**Requirement.** Parent §7 "Downloads / popups / file pickers — blocked". C2 — shipped API, no
hand-written handler.

**Revision.** The architecture table's "Not this: a `download-started` signal handler" is rewritten
to "not a **hand-written** `download-started` handler — wry already installs one
(`web_context.rs:317`) and `on_download` is its front door." The table and the A-filter section
contradicted each other two pages apart; the table was the sloppy one.

## B8 — PDF stays unwired, both platforms

**Proposal.** No `RESPONSE` policy subscription. `scheme_guard::pdf_decision` remains
`#[allow(dead_code)]` on both platforms (`scheme_guard.rs:36-40`), for the reason already on
record (`scheme_guard.rs:161-175`). Wiring it on both platforms in one design is a recorded
ponytail with an owner.

**Requirement.** C3 — parity with what Windows *actually enforces*. Wiring Linux only would be a
silent divergence in the *stricter* direction and would drag `ResponsePolicyDecision` back in,
which P2-A:71-74 explicitly forbids without re-deriving the floor.

## B9 — Keep-awake: `systemd-inhibit --mode=block` child

**Proposal.** `#[cfg(target_os = "linux")]`, gated on `display.keep_awake` (`schema.rs:144`):
spawn `systemd-inhibit --what=idle:sleep --who=kiosk-browser --why="kiosk display" --mode=block cat`
with piped stdin, keep the `Child` for process life.

**Requirement.** PF-07 / M8 / H5 — and note the parent scopes this honestly: "*Linux/Wayland:
`systemd-inhibit` blocks **suspend** only, display blanking is compositor-owned — PRIMARY is
configuring cage/wlroots not to blank*" (parent §7 keep-awake row). B9 is the **secondary** half;
the primary is P2-G's image contract. Claiming more than that would be the invention Q1 punishes.

**Evidence.** Windows analogue verified: `main.rs:949-966`, return value discarded at `:964` (silent
failure) — so "matching Windows' silent failure mode" holds. Rejection of
`gtk::Application::inhibit`: see §C assumption 10 — I am replacing the justification.

**Revision** (from FALSE #74): see §C assumption 9 and B12 scenario 12.

## B10 — Feature/floor accounting

**Proposal.** `[target.'cfg(target_os = "linux")'.dependencies] webkit2gtk = { version = "2.0.2",
features = ["v2_32"] }` (cumulative — `v2_32 = ["v2_30", "ffi/v2_32"]` chains to `v2_2 = []`,
verified in `$R/webkit2gtk-2.0.2/Cargo.toml`), superseding P2-A:63-64's proposed `["v2_16"]`.
Called-symbol floor is **2.32**, driven by `remove_script` (`user_content_manager.rs:166`);
`connect_script_dialog` is `v2_24`, `remove_filter_by_id` `v2_26`. **B1 introduces no gated symbol
at all** — `connect_resource_load_started`, `URIRequest::uri`/`set_uri` and `connect_local` are all
ungated, so the restructure does not move the floor.

**Requirement.** P2-A:71-74's explicit hand-forward ("Do not reintroduce … without re-deriving
that floor"). Discharged: highest gate called is `v2_32`, no `v2_40` symbol, no
`ResponsePolicyDecision`.

**Revision** (FALSE #28/#29): see §C.

## B11 — A-filter re-derivation

**Proposal/Evidence.** P2-A:223-226 hands forward: "*this filter assumes P2-A installs exactly one
`navigation_handler` and subscribes no RESPONSE or download decision. P2-B adds both … P2-B must
re-derive this filter*." B adds **neither**: B8 adds no RESPONSE subscription, and B7's route is
`WebContext::connect_download_started` → `Download::connect_decide_destination`
(`$R/wry-0.55.1/src/webkitgtk/web_context.rs:307-320`), never `decide-policy` — verified. So no new
producer class of `FrameLoadInterruptedByPolicyChange`; the stateless drop stands.

**New under B1:** a `send-request` cancel is not a policy decision and does not raise
`FrameLoadInterruptedByPolicyChange` on the *main frame* — but that is a runtime claim I cannot
settle from bindings, so scenario 8 asserts it directly: a blocked subresource produces no
`NavigationFailed`, no `nav.error`, no error-page transition. Same assertion shape P2-A used for
scenario 5 (P2-A:299-302), for the same reason.

**Fix:** the spec says "discharging **rev 2's** recorded invariant"; the invariant is in A **rev 3**
at `:223-226`. Corrected.

## B12 — Smoke scenarios 8–12

8–11 blocking, 12 revised (below). Scenario 8 is rewritten against B1:
(a) off-allowlist `<img>` + `fetch()` → **cancelled**, exactly one `nav.blocked{egress}` per
request class, and the page's own allowlisted resources still load;
(b) a *path-scoped* second allowlisted host: in-pattern path loads, off-pattern path is
**blocked and reported** — this is the assertion that proves the silent-divergence is gone;
(c) service-worker-initiated off-list fetch → the SW coverage question, unchanged in intent and
still smoke-pinned, now against `resource-load-started` rather than content rules;
(d) **new, blocking:** `SignalId::lookup("send-request", …)` returns `Some` on the target
WebKitGTK, and the cancel actually takes. This is the B1 assumption's pin.
(e) **new:** no `NavigationFailed`/`nav.error`/error-page transition from a blocked subresource
(the B11 re-derivation).

Scenario 12 is rewritten — see §C assumption 9.

---

## Response to the verification record

### §A — FALSE #35: "allowlist patterns are globs". **CONCEDE**, and it is fatal to B2.

Verified myself: `crates/kiosk-core/src/nav/allowlist.rs:26-27` — "*the URL must match one compiled
**URLPattern** (spec §3.6 — URLPattern semantics, **not globs**), matched on parsed components*";
`:31` `patterns: Vec<UrlPattern>`; `:119-122` compiled via
`urlpattern::quirks::process_construct_pattern_input`. The verifier is right, and right that B
inherited the error from `nav_policy.rs:177` — which is also wrong and which I will fix in the same
edit rather than propagate.

**Is a URLPattern → WebKit `url-filter` regex compiler expressible? No — and that is the answer,
not a difficulty.** The two matchers do not take the same input. `Allowlist::allows` parses with
`url::Url` and matches **components of the normalised URL**; `url-filter` is one regex over the
**raw URL string**. The gap is not cosmetic — it is exactly the gap the reviewed adversarial
battery in `allowlist.rs` exists to pin:

- `allowlist.rs:497-564` — `https://app.example.com\@evil.com` normalises to host
  `app.example.com` and **allows**, while `https://evil.com\@app.example.com` normalises to host
  `evil.com` and **blocks**. Same bytes, opposite verdicts, decided by WHATWG authority parsing.
- `:517-528` — tab stripped, `%2e` → `.`, U+3002 → `.` all fold onto the same real host.
- `:286-301` — a Unicode pattern must match the punycode URL **and** the reverse.
- `:628-642` — `%2e%2e` dot-segments resolve *before* matching.
- `:580-590` — host case folds, path case does not.

A regex over the raw string reproduces none of these. Every divergence is either a false block
(breaks a live deployment silently) or a false allow (a SEC-10 hole in the exfiltration boundary).
And building it is a second implementation of the allowlist decision — the thing **C1 forbids in
one sentence**. The spec's own escape hatch ("patterns that cannot be expressed fail the compile
loudly") does not save it: the dangerous cases are the ones that *do* compile and mean something
subtly different.

**Concrete revision:** B2 withdrawn as primary (above). B1 needs no pattern translation whatsoever
— it calls `resource_allowed`, which calls `Allowlist::allows`, which is the one authority. Every
one of the battery's properties transfers for free. The three places B said "glob" are struck; the
Testing section's "glob→`url-filter` regex" host test is deleted with the compiler it tested.

### §B — FALSE #33/#33b: "WebKitGTK has no request-level host API". **CONCEDE**, twice over.

**The fact.** Verified myself: `$R/webkit2gtk-2.0.2/src/auto/web_view.rs:2523` —
`fn connect_resource_load_started<F: Fn(&Self, &WebResource, &URIRequest) + 'static>(…)`, ungated.
B's sentence is false as written. The precise true statement is narrower still than the verifier's:
*no cancel-capable request-level API is **bound** in webkit2gtk-2.0.2* — `WebResource` binds only
`connect_failed` (`web_resource.rs:118`), `connect_failed_with_tls_errors` (`:149`),
`connect_finished` (`:185`), `connect_received_data` (`:208`), `connect_sent_request` (`:234`) and
two notifies; no `send-request`. But **unbound ≠ unreachable**: `send-request` is a GObject signal,
and `glib::ObjectExt::connect_local` (`$R/glib-0.18.5/src/object.rs:1824`) connects signals by
**name** with an `Option<Value>` return — no gir binding, no sys symbol, no `unsafe`. So the
correct statement is narrower again: *the cancel-capable API is not bound, and is reachable by name
if the runtime defines it.*

**The traceability failure (Frame Q1).** Parent §7 line 700 names `WebKitGTK
`resource-load-started`` as THE mechanism, in the same clause as WebView2's
`WebResourceRequested` — the mechanism B *does* use on Windows. B substituted a content filter,
never cited the parent's choice, and never argued against it. That is invention displacing
traceability, and it is my error, not a wording slip. I concede it as such.

**Does the two-layer design survive, change, or get replaced?** **Layer 1 is replaced. Layer 2
survives with a demoted job.**
- Layer 1 (B1) becomes the parent's mechanism, calling the existing pure decision function. Its
  authority claim gets *stronger*: it now inherits every property of the reviewed
  `Allowlist`/`resource_allowed` pair instead of approximating them in a regex.
- Layer 2 (B3) was justified as "the observability layer" because Layer 1 was structurally silent
  ("WebKit fires no per-block callback"). That justification is gone: B1 emits
  `nav.blocked{egress}` at the decision point, which is what Windows does. The CSP belt survives
  **only** on Q1 — the parent's "plus an injected restrictive CSP" — as renderer-side
  defence-in-depth. Its `securitypolicyviolation` listener and the `#[cfg(not(windows))]` Tauri
  command are **dropped** (Q2: their only value was observability B1 now provides). That also
  disposes of the verifier's note about `tauri::generate_handler!` at `main.rs:990` being
  unconditional — nothing needs to touch it.
- The headline divergence **"path-scoped egress blocks are enforced but silent on Linux"** is
  withdrawn from the spec's divergence list. It was an artefact of the wrong mechanism.
- The residual honesty that *survives*: CSP's structural gaps (`nav_policy.rs:152-167`) and the
  service-worker question. A `resource-load-started` handler sees network-layer loads, so it is
  not subject to CSP's gaps — but whether WebKit routes service-worker fetches through it is the
  same open question, unchanged, still pinned by scenario 8(c) rather than assumed.

**The one new assumption this creates is declared and pinned in B1** (`send-request` existence and
cancel semantics; `SignalId::lookup` boot probe + blocking scenario 8(d); residual risk stated).
I am not trading a false claim for a hidden one.

### FALSE #28/#29 — "wry itself enables `webkit2gtk/v2_40`". **CONCEDE.**

Verified myself: `$R/wry-0.55.1/Cargo.toml` — `[target.…dependencies.webkit2gtk] version = "=2.0.2",
features = ["v2_38"]`. `v2_40` appears only inside the non-default `linux-body` feature, and wry's
`default = ["protocol","os-webview","x11"]` excludes it. The v2_40 in our build comes from
`$R/tauri-runtime-wry-2.11.4/Cargo.toml` — `[dependencies.wry] features = ["protocol","os-webview","linux-body"]`.

**Revision (exact replacement wording):** "*wry's own dependency is `webkit2gtk` with
`features = ["v2_38"]` (`wry-0.55.1/Cargo.toml`); `v2_40` reaches the build only because
`tauri-runtime-wry-2.11.4` enables wry's non-default `linux-body` feature
(`tauri-runtime-wry-2.11.4/Cargo.toml`). Feature unification therefore builds every gate ≤ v2_40
today — but that is a **dependency-of-a-dependency's feature choice**, not a guarantee, which is
precisely why we declare `["v2_32"]` ourselves rather than lean on it.*"

Note this correction **strengthens** B10's own conclusion: the paragraph's point was always "declare
what our code calls"; the corrected evidence makes the reason for it real rather than rhetorical.
It also makes the B5 deprecation revision load-bearing — the v2_40 that triggers the
`InstallMissingMediaPlugins` deprecation arrives from tauri-runtime-wry, and is present today.

### FALSE #74 — smoke scenario 12 tests a path that cannot fire. **CONCEDE.**

Reproduced myself: `which systemd-inhibit` → `/usr/bin/systemd-inhibit`; `/run/systemd/system` does
not exist; running the spec's exact command → `Failed to connect to bus: No such file or directory`,
`exit=1`. So `Command::spawn` **succeeds** and the *child* dies. A design that only handles
`spawn()`'s `Err` arm is not merely untested — it would leave a dead inhibitor **silent on real
hardware**, which is the project's named defect class (Q3).

**Concrete revision to B9:** handle both failure modes, and make the second one the loud one.
```
match Command::new("systemd-inhibit").args([...]).stdin(Stdio::piped()).spawn() {
    Err(e) => telem.config_warn("display.keep_awake", &format!("spawn failed: {e}")),
    Ok(child) => {
        // The child must still be alive a moment later: a bus-less host exits ~immediately.
        // try_wait() is the check; a child that already exited is a dead inhibitor.
        keep_awake_child = Some(child);   // held for process life
    }
}
```
with an explicit `try_wait()` check on the *first* health sample (P2's health sampler already runs;
no new timer) → `Some(status)` ⇒ `config.warn("display.keep_awake", "inhibitor exited: {status}")`.
`config.warn` replaces the spec's `eprintln` — best-effort still (C4, never blocks boot), but
**observable**, which `eprintln` in a systemd-launched process is not.

**Scenario 12 revised, and promoted from degrade-only:** assert that (a) spawn succeeds, (b) the
child exits non-zero on this bus-less container, (c) exactly one `config.warn{display.keep_awake}`
is spooled, (d) the kiosk is unaffected. That is a gate that *can* run here and that actually
exercises the code (C9). The positive assertion (`systemd-inhibit --list` shows the hold) stays on
the deferred hardware checklist with cage.

### FALSE #76 — "clippy `-D warnings` both platforms". **CONCEDE.**

Verified myself: `.github/workflows/ci.yml:24` runs `cargo clippy --workspace --all-targets --
-D warnings` in `lint-test` on `ubuntu-22.04` only; `build-windows` (`:30-42`) runs
`cargo build --release -p kiosk-main -p kiosk-launcher` and nothing else.

**Revision:** "*Existing gates unchanged: clippy `-D warnings` on the ubuntu `lint-test` job,
`cargo test --workspace`, the Linux `cargo check -p kiosk-main`, and the Windows release build.
There is no Windows clippy job; P2-B does not add one (P2-F owns CI changes).*" P2-A:327 carries
the same error — flagged for the moderator as a LOW correction against A's text, not re-argued here.

---

### Undeclared assumptions

1. **Content rules cover custom-scheme (app/asset) origins.** **CONCEDE — and MOOT.** The premise
   died with B2. Under B1 the equivalent question has an answer already in the codebase, not an
   assumption: `resource_allowed` returns `true` for every non-remote origin
   (`nav_policy.rs:131-134`), and P2-A:96-110 makes `tauri://localhost` / `kioskasset://localhost`
   non-remote. Bundled pages and the offline mp4 are admitted **by the same function Windows uses**,
   with no per-origin allow-entries to get wrong.

2. **`data:` / `blob:` / hostless subresources survive Layer 1.** **CONCEDE — and MOOT for B1** for
   the same reason: `resource_allowed`'s rule 1 explicitly allows them, and `nav_policy.rs:110-117`
   records why. The verifier is right that B's three-bucket filter would have broken them, tighter
   than Windows, un-named. That is a divergence B2 would have had to declare and B1 does not create.

3. **Same for Layer 2's CSP.** **CONCEDE — live, with a revision.** P1's `csp_policy` carries
   `img-src … data:` and `font-src … data:` (`nav_policy.rs:189,191`) precisely to keep bundled
   assets working, and B3's derivation as written ("no path components survive", origins only)
   would drop them — the exact silent-breakage P1 refused to ship, in a new direction.
   **Revision:** B3's derivation is `allowlist origins ∪ {content origin, app origin, asset origin}`
   **∪ `{data:, blob:}`**, and its host test asserts the `data:`/`blob:` sources are present, not
   merely that paths are absent. The superset property is stated against `resource_allowed`, which
   is the thing the belt must not be tighter than.

4. **Allowlist pattern strings reachable at the point of derivation.** **CONCEDE — with a revision,
   and the scope shrinks.** Verified: `NavPolicy.allowlist` is private (`nav_policy.rs:28`);
   `Allowlist` exposes only `allows()` (`allowlist.rs:72`) and `invalid_patterns()` (`:67`).
   **B1 needs nothing** — it calls `resource_allowed`. Only B3 needs origins.
   **Revision:** add one accessor **in kiosk-core, next to the matcher it must agree with**:
   `pub fn origins(&self) -> Vec<String>` on `Allowlist`, built from the compiled patterns'
   own components — `UrlPattern::protocol()` / `hostname()` / `port()`, verified public at
   `$R/urlpattern-*/src/lib.rs:431,446,451`. This is why the *CSP* direction is expressible where
   the `url-filter` direction is not: CSP source expressions accept exactly these shapes —
   `https://*.example.com`, a bare scheme, `*` — so a wildcard pattern maps to a wildcard source
   and the loss stays in the **permissive** direction by construction. Host-tested in
   `allowlist.rs` beside the battery, so the belt cannot drift tighter than the matcher.

5. **`webkit2gtk-sys` needs its own dependency + `ffi/v2_24`.** **CONCEDE.** Verified: the store
   symbols are gated in the sys crate (`webkit2gtk-sys-2.0.2/src/lib.rs:5409` et seq) and
   `crates/kiosk-main/Cargo.toml` has no `webkit2gtk-sys` dependency (nor any Linux target block).
   Moot for the primary design; written into B2's contingency cost, where it belongs — and it is
   part of why B2 is the contingency and not the plan (Q2, C6).

6. **`REASON_EGRESS` visibility.** **CONCEDE.** `egress.rs:22` is private, exactly like
   `REASON_DOWNLOAD` which B *did* call out. **Revision:** both become `pub(crate)` in the same
   edit; the spec names both.

7. **"Operator dashboards see host-level egress blocks identically on both platforms."**
   **CONCEDE as written** — under the old design Windows emitted one event per blocked *request*
   and Linux one per *CSP violation report*, which are different populations, and the spec
   contradicted itself two paragraphs later. **Under B1 the claim becomes true and is kept**, with
   its basis stated rather than asserted: the same `resource_allowed` call, the same
   `REASON_EGRESS` label, the same per-request granularity, the same `nav.blocked` rate bucket
   (`egress.rs:112-118`; 20/burst pinned at `crates/kiosk-core/src/logging/ratelimit.rs:182`).

8. **`confirm_set_confirmed(true)` == "leave the page"; returning `true` from the three signals
   suppresses.** **ASSUMPTION, WITH PINNING.** These are WebKit runtime semantics; the bindings
   give signatures only (`script_dialog.rs:28`, `web_view.rs:2074/2428/2649`) and no tier 1–4
   artifact settles them. Declared as assumptions in the spec.
   **Pinning:** B12 scenario 10, extended to assert all three arms explicitly — right-click paints
   no menu, an `alert()` loop paints nothing and does not wedge, a `beforeunload` page navigates
   away **without prompting**; and scenario 11 covers the permission-request return value
   (`getUserMedia` denied ⇒ the `true` return was honoured, since an unhandled request would
   default-deny *silently and identically* — so scenario 11 additionally asserts the
   `permissions.camera=true` fixture **allows**, which only passes if our handler is the one
   deciding). All blocking.
   **Residual risk:** low and loud — every one of these fails visibly in scenario 10/11, not in the
   field. If a return value is inverted, the smoke shows chrome painting, which is unmissable.

9. **The `systemd-inhibit … cat` EOF-release chain ("exit symmetry comes free").**
   **ASSUMPTION, WITH PINNING — and the "comes free" wording is withdrawn.** It depends on logind
   releasing on child exit and on our pipe being the only stdin holder; unverifiable without a
   systemd host, and none is available here (verified above).
   **Pinning:** the deferred hardware checklist row with cage, which asserts both directions —
   `systemd-inhibit --list` shows the hold while the kiosk runs, and shows it **gone** after
   `kiosk-main` is killed. Plus the new `try_wait()` liveness check from #74, which catches the
   *other* half (a hold that was never taken) on every device, in the field, loudly.
   **Residual risk:** a leaked inhibitor after an abnormal exit would keep a device awake — a
   power/burn-in cost, not a security or availability one, and P2-G's `IdleAction=ignore` image
   contract makes the inhibitor the belt rather than the boundary (parent §7: "PRIMARY is
   configuring cage/wlroots not to blank").

10. **`gtk::Application::inhibit` silently no-ops under cage/weston.** **CONCEDE the justification.**
    It was the sole reason given for rejecting a shipped-API route, which C2 prefers, and it is an
    uncited runtime claim. **Revision — replace it with the checkable reason the verifier
    identified and I confirmed:** tao's `gtk::Application` is `pub(crate)`
    (`$R/tao-0.35.3/src/platform_impl/linux/event_loop.rs:58` — `pub(crate) app: gtk::Application,`),
    so it is **unreachable from `kiosk-main` through tao's public API**. That disposes of the route
    on tier 4 alone. The session-manager argument is demoted to a parenthetical marked as an
    external-tier expectation, and nothing depends on it.

11. **`InstallMissingMediaPluginsPermissionRequest` is safe to name.** **CONCEDE.** Verified
    deprecated-since-2.40 in `auto/mod.rs`, v2_40 is enabled in this build (via tauri-runtime-wry,
    per #28), and CI is `clippy -D warnings` (`ci.yml:24`). **Revision:** the type is removed from
    B5's named list; the `_ => Other` arm covers it with the correct behaviour and no deprecated
    symbol. Noted in the spec so nobody "helpfully" adds it back.

---

## Withdrawals / restructuring

1. **B2 (content-filter sys-FFI shim) — withdrawn as the primary mechanism.** Two independent
   reasons, either sufficient: its compiler is not faithfully expressible (§A) and would be a
   second implementation of the allowlist decision (C1); and the parent named a different
   mechanism that B never cited (§B, Q1). Retained as a written contingency with its full cost —
   sys dependency, `ffi/v2_24`, `unsafe` shim, async save, filter-id lifecycle — activated only if
   scenario 8(d) shows `send-request` cannot cancel.

2. **B1 — restructured** onto `resource-load-started` + `resource_allowed` + `send-request`. This is
   the largest change in the turn and it is a simplification, not an addition.

3. **B3 (CSP belt) — demoted, not withdrawn.** It loses its observability job and its violation
   listener and Tauri command; it keeps its looser-not-tighter derivation and its parent-named
   existence. Q2 would delete it; Q1 keeps it, and Q1 outranks Q2 here because the parent names it
   in the same clause as the filter.

4. **The spec's headline divergence "path-scoped egress blocks are silent on Linux" — withdrawn.**
   It was a consequence of the withdrawn mechanism. The remaining declared divergences are:
   clipboard-read always denied (stricter, B6), and PDF unwired on both platforms (parity, B8).

5. **The script-dialog budget mirror — withdrawn** (B4). Windows' counter is a verified no-op
   (`hardening.rs:283-295`); mirroring it ports dead code for nothing.

6. **Scenario 12 — restructured** from degrade-only to a real assertion, per §FALSE #74, and B9
   gains a `try_wait()` liveness check plus `config.warn` in place of `eprintln`.

7. **Not withdrawn, and I will defend them as-is:** B4 (autofill as a documented no-op — the
   parent's row names a control the bindings do not expose; an honest no-op beats an invented
   setting), B8 (PDF parity — wiring Linux alone would be a silent stricter divergence and would
   drag `ResponsePolicyDecision` back across P2-A:71-74's floor rule), and B9's scoping as the
   *secondary* keep-awake half with P2-G owning the primary, which is the parent's own division.
