//! Client IP for per-IP rate limiting on the public listener.
//!
//! The client is the TCP peer, unless the peer is a trusted reverse proxy (listed in
//! `TRUSTED_PROXY_CIDRS`), in which case it is the rightmost `X-Forwarded-For` address —
//! the one the proxy itself appended. Addresses further left are client-supplied and
//! never trusted.

use actix_governor::governor::NotUntil;
use actix_governor::governor::clock::{Clock, DefaultClock, QuantaInstant};
use actix_governor::{KeyExtractor, SimpleKeyExtractionError};
use actix_web::dev::ServiceRequest;
use actix_web::http::header::{ContentType, HeaderMap};
use actix_web::{HttpResponse, HttpResponseBuilder};
use ipnet::IpNet;
use std::net::IpAddr;
use std::sync::Arc;

pub const X_FORWARDED_FOR: &str = "x-forwarded-for";

/// Parses a comma-separated list of CIDRs or bare IPs. Empty input → no trusted proxies.
pub fn parse_trusted_proxies(raw: &str) -> Result<Vec<IpNet>, String> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| {
            s.parse::<IpNet>()
                .or_else(|_| s.parse::<IpAddr>().map(IpNet::from))
                .map_err(|_| format!("invalid CIDR in TRUSTED_PROXY_CIDRS: {s:?}"))
        })
        .collect()
}

/// Resolves the client address. `forwarded_for` is every `X-Forwarded-For` header value
/// in order; only the rightmost address of the last one is used, and only when `peer`
/// is trusted. Falls back to the peer if that address is missing or unparsable.
pub fn client_ip<'a>(
    peer: Option<IpAddr>,
    forwarded_for: impl DoubleEndedIterator<Item = &'a str>,
    trusted: &[IpNet],
) -> Option<IpAddr> {
    let peer = peer?;
    if !trusted.iter().any(|net| net.contains(&peer)) {
        return Some(peer);
    }
    let forwarded = forwarded_for
        .rev()
        .find_map(|value| value.rsplit(',').map(str::trim).find(|s| !s.is_empty()))
        .and_then(|s| s.parse::<IpAddr>().ok());
    Some(forwarded.unwrap_or(peer))
}

fn header_values(headers: &HeaderMap) -> Vec<&str> {
    headers
        .get_all(X_FORWARDED_FOR)
        .filter_map(|v| v.to_str().ok())
        .collect()
}

/// actix-governor key extractor keyed by [`client_ip`].
#[derive(Clone, Debug, Default)]
pub struct ClientIpExtractor {
    trusted: Arc<Vec<IpNet>>,
}

impl ClientIpExtractor {
    pub fn new(trusted: Vec<IpNet>) -> Self {
        Self {
            trusted: Arc::new(trusted),
        }
    }
}

impl KeyExtractor for ClientIpExtractor {
    type Key = IpAddr;
    type KeyExtractionError = SimpleKeyExtractionError<&'static str>;

    fn extract(&self, req: &ServiceRequest) -> Result<Self::Key, Self::KeyExtractionError> {
        let peer = req.peer_addr().map(|a| a.ip());
        client_ip(
            peer,
            header_values(req.headers()).into_iter(),
            &self.trusted,
        )
        .ok_or_else(|| SimpleKeyExtractionError::new("could not determine client address"))
    }

    fn exceed_rate_limit_response(
        &self,
        negative: &NotUntil<QuantaInstant>,
        mut response: HttpResponseBuilder,
    ) -> HttpResponse {
        let wait = negative
            .wait_time_from(DefaultClock::default().now())
            .as_secs()
            .max(1);
        response
            .content_type(ContentType::json())
            .insert_header(("Retry-After", wait.to_string()))
            .body(
                serde_json::json!({
                    "error": format!("too many requests, retry in {wait}s")
                })
                .to_string(),
            )
    }
}
