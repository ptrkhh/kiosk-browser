//! The HTTP seam (spec §6, TEL-05).
//!
//! Every byte of network I/O in the telemetry subsystem goes through the
//! [`Transport`] trait. This is the ONLY file in the crate that may name
//! `reqwest`: the token source and the Cloud Logging client are written against
//! the trait, so they are unit-testable against a fake with no live server and
//! no HTTP mock. Do not let a `reqwest` type appear in any signature here -
//! [`HttpResponse`] and [`TransportError`] are the whole vocabulary.

use std::time::Duration;

/// A minimal HTTP response. Deliberately owned + plain, so nothing downstream
/// has to know what client produced it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl HttpResponse {
    /// Case-insensitive header lookup (HTTP field names are case-insensitive;
    /// the `Date` harvest in [`crate::logging::time`] depends on this).
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    /// DNS failure, TCP/TLS failure, timeout - the request never got an answer.
    #[error("network error: {0}")]
    Network(String),
    /// The client itself could not be constructed (bad TLS backend, etc.).
    #[error("http client setup failed: {0}")]
    Setup(String),
}

/// A blocking HTTP POST. `Send + Sync` so the logger thread can own one and a
/// fake can be shared by tests.
pub trait Transport: Send + Sync {
    fn post(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        body: &str,
    ) -> Result<HttpResponse, TransportError>;
}

/// The real transport. A non-2xx status is a perfectly good [`HttpResponse`] -
/// only a failure to *get* an answer is a [`TransportError`], because the
/// callers need to branch on 401 / 429 / 5xx themselves.
pub struct ReqwestTransport {
    client: reqwest::blocking::Client,
}

impl ReqwestTransport {
    pub fn new(timeout: Duration) -> Result<Self, TransportError> {
        let client = reqwest::blocking::Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|e| TransportError::Setup(e.to_string()))?;
        Ok(Self { client })
    }
}

impl Transport for ReqwestTransport {
    fn post(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        body: &str,
    ) -> Result<HttpResponse, TransportError> {
        let mut req = self.client.post(url);
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        let resp = req
            .body(body.to_string())
            .send()
            .map_err(|e| TransportError::Network(e.to_string()))?;

        let status = resp.status().as_u16();
        let headers = resp
            .headers()
            .iter()
            .map(|(k, v)| (k.as_str().to_string(), v.to_str().unwrap_or("").to_string()))
            .collect();
        let body = resp
            .text()
            .map_err(|e| TransportError::Network(e.to_string()))?;

        Ok(HttpResponse {
            status,
            headers,
            body,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_lookup_is_case_insensitive() {
        let r = HttpResponse {
            status: 200,
            headers: vec![("Date".into(), "Sun, 12 Jul 2026 08:30:00 GMT".into())],
            body: String::new(),
        };
        assert_eq!(r.header("date"), r.header("DATE"));
        assert_eq!(r.header("date"), Some("Sun, 12 Jul 2026 08:30:00 GMT"));
        assert_eq!(r.header("etag"), None);
    }
}
