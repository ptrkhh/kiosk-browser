# P2-B — Linux Hardening + Subresource Egress + Keep-Awake Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **EXECUTION HOST:** Linux/Wayland. Tasks 1–3, 6, 8 are pure/host-tested. Tasks 4, 5, 7, 9, 10 compile on Linux and are proven by the Task 11–12 smoke, which extends P2-A's harness.

**Goal:** `hardening.rs`, `egress.rs` and downloads get Linux bodies with honest parity; `display.keep_awake` gets a Linux route; a bundled on-screen keyboard (B13) and the native `print` suppression (B14) ship. Closes P2-A's residual (`p2a:42`, "do not field a Linux device before P2-B") at **(scheme, host, port)** granularity for subresources.

**Architecture:** SEC-10 splits into two layers because WebKitGTK has no cancel-capable request-level API in this process. **Layer 1** is a WebKit content filter — compiled in `kiosk-core` from the same allowlist the matcher uses, installed through a contained `unsafe` sys-FFI shim — and is the enforcement authority. **Layer 2** is an injected CSP belt that returns `None` (inject nothing) whenever any pattern is not CSP-expressible. `resource-load-started` is observe-only telemetry, never enforcement.

**Tech Stack:** Rust 2021, webkit2gtk 2.0.2 (`v2_32`), webkit2gtk-sys 2.0.2 (`v2_24`), `regex` (kiosk-core dev-dependency only), systemd-inhibit.

**Spec:** `docs/superpowers/specs/2026-08-06-p2b-linux-hardening-egress-design.md` (rev 3)

**Depends on:** P2-A (nav guard, load lifecycle, origin constants). Order-independent with P2-C.

## Global Constraints

- **Dependencies** (reconcile by union if P2-C/P2-D landed the `webkit2gtk` line first):
  ```toml
  [target.'cfg(target_os = "linux")'.dependencies]
  webkit2gtk     = { version = "2.0.2", features = ["v2_32"] }
  webkit2gtk-sys = { version = "2.0.2", features = ["v2_24"] }
  ```
  The sys crate declares `features = ["v2_24"]`, **not** `ffi/v2_24` — `ffi/…` is the `webkit2gtk` crate's alias for *its* sys dependency's feature.
- **No `v2_40`-gated symbol may be called.** The declared floor is a review convention, not build-enforced (Cargo unifies `v2_40` in from tauri/wry regardless). Enforcement is code review against this line.
- **Blanket rule: no dynamic signal connection anywhere.** No `glib::ObjectExt::connect_local`, no `connect_closure`. Every signal used is a typed generated binding. Grep-checkable; a mis-typed dynamic connect panics *inside signal emission*, which under the launcher is a crash-restart loop.
- **Do not connect `sent-request`** in an attempt to cancel: it is past-tense and void-return. The gboolean `send-request` is a `WebKitWebPage` web-process-extension signal this crate does not bind.
- **Do not name `InstallMissingMediaPluginsPermissionRequest`** in any downcast list — it is `deprecated = "Since 2.40"` and CI runs clippy `-D warnings`, so naming it is a hard CI failure. The `_ => Other` catch-all already denies it.
- **Never call `remove_all_scripts`** — it would destroy wry's own injected bootstrap script living in the same `UserContentManager`. Use `remove_script(&old)` + `add_script(&new)`.
- **Escalation levels:** absent Layer 1 ⇒ `config.error("egress.filter_absent")`; absent Layer 2 ⇒ `config.error("egress.csp_absent")`; per-pattern refusals in either layer ⇒ `config.warn`. **Neither is boot-blocking.**
- **Windows stays byte-unchanged.** No `main.rs` `generate_handler!` edit, no `capabilities/default.json` edit, no Windows clippy job added (P2-F owns CI).

## File Structure

| File | Responsibility |
|---|---|
| `crates/kiosk-core/src/nav/filter.rs` | **new** — `compile_filter`: allowlist → WebKit content-rule JSON, plus the refusal set |
| `crates/kiosk-core/src/nav/allowlist.rs` | gains `Allowlist::origins()`; its `mod tests` gains the corpus implication test and the new `http`/`ws` rows |
| `crates/kiosk-main/src/egress.rs` | gains `#[cfg(not(windows))] mod linux_impl` — the sys-FFI filter shim, install/swap, the observe-only companion |
| `crates/kiosk-main/src/nav_policy.rs` | gains `derive_csp(&Allowlist, ...) -> Option<String>` (the expressibility gate) |
| `crates/kiosk-main/src/hardening.rs` | gains `classify_user_media` + `#[cfg(not(windows))] mod linux_impl` |
| `crates/kiosk-main/src/inject.rs` | `build_injection` gains a third `on_screen_keyboard: bool` parameter; `include_str!`s `keyboard.js` |
| `crates/kiosk-main/src/keyboard.js` | **new** — the bundled OSK, kept **out of `bundled/`** (that dir is the served frontend dist) |
| `crates/kiosk-main/src/main.rs` | `on_download` builder line; keep-awake child; passes `cfg!(target_os = "linux")` to `build_injection` |
| `crates/kiosk-main/src/scheme_guard.rs` | stub message updated (downloads covered by the builder hook; PDF disposition) |
| `crates/kiosk-core/src/config/schema.rs` | new doc comment on `clipboard_read` recording it is unsatisfiable on Linux |

---

### Task 1: `compile_filter` — allowlist to WebKit content rules

