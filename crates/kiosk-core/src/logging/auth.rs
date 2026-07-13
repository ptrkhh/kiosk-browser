//! Service-account authentication (spec §6, TEL-05).
//!
//! Google's server-to-server OAuth flow: we sign a short-lived RS256 JWT with
//! the service account's private key and exchange it at the token endpoint for
//! a bearer access token.
//!
//! Two properties are load-bearing:
//!
//! 1. **The access token is a secret**, and so is the RSA private key. Both live
//!    in memory only, wrapped in [`Secret`], whose `Debug` AND `Display` redact.
//!    That makes the redaction structural rather than aspirational: a
//!    `#[derive(Debug)]` on any struct holding one is safe, and the raw value is
//!    reachable only through the explicitly-named, greppable [`Secret::expose`].
//! 2. **`iat`/`exp` come from the trusted clock, never `Utc::now()`.** A kiosk
//!    with a dead CMOS battery has a local clock that is years wrong; a JWT
//!    signed with it is rejected by Google with an opaque `invalid_grant`. If
//!    trusted time is not yet established we return [`AuthError::NoTrustedTime`]
//!    rather than signing with a clock we know may be wrong - a clean, named
//!    failure the caller can retry, instead of an undiagnosable 400 forever.

use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::logging::time::TrustedClock;
use crate::logging::transport::{Transport, TransportError};

/// The only scope this device ever needs.
pub const LOGGING_WRITE_SCOPE: &str = "https://www.googleapis.com/auth/logging.write";

/// Google's maximum assertion lifetime.
const JWT_LIFETIME_SECONDS: i64 = 3600;

/// Refresh this far ahead of expiry so an in-flight flush never races the clock.
const REFRESH_MARGIN_SECONDS: i64 = 300;

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("service account JSON is malformed: {0}")]
    MalformedJson(String),
    #[error("service account is missing or has an empty `{0}`")]
    MissingField(&'static str),
    #[error("service account private key is not a usable RSA PEM: {0}")]
    BadPrivateKey(String),
    #[error("failed to sign the assertion JWT: {0}")]
    Signing(String),
    /// Trusted time is not established yet (no HTTP `Date` observed). We refuse
    /// to sign with the local clock - see the module docs.
    #[error("no trusted time available yet; refusing to sign a JWT with the local clock")]
    NoTrustedTime,
    #[error("token endpoint unreachable: {0}")]
    Transport(#[from] TransportError),
    /// A non-2xx from the token endpoint. The body is Google's error JSON; it
    /// never contains an access token.
    #[error("token endpoint returned HTTP {status}: {body}")]
    TokenEndpoint { status: u16, body: String },
    #[error("token endpoint response was not the expected JSON: {0}")]
    MalformedTokenResponse(String),
    /// An `expires_in` inside the refresh margin (so: unusable by construction).
    /// See `mint` for why this is rejected rather than clamped.
    #[error("token endpoint returned an unusable expires_in: {0}s (must exceed the {REFRESH_MARGIN_SECONDS}s refresh margin)")]
    InvalidExpiry(i64),
}

/// The token endpoint's body is attacker-influenceable and unbounded, and it
/// ends up inside an error that will be logged. Two reasons to cap it: a hostile
/// endpoint could inflate a log line arbitrarily, and a misconfigured
/// `token_uri` pointing at a request-reflecting endpoint would echo our signed
/// assertion JWT straight back into the error string - and thence into a log.
const MAX_ERROR_BODY_BYTES: usize = 256;

fn truncate_for_error(body: &str) -> String {
    if body.len() <= MAX_ERROR_BODY_BYTES {
        return body.to_string();
    }
    // Do not split a UTF-8 code point.
    let mut end = MAX_ERROR_BODY_BYTES;
    while end > 0 && !body.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}... [{} bytes truncated]", &body[..end], body.len() - end)
}

