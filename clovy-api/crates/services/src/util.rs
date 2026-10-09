use crate::error::ServiceError;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, time::Duration};

pub(crate) fn ceil_seconds(duration: Duration) -> Result<u64, ServiceError> {
    let millis = u64::try_from(duration.as_millis()).map_err(|_| ServiceError::PriceOverflow)?;
    millis
        .checked_add(999)
        .map(|value| value / 1000)
        .ok_or(ServiceError::PriceOverflow)
}

/// The model a media request is served with: the named model when it is
/// priced, otherwise `default_model` — for a request that names none, or a
/// retired or unknown id a shipped client keeps sending (ADR 0021: map an id
/// that stopped being served to a working fallback, never reject it).
pub(crate) fn served_model<V>(
    pricing: &BTreeMap<String, V>,
    requested: Option<&str>,
    default_model: &str,
) -> String {
    requested
        .map(str::trim)
        .filter(|model| !model.is_empty() && pricing.contains_key(*model))
        .unwrap_or(default_model)
        .to_string()
}

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    hex_lower(&digest)
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}