**Files:**
- Create: `crates/kiosk-core/src/nav/filter.rs`
- Modify: `crates/kiosk-core/src/nav/mod.rs` (add `pub mod filter;`)
- Modify: `crates/kiosk-core/Cargo.toml` (`[dev-dependencies] regex = "1"` — already in `Cargo.lock`, so no crate joins the graph or the shipped binary)

**Interfaces:**
- Produces: `pub struct FilterOutput { pub json: String, pub refused: Vec<String> }` and `pub fn compile_filter(allow: &Allowlist) -> FilterOutput`
- Consumes: `Allowlist`'s compiled patterns and home URL

**The emitted rule set** is a single block rule with `url-filter` `^(https?|wss?)://`, followed by `ignore-previous-rules` entries for the allowed set. The narrowed block rule is load-bearing three ways: custom-scheme origins (`tauri://`, `kioskasset://`, `ipc://`) never match it, so bundled pages are untouched **whether or not** WebKit applies content rules to custom schemes; hostless URLs (`data:`, `blob:`, `about:`) never match, mirroring `resource_allowed`'s rule 1; and `ws`/`wss` are covered because `resource_allowed` polices them scheme-included.

**Accepted pattern shapes — anything else emits no rule and is `refused`:**

| Component | Accepted | Emitted |
|---|---|---|
| scheme | literal ∈ {`http`,`https`,`ws`,`wss`} | literal |
| host | literal, **or** leading `*.` + literal suffix | `regex::escape(host)` / `[a-z0-9-]+(\.[a-z0-9-]+)*\.` + escaped suffix |
| port | explicit in the pattern, else the scheme default | **exact**: `(:443)?` / `(:80)?` / `:8443` — never `[0-9]+` |
| path / query | — | not compiled (the declared divergence) |