/// A secret string (an RSA private key, a bearer token) that cannot be printed
/// by accident.
///
/// Both `Debug` and `Display` redact, so neither `{:?}` nor `{}` can leak it -
/// including through a `#[derive(Debug)]` on any struct that holds one, and
/// including `format!`/`panic!`/`tracing::info!`. The raw value is reachable
/// only via [`Secret::expose`], which makes every disclosure deliberate and,
/// crucially, greppable: `grep -rn '\.expose()'` enumerates every place a
/// secret escapes. T7/T8 will hold these values; this is what stops a future
/// debug line from becoming a breach.
#[derive(Clone, Deserialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct Secret(String);

impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// The raw secret. Every call site is a deliberate disclosure - do not add
    /// one without a reason.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("<redacted>")
    }
}

impl std::fmt::Display for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("<redacted>")
    }
}

/// The fields we need from a Google service-account key file. Everything else
/// (`project_id`, `private_key_id`, ...) is ignored here.
///
/// `Debug` is derived - and that is safe *by construction*, because the private
/// key is a [`Secret`], which redacts itself. There is no hand-written `Debug`
/// to fall out of sync with the fields.
#[derive(Debug, Clone, Deserialize)]
pub struct ServiceAccount {
    pub client_email: String,
    /// The RSA private key PEM. Private field: only `sign_assertion` (in this
    /// module) needs it, so no accessor is offered.
    private_key: Secret,
    pub token_uri: String,
}

impl ServiceAccount {
    pub fn from_json(json: &str) -> Result<Self, AuthError> {
        let sa: ServiceAccount =
            serde_json::from_str(json).map_err(|e| AuthError::MalformedJson(e.to_string()))?;
        if sa.client_email.trim().is_empty() {
            return Err(AuthError::MissingField("client_email"));
        }
        if sa.private_key.expose().trim().is_empty() {
            return Err(AuthError::MissingField("private_key"));
        }
        if sa.token_uri.trim().is_empty() {
            return Err(AuthError::MissingField("token_uri"));
        }
        Ok(sa)
    }
}

/// The RS256 assertion Google's server-to-server flow expects.
#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    iss: String,
    scope: String,
    aud: String,
    exp: i64,
    iat: i64,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: Secret,
    expires_in: i64,
}

/// A cached access token. Held in memory only, and never rendered.
#[derive(Debug)]
struct CachedToken {
    value: Secret,
    /// Absolute expiry, in trusted time.
    expires_at: DateTime<Utc>,
}

/// Mints and caches an OAuth2 access token for the service account.
///
/// A flush happens every ~10 s; minting per flush would be ~8,640 token
/// requests per device per day. So we cache and only re-mint when the token is
/// inside the refresh margin, or when [`TokenSource::invalidate`] is called
/// (the 401 path: exactly one forced refresh-and-retry).
pub struct TokenSource {
    service_account: ServiceAccount,
    transport: Arc<dyn Transport>,
    clock: TrustedClock,
    cached: Option<CachedToken>,
}

/// Hand-written only because `Arc<dyn Transport>` is not `Debug`. The secrets
/// need no special handling here: the private key and the cached token are both
/// [`Secret`]s, which redact themselves, so this delegates to their `Debug`
/// rather than re-implementing redaction (which could fall out of sync). See
/// TEL-05.
impl std::fmt::Debug for TokenSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TokenSource")
            .field("service_account", &self.service_account)
            .field("cached", &self.cached)
            .finish_non_exhaustive()
    }
}

impl TokenSource {
    pub fn new(
        service_account: ServiceAccount,
        transport: Arc<dyn Transport>,
        clock: TrustedClock,
    ) -> Self {
        Self {
            service_account,
            transport,
            clock,
            cached: None,
        }
    }

    /// Drops the cached token, forcing the next [`TokenSource::token`] call to
    /// mint exactly one new one. Called when Cloud Logging answers 401.
    pub fn invalidate(&mut self) {
        self.cached = None;
    }

