//! RT-09 live smoke.
//!
//! Run explicitly with:
//! KIOSK_LIVE_CREDENTIAL=/run/secrets/kiosk.json +//! KIOSK_LIVE_PROJECT_ID=throwaway-project +//! cargo test -p kiosk-core --test live_token_exchange -- --ignored --nocapture
//!
//! The credential is supplied out of band and is never committed.

use std::sync::Arc;
use std::time::Duration;

use kiosk_core::logging::auth::{ServiceAccount, TokenSource};
use kiosk_core::logging::time::TrustedClock;
use kiosk_core::logging::transport::{ReqwestTransport, Transport};

#[test]
#[ignore = "requires a throwaway service account and live Google endpoints"]
fn live_rs256_token_exchange_and_entries_write() {
    let credential_path =
        std::env::var("KIOSK_LIVE_CREDENTIAL").expect("set KIOSK_LIVE_CREDENTIAL");
    let project_id = std::env::var("KIOSK_LIVE_PROJECT_ID").expect("set KIOSK_LIVE_PROJECT_ID");
    let service_account = ServiceAccount::from_json(
        &std::fs::read_to_string(credential_path).expect("read live credential"),
    )
    .expect("parse live credential");
    let clock = TrustedClock::new();
    clock
        .observe_http_date(&chrono::Utc::now().to_rfc2822())
        .expect("seed trusted time for the live JWT");
    let transport = Arc::new(
        ReqwestTransport::new(Duration::from_secs(20)).expect("construct HTTPS transport"),
    );
    let mut source = TokenSource::new(service_account, transport.clone(), clock);
    let token = source.token().expect("OAuth2 JWT exchange must succeed");

    let body = serde_json::json!({
        "entries": [{
            "logName": format!("projects/{project_id}/logs/kiosk-rt09"),
            "resource": {"type": "global"},
            "severity": "INFO",
            "jsonPayload": {"event": "rt09.live_token_exchange"}
        }],
        "partialSuccess": true
    });
    let response = transport
        .post(
            "https://logging.googleapis.com/v2/entries:write",
            &[
                ("Authorization", &format!("Bearer {}", token.expose())),
                ("Content-Type", "application/json"),
            ],
            &body.to_string(),
        )
        .expect("entries:write request must receive a response");
    assert!(
        (200..300).contains(&response.status),
        "entries:write returned {}: {}",
        response.status,
        response.body
    );
}
