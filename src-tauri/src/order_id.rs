/// System Order ID generation
///
/// Format:  `{bot_id:04}_{machine_hash:08}_{timestamp_ms:019}`
/// Example: `0001_a3f2b9c7_0001748262473512`
///
/// - `system_order_id` = UTC timestamp in milliseconds (u64)
/// - `bot_id`          = configured instance ID (default 1, fits in u16)
/// - `machine_hash`    = first 8 hex chars of SHA-256(hostname)
///
/// The full string is ≤ 35 characters, well within all exchange clOrdId limits
/// (Deribit label: 64 chars, OKX clOrdId: 32 chars, Bybit orderLinkId: 36 chars).

use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};

/// Returns the current UTC timestamp in milliseconds — this is the system_order_id.
pub fn new_system_order_id() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Returns the first 8 hex chars of SHA-256 of the machine hostname.
/// Stable for the lifetime of the process (computed once via lazy static).
fn machine_hash() -> &'static str {
    use std::sync::OnceLock;
    static HASH: OnceLock<String> = OnceLock::new();
    HASH.get_or_init(|| {
        let hostname = hostname::get()
            .map(|h| h.to_string_lossy().to_string())
            .unwrap_or_else(|_| "unknown".to_string());
        let digest = Sha256::digest(hostname.as_bytes());
        format!("{:02x}{:02x}{:02x}{:02x}", digest[0], digest[1], digest[2], digest[3])
    })
}

/// Build the full client order ID string.
///
/// `system_order_id` must be obtained from `new_system_order_id()`.
pub fn build_client_order_id(system_order_id: u64, bot_id: u16) -> String {
    format!("{:04}_{}_{}",
        bot_id,
        machine_hash(),
        system_order_id,
    )
}

/// Parse a client order ID back into its components.
/// Returns `(bot_id, machine_hash, system_order_id)` or None if format is invalid.
pub fn parse_client_order_id(clord_id: &str) -> Option<(u16, String, u64)> {
    let parts: Vec<&str> = clord_id.splitn(3, '_').collect();
    if parts.len() != 3 { return None; }
    let bot_id: u16 = parts[0].parse().ok()?;
    let machine_hash = parts[1].to_string();
    let ts_ms: u64 = parts[2].parse().ok()?;
    Some((bot_id, machine_hash, ts_ms))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let ts = new_system_order_id();
        let id = build_client_order_id(ts, 1);
        let (bot_id, _, parsed_ts) = parse_client_order_id(&id).expect("parse failed");
        assert_eq!(bot_id, 1);
        assert_eq!(parsed_ts, ts);
    }

    #[test]
    fn format_check() {
        let id = build_client_order_id(1748262473512, 42);
        assert!(id.starts_with("0042_"));
        assert!(id.ends_with("_1748262473512"));
    }
}
