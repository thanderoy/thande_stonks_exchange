//! Guest participants: identities created by `POST /v1/join` on the public listener.

use rand::Rng;
use rust_decimal::Decimal;

pub const CLIENT_ID_PREFIX: &str = "g-";
const CLIENT_ID_RANDOM_LEN: usize = 10;
const CLIENT_ID_ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";

pub const DISPLAY_NAME_MIN: usize = 2;
pub const DISPLAY_NAME_MAX: usize = 24;

/// Risk limits every guest is registered with.
pub fn max_notional_per_order() -> Decimal {
    Decimal::from(100_000u32)
}

pub fn max_position() -> Decimal {
    Decimal::from(10_000u32)
}

pub const MAX_ORDERS_PER_SECOND: u32 = 5;

/// `g-` followed by ten random lowercase alphanumerics.
pub fn new_client_id() -> String {
    let mut rng = rand::thread_rng();
    let suffix: String = (0..CLIENT_ID_RANDOM_LEN)
        .map(|_| CLIENT_ID_ALPHABET[rng.gen_range(0..CLIENT_ID_ALPHABET.len())] as char)
        .collect();
    format!("{CLIENT_ID_PREFIX}{suffix}")
}

/// Trims the name and checks it is 2-24 characters of ASCII letters, digits, space, `_` or `-`.
pub fn validate_display_name(raw: &str) -> Result<String, String> {
    let name = raw.trim();
    let len = name.chars().count();
    if !(DISPLAY_NAME_MIN..=DISPLAY_NAME_MAX).contains(&len) {
        return Err(format!(
            "display_name must be {DISPLAY_NAME_MIN}-{DISPLAY_NAME_MAX} characters after trimming"
        ));
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '_' | '-'))
    {
        return Err(
            "display_name may contain only letters, digits, spaces, '_' and '-'".to_string(),
        );
    }
    Ok(name.to_string())
}
