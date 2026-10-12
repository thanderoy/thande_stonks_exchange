//! Client IP extraction and environment parsing for the public listener.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use server::app::{parse_cookie_secure, parse_session_key};
use server::client_ip::{client_ip, parse_trusted_proxies};
use std::net::IpAddr;

fn ip(s: &str) -> IpAddr {
    s.parse().unwrap()
}

fn resolve(peer: &str, xff: &[&str], trusted: &str) -> Option<IpAddr> {
    let trusted = parse_trusted_proxies(trusted).unwrap();
    client_ip(Some(ip(peer)), xff.iter().copied(), &trusted)
}

#[test]
fn untrusted_peer_ignores_forwarded_for() {
    assert_eq!(
        resolve("198.51.100.7", &["1.2.3.4"], ""),
        Some(ip("198.51.100.7"))
    );
    assert_eq!(
        resolve("198.51.100.7", &["1.2.3.4"], "172.16.0.0/12"),
        Some(ip("198.51.100.7"))
    );
}

#[test]
fn trusted_peer_uses_rightmost_forwarded_for() {
    assert_eq!(
        resolve("172.18.0.5", &["6.6.6.6, 203.0.113.9"], "172.16.0.0/12"),
        Some(ip("203.0.113.9"))
    );
}

#[test]
fn trusted_peer_uses_rightmost_address_of_the_last_header() {
    assert_eq!(
        resolve(
            "10.0.0.2",
            &["6.6.6.6", "7.7.7.7, 203.0.113.10 "],
            "10.0.0.2"
        ),
        Some(ip("203.0.113.10"))
    );
}

#[test]
fn trusted_peer_without_usable_forwarded_for_falls_back_to_peer() {
    assert_eq!(
        resolve("172.18.0.5", &[], "172.16.0.0/12"),
        Some(ip("172.18.0.5"))
    );
    assert_eq!(
        resolve("172.18.0.5", &["not-an-ip"], "172.16.0.0/12"),
        Some(ip("172.18.0.5"))
    );
}

#[test]
fn ipv6_peers_and_forwarded_addresses_work() {
    assert_eq!(
        resolve("fd00::1", &["2001:db8::42"], "fd00::/8"),
        Some(ip("2001:db8::42"))
    );
}

#[test]
fn missing_peer_has_no_client_ip() {
    assert_eq!(client_ip(None, ["1.2.3.4"].into_iter(), &[]), None);
}

#[test]
fn trusted_proxy_list_parsing() {
    assert!(parse_trusted_proxies("").unwrap().is_empty());
    assert_eq!(
        parse_trusted_proxies(" 172.16.0.0/12 , 10.0.0.1 ,")
            .unwrap()
            .len(),
        2
    );
    assert!(parse_trusted_proxies("172.16.0.0/40").is_err());
    assert!(parse_trusted_proxies("proxy.local").is_err());
}

#[test]
fn session_key_accepts_hex_and_base64_of_at_least_64_bytes() {
    let bytes: Vec<u8> = (0..64).collect();
    assert!(parse_session_key(&hex::encode(&bytes)).is_ok());
    assert!(parse_session_key(&STANDARD.encode(&bytes)).is_ok());
    assert!(parse_session_key(&hex::encode(&bytes[..32])).is_err());
    assert!(parse_session_key(&STANDARD.encode(&bytes[..32])).is_err());
    assert!(parse_session_key("not a key!").is_err());
}

#[test]
fn cookie_secure_defaults_on() {
    assert!(parse_cookie_secure(None));
    assert!(parse_cookie_secure(Some("true")));
    assert!(parse_cookie_secure(Some("")));
    assert!(!parse_cookie_secure(Some("false")));
    assert!(!parse_cookie_secure(Some("FALSE")));
    assert!(!parse_cookie_secure(Some("0")));
}