- [ ] **Step 1: Write the failing tests** in `filter.rs`

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::nav::allowlist::Allowlist;

    fn compile(patterns: &[&str], home: &str) -> FilterOutput {
        let owned: Vec<String> = patterns.iter().map(|s| s.to_string()).collect();
        compile_filter(&Allowlist::new(&owned, home))
    }

    #[test]
    fn the_block_rule_is_narrowed_to_network_schemes() {
        let out = compile(&["https://app.example.com/*"], "https://app.example.com/");
        assert!(out.json.contains(r#""url-filter":"^(https?|wss?)://""#));
        assert!(!out.json.contains(r#""url-filter":".*""#));
    }

    #[test]
    fn a_literal_host_emits_an_exact_default_port() {
        let out = compile(&["https://app.example.com/*"], "https://app.example.com/");
        assert!(out.json.contains(r"app\.example\.com(:443)?"));
        // Never a wildcarded port: `(:[0-9]+)?` admits https://app.example.com:8443/,
        // which URLPattern blocks (`the_port_is_pinned_not_wildcarded`).
        assert!(!out.json.contains("[0-9]+"));
    }

    #[test]
    fn an_explicit_port_is_emitted_exactly() {
        let out = compile(&["https://app.example.com:8443/*"], "https://app.example.com:8443/");
        assert!(out.json.contains(":8443"));
    }

    #[test]
    fn a_leading_label_wildcard_expands_to_a_label_class() {
        let out = compile(&["https://*.example.com/*"], "https://app.example.com/");
        assert!(out.json.contains(r"[a-z0-9-]+(\.[a-z0-9-]+)*\.example\.com"));
    }

    /// The three inexpressible shapes are refused, not guessed. No rule ⇒ blocked ⇒
    /// the safe direction. Layer 2's expressibility gate refuses the same three.
    #[test]
    fn inexpressible_shapes_are_refused() {
        for p in [
            "https://api-*.example.com/*",   // mid-label wildcard
            "*://example.com/*",             // non-literal scheme
            "https://:sub.example.com/*",    // named group in the host
        ] {
            let out = compile(&[p], "https://example.com/");
            assert!(out.refused.contains(&p.to_string()), "{p} should be refused");
        }
    }

    /// Rule 2: the exact home URL, widened to its origin — inside the declared
    /// host-granularity divergence, emitted rather than left implicit.
    #[test]
    fn the_home_origin_is_emitted_even_with_a_populated_allowlist() {
        let out = compile(&["https://cdn.example.com/*"], "https://home.test/app");
        assert!(out.json.contains(r"home\.test"));
    }

    /// Rule 3: an empty configured list origin-locks. It does NOT mean allow-all.
    #[test]
    fn an_empty_allowlist_emits_only_the_home_origin() {
        let out = compile(&[], "https://home.test/app");
        assert!(out.json.contains(r"home\.test"));
        assert!(!out.json.contains("example"));
    }

    #[test]
    fn all_four_schemes_are_accepted() {
        for p in ["http://a.test/*", "https://a.test/*", "ws://a.test/*", "wss://a.test/*"] {
            let out = compile(&[p], "https://a.test/");
            assert!(out.refused.is_empty(), "{p} should compile");
        }
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p kiosk-core filter`
Expected: FAIL — module does not exist.

- [ ] **Step 3: Implement `compile_filter`**

Emit a JSON array: first the block rule, then one `ignore-previous-rules` rule per accepted pattern plus one for the home origin. Build each `url-filter` as `^scheme://host(:port)?` per the table. Use `regex::escape`-equivalent escaping written by hand (the `regex` crate is dev-only, so do not call it from library code — escape `.`, `+`, `?`, `*`, `[`, `]`, `(`, `)`, `\`, `^`, `$`, `|`, `{`, `}` yourself in a small `escape_literal` helper, and unit-test it).

Read `Allowlist`'s compiled patterns through their own components; if the accessor needed does not exist yet, add it beside `origins()` in Task 2 rather than reaching into private fields.

Verify each emitted form against WebKit's documented `url-filter` regex subset (spec §Open decisions). A form that is not expressible must fail the compile **loudly** (add it to `refused`, which raises `config.warn`), never silently to allow.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p kiosk-core filter`
Expected: PASS (9 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/kiosk-core/src/nav/filter.rs crates/kiosk-core/src/nav/mod.rs crates/kiosk-core/Cargo.toml
git commit -m "feat(core): compile the allowlist to WebKit content-filter rules"
```

---

### Task 2: `Allowlist::origins()` and the corpus implication test

**Files:**
- Modify: `crates/kiosk-core/src/nav/allowlist.rs` — add `origins()`, add the implication test **inside its own `#[cfg(test)] mod tests`**, add the new corpus rows

**Interfaces:**
- Produces: `pub fn origins(&self) -> Vec<String>` — built from the compiled patterns' own components, so the CSP belt cannot drift tighter than the matcher
- Consumes: `compile_filter` (Task 1) for the implication test

**The soundness claim, stated precisely.** Let `H(u) = (scheme, host, port)`. For every URL `u` the content blocker matches:

> **regex matches `u` ⇒ ∃ an allowlist pattern `p` accepted by the table (or the home URL), with `H(u) ∈ H(AllowSet(p))`.**

One direction only; over `H(u)` only, not full URLs. It is **not** `re.is_match(u) ⇒ allow.allows(u)` — that is the withdrawn full-URL claim and it **cannot pass**, falsified by the path divergence at `allowlist.rs:641` and the home-origin widening at `:387-397`.

- [ ] **Step 1: Add the new corpus rows**

The adversarial battery is https-only today while the compiler and the block rule both accept four schemes. Add to the battery: one `http://` row, one `ws://` row and one `wss://` row, plus explicit rows for the two false-allows found under review:

```rust
// Reproduced under review as false-allows of the rejected permissive forms; kept as
// battery rows so a future compiler change cannot reintroduce either.
("https://*.example.com/*", "https://evil.com\\@x.example.com/steal?d=secret", false),
("https://app.example.com/*", "https://app.example.com:8443/", false),
```

- [ ] **Step 2: Write the failing implication test**

```rust
/// The compiled filter's soundness, in `H(u)` terms. Lives here, beside the battery,
/// so a new battery row reaches this test on the day it is added. `regex` is a
/// dev-dependency used ONLY to evaluate the emitted pattern — WebKit compiles the JSON
/// at runtime; we never match with it.
#[test]
fn every_url_the_filter_matches_shares_scheme_host_port_with_an_accepted_pattern() {
    for (pattern, url, _expected) in BATTERY {
        let allow = Allowlist::new(&[pattern.to_string()], "https://home.test/app");
        let out = crate::nav::filter::compile_filter(&allow);
        for rule_regex in extract_ignore_rule_regexes(&out.json) {
            let re = regex::Regex::new(&rule_regex).expect("emitted regex compiles");
            if re.is_match(url) {
                assert!(
                    same_scheme_host_port(url, pattern) || same_scheme_host_port(url, "https://home.test/app"),
                    "filter matched {url} with no accepted pattern sharing (scheme, host, port)"
                );
            }
        }
    }
}
```

Write `extract_ignore_rule_regexes` and `same_scheme_host_port` as small test helpers in the same module — `same_scheme_host_port` parses both with `url::Url` and compares `(scheme, host_str, port_or_known_default)`.

- [ ] **Step 3: Run test to verify it fails, then passes**

Run: `cargo test -p kiosk-core allowlist`
Expected: FAIL first (no `origins`, no `filter` reference), PASS after Step 4.

- [ ] **Step 4: Implement `origins()`**

```rust
/// The allowlist's origins (`scheme://host[:port]`), built from the compiled patterns'
/// own components — the same source the matcher uses — so a CSP belt derived from this
/// cannot be tighter than the matcher. Home origin included when it parsed.
pub fn origins(&self) -> Vec<String> { /* ... */ }
```

Test it directly too: a literal host, a `*.`-prefixed host, an explicit port, and the empty-allowlist origin-lock case.

- [ ] **Step 5: Commit**

```bash
git add crates/kiosk-core/src/nav/allowlist.rs
git commit -m "feat(core): Allowlist::origins plus the filter soundness implication test"
```

---

### Task 3: CSP derivation with the expressibility gate

**Files:**
- Modify: `crates/kiosk-main/src/nav_policy.rs` — add `derive_csp`
- Modify: `crates/kiosk-core/src/config/schema.rs:89` — new doc comment on `clipboard_read`

**Interfaces:**
- Produces: `pub fn derive_csp(allow: &Allowlist, app_origin: &str, asset_origin: &str) -> Option<String>`
- Consumes: `Allowlist::origins()` (Task 2)

**Why `Option`:** the "looser by construction" property is **withdrawn** — it was false in both halves. URLPattern component accessors return the component's `pattern_string`, and several live entries yield source expressions that are not valid CSP; an unparseable source expression is *ignored* by the CSP parser while the rest still applies, leaving the belt silently **tighter** than the authority. That is verbatim the bug D2b refused to ship (`nav_policy.rs:169-184`).

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn an_inexpressible_pattern_skips_the_whole_belt() {
    for p in ["https://api-*.example.com/*", "*://example.com/*", "https://:sub.example.com/*"] {
        let allow = Allowlist::new(&[p.to_string()], "https://home.test/app");
        assert_eq!(derive_csp(&allow, APP_ORIGIN, ASSET_ORIGIN), None, "{p}");
    }
}

#[test]
fn the_belt_carries_data_and_blob_sources() {
    let allow = Allowlist::new(&["https://cdn.example.com/*".into()], "https://home.test/app");
    let csp = derive_csp(&allow, APP_ORIGIN, ASSET_ORIGIN).expect("expressible");
    assert!(csp.contains("data:"));
    assert!(csp.contains("blob:"));
}

#[test]
fn no_path_component_survives_into_the_belt() {
    let allow = Allowlist::new(&["https://cdn.example.com/assets/*".into()], "https://home.test/app");
    let csp = derive_csp(&allow, APP_ORIGIN, ASSET_ORIGIN).expect("expressible");
    assert!(!csp.contains("/assets"));
}

/// Non-origin dimensions are opened, because the authority does not restrict them: a
/// `default-src` without `'unsafe-inline'` blocks every inline script on an ALLOWLISTED
/// page, which `resource_allowed` does not restrict at all.
#[test]
fn inline_and_eval_are_opened() {
    let allow = Allowlist::new(&["https://cdn.example.com/*".into()], "https://home.test/app");
    let csp = derive_csp(&allow, APP_ORIGIN, ASSET_ORIGIN).expect("expressible");
    assert!(csp.contains("'unsafe-inline'"));
    assert!(csp.contains("'unsafe-eval'"));
}

/// Three deliberate restrictions, declared as a hardening decision rather than
/// derivation output.
#[test]
fn the_three_declared_restrictions_are_present() {
    let allow = Allowlist::new(&["https://cdn.example.com/*".into()], "https://home.test/app");
    let csp = derive_csp(&allow, APP_ORIGIN, ASSET_ORIGIN).expect("expressible");
    assert!(csp.contains("object-src 'none'"));
    assert!(csp.contains("base-uri 'none'"));
    assert!(csp.contains("frame-ancestors 'none'"));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p kiosk-main derive_csp`
Expected: FAIL with "cannot find function `derive_csp`".

- [ ] **Step 3: Implement**

Sources = `Allowlist::origins()` ∪ {content origin, app origin, asset origin} ∪ `data:` ∪ `blob:`. Return `None` when any pattern is inexpressible — the same three shapes Task 1 refuses, so the two layers agree on what they cannot express. Callers emit `config.warn("egress.csp_skipped", pattern)` naming the offender **and** `config.error("egress.csp_absent")`.

Add the new doc comment on `schema.rs:89`:

```rust
/// Clipboard read. **Unsatisfiable on Linux** — webkit2gtk-rs 2.0.2 has no clipboard
/// permission request type at all (the nine `*permission_request.rs` files are the
/// complete set), so clipboard read is always denied there regardless of this value.
/// ponytail: revisit on a bindings/floor bump.
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p kiosk-main nav_policy`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/kiosk-main/src/nav_policy.rs crates/kiosk-core/src/config/schema.rs
git commit -m "feat(linux): CSP belt derivation with an all-or-nothing expressibility gate"
```

---

### Task 4: The content-filter shim — install, swap, degrade

**Files:**
- Modify: `crates/kiosk-main/Cargo.toml` (both Linux deps)
- Modify: `crates/kiosk-main/src/egress.rs` — add `#[cfg(not(windows))] mod linux_impl`

**Interfaces:**
- Consumes: `compile_filter` (Task 1), `Telemetry::{config_error, config_warn}`
- Produces: `install(window, telem, nav_policy)` on Linux; a filter-id handle the `ConfigApplied` path swaps

The safe `add_filter`/`remove_filter` are commented out of webkit2gtk-rs 2.0.2 (`user_content_manager.rs:53,147`) because gir could not bind `WebKitUserContentFilter`; `webkit2gtk-sys` is complete (`lib.rs:5411` store new, `:5467` save, `:5477` save_finish, `:5511` add_filter). Removal uses the **safe** `remove_filter_by_id` (`user_content_manager.rs:154`, `v2_26`).

- [ ] **Step 1: Write the shim**

Store at `data_dir/content-filters/`. Async `save` → `add_filter` on the webview's existing `UserContentManager` through the sys shim. Keep `unsafe` confined to one module with a header comment naming the exact `lib.rs` line for each extern used. Prefer `glib::translate` over raw pointers end-to-end where the crate's own wrappers reach (spec §Open decisions — resolve this at implementation and record which route was taken).

- [ ] **Step 2: Implement the swap and the degrade path**

On every `ConfigApplied`: compile under a **fresh id**, `add_filter`, **then** `remove_filter_by_id` the previous — never a gap with no filter while a page is live.

Every failure — `create_dir_all`, compile, save, `add_filter` — takes `config.error("egress.filter_absent")` exactly once and continues. Refused patterns take `config.warn("egress.filter_pattern", pattern)`. Neither blocks boot.

- [ ] **Step 3: Wire the observe-only companion**

```rust
// `connect_resource_load_started` (web_view.rs:2523, ungated) fires per resource with
// (&WebResource, &URIRequest). Observe-only: enforcement is the filter. Emits the SAME
// label Windows emits (REASON_EGRESS, made pub(crate)) into the SAME nav.blocked rate
// bucket — no second limiter (egress.rs:112-118's doctrine).
```

Connect `WebResource::connect_failed` (`web_resource.rs:118`) and, when `!resource_allowed(uri)`, emit `telem.nav_blocked(REASON_EGRESS, uri)`. Make `REASON_EGRESS` `pub(crate)`.

Whether a content-blocked load reaches this signal at all is **pinned by smoke 8(b)**, not asserted. If it does not, host-scoped blocks are enforced but silent on Linux — record that residual in the spec before merge.

- [ ] **Step 4: Verify it builds**

Run: `cargo build -p kiosk-main && cargo clippy -p kiosk-main --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add crates/kiosk-main/Cargo.toml crates/kiosk-main/src/egress.rs
git commit -m "feat(linux): SEC-10 content filter install/swap plus observe-only egress telemetry"
```

---

### Task 5: `hardening.rs` Linux body

**Files:**
- Modify: `crates/kiosk-main/src/hardening.rs` — add `classify_user_media` (pure), add `#[cfg(not(windows))] mod linux_impl`

**Interfaces:**
- Produces: `fn classify_user_media(audio: bool, video: bool) -> Verdict`, `enum Verdict { Deny, Kind(PermissionKind), Both }`
- Consumes: `permission_allowed`, `SharedNavPolicy`, `Telemetry::config_warn`

| Windows | Linux |
|---|---|
| `SetZoomFactor` | `set_zoom_level` + explicit `set_zoom_text_only(false)` for full-content zoom parity |
| context menus off | `connect_context_menu` → return `true` |
| devtools off | `set_enable_developer_extras(false)` explicitly — belt against a feature-flag mistake |
| autofill/password-save off | documented **no-op** — WebKitGTK ships no such store |
| script dialogs | `connect_script_dialog` → `true` always; `BeforeUnloadConfirm` → `confirm_set_confirmed(true)` (leave the page, matching Windows) |
| printing (H1) | **B14:** `connect_print` → `true` (`web_view.rs:2461`, ungated) |
| `PermissionRequested` | `connect_permission_request` → classify by **runtime type** (WebKit subtypes are GObject classes, not an enum) |

Do **not** mirror Windows' `SCRIPT_DIALOG_BUDGET` — it is an explicit no-op there (`hardening.rs:283-295`); Linux suppresses unconditionally.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn user_media_with_neither_flag_denies_outright() {
    // Display/screen capture, or an unknown request. NOT Camera: a kiosk with
    // camera=true for a video-call page must not thereby grant screen capture.
    assert_eq!(classify_user_media(false, false), Verdict::Deny);
}

#[test]
fn audio_only_is_microphone_and_video_only_is_camera() {
    assert_eq!(classify_user_media(true, false), Verdict::Kind(PermissionKind::Microphone));
    assert_eq!(classify_user_media(false, true), Verdict::Kind(PermissionKind::Camera));
}

/// `PermissionKind` is one-of, so silently picking either for a both-request is a
/// fail-open. Outcome-equivalent to Windows, where WebView2 raises CAMERA and
/// MICROPHONE as separate events, each checked separately.
#[test]
fn audio_and_video_requires_both_permissions() {
    assert_eq!(classify_user_media(true, true), Verdict::Both);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p kiosk-main hardening`
Expected: FAIL with "cannot find function `classify_user_media`".

- [ ] **Step 3: Implement the classifier**

```rust
fn classify_user_media(audio: bool, video: bool) -> Verdict {
    match (audio, video) {
        (false, false) => Verdict::Deny,                          // display/screen capture, unknown
        (true,  false) => Verdict::Kind(PermissionKind::Microphone),
        (false, true ) => Verdict::Kind(PermissionKind::Camera),
        (true,  true ) => Verdict::Both,   // require camera AND microphone
    }
}
```

- [ ] **Step 4: Implement `linux_impl::apply`**

Downcast the permission request by runtime type: `GeolocationPermissionRequest` → `Geolocation`; `NotificationPermissionRequest` → `Notifications`; `UserMediaPermissionRequest` → `classify_user_media(is_for_audio_device(), is_for_video_device())`; everything else → `Other` → deny. Then `request.allow()`/`deny()` (`permission_request.rs:27,34`) and return `true`. Keep the GObject downcasts confined to the signal handler.

Add a module comment recording the **declared assumption pinned by smoke 10/11**: that returning `true` from `connect_context_menu` / `connect_script_dialog` / `connect_permission_request` / `connect_print` suppresses the default handler, and that `confirm_set_confirmed(true)` means "leave the page". The bindings give signatures only.

- [ ] **Step 5: Run tests and clippy**

Run: `cargo test -p kiosk-main hardening && cargo clippy -p kiosk-main --all-targets -- -D warnings`
Expected: PASS, clean. If clippy flags a deprecated permission type, you named `InstallMissingMediaPluginsPermissionRequest` — remove it; the catch-all covers it.

- [ ] **Step 6: Commit**

```bash
git add crates/kiosk-main/src/hardening.rs
git commit -m "feat(linux): hardening settings, dialogs, print suppression and permission policy"
```

---

### Task 6: B13 — the bundled on-screen keyboard

**Files:**
- Create: `crates/kiosk-main/src/keyboard.js`
- Modify: `crates/kiosk-main/src/inject.rs` — third parameter + the appended block
- Modify: `crates/kiosk-main/src/main.rs:1041-1048` — pass `cfg!(target_os = "linux")`

**Interfaces:**
- Produces: `pub fn build_injection(cursor_autohide_seconds: u64, select_text: bool, on_screen_keyboard: bool) -> String`
- Consumes: nothing. **Not RT-16** — a bundled always-on keyboard needs no re-injection path, which is the whole reason the two were separable.

**Platform gating is a parameter, not `cfg!` inside the function**, so both arms stay host-testable on the one job that runs tests. With the flag `false` the emitted string is **byte-identical to today's** — that is the C8 pin, a host assertion rather than a claim.

`keyboard.js` is kept **out of `bundled/`**: that directory is the served frontend dist (`tauri.conf.json:6`), and the keyboard is injected code, not a navigable page.

- [ ] **Step 1: Write the failing tests** in `inject.rs`

```rust
#[test]
fn the_keyboard_block_is_present_only_when_enabled() {
    let with = build_injection(5, false, true);
    let without = build_injection(5, false, false);
    assert!(with.contains("focusin"));
    assert!(!without.contains("focusin"));
}

/// C8: with the flag false the Windows string is unchanged. Pinned as an assertion, not
/// a claim — this is what keeps Windows green.
#[test]
fn the_disabled_arm_is_byte_identical_to_the_two_argument_era() {
    let s = build_injection(5, false, false);
    assert!(s.ends_with("})();\n"));
    assert!(!s.contains("kiosk-osk"));
}

#[test]
fn the_keyboard_sets_its_own_user_select_none_either_way() {
    // With allow_text_selection = true the blanket rule is omitted, so a long-press on a
    // key could start a selection. The keyboard sets it on its own container regardless.
    let s = build_injection(5, true, true);
    assert!(s.contains("user-select"));
}
```

Update the three existing tests (`inject.rs:67-95`) to pass the new third argument `false`.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p kiosk-main inject`
Expected: FAIL — arity mismatch on `build_injection`.

- [ ] **Step 3: Write `keyboard.js`**

Requirements, each load-bearing:

- Markup built with `document.createElement`, direct `.style` writes and `addEventListener` — **no `<style>` element, no inline handler, no external asset, no `data:` URI, no font**. Nothing for a deployed site's CSP to refuse; no dependence on Layer 2 opening `'unsafe-inline'`.
- `focusin`/`focusout` on `document`, **capture phase** (`true`) — `focus`/`blur` do not bubble and would miss every field.
- Show when the target is `<textarea>`, `isContentEditable`, or an `<input>` whose effective type is text-entry (`text`, `search`, `url`, `tel`, `email`, `password`, `number` — **not** `button`/`checkbox`/`radio`/`file`/`range`/`color`/`submit`). Hide on `focusout`. With no focused text field the container is **not in the DOM at all**.
- Keys never take focus: `pointerdown` → `preventDefault()`, and apply the keystroke on that same event.
- Delivery: for `<input>`/`<textarea>` splice at `selectionStart`/`selectionEnd`, restore the caret, dispatch `new InputEvent('input',{bubbles:true})` (and `change` on hide). For contenteditable use `document.execCommand('insertText', …)`.
- Minimum usable set: letters, backspace, shift, a symbols layer. No autocomplete, no IME, no non-Latin layout.
- The container sets `user-select:none` on itself **either way**.

Record the ceiling in a header comment: synthetic events carry `isTrusted === false` and **no `KeyboardEvent` is delivered at all**, so a site gating on trusted key events, or reading `keydown` instead of `input`, will not update. H4b surfaces this per site.

- [ ] **Step 4: Append the block in `build_injection`**

Append **last**, wrapped in its own `try{…}catch(e){}` IIFE, so a defect in it cannot prevent the earlier blocks (selection, drag/drop, print override, autohide) from having already run:

```rust
if on_screen_keyboard {
    script.push_str("try{");
    script.push_str(include_str!("keyboard.js"));
    script.push_str("}catch(e){}\n");
}
```

Then at `main.rs`'s single call site: `inject::build_injection(display.cursor_autohide_seconds, allow_text_selection, cfg!(target_os = "linux"))`.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p kiosk-main inject`
Expected: PASS (3 updated + 3 new).

- [ ] **Step 6: Commit**

```bash
git add crates/kiosk-main/src/keyboard.js crates/kiosk-main/src/inject.rs crates/kiosk-main/src/main.rs
git commit -m "feat(linux): bundled on-screen keyboard injected document-start (B13)"
```

---

### Task 7: Downloads deny and the M4 disposition

**Files:**
- Modify: `crates/kiosk-main/src/main.rs` — the `on_download` builder line
- Modify: `crates/kiosk-main/src/scheme_guard.rs:58-63` — stub message; `pdf_decision`'s recorded reason

**Interfaces:**
- Consumes: `REASON_DOWNLOAD` (`scheme_guard.rs:46`, made `pub(crate)` alongside `REASON_EGRESS`)

- [ ] **Step 1: Add the builder line**

```rust
// Returning false really cancels on Linux: wry-0.55.1/src/webkitgtk/web_context.rs:355-358
// `else { download.cancel(); }`. The cancel lands at `decide-destination`, i.e. after
// response headers — the same point Windows' DownloadStarting fires, so the parity claim
// is derived, not asserted.
#[cfg(not(windows))]
{
    let dl_telem = telem.clone();
    builder = builder.on_download(move |_webview, event| {
        if let tauri::webview::DownloadEvent::Requested { url, .. } = event {
            dl_telem.nav_blocked(scheme_guard::REASON_DOWNLOAD, url.as_str());
        }
        false
    });
}
```

Check `DownloadEvent`'s exact variant shape in `tauri-2.11.5/src/webview/mod.rs:77` and match it; emit the `nav.blocked` **once** per requested download.

- [ ] **Step 2: Update `scheme_guard.rs`'s stub and the PDF reason**

The stub message says downloads are covered by the builder hook and external schemes ride the nav guard (P2-A). `pdf_decision` stays `#[allow(dead_code)]` and host-tested, wired to nothing on either platform; **rewrite its recorded reason** from "descoped, parity" to: *no call site is needed while content is operator-controlled and no engine on either platform exposes a viewer toolbar.* Record the assumption it rests on — the deployment owning its content origin — as inspectable text, **not as a task**: no follow-up item, no owner, no schedule.

- [ ] **Step 3: Verify**

Run: `cargo test -p kiosk-main scheme_guard && cargo clippy -p kiosk-main --all-targets -- -D warnings`
Expected: PASS (the existing `pdf_decision` tests at `:203-226` keep passing untouched).

- [ ] **Step 4: Commit**

```bash
git add crates/kiosk-main/src/main.rs crates/kiosk-main/src/scheme_guard.rs
git commit -m "feat(linux): deny every download at the builder hook; record the PDF disposition"
```

---

### Task 8: Keep-awake via a `systemd-inhibit` child

**Files:**
- Modify: `crates/kiosk-main/src/main.rs:949-966` (beside the Windows `SetThreadExecutionState` block)

**Interfaces:**
- Consumes: `display.keep_awake` (`schema.rs:144`), `Telemetry::config_warn`

- [ ] **Step 1: Implement**

```rust
#[cfg(target_os = "linux")]
if display.keep_awake {
    match std::process::Command::new("systemd-inhibit")
        .args([
            "--what=idle:sleep",
            "--who=kiosk-browser",
            "--why=kiosk display",
            "--mode=block",
            "cat",
        ])
        .stdin(std::process::Stdio::piped())
        .spawn()
    {
        Ok(mut child) => {
            // THIS is what holds the inhibitor open. It must be `let _inhibit_pipe`,
            // never `let _ = child.stdin.take();` — the latter drops the pipe
            // immediately, `cat` gets EOF, and the inhibitor is released within
            // milliseconds while the watcher below dutifully reports it: a control that
            // silently does nothing while looking instrumented.
            //
            // Taking the pipe BEFORE wait() is likewise load-bearing: `Child::wait`
            // closes stdin before waiting, which would kill the very inhibitor being
            // held. Exit symmetry comes from the pipe — kiosk-main dying EOFs it, `cat`
            // exits, logind releases the lock.
            let _inhibit_pipe = child.stdin.take();
            let telem_inhibit = telem.clone();
            std::thread::spawn(move || {
                let status = child.wait();
                telem_inhibit.config_warn(
                    "display.keep_awake",
                    &format!("inhibitor exited: {status:?}"),
                );
            });
        }
        Err(e) => telem.config_warn("display.keep_awake", &format!("spawn failed: {e}")),
    }
}
```

`config.warn`, **not** `eprintln`: best-effort still, but observable — which `eprintln` in a systemd-launched process is not.

- [ ] **Step 2: Record the honest relabel**

Add above the block: under P2-G's runbook this child is defence-in-depth with **no current effect** — P2-G masks the sleep targets, and logind's `IdleAction` is already `ignore` with nothing raising the idle hint. Parent §11's "confirm cage honours idle-inhibit" is answered **negatively**. It is kept because it is the only thing that still functions if an operator unmasks a sleep target, and it costs one `cat`. It is **not** credited as discharging PF-07 / M8 / H5 — the PRIMARY keep-awake is compositor configuration, which is P2-G's image contract.

- [ ] **Step 3: Verify**

Run: `cargo build -p kiosk-main && cargo clippy -p kiosk-main --all-targets -- -D warnings`
Expected: clean. Do **not** add a `try_wait()` liveness check to the health sampler — `health::run` is spawned *before* this block and `interval`'s first tick completes immediately, so the check would inspect a child that does not exist yet.

- [ ] **Step 4: Commit**

```bash
git add crates/kiosk-main/src/main.rs
git commit -m "feat(linux): keep-awake via a systemd-inhibit child with a watcher thread"
```

---

### Task 9: Smoke scenarios 8–9 — egress and downloads

**Files:**
- Modify: `packaging/smoke/run-smoke.sh`
- Create: `packaging/smoke/fixtures/egress.html`, `egress-sw.js`, `paths.html`, `download.html`

- [ ] **Step 1: Scenario 8(a) — the four SEC-10 request classes**

Against an off-allowlist host, each asserted **individually** as blocked: `<img src>`, CSS `url()`, `fetch()`, `navigator.sendBeacon`. **And** a bundled `data:` image renders — the hostless rule the narrowed block rule must not touch.

- [ ] **Step 2: Scenario 8(b) — observability and the path divergence**

On a *second* allowlisted host with a path-scoped pattern: an in-pattern path loads; an **off-pattern path also loads** (the declared looser divergence, asserted so it is pinned rather than assumed); and a blocked off-list request emits exactly one `nav.blocked{egress}`. If a content-blocked load never reaches `resource-load-started`, record in the spec **before merge** that host-scoped blocks are silent on Linux.

- [ ] **Step 3: Scenario 8(c) — service worker**

A service-worker-initiated off-list fetch → blocked. Pins whether WebKit's content-rule engine covers SW-initiated requests.

- [ ] **Step 4: Scenario 8(d) — the degrade path, with no product flag**

The fixture creates `data_dir/content-filters` as a **regular file**, so `create_dir_all` fails for **every uid, including root**. Assert: exactly one `config.error{egress.filter_absent}`; an off-list `fetch()` is **still blocked** by the belt, recorded by the fixture page's own `securitypolicyviolation` listener writing to a DOM node the harness reads; and the kiosk boots and serves.

Do **not** use `chmod 000` (root ignores DAC denial via `CAP_DAC_OVERRIDE`, and root is the only supported principal) and do **not** add a `--no-egress-filter` product flag (shipped code whose only function is to disable the sole SEC-10 control on Linux).

- [ ] **Step 5: Scenario 9 — downloads**

Click a `Content-Disposition: attachment` link → no file appears, exactly one `nav.blocked{download}`, kiosk stays on the page. **Capture the load-event sequence** and record it against P2-A's policy-cancellation filter question: it must not be looser than Windows, where a cancelled download's `NavigationCompleted(IsSuccess=false)`, if it fires, legitimately reaches the FSM. If Linux swallows an event Windows delivers, record the resolution in the smoke README.

- [ ] **Step 6: Run and commit**

Run: `bash packaging/smoke/run-smoke.sh`
Expected: 8(a)–(d) and 9 PASS.

```bash
git add packaging/smoke
git commit -m "test(linux): smoke 8-9 — egress layers, degrade path, downloads"
```

---

### Task 10: Smoke scenarios 10–12 and the A re-run

**Files:**
- Modify: `packaging/smoke/run-smoke.sh`
- Create: `packaging/smoke/fixtures/dialogs.html`, `keyboard.html`, `print-iframe.html`, `permissions.html`

- [ ] **Step 1: Scenario 10 (a)–(c) — dialogs and chrome**

An `alert()`-loop page does not wedge the kiosk and paints nothing; right-click produces no context menu; a `beforeunload` page navigates away **without prompting**. All three asserted explicitly — this is the pin for the return-value / `confirm_set_confirmed` assumption.

- [ ] **Step 2: Scenario 10(d) — the keyboard**

A page with one `<input type="text">`: focus it → the keyboard's container is in the DOM; click a key → the input's `value` gained that character and one `input` event fired; blur → the container is gone. Fails if the block is not injected, if the focus predicate is wrong, if a key steals focus, or if the CSP-independence claim is wrong.

- [ ] **Step 3: Scenario 10(e) — print**

A page that creates an `about:blank` iframe and calls `iframe.contentWindow.print()` → **no print dialog paints** and the kiosk stays on the page. Written this way deliberately: calling `window.print()` from the main document cannot fail (the override is non-writable, non-configurable), so it would assert nothing about `connect_print`.

- [ ] **Step 4: Scenario 11 — permissions**

A `geolocation.getCurrentPosition` + `getUserMedia` probe page → denied under default-deny, **and allowed** when the fixture config flips `permissions.camera`. The positive arm is what proves our handler is the one deciding — an unhandled request would default-deny silently and identically.

- [ ] **Step 5: Scenario 12 — keep-awake**

*Preconditions* (properties of the bus-less container, not assertions): spawn succeeds, and the child exits non-zero. *Assertion:* exactly one `config.warn{display.keep_awake}` carrying the child's exit status. *Non-regression:* the kiosk is unaffected. The positive hold assertion belongs to P2-G's hardware checklist.

- [ ] **Step 6: Re-run A's scenarios 1–7 with the filter installed**

Named here as the pin for the custom-scheme assumption, not a generic regression sweep: scenario 3 (bundled offline page) and scenario 7 (`safe.html` from the app origin) carry it. If either fails, the `http://` app-origin form is matching the block rule — apply the recorded one-line ignore rule `^https?://(tauri|kioskasset|ipc)\.localhost/`.

- [ ] **Step 7: Run the full gate and commit**

Run: `bash packaging/smoke/run-smoke.sh`
Expected: 1–12 PASS.

```bash
git add packaging/smoke
git commit -m "test(linux): smoke 10-12 plus A's 1-7 re-run under the content filter"
```

---

## Self-Review

**Spec coverage:** Layer 1 compiler → T1; soundness test + `origins()` → T2; Layer 2 belt → T3; shim/install/swap/degrade + observe-only companion → T4; `hardening.rs` incl. B14 → T5; B13 → T6; downloads + M4 → T7; keep-awake → T8; smoke 8–12 + A re-run → T9, T10; `clipboard_read` doc → T3.

**Open decisions carried into implementation, each with a landing place:** the exact sys-FFI shim shape (T4 Step 1 — record which route was taken); the `url-filter` regex dialect limits (T1 Step 3 — a non-expressible form fails loudly, never silently to allow); config-apply → filter/belt refresh ordering against the driver (T4 Step 2).

**Not covered, deliberately:** RT-16 `inject_css`/`inject_js` (deferred out of P2 by the owner; B13 does not depend on it); the Windows half of PF-02; M4/OD-8 enforcement (not a live control for this deployment — no code, no gate, no follow-up item).
