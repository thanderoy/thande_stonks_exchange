//! Client IP for per-IP rate limiting on the public listener.
//!
//! The client is the TCP peer, unless the peer is a trusted reverse proxy (listed in
//! `TRUSTED_PROXY_CIDRS`). Then `X-Forwarded-For` is read from the right, skipping
//! addresses that are themselves trusted proxies, and the first untrusted address is the
//! client. That handles a chain of proxies (host proxy → web container → API). Addresses
//! left of it were supplied by the client and are never used.

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
/// in order. When `peer` is trusted, addresses are walked from the rightmost one leftwards,
/// skipping trusted proxies; the first untrusted address is returned. If every address is
/// trusted, the leftmost one is used. Falls back to the peer when the header is missing or
/// an address in the walk can't be parsed.
pub fn client_ip<'a>(
    peer: Option<IpAddr>,
    forwarded_for: impl DoubleEndedIterator<Item = &'a str>,
    trusted: &[IpNet],
) -> Option<IpAddr> {
    let peer = peer?;
    let is_trusted = |ip: &IpAddr| trusted.iter().any(|net| net.contains(ip));
    if !is_trusted(&peer) {
        return Some(peer);
    }
    let mut last_trusted = None;
    for value in forwarded_for.rev() {
        for raw in value.rsplit(',').map(str::trim).filter(|s| !s.is_empty()) {
            let Ok(ip) = raw.parse::<IpAddr>() else {
                return Some(last_trusted.unwrap_or(peer));
            };
            if !is_trusted(&ip) {
                return Some(ip);
            }
            last_trusted = Some(ip);
        }
    }
    Some(last_trusted.unwrap_or(peer))
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
