//! The URL allowlist matcher (spec §3.6; adversarial battery RT-03).
//!
//! Read the module docs in [`super`] first — in particular, this is *not* an exfiltration
//! boundary (SEC-10).

use url::Url;
use urlpattern::quirks::{process_construct_pattern_input, StringOrInit};
use urlpattern::{UrlPattern, UrlPatternMatchInput, UrlPatternOptions};

use super::{BlockReason, Decision};

/// Decides whether a top-level (main-frame) navigation may proceed.
///
/// Four rules, in the order [`Allowlist::allows`] applies them:
///
/// 1. **Default-deny on parse failure.** A URL `url::Url` cannot parse is
///    [`BlockReason::Unparseable`]. There is no string-matching fallback — the classic
///    bypasses all rely on one.
/// 2. **The home URL is always implicitly allowed** (cfg-02), so a mis-typed allowlist can
///    never self-block the initial navigation. The caller passes the home URL *already*
///    expanded through [`crate::identity::expand_device_id_template`]; this type does not
///    expand it.
/// 3. **An empty allowlist origin-locks** to the home URL's origin — scheme + host + port,
///    all paths (arch-08). It emphatically does **not** mean "allow everything": a signed
///    `{}` config leaves the device locked to its home origin.
/// 4. Otherwise the URL must match one compiled **URLPattern** (spec §3.6 — URLPattern
///    semantics, not globs), matched on parsed components.
#[derive(Debug)]
pub struct Allowlist {
    /// Compiled patterns. Only the ones that compiled — see `invalid`.
    patterns: Vec<UrlPattern>,
    /// The effective home URL, if it parsed. `None` ⇒ no implicit home allow and no origin
    /// lock, i.e. everything is denied. That is the safe direction.
    home: Option<Url>,
    /// arch-08. True iff the *configured* list was empty. Deliberately keyed to the list as
    /// given, not to `patterns.is_empty()`: if an operator configured patterns and every
    /// one of them failed to compile, widening to the home origin would invent a policy
    /// nobody wrote. In that case only the home URL loads, loudly and safely.
    origin_locked: bool,
    /// Patterns that failed to compile. They are **dropped** (they can never match), and
    /// surfaced here so the config layer can raise a `config.error` for the operator rather
    /// than failing silently.
    invalid: Vec<String>,
}

impl Allowlist {
    /// `home_url` is `content.url` **after** `{device_id}` expansion (spec cfg-02).
    pub fn new(patterns: &[String], home_url: &str) -> Self {
        let mut compiled = Vec::with_capacity(patterns.len());
        let mut invalid = Vec::new();
        for p in patterns {
            match compile(p) {
                Ok(pattern) => compiled.push(pattern),
                Err(_) => invalid.push(p.clone()),
            }
        }

        Self {
            patterns: compiled,
            home: Url::parse(home_url).ok(),
            origin_locked: patterns.is_empty(),
            invalid,
        }
    }

    /// The configured patterns that could not be compiled and are therefore inert.
    pub fn invalid_patterns(&self) -> &[String] {
        &self.invalid
    }

    /// The verdict for one top-level navigation.
    pub fn allows(&self, url: &str) -> Decision {
        // Rule 1. Parse first. Everything below matches on *components* of this `Url`;
        // nothing in this function ever looks at `url` the string again.
        let Ok(candidate) = Url::parse(url) else {
            return Decision::Block(BlockReason::Unparseable);
        };

        if let Some(home) = &self.home {
            // Rule 2 (cfg-02). `Url`'s equality is on the parsed, normalised form, so this
            // is insensitive to host case and default ports but exact on path/query/fragment.
            if *home == candidate {
                return Decision::Allow;
            }
            // Rule 3 (arch-08).
            if self.origin_locked && same_origin(home, &candidate) {
                return Decision::Allow;
            }
        }

        // Rule 4.
        for pattern in &self.patterns {
            // `unwrap_or(false)`: a matcher error is a denial, never an allow.
            let matched = pattern
                .test(UrlPatternMatchInput::Url(candidate.clone()))
                .unwrap_or(false);
            if matched {
                return Decision::Allow;
            }
        }

        Decision::Block(BlockReason::NotAllowlisted)
    }
}

