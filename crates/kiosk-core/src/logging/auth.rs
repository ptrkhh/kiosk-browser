//! Service-account authentication (spec §6, TEL-05).
//!
//! Google's server-to-server OAuth flow: we sign a short-lived RS256 JWT with
//! the service account's private key and exchange it at the token endpoint for
//! a bearer access token.
//!
//! Two properties are load-bearing:
//!
//! 1. **The access token is a secret.** It lives in memory only. It is never
//!    spooled, never logged, and [`TokenSource`]'s `Debug` is written by hand so
//!    the token cannot leak into a panic message or a `dbg!`.
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
}

/// The fields we need from a Google service-account key file. Everything else
/// (`project_id`, `private_key_id`, ...) is ignored here.
#[derive(Clone, Deserialize)]
pub struct ServiceAccount {
    pub client_email: String,
    /// PEM. Secret material - never rendered anywhere (see the hand-written
    /// `Debug` below).
    pub private_key: String,
    pub token_uri: String,
}

/// Hand-written: a derived `Debug` here would print the RSA private key.
impl std::fmt::Debug for ServiceAccount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServiceAccount")
            .field("client_email", &self.client_email)
            .field("token_uri", &self.token_uri)
            .field("private_key", &"<redacted>")
            .finish()
    }
}

impl ServiceAccount {
    pub fn from_json(json: &str) -> Result<Self, AuthError> {
        let sa: ServiceAccount =
            serde_json::from_str(json).map_err(|e| AuthError::MalformedJson(e.to_string()))?;
        if sa.client_email.trim().is_empty() {
            return Err(AuthError::MissingField("client_email"));
        }
        if sa.private_key.trim().is_empty() {
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
    access_token: String,
    expires_in: i64,
}

/// A cached access token. Held in memory only, and never rendered.
struct CachedToken {
    value: String,
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

/// Hand-written so the bearer token (and the private key) can never reach a log
/// line, a panic message, or a `dbg!`. See TEL-05.
impl std::fmt::Debug for TokenSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TokenSource")
            .field("client_email", &self.service_account.client_email)
            .field("token_uri", &self.service_account.token_uri)
            .field("private_key", &"<redacted>")
            .field(
                "cached_token",
                &match &self.cached {
                    Some(_) => "<redacted>",
                    None => "<none>",
                },
            )
            .field(
                "cached_token_expires_at",
                &self.cached.as_ref().map(|c| c.expires_at),
            )
            .finish()
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
    pub fn token(&mut self) -> Result<String, AuthError> {
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
                body: response.body,
            });
        }

        let parsed: TokenResponse = serde_json::from_str(&response.body)
            .map_err(|e| AuthError::MalformedTokenResponse(e.to_string()))?;

