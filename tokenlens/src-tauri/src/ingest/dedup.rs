//! Deduplication. Each event gets a stable SHA-256 hash from a canonicalized
//! subset of fields. The DB enforces uniqueness via UNIQUE(event_hash).

use sha2::{Digest, Sha256};

/// Build a canonical event hash from identifying fields. We deliberately
/// include timestamp + model + total + session + provider so that legitimate
/// re-runs of the same model on the same session at the same moment collapse
/// to one row, but different events do not.
pub fn hash_event(
    timestamp_iso: &str,
    provider: &str,
    model: &str,
    session_id: &str,
    event_type: &str,
    total_tokens: i64,
    input_tokens: i64,
    output_tokens: i64,
) -> String {
    let mut h = Sha256::new();
    h.update(timestamp_iso.as_bytes());
    h.update(b"|");
    h.update(provider.as_bytes());
    h.update(b"|");
    h.update(model.as_bytes());
    h.update(b"|");
    h.update(session_id.as_bytes());
    h.update(b"|");
    h.update(event_type.as_bytes());
    h.update(b"|");
    h.update(total_tokens.to_le_bytes());
    h.update(b"|");
    h.update(input_tokens.to_le_bytes());
    h.update(b"|");
    h.update(output_tokens.to_le_bytes());
    hex::encode(h.finalize())
}

mod hex {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    pub fn encode(bytes: impl AsRef<[u8]>) -> String {
        let bytes = bytes.as_ref();
        let mut s = String::with_capacity(bytes.len() * 2);
        for b in bytes {
            s.push(HEX[(b >> 4) as usize] as char);
            s.push(HEX[(b & 0x0F) as usize] as char);
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_inputs_same_hash() {
        let h1 = hash_event("2025-01-01T00:00:00Z", "openai", "gpt-4o", "s1", "message", 100, 50, 50);
        let h2 = hash_event("2025-01-01T00:00:00Z", "openai", "gpt-4o", "s1", "message", 100, 50, 50);
        assert_eq!(h1, h2);
    }

    #[test]
    fn different_totals_different_hash() {
        let h1 = hash_event("2025-01-01T00:00:00Z", "openai", "gpt-4o", "s1", "message", 100, 50, 50);
        let h2 = hash_event("2025-01-01T00:00:00Z", "openai", "gpt-4o", "s1", "message", 101, 50, 50);
        assert_ne!(h1, h2);
    }

    #[test]
    fn hash_is_64_hex() {
        let h = hash_event("t", "p", "m", "s", "e", 0, 0, 0);
        assert_eq!(h.len(), 64);
        assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