    /// A valid access token, minting one only if the cache is empty or the
    /// cached token is within [`REFRESH_MARGIN_SECONDS`] of expiry.
    ///
    /// Returns a [`Secret`]: the caller must say `.expose()` to get the bearer
    /// string, so it cannot be logged by accident.
    pub fn token(&mut self) -> Result<Secret, AuthError> {
        let now = self.clock.trusted_utc().ok_or(AuthError::NoTrustedTime)?;

        if let Some(cached) = &self.cached {
            if now + Duration::seconds(REFRESH_MARGIN_SECONDS) < cached.expires_at {
                return Ok(cached.value.clone());
            }
        }

        let fresh = self.mint(now)?;
        let value = fresh.value.clone();
        self.cached = Some(fresh);
        Ok(value)
    }

    fn mint(&self, now: DateTime<Utc>) -> Result<CachedToken, AuthError> {
        let assertion = self.sign_assertion(now)?;

        // `grant_type` is percent-encoded by hand: the JWT itself is base64url
        // plus dots, all form-safe, so no encoder dependency is warranted.
        let body = format!(
            "grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Ajwt-bearer&assertion={assertion}"
        );

        let response = self.transport.post(
            &self.service_account.token_uri,
            &[("Content-Type", "application/x-www-form-urlencoded")],
            &body,
        )?;

        // Feed the response's Date back to the trusted clock: the token endpoint
        // is itself a clock source, and it is reachable on 443.
        if let Some(date) = response.header("Date") {
            let _ = self.clock.observe_http_date(date);
        }

        if !(200..300).contains(&response.status) {
            return Err(AuthError::TokenEndpoint {
                status: response.status,
                body: truncate_for_error(&response.body),
            });
        }

        // Truncated for the same reason as the error body below: serde echoes the
        // OFFENDING value verbatim, so a 200 with a mistyped field (say a 100 KB
        // string where `expires_in` should be) would otherwise inflate an error
        // string - and thence a log line - without bound.
        let parsed: TokenResponse = serde_json::from_str(&response.body)
            .map_err(|e| AuthError::MalformedTokenResponse(truncate_for_error(&e.to_string())))?;

        // NEVER trust the endpoint's `expires_in`. The usable range is
        // (REFRESH_MARGIN_SECONDS, JWT_LIFETIME_SECONDS].
        //
        // The LOWER bound is the refresh margin, not zero. The cache predicate
        // is `now + REFRESH_MARGIN_SECONDS < expires_at`, so ANY lifetime inside
        // the margin - 1 s, 60 s, 300 s, as much as a non-positive value - fails
        // that predicate on the very next call and is re-minted every single
        // time: an unbounded, backoff-free mint storm through the SUCCESS path,
        // which is precisely what the cache exists to prevent. Such a token is
        // unusable by construction, so we REJECT rather than clamp: an error is
        // visible, is not cached, and is rate-limited by the caller's backoff.
        // (Clamping a bad value UP into the margin would just re-create the
        // storm quietly.)
        //
        // The UPPER bound is the mirror image (cache a long-dead token and 401
        // forever), but it is safe to clamp DOWN: worst case we re-mint an hour
        // early.
        if parsed.expires_in <= REFRESH_MARGIN_SECONDS {
            return Err(AuthError::InvalidExpiry(parsed.expires_in));
        }
        let lifetime = parsed.expires_in.min(JWT_LIFETIME_SECONDS);

        Ok(CachedToken {
            value: parsed.access_token,
            expires_at: now + Duration::seconds(lifetime),
        })
    }

    /// Builds and RS256-signs the assertion. `iat`/`exp` come from `now`, which
    /// the caller has already taken from the trusted clock.
    fn sign_assertion(&self, now: DateTime<Utc>) -> Result<String, AuthError> {
        let iat = now.timestamp();
        let claims = Claims {
            iss: self.service_account.client_email.clone(),
            scope: LOGGING_WRITE_SCOPE.to_string(),
            aud: self.service_account.token_uri.clone(),
            iat,
            exp: iat + JWT_LIFETIME_SECONDS,
        };

        let key = jsonwebtoken::EncodingKey::from_rsa_pem(
            self.service_account.private_key.expose().as_bytes(),
        )
        .map_err(|e| AuthError::BadPrivateKey(e.to_string()))?;
        let header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::RS256);