/// Compile one allowlist entry into a [`UrlPattern`].
///
/// Two deliberate choices:
///
/// * **No base URL.** Every allowlist entry must carry an explicit scheme. Without a base
///   URL, `urlpattern` rejects a schemeless pattern outright — which is exactly what makes
///   `*`, `//evil.com/*` and `/kiosk/*` *uncompilable* rather than dangerously broad. A
///   base URL would instead have them silently inherit the home scheme/host.
/// * We go through `quirks::process_construct_pattern_input` rather than
///   `UrlPatternInit::parse_constructor_string::<R>`, because the latter's `R: RegExp` bound
///   names a trait the crate does not export, so it cannot be called without also taking a
///   direct dependency on `regex` purely to name `regex::Regex`. The `quirks` helper is the
///   crate's own public wrapper around the identical call.
fn compile(pattern: &str) -> Result<UrlPattern, urlpattern::Error> {
    let init = process_construct_pattern_input(StringOrInit::String(pattern.to_owned()), None)?;
    <UrlPattern>::parse(init, UrlPatternOptions::default())
}

/// Web-origin equality: scheme + host + port, compared on parsed components.
///
/// The spec phrases the origin lock as "scheme+host, all paths"; we also pin the port,
/// because an origin *is* scheme+host+port and pinning it is the default-deny direction.
/// It cannot brick a deployment: the home URL's own port is what gets pinned, and the home
/// URL is allowed outright by rule 2 regardless.
///
/// A URL with no host (`data:`, `blob:`, `javascript:`, `about:`) has no origin to lock to,
/// so it never matches — again, the safe direction.
fn same_origin(home: &Url, candidate: &Url) -> bool {
    match (home.host(), candidate.host()) {
        (Some(h), Some(c)) => {
            h == c
                && home.scheme() == candidate.scheme()
                && home.port_or_known_default() == candidate.port_or_known_default()
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use url::Url;

    const HOME: &str = "https://app.example.com/kiosk";
    const PAT: &str = "https://app.example.com/*";

    fn allowlist(patterns: &[&str], home: &str) -> Allowlist {
        let owned: Vec<String> = patterns.iter().map(|s| (*s).to_string()).collect();
        Allowlist::new(&owned, home)
    }

    /// Every `Block` in this file is this one. Spelling it out keeps the asserts readable.
    const BLOCKED: Decision = Decision::Block(BlockReason::NotAllowlisted);
    const UNPARSEABLE: Decision = Decision::Block(BlockReason::Unparseable);

    // ---------------------------------------------------------------------------------
    // The adversarial battery (spec §10, RT-03).
    //
    // Each of these works by making the URL *look* like it carries the allowlisted host
    // while parsing to a different one. Where that is the point of the test, the test
    // first asserts the *premise* — what the URL actually parses to — so that a reader
    // can see why a substring check would be fooled.
    // ---------------------------------------------------------------------------------

    #[test]
    fn host_suffix_attack_is_blocked() {
        // `app.example.com.evil.com` starts with the allowlisted host. A substring check
        // on the raw URL says yes; anchored component matching says no.
        let a = allowlist(&[PAT], HOME);
        assert_eq!(a.allows("https://app.example.com.evil.com/"), BLOCKED);
        assert_eq!(a.allows("https://app.example.com.evil.com/kiosk"), BLOCKED);
    }

    #[test]
    fn host_prefix_attack_is_blocked() {
        // No dot boundary: `evilapp.example.com` is a different host that *ends* with the
        // allowlisted one's tail.
        let a = allowlist(&[PAT], HOME);
        assert_eq!(a.allows("https://evilapp.example.com/"), BLOCKED);
        assert_eq!(a.allows("https://not-app.example.com/"), BLOCKED);
    }

    #[test]
    fn userinfo_attack_is_blocked() {
        let a = allowlist(&[PAT], HOME);

        // Premise: the allowlisted string is only the *username*; the host is evil.com.
        let u = Url::parse("https://app.example.com@evil.com/").expect("parses");
        assert_eq!(u.username(), "app.example.com");
        assert_eq!(u.host_str(), Some("evil.com"));

        assert_eq!(a.allows("https://app.example.com@evil.com/"), BLOCKED);
        // …and with a password, and with a path that also name-drops the host.
        assert_eq!(a.allows("https://app.example.com:pw@evil.com/"), BLOCKED);
        assert_eq!(
            a.allows("https://app.example.com@evil.com/app.example.com/"),
            BLOCKED
        );
    }

    #[test]
    fn scheme_downgrade_is_blocked() {
        let a = allowlist(&[PAT], HOME);
        assert_eq!(a.allows("http://app.example.com/"), BLOCKED);
        assert_eq!(a.allows("http://app.example.com/kiosk"), BLOCKED);
    }

    #[test]
    fn path_traversal_cannot_escape_the_patterns_path_constraint() {
        let a = allowlist(&["https://app.example.com/kiosk/*"], HOME);

        // Premise: the url crate resolves the dot-segments, so the real path is /etc/passwd
        // — outside the pattern's /kiosk/ constraint.
        let u = Url::parse("https://app.example.com/kiosk/../../etc/passwd").expect("parses");
        assert_eq!(u.path(), "/etc/passwd");

        assert_eq!(
            a.allows("https://app.example.com/kiosk/../../etc/passwd"),
            BLOCKED
        );
        // A path *inside* the constraint still works.
        assert_eq!(
            a.allows("https://app.example.com/kiosk/page"),
            Decision::Allow
        );
    }

    #[test]
    fn embedded_url_in_query_is_blocked() {
        // The allowlisted origin appears in the query of an attacker-controlled host.
        let a = allowlist(&[PAT], HOME);
        assert_eq!(
            a.allows("https://evil.com/?next=https://app.example.com/"),
            BLOCKED
        );
        assert_eq!(
            a.allows("https://evil.com/#https://app.example.com/"),
            BLOCKED
        );
    }

    #[test]
    fn unparseable_urls_are_blocked_and_never_string_matched() {
        let a = allowlist(&[PAT], HOME);
        for bad in [":::", "", "   ", "not a url", "https://", "http://[::1"] {
            assert_eq!(a.allows(bad), UNPARSEABLE, "input {bad:?}");
        }
    }

    #[test]
    fn scheme_relative_spellings_are_resolved_not_rejected() {
        // WHATWG lets a *special* scheme omit the `//`, so `https:app.example.com` is a
        // perfectly valid URL that really does navigate to `https://app.example.com/`.
        // Allowing it is therefore correct — it IS the allowlisted host. This is precisely
        // where a string matcher goes wrong in *both* directions: it would reject the
        // benign spelling and, worse, accept the hostile one below.
        let a = allowlist(&[PAT], HOME);

        for spelling in [
            "https:app.example.com",
            "https:/app.example.com",
            "https:///app.example.com",
        ] {
            let u = Url::parse(spelling).expect("special schemes tolerate a missing //");
            assert_eq!(u.host_str(), Some("app.example.com"), "{spelling:?}");
            assert_eq!(a.allows(spelling), Decision::Allow, "{spelling:?}");
        }

        // The same spelling pointed at an attacker host stays blocked…
        assert_eq!(a.allows("https:evil.com"), BLOCKED);
        // …and so does the userinfo trick wearing it.
        assert_eq!(a.allows("https:app.example.com@evil.com"), BLOCKED);
    }

    // ---- IDN / punycode: both directions must be right ------------------------------
    //
    // A false BLOCK bricks a legitimate IDN deployment; a false ALLOW on a homoglyph is a
    // security hole. Both are tested.

    #[test]
    fn idn_unicode_pattern_matches_the_same_hosts_punycode_url() {
        let a = allowlist(&["https://münchen.example.com/*"], HOME);
        assert_eq!(
            a.allows("https://xn--mnchen-3ya.example.com/page"),
            Decision::Allow
        );
    }

    #[test]
    fn idn_punycode_pattern_matches_the_same_hosts_unicode_url() {
        let a = allowlist(&["https://xn--mnchen-3ya.example.com/*"], HOME);
        assert_eq!(
            a.allows("https://münchen.example.com/page"),
            Decision::Allow
        );
    }

    #[test]
    fn idn_unicode_pattern_matches_unicode_url() {
        let a = allowlist(&["https://münchen.example.com/*"], HOME);
        assert_eq!(
            a.allows("https://münchen.example.com/page"),
            Decision::Allow
        );
    }

    #[test]
    fn cyrillic_homoglyph_url_is_blocked() {
        let a = allowlist(&[PAT], HOME);
        // `аpp` — U+0430 CYRILLIC SMALL LETTER A, not ASCII 'a'.
        let homoglyph = "https://\u{0430}pp.example.com/";

        // Premise: it punycodes to a demonstrably *different* host.
        let u = Url::parse(homoglyph).expect("parses");
        assert_eq!(u.host_str(), Some("xn--pp-6kc.example.com"));

        assert_eq!(a.allows(homoglyph), BLOCKED);
    }

    #[test]
    fn cyrillic_homoglyph_pattern_does_not_match_the_ascii_host() {
        // The mirror image: an operator who pastes a homoglyph into the allowlist does not
        // thereby allow the real ASCII host.
        let a = allowlist(&["https://\u{0430}pp.example.com/*"], HOME);
        assert_eq!(a.allows("https://app.example.com/x"), BLOCKED);
    }

    // ---- Patterns crossing `?`, `#`, `/` (spec §3.6) ---------------------------------

    #[test]
    fn a_path_appearing_only_in_the_query_does_not_satisfy_a_path_pattern() {
        let a = allowlist(&["https://app.example.com/kiosk/*"], HOME);
        assert_eq!(
            a.allows("https://app.example.com/other?redirect=/kiosk/x"),
            BLOCKED
        );
    }

    #[test]
    fn a_path_appearing_only_in_the_fragment_does_not_satisfy_a_path_pattern() {
        let a = allowlist(&["https://app.example.com/kiosk/*"], HOME);
        assert_eq!(a.allows("https://app.example.com/other#/kiosk/x"), BLOCKED);
    }

    #[test]
    fn a_pattern_with_no_query_constraint_admits_any_query_and_fragment() {
        let a = allowlist(&["https://app.example.com/kiosk/*"], HOME);
        assert_eq!(
            a.allows("https://app.example.com/kiosk/page?x=1#top"),
            Decision::Allow
        );
    }

    #[test]
    fn an_explicit_query_in_the_pattern_constrains_the_query() {
        let a = allowlist(&["https://app.example.com/p?mode=kiosk"], HOME);
        assert_eq!(
            a.allows("https://app.example.com/p?mode=kiosk"),
            Decision::Allow
        );
        assert_eq!(a.allows("https://app.example.com/p?mode=admin"), BLOCKED);
        assert_eq!(a.allows("https://app.example.com/p"), BLOCKED);
    }

    #[test]
    fn a_trailing_slash_wildcard_matches_nested_paths_and_the_bare_root() {
        let a = allowlist(&[PAT], HOME);
        assert_eq!(a.allows("https://app.example.com/"), Decision::Allow);
        assert_eq!(a.allows("https://app.example.com/a/b/c"), Decision::Allow);
    }

    // ---- cfg-02: the home URL is always implicitly allowed ---------------------------

    #[test]
    fn home_url_is_allowed_even_when_the_allowlist_would_block_it() {
        // A mis-typed allowlist must never self-block the *initial* navigation.
        let a = allowlist(&["https://somewhere-else.example/*"], HOME);
        assert_eq!(a.allows(HOME), Decision::Allow);
    }

    #[test]
    fn a_populated_allowlist_does_not_implicitly_allow_the_home_origin() {
        // Only the *exact* home URL is implicit — not its whole origin.
        let a = allowlist(&["https://somewhere-else.example/*"], HOME);
        assert_eq!(a.allows("https://app.example.com/other"), BLOCKED);
        assert_eq!(a.allows("https://app.example.com/"), BLOCKED);
        // …and what the patterns *do* say still holds.
        assert_eq!(
            a.allows("https://somewhere-else.example/page"),
            Decision::Allow
        );
    }

    // ---- arch-08: an empty allowlist origin-locks; it does NOT open the device --------

    #[test]
    fn empty_allowlist_origin_locks_to_the_home_origin() {
        let a = allowlist(&[], HOME);
        // All paths on the home origin.
        assert_eq!(a.allows(HOME), Decision::Allow);
        assert_eq!(
            a.allows("https://app.example.com/other/path?q=1#f"),
            Decision::Allow
        );
        // Anything else is denied — an empty allowlist is a lock, not an opening.
        assert_eq!(a.allows("https://evil.com/"), BLOCKED);
        assert_eq!(a.allows("http://app.example.com/"), BLOCKED);
        assert_eq!(a.allows("https://app.example.com.evil.com/"), BLOCKED);
        assert_eq!(a.allows("https://app.example.com@evil.com/"), BLOCKED);
    }

    #[test]
    fn a_signed_empty_config_stays_origin_locked_rather_than_open() {
        // The `{}` document: `Content::default()` carries an empty allowlist.
        let content = crate::config::schema::Content::default();
        assert!(
            content.allowlist.is_empty(),
            "premise: {{}} has no allowlist"
        );

        let a = Allowlist::new(&content.allowlist, HOME);
        assert_eq!(
            a.allows("https://app.example.com/anything"),
            Decision::Allow
        );
        assert_eq!(a.allows("https://evil.com/"), BLOCKED);
    }

    #[test]
    fn an_unparseable_home_denies_everything_rather_than_opening_up() {
        let a = allowlist(&[], ":::");
        assert_eq!(a.allows("https://evil.com/"), BLOCKED);
        assert_eq!(a.allows("https://app.example.com/"), BLOCKED);
        assert_eq!(a.allows(":::"), UNPARSEABLE);
    }

    // ---- An uncompilable pattern is DROPPED, never "match everything" -----------------

    #[test]
    fn a_pattern_with_no_scheme_is_dropped_not_treated_as_match_all() {
        // `*` and `//evil.com/*` carry no protocol. Compiled with no base URL they do not
        // compile at all — and a dropped pattern must never become "allow everything".
        for bad in ["*", "//evil.com/*", "", "/kiosk/*"] {
            let a = allowlist(&[bad], HOME);
            assert_eq!(a.invalid_patterns(), [bad.to_string()], "pattern {bad:?}");
            assert_eq!(a.allows("https://evil.com/"), BLOCKED, "pattern {bad:?}");
            // The home URL still loads (cfg-02) — the kiosk fails *safe*, not *dead*.
            assert_eq!(a.allows(HOME), Decision::Allow, "pattern {bad:?}");
        }
    }

    #[test]
    fn a_configured_but_all_invalid_allowlist_does_not_fall_back_to_the_origin_lock() {
        // The operator *did* configure an allowlist, so arch-08's "absent/empty" rule does
        // not apply. Silently widening to the home origin would invent policy nobody wrote.
        let a = allowlist(&["*"], HOME);
        assert_eq!(a.allows("https://app.example.com/other"), BLOCKED);
    }

    #[test]
    fn valid_patterns_survive_alongside_invalid_ones() {
        let a = allowlist(&["*", PAT], HOME);
        assert_eq!(a.invalid_patterns(), ["*".to_string()]);
        assert_eq!(a.allows("https://app.example.com/x"), Decision::Allow);
        assert_eq!(a.allows("https://evil.com/"), BLOCKED);
    }

    #[test]
    fn multiple_patterns_are_all_consulted() {
        let a = allowlist(&[PAT, "https://cdn.example.com/assets/*"], HOME);
        assert_eq!(a.allows("https://app.example.com/x"), Decision::Allow);
        assert_eq!(
            a.allows("https://cdn.example.com/assets/x.png"),
            Decision::Allow
        );
        assert_eq!(a.allows("https://cdn.example.com/other"), BLOCKED);
    }

    // =================================================================================
    // Step 5: inputs that are NOT in the spec's battery, found by attacking the matcher
    // after it went green. Each one is pinned here so the behaviour cannot drift.
    // =================================================================================

    /// WHATWG normalises several *spellings* onto the same real host: `\` is a `/` for
    /// special schemes, tab/CR/LF are stripped, `%2e` is a dot, and U+3002 IDEOGRAPHIC
    /// FULL STOP is a label separator. Each of these therefore genuinely lands on the
    /// allowlisted host, and allowing them is correct.
    ///
    /// The point of the test is the **pairing**: for every benign spelling there is a
    /// hostile twin that puts the *same bytes* in front of a different host, and the twin
    /// must block. A string matcher gets at least one side of every pair wrong.
    #[test]
    fn whatwg_spelling_tricks_resolve_to_the_real_host_in_both_directions() {
        let a = allowlist(&[PAT], HOME);

        // Benign: the authority really is app.example.com.
        for (label, url, host) in [
            // The backslash TERMINATES the authority, so `@evil.com` is a *path*, not a host.
            (
                "backslash",
                r"https://app.example.com\@evil.com",
                "app.example.com",
            ),
            (
                "backslash authority",
                r"https:\\app.example.com\",
                "app.example.com",
            ),
            (
                "tab stripped",
                "https://app.exam\tple.com/",
                "app.example.com",
            ),
            (
                "pct-encoded dots",
                "https://app%2eexample%2ecom/",
                "app.example.com",
            ),
            (
                "ideographic dot",
                "https://app\u{3002}example.com/",
                "app.example.com",
            ),
            (
                "double-slash path",
                "https://app.example.com//evil.com",
                "app.example.com",
            ),
            (
                "fragment",
                "https://app.example.com#@evil.com",
                "app.example.com",
            ),
        ] {
            let u = Url::parse(url).unwrap_or_else(|e| panic!("{label}: {e}"));
            assert_eq!(u.host_str(), Some(host), "{label}: premise");
            assert_eq!(a.allows(url), Decision::Allow, "{label}");
        }

        // Hostile twins: the same tricks aimed at a host that is NOT allowlisted.
        for (label, url) in [
            ("backslash to evil", r"https://evil.com\@app.example.com"),
            ("newline splice", "https://app.example.com\n@evil.com/"),
            (
                "ideographic dot suffix",
                "https://app.example.com\u{3002}evil.com/",
            ),
            (
                "port-shaped userinfo",
                "https://app.example.com:8443@evil.com/",
            ),
            (
                "allowlisted host in path",
                "https://evil.com/@app.example.com/",
            ),
        ] {
            assert_eq!(a.allows(url), BLOCKED, "{label}");
        }
    }

    #[test]
    fn a_trailing_dot_host_is_blocked() {
        // `app.example.com.` is the fully-qualified spelling of the same DNS name, but the
        // url crate keeps the dot, so it is a *different* host string and does not match.
        // We block: that is the fail-safe direction, and it is pinned here so it cannot
        // silently flip to Allow (which is the direction that would be a bypass).
        let a = allowlist(&[PAT], HOME);
        let u = Url::parse("https://app.example.com./").expect("parses");
        assert_eq!(u.host_str(), Some("app.example.com."));
        assert_eq!(a.allows("https://app.example.com./"), BLOCKED);
        assert_eq!(a.allows("https://app.example.com.evil.com./"), BLOCKED);
    }

    #[test]
    fn host_case_folds_but_path_case_does_not() {
        // DNS is case-insensitive, URL paths are not. Both directions matter: folding the
        // path would let `/ADMIN` through a `/admin`-shaped pattern on a case-insensitive
        // origin, and *not* folding the host would block a legitimate `HTTPS://APP…`.
        let a = allowlist(&["https://app.example.com/kiosk/*"], HOME);
        assert_eq!(a.allows("https://APP.EXAMPLE.COM/kiosk/x"), Decision::Allow);
        assert_eq!(a.allows("https://app.example.com/KIOSK/x"), BLOCKED);
        // An uppercase host in the *pattern* folds the same way.
        let up = allowlist(&["https://APP.EXAMPLE.COM/*"], HOME);
        assert_eq!(up.allows("https://app.example.com/x"), Decision::Allow);
    }

    #[test]
    fn the_port_is_pinned_not_wildcarded() {
        // URLPattern's constructor-string parser sets an absent port to "" (the default
        // port), it does NOT leave it as a wildcard. So a pattern written without a port
        // admits only the scheme's default port — proper origin semantics.
        let a = allowlist(&[PAT], HOME);
        assert_eq!(a.allows("https://app.example.com:443/"), Decision::Allow);
        assert_eq!(a.allows("https://app.example.com:8443/"), BLOCKED);

        // And a pattern that *names* a port admits only that port.
        let p = allowlist(&["https://app.example.com:8443/*"], HOME);
        assert_eq!(p.allows("https://app.example.com:8443/x"), Decision::Allow);
        assert_eq!(p.allows("https://app.example.com/x"), BLOCKED);
        assert_eq!(p.allows("https://app.example.com:9443/x"), BLOCKED);
    }

    #[test]
    fn opaque_and_local_schemes_are_blocked() {
        let a = allowlist(&[PAT], HOME);
        for url in [
            "data:text/html,<script>alert(1)</script>",
            "javascript:alert(1)",
            "about:blank",
            "file:///etc/passwd",
            "mailto:a@b.com",
            "kiosk://safe-mode",
            // These two carry the allowlisted origin *inside* them. A substring matcher
            // allows both; we block, because the outer scheme is not https.
            "blob:https://app.example.com/uuid-1234",
            "view-source:https://app.example.com/",
        ] {
            assert_eq!(a.allows(url), BLOCKED, "{url:?}");
        }
    }

    #[test]
    fn percent_encoded_path_traversal_cannot_escape_either() {
        // The url crate decodes %2e before resolving dot-segments, so the encoded form
        // normalises to exactly the same path as the plain one and is blocked with it.
        let a = allowlist(&["https://app.example.com/kiosk/*"], HOME);
        for url in [
            "https://app.example.com/kiosk/../../etc/passwd",
            "https://app.example.com/kiosk/%2e%2e/%2e%2e/etc/passwd",
            "https://app.example.com/kiosk/%2E%2E/%2E%2E/etc/passwd",
        ] {
            assert_eq!(Url::parse(url).unwrap().path(), "/etc/passwd", "{url:?}");
            assert_eq!(a.allows(url), BLOCKED, "{url:?}");
        }
        // A sibling path that merely *starts with* the constrained segment is not inside it.
        assert_eq!(a.allows("https://app.example.com/kiosk-admin/x"), BLOCKED);
    }

    #[test]
    fn an_over_long_host_is_blocked_without_panicking() {
        let a = allowlist(&[PAT], HOME);
        let long = format!("https://{}.example.com/", "a".repeat(300));
        assert_eq!(a.allows(&long), BLOCKED);
    }

    #[test]
    fn a_wildcard_host_pattern_still_resists_the_suffix_and_userinfo_tricks() {
        let a = allowlist(&["https://*.example.com/*"], HOME);
        assert_eq!(a.allows("https://app.example.com/x"), Decision::Allow);
        assert_eq!(a.allows("https://a.b.example.com/x"), Decision::Allow);
        assert_eq!(a.allows("https://app.example.com.evil.com/"), BLOCKED);
        assert_eq!(a.allows("https://app.example.com@evil.com/"), BLOCKED);
        assert_eq!(a.allows("https://evilexample.com/x"), BLOCKED);
        // `*.` requires the dot, so the bare apex is not included.
        assert_eq!(a.allows("https://example.com/x"), BLOCKED);
    }

    #[test]
    fn the_origin_lock_is_an_exact_origin_not_a_domain_suffix() {
        let a = allowlist(&[], HOME);
        // A *subdomain* of the home host is a different origin.
        assert_eq!(a.allows("https://sub.app.example.com/"), BLOCKED);
        assert_eq!(a.allows("https://app.example.com:8443/"), BLOCKED);
        assert_eq!(a.allows("https://app.example.com:443/x"), Decision::Allow);
        assert_eq!(a.allows("https://APP.EXAMPLE.COM/x"), Decision::Allow);
        // Hostless schemes have no origin to lock to.
        assert_eq!(a.allows("blob:https://app.example.com/u"), BLOCKED);
        assert_eq!(a.allows("data:text/html,hi"), BLOCKED);
    }

    #[test]
    fn the_origin_lock_compares_canonical_hosts_not_strings() {
        // An IPv6 home: the expanded and compressed spellings are the same host.
        let a = allowlist(&[], "https://[::1]:8443/kiosk");
        assert_eq!(a.allows("https://[::1]:8443/other"), Decision::Allow);
        assert_eq!(
            a.allows("https://[0:0:0:0:0:0:0:1]:8443/other"),
            Decision::Allow
        );
        assert_eq!(a.allows("https://[::1]:9/other"), BLOCKED);
    }

    #[test]
    fn a_host_only_pattern_covers_the_whole_host() {
        // No pathname in the pattern ⇒ the pathname component defaults to `*`. An operator
        // writing a bare origin gets every path on it — scoped to the host they named.
        let a = allowlist(&["https://app.example.com"], HOME);
        assert!(a.invalid_patterns().is_empty());
        assert_eq!(a.allows("https://app.example.com/"), Decision::Allow);
        assert_eq!(
            a.allows("https://app.example.com/deep/path"),
            Decision::Allow
        );
        assert_eq!(a.allows("https://evil.com/"), BLOCKED);
    }

    #[test]
    fn a_wildcard_protocol_pattern_is_as_wide_as_it_looks() {
        // `*://*/*` means what it says: every scheme, every host. It even admits `file:`.
        // Pinned as a WARNING, not an endorsement — config validation should reject a
        // wildcard *protocol* component, because it silently un-kiosks the device. It is
        // not reachable by the threat model's adversary (the user at the kiosk cannot
        // author signed config), but it is a live operator footgun.
        let wide = allowlist(&["*://*/*"], HOME);
        assert_eq!(wide.allows("https://evil.com/x"), Decision::Allow);
        assert_eq!(wide.allows("file:///etc/passwd"), Decision::Allow);

        // Pinning the scheme is enough to keep `file:` and friends out.
        let https_only = allowlist(&["https://*/*"], HOME);
        assert_eq!(https_only.allows("https://evil.com/x"), Decision::Allow);
        assert_eq!(https_only.allows("file:///etc/passwd"), BLOCKED);
        assert_eq!(https_only.allows("http://evil.com/"), BLOCKED);
    }

    #[test]
    fn a_query_written_after_a_wildcard_path_fails_closed() {
        // URLPattern gotcha: in `/*?x=1` the `?` is the *optional modifier* on `*`, not the
        // search separator. The pattern therefore compiles (no `invalid_patterns` entry)
        // but matches nothing useful. It fails CLOSED, which is why this is a usability
        // finding and not a security one — pinned so it cannot become fail-open.
        let a = allowlist(&["https://app.example.com/*?x=1"], HOME);
        assert!(a.invalid_patterns().is_empty(), "it compiles…");
        assert_eq!(a.allows("https://app.example.com/p?x=1"), BLOCKED);
        assert_eq!(a.allows("https://app.example.com/p?x=2"), BLOCKED);
        assert_eq!(a.allows("https://app.example.com/p"), BLOCKED);
    }
}