        Ok(CachedToken {
            value: parsed.access_token,
            expires_at: now + Duration::seconds(parsed.expires_in),
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

        let key =
            jsonwebtoken::EncodingKey::from_rsa_pem(self.service_account.private_key.as_bytes())
                .map_err(|e| AuthError::BadPrivateKey(e.to_string()))?;
        let header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::RS256);

        jsonwebtoken::encode(&header, &claims, &key).map_err(|e| AuthError::Signing(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logging::transport::HttpResponse;
    use std::sync::Mutex;

    const SA_JSON: &str = include_str!("testdata/test_service_account.json");
    const PUBLIC_PEM: &str = include_str!("testdata/test_service_account.pub.pem");
    const SECRET_TOKEN: &str = "ya29.SUPER-SECRET-BEARER-TOKEN";

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

    /// A clock whose trusted time is `year`-ish, established from an HTTP Date,
    /// so it is nowhere near the local clock.
    fn clock_at(http_date: &str) -> TrustedClock {
        let c = TrustedClock::new();
        c.observe_http_date(http_date).expect("valid HTTP date");
        c
    }

    fn sa() -> ServiceAccount {
        ServiceAccount::from_json(SA_JSON).expect("fixture parses")
    }

    fn decode_claims(jwt: &str) -> Claims {
        let key = jsonwebtoken::DecodingKey::from_rsa_pem(PUBLIC_PEM.as_bytes())
            .expect("test public key");
        let mut v = jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::RS256);
        // We are asserting on the claims ourselves; do not let the validator
        // reject the token just because the trusted clock is far from `now`.
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
        assert!(sa.private_key.starts_with("-----BEGIN PRIVATE KEY-----"));

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
        let mut ts = TokenSource::new(sa(), t.clone(), clock_at("Sun, 12 Jul 2026 08:30:00 GMT"));
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
        let mut ts = TokenSource::new(sa(), t.clone(), clock_at("Sun, 12 Jul 2026 08:30:00 GMT"));
        let a = ts.token().expect("mints");
        let b = ts.token().expect("cached");
        assert_eq!(a, SECRET_TOKEN);
        assert_eq!(a, b);
        assert_eq!(
            t.call_count(),
            1,
            "a second call must reuse the cache, not re-mint"
        );
    }

    #[test]
    fn a_token_near_expiry_is_refreshed_proactively() {
        // 120 s of life left: inside the 5-minute refresh margin.
        let t = FakeTransport::with(vec![
            FakeTransport::ok_token("first-token", 120),
            FakeTransport::ok_token("second-token", 3600),
        ]);
        let mut ts = TokenSource::new(sa(), t.clone(), clock_at("Sun, 12 Jul 2026 08:30:00 GMT"));
        assert_eq!(ts.token().unwrap(), "first-token");
        assert_eq!(
            ts.token().unwrap(),
            "second-token",
            "a token inside the refresh margin must be re-minted"
        );
        assert_eq!(t.call_count(), 2);
    }

    #[test]
    fn invalidate_forces_exactly_one_refresh() {
        let t = FakeTransport::with(vec![
            FakeTransport::ok_token("first-token", 3600),
            FakeTransport::ok_token("second-token", 3600),
        ]);
        let mut ts = TokenSource::new(sa(), t.clone(), clock_at("Sun, 12 Jul 2026 08:30:00 GMT"));
        assert_eq!(ts.token().unwrap(), "first-token");
        ts.invalidate(); // models the 401 from Cloud Logging
        assert_eq!(ts.token().unwrap(), "second-token");
        assert_eq!(ts.token().unwrap(), "second-token");
        assert_eq!(
            t.call_count(),
            2,
            "invalidate must force exactly ONE refresh, not one per call"
        );
    }

    #[test]
    fn the_token_never_appears_in_debug_output() {
        let t = FakeTransport::with(vec![FakeTransport::ok_token(SECRET_TOKEN, 3600)]);
        let mut ts = TokenSource::new(sa(), t.clone(), clock_at("Sun, 12 Jul 2026 08:30:00 GMT"));
        assert_eq!(ts.token().unwrap(), SECRET_TOKEN);

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

    #[test]
    fn a_token_endpoint_failure_is_reported_not_panicked() {
        for status in [429u16, 500, 401] {
            let t = FakeTransport::with(vec![Ok(HttpResponse {
                status,
                headers: vec![],
                body: r#"{"error":"unavailable"}"#.into(),
            })]);
            let mut ts =
                TokenSource::new(sa(), t.clone(), clock_at("Sun, 12 Jul 2026 08:30:00 GMT"));
            match ts.token() {
                Err(AuthError::TokenEndpoint { status: s, .. }) => assert_eq!(s, status),
                other => panic!("expected TokenEndpoint error for {status}, got {other:?}"),
            }
        }

        // A dead network is an error too, not a panic.
        let t = FakeTransport::with(vec![Err("dns failure".to_string())]);
        let mut ts = TokenSource::new(sa(), t.clone(), clock_at("Sun, 12 Jul 2026 08:30:00 GMT"));
        assert!(matches!(ts.token(), Err(AuthError::Transport(_))));

        // Garbage in the 200 body is an error, not a panic.
        let t = FakeTransport::with(vec![Ok(HttpResponse {
            status: 200,
            headers: vec![],
            body: "not json".into(),
        })]);
        let mut ts = TokenSource::new(sa(), t.clone(), clock_at("Sun, 12 Jul 2026 08:30:00 GMT"));
        assert!(matches!(
            ts.token(),
            Err(AuthError::MalformedTokenResponse(_))
        ));
    }
}