        jsonwebtoken::encode(&header, &claims, &key).map_err(|e| AuthError::Signing(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logging::transport::HttpResponse;
    use std::sync::{LazyLock, Mutex};

    const SECRET_TOKEN: &str = "ya29.SUPER-SECRET-BEARER-TOKEN";

    /// The RSA keypair used by every test, generated ONCE per test binary.
    ///
    /// Generated at run time rather than committed as a fixture. A committed
    /// `-----BEGIN PRIVATE KEY-----` inside a file named `*service_account*` is
    /// the single highest-signal pattern that GitHub push protection, gitleaks,
    /// and TruffleHog scan for. It would raise a permanent "leaked credential"
    /// alert that a human has to triage and dismiss forever - training people to
    /// ignore precisely the alert that will one day be real. Keygen is ~200 ms,
    /// paid once for the whole binary; that is far cheaper than that.
    ///
    /// (It also means the repo cannot repeat the `.gitignore`-swallows-the-PEM
    /// bug: there is no `.pem` to forget to commit.)
    struct TestKey {
        private_pem: String,
        public_pem: String,
    }

    static TEST_KEY: LazyLock<TestKey> = LazyLock::new(|| {
        use rsa::pkcs8::{EncodePrivateKey, EncodePublicKey, LineEnding};
        use rsa::{RsaPrivateKey, RsaPublicKey};

        let mut rng = rand::thread_rng();
        let private = RsaPrivateKey::new(&mut rng, 2048).expect("generate test RSA key");
        let public = RsaPublicKey::from(&private);
        TestKey {
            private_pem: private
                .to_pkcs8_pem(LineEnding::LF)
                .expect("encode private pem")
                .to_string(),
            public_pem: public
                .to_public_key_pem(LineEnding::LF)
                .expect("encode public pem"),
        }
    });

    /// A service-account JSON document, shaped like Google's, signed by the
    /// generated key.
    fn sa_json() -> String {
        serde_json::json!({
            "type": "service_account",
            "project_id": "test-project",
            "private_key": TEST_KEY.private_pem,
            "client_email": "kiosk-logger@test-project.iam.gserviceaccount.com",
            "token_uri": "https://oauth2.googleapis.com/token",
        })
        .to_string()
    }

    fn sa() -> ServiceAccount {
        ServiceAccount::from_json(&sa_json()).expect("fixture parses")
    }

    struct FakeTransport {
        requests: Mutex<Vec<(String, String)>>,
        /// Consumed in order; the last one repeats once exhausted.
        responses: Mutex<std::collections::VecDeque<Result<HttpResponse, String>>>,
    }

    impl FakeTransport {
        fn with(responses: Vec<Result<HttpResponse, String>>) -> Arc<Self> {
            Arc::new(Self {
                requests: Mutex::new(Vec::new()),
                responses: Mutex::new(responses.into()),
            })
        }

        fn ok_token(token: &str, expires_in: i64) -> Result<HttpResponse, String> {
            Ok(HttpResponse {
                status: 200,
                headers: vec![("Content-Type".into(), "application/json".into())],
                body: format!(
                    r#"{{"access_token":"{token}","expires_in":{expires_in},"token_type":"Bearer"}}"#
                ),
            })
        }

        fn call_count(&self) -> usize {
            self.requests.lock().unwrap().len()
        }

        /// The `assertion=` JWT from the Nth request.
        fn assertion(&self, n: usize) -> String {
            let (_, body) = self.requests.lock().unwrap()[n].clone();
            body.split("assertion=").nth(1).unwrap().to_string()
        }
    }

    impl Transport for FakeTransport {
        fn post(
            &self,
            url: &str,
            _headers: &[(&str, &str)],
            body: &str,
        ) -> Result<HttpResponse, TransportError> {
            self.requests
                .lock()
                .unwrap()
                .push((url.to_string(), body.to_string()));
            let mut responses = self.responses.lock().unwrap();
            let next = if responses.len() > 1 {
                responses.pop_front().unwrap()
            } else {
                responses
                    .front()
                    .expect("FakeTransport ran out of canned responses")
                    .clone()
            };
            next.map_err(TransportError::Network)
        }
    }

    /// A clock whose trusted time is established from an HTTP `Date`, so it is
    /// nowhere near the local clock.
    fn clock_at(http_date: &str) -> TrustedClock {
        let c = TrustedClock::new();
        c.observe_http_date(http_date).expect("valid HTTP date");
        c
    }

    /// A TokenSource on a known-good clock with the given canned responses.
    fn token_source(t: Arc<FakeTransport>) -> TokenSource {
        TokenSource::new(sa(), t, clock_at("Sun, 12 Jul 2026 08:30:00 GMT"))
    }

    fn decode_claims(jwt: &str) -> Claims {
        let key = jsonwebtoken::DecodingKey::from_rsa_pem(TEST_KEY.public_pem.as_bytes())
            .expect("test public key");
        let mut v = jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::RS256);
        // We assert on the claims ourselves; do not let the validator reject the
        // token merely because the trusted clock is far from `now`.
        v.validate_exp = false;
        v.validate_aud = false;
        jsonwebtoken::decode::<Claims>(jwt, &key, &v)
            .expect("the assertion must verify under the service account's key")
            .claims
    }

    #[test]
    fn service_account_json_parses_the_fields_we_need() {
        let sa = sa();
        assert_eq!(
            sa.client_email,
            "kiosk-logger@test-project.iam.gserviceaccount.com"
        );
        assert_eq!(sa.token_uri, "https://oauth2.googleapis.com/token");
        assert!(sa
            .private_key
            .expose()
            .starts_with("-----BEGIN PRIVATE KEY-----"));

        assert!(matches!(
            ServiceAccount::from_json("{ not json"),
            Err(AuthError::MalformedJson(_))
        ));
        assert!(matches!(
            ServiceAccount::from_json(
                r#"{"client_email":"","private_key":"x","token_uri":"https://t"}"#
            ),
            Err(AuthError::MissingField("client_email"))
        ));
    }

    #[test]
    fn jwt_claims_match_googles_server_to_server_contract() {
        let t = FakeTransport::with(vec![FakeTransport::ok_token(SECRET_TOKEN, 3600)]);
        let mut ts = token_source(t.clone());
        ts.token().expect("mints");

        let claims = decode_claims(&t.assertion(0));
        assert_eq!(
            claims.iss,
            "kiosk-logger@test-project.iam.gserviceaccount.com"
        );
        assert!(
            claims.scope.contains("logging.write"),
            "scope was {:?}",
            claims.scope
        );
        assert_eq!(claims.scope, LOGGING_WRITE_SCOPE);
        assert_eq!(
            claims.aud, "https://oauth2.googleapis.com/token",
            "a wrong `aud` is a silent 400 forever"
        );
        assert!(
            claims.exp - claims.iat <= 3600 && claims.exp > claims.iat,
            "lifetime {} is outside (0, 3600]",
            claims.exp - claims.iat
        );

        // And it went to the token endpoint with the JWT-bearer grant.
        let (url, body) = t.requests.lock().unwrap()[0].clone();
        assert_eq!(url, "https://oauth2.googleapis.com/token");
        assert!(
            body.contains("grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Ajwt-bearer"),
            "body was {body}"
        );
    }

    #[test]
    fn jwt_iat_and_exp_use_trusted_time_not_the_local_clock() {
        // A date a decade away from any plausible local clock.
        let http_date = "Wed, 12 Jul 2045 08:30:00 GMT";
        let expected = DateTime::parse_from_rfc2822("Wed, 12 Jul 2045 08:30:00 +0000")
            .unwrap()
            .with_timezone(&Utc)
            .timestamp();

        let t = FakeTransport::with(vec![FakeTransport::ok_token(SECRET_TOKEN, 3600)]);
        let mut ts = TokenSource::new(sa(), t.clone(), clock_at(http_date));
        ts.token().expect("mints");

        let claims = decode_claims(&t.assertion(0));
        assert!(
            (claims.iat - expected).abs() <= 5,
            "iat {} is not the trusted time {} (local clock leaked in?)",
            claims.iat,
            expected
        );
        assert!(
            (claims.iat - Utc::now().timestamp()).abs() > 86_400,
            "iat must NOT track the local clock"
        );
        assert_eq!(claims.exp, claims.iat + 3600);
    }

    #[test]
    fn no_trusted_time_is_a_named_error_not_a_wrongly_signed_jwt() {
        let t = FakeTransport::with(vec![FakeTransport::ok_token(SECRET_TOKEN, 3600)]);
        // A fresh clock has never seen an HTTP Date.
        let mut ts = TokenSource::new(sa(), t.clone(), TrustedClock::new());
        assert!(matches!(ts.token(), Err(AuthError::NoTrustedTime)));
        assert_eq!(t.call_count(), 0, "must not even try to mint");
    }

    #[test]
    fn the_token_is_cached_and_not_reminted_on_every_call() {
        let t = FakeTransport::with(vec![FakeTransport::ok_token(SECRET_TOKEN, 3600)]);
        let mut ts = token_source(t.clone());
        let a = ts.token().expect("mints");
        let b = ts.token().expect("cached");
        assert_eq!(a.expose(), SECRET_TOKEN);
        assert_eq!(a.expose(), b.expose());
        assert_eq!(
            t.call_count(),
            1,
            "a second call must reuse the cache, not re-mint"
        );
    }

    /// A full-life token, held until the clock advances to within the 5-minute
    /// margin of its expiry, must be re-minted BEFORE it dies.
    ///
    /// This drives the refresh the way it actually happens in production - time
    /// passes - rather than by having the server hand back a stub lifetime. (It
    /// no longer *can*: a lifetime inside the margin is now rejected outright,
    /// see `an_expires_in_inside_the_refresh_margin_is_rejected...`.)
    #[test]
    fn a_token_near_expiry_is_refreshed_proactively() {
        let t = FakeTransport::with(vec![
            FakeTransport::ok_token("first-token", 3600),
            FakeTransport::ok_token("second-token", 3600),
        ]);
        let clock = clock_at("Sun, 12 Jul 2026 08:30:00 GMT");
        let mut ts = TokenSource::new(sa(), t.clone(), clock.clone());

        assert_eq!(ts.token().unwrap().expose(), "first-token");
        assert_eq!(t.call_count(), 1);

        // 09:00:00 - 30 min in, 30 min of life left: comfortably outside the
        // margin, so the cache must still hold.
        clock
            .observe_http_date("Sun, 12 Jul 2026 09:00:00 GMT")
            .unwrap();
        assert_eq!(ts.token().unwrap().expose(), "first-token");
        assert_eq!(t.call_count(), 1, "still outside the refresh margin");

        // 09:26:00 - the token expires at 09:30:00, so only 4 min remain: inside
        // the 5-minute margin. It must be refreshed proactively, while the old
        // one is still technically valid.
        clock
            .observe_http_date("Sun, 12 Jul 2026 09:26:00 GMT")
            .unwrap();
        assert_eq!(
            ts.token().unwrap().expose(),
            "second-token",
            "a token inside the refresh margin must be re-minted before it dies"
        );
        assert_eq!(t.call_count(), 2);
    }

    #[test]
    fn invalidate_forces_exactly_one_refresh() {
        let t = FakeTransport::with(vec![
            FakeTransport::ok_token("first-token", 3600),
            FakeTransport::ok_token("second-token", 3600),
        ]);
        let mut ts = token_source(t.clone());
        assert_eq!(ts.token().unwrap().expose(), "first-token");
        ts.invalidate(); // models the 401 from Cloud Logging
        assert_eq!(ts.token().unwrap().expose(), "second-token");
        assert_eq!(ts.token().unwrap().expose(), "second-token");
        assert_eq!(
            t.call_count(),
            2,
            "invalidate must force exactly ONE refresh, not one per call"
        );
    }

    #[test]
    fn the_token_never_appears_in_debug_output() {
        let t = FakeTransport::with(vec![FakeTransport::ok_token(SECRET_TOKEN, 3600)]);
        let mut ts = token_source(t.clone());
        assert_eq!(ts.token().unwrap().expose(), SECRET_TOKEN);

        let rendered = format!("{ts:?}");
        assert!(
            !rendered.contains(SECRET_TOKEN),
            "the bearer token leaked into Debug: {rendered}"
        );
        assert!(
            !rendered.contains("BEGIN PRIVATE KEY"),
            "the private key leaked into Debug: {rendered}"
        );
        // The useful, non-secret bits are still there.
        assert!(rendered.contains("kiosk-logger@test-project"));
    }

    /// Pins the redaction on `ServiceAccount` ITSELF.
    ///
    /// `TokenSource`'s `Debug` used to reach past `ServiceAccount` and print its
    /// own fields, so `the_token_never_appears_in_debug_output` never invoked
    /// `ServiceAccount`'s `Debug` at all - its "no private key" assertion passed
    /// for an unrelated reason, and the private-key redaction was protected by
    /// nothing. This formats a `ServiceAccount` directly.
    #[test]
    fn the_private_key_never_appears_in_service_account_debug_output() {
        let sa = sa();
        let rendered = format!("{sa:?}");
        assert!(
            !rendered.contains("BEGIN PRIVATE KEY"),
            "the private key leaked into ServiceAccount's Debug: {rendered}"
        );
        assert!(
            !rendered.contains(sa.private_key.expose()),
            "the private key leaked into ServiceAccount's Debug"
        );
        assert!(rendered.contains("kiosk-logger@test-project"));
    }

    /// A `Secret` must not leak through `Display` either - `{}` is at least as
    /// easy to type as `{:?}` in a log line.
    #[test]
    fn a_secret_redacts_in_both_debug_and_display() {
        let s = Secret::new("hunter2");
        assert!(!format!("{s:?}").contains("hunter2"));
        assert!(!format!("{s}").contains("hunter2"));
        assert_eq!(s.expose(), "hunter2");
    }

    #[test]
    fn a_token_endpoint_failure_is_reported_not_panicked() {
        for status in [429u16, 500, 401] {
            let t = FakeTransport::with(vec![Ok(HttpResponse {
                status,
                headers: vec![],
                body: r#"{"error":"unavailable"}"#.into(),
            })]);
            let mut ts = token_source(t.clone());
            match ts.token() {
                Err(AuthError::TokenEndpoint { status: s, .. }) => assert_eq!(s, status),
                other => panic!("expected TokenEndpoint error for {status}, got {other:?}"),
            }
        }

        // A dead network is an error too, not a panic.
        let t = FakeTransport::with(vec![Err("dns failure".to_string())]);
        let mut ts = token_source(t.clone());
        assert!(matches!(ts.token(), Err(AuthError::Transport(_))));

        // Garbage in the 200 body is an error, not a panic.
        let t = FakeTransport::with(vec![Ok(HttpResponse {
            status: 200,
            headers: vec![],
            body: "not json".into(),
        })]);
        let mut ts = token_source(t.clone());
        assert!(matches!(
            ts.token(),
            Err(AuthError::MalformedTokenResponse(_))
        ));
    }

    /// An `expires_in` that lands INSIDE the refresh margin is unusable by
    /// construction: the cache predicate is
    /// `now + REFRESH_MARGIN_SECONDS < expires_at`, so such a token fails it on
    /// the very next `token()` call and is re-minted every single time -
    /// an unbounded, backoff-free mint storm through the SUCCESS path.
    ///
    /// The boundary is therefore the refresh margin, NOT zero. `expires_in: 1`
    /// and `expires_in: 300` storm exactly as hard as `expires_in: 0` does; a
    /// `<= 0` check only ever caught them by accident of adjacency. This test
    /// pins the *property* (nothing inside the margin is ever cached), not the
    /// old boundary.
    #[test]
    fn an_expires_in_inside_the_refresh_margin_is_rejected_not_turned_into_a_mint_storm() {
        for bad in [-1i64, 0, 1, 60, 299, REFRESH_MARGIN_SECONDS] {
            let t = FakeTransport::with(vec![FakeTransport::ok_token(SECRET_TOKEN, bad)]);
            let mut ts = token_source(t.clone());
            match ts.token() {
                Err(AuthError::InvalidExpiry(v)) => assert_eq!(v, bad),
                other => panic!("expected InvalidExpiry for expires_in={bad}, got {other:?}"),
            }
            assert!(ts.cached.is_none(), "an unusable token must not be cached");
        }
    }

    /// The other side of the boundary: one second past the refresh margin is
    /// usable, and must be cached rather than re-minted.
    #[test]
    fn an_expires_in_just_past_the_refresh_margin_is_accepted_and_cached() {
        let good = REFRESH_MARGIN_SECONDS + 1;
        let t = FakeTransport::with(vec![FakeTransport::ok_token(SECRET_TOKEN, good)]);
        let mut ts = token_source(t.clone());
        assert_eq!(ts.token().expect("usable").expose(), SECRET_TOKEN);
        ts.token().expect("still usable");
        assert_eq!(
            t.call_count(),
            1,
            "a token past the margin must be cached, not re-minted"
        );
    }

    /// The mirror image: an absurd `expires_in` must not cache a dead token
    /// forever. Clamped DOWN to the 3600 Google actually issues - worst case we
    /// re-mint an hour early.
    #[test]
    fn an_absurd_expires_in_is_clamped_to_one_hour() {
        let t = FakeTransport::with(vec![FakeTransport::ok_token(SECRET_TOKEN, i64::MAX / 2)]);
        let mut ts = token_source(t.clone());
        let now = ts.clock.trusted_utc().unwrap();
        ts.token().expect("mints");

        let expires_at = ts.cached.as_ref().unwrap().expires_at;
        let lifetime = (expires_at - now).num_seconds();
        assert!(
            lifetime <= 3600,
            "expires_in must be clamped to <= 3600, got {lifetime}"
        );
        // Still cached, though: one absurd value must not cause a re-mint storm.
        ts.token().expect("cached");
        assert_eq!(t.call_count(), 1);
    }

    /// The endpoint's body is attacker-influenceable and ends up in a logged
    /// error. A request-reflecting `token_uri` would otherwise echo our signed
    /// assertion JWT into the log.
    #[test]
    fn an_oversized_error_body_is_truncated() {
        let huge = "A".repeat(100_000);
        let t = FakeTransport::with(vec![Ok(HttpResponse {
            status: 500,
            headers: vec![],
            body: huge,
        })]);
        let mut ts = token_source(t.clone());
        let err = ts.token().expect_err("500 is an error");
        let rendered = err.to_string();
        assert!(
            rendered.len() < 1000,
            "error body was not truncated: {} bytes",
            rendered.len()
        );
        assert!(rendered.contains("truncated"));
    }

    /// The same unbounded-log-line vector through a different variant: a 200
    /// whose `expires_in` is a huge string makes serde echo that value verbatim
    /// into its error message.
    #[test]
    fn an_oversized_malformed_body_error_is_truncated() {
        let huge = "B".repeat(100_000);
        let t = FakeTransport::with(vec![Ok(HttpResponse {
            status: 200,
            headers: vec![],
            body: format!(r#"{{"access_token":"x","expires_in":"{huge}"}}"#),
        })]);
        let mut ts = token_source(t.clone());
        let err = ts.token().expect_err("a string expires_in is malformed");
        assert!(matches!(err, AuthError::MalformedTokenResponse(_)));
        let rendered = err.to_string();
        assert!(
            rendered.len() < 1000,
            "the serde error was not truncated: {} bytes",
            rendered.len()
        );
    }
}
