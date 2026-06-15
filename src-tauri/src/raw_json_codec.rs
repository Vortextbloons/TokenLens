//! Compression helpers for the `usage_events.raw_json` column.
//!
//! New writes go to the `raw_json_zstd` BLOB column as a zstd-compressed
//! buffer. Reads pick the compressed copy if present and decompress
//! on-the-fly, falling back to the legacy `raw_json` TEXT column for rows
//! written before schema v3.
//!
//! Typical compression ratio for raw JSONL is 5-10×; a 1 KB row becomes
//! 100-200 bytes. At 100k events that's 100 MB → 10-20 MB on disk.

use rusqlite::types::Value;

/// zstd level 3 hits a good speed/ratio balance for JSONL. Higher levels
/// don't meaningfully improve ratio on small JSON documents.
const ZSTD_LEVEL: i32 = 3;

/// Compress a JSON string with zstd. Returns `None` for empty input so
/// the BLOB column stays NULL and we don't pay the dictionary overhead.
pub fn compress(s: &str) -> Option<Vec<u8>> {
    if s.is_empty() {
        return None;
    }
    zstd::encode_all(s.as_bytes(), ZSTD_LEVEL).ok()
}

/// Decompress a zstd payload back to a UTF-8 string. Returns `None` on
/// any error so the read path can fall through to the legacy column.
pub fn decompress(blob: &[u8]) -> Option<String> {
    let bytes = zstd::decode_all(blob).ok()?;
    String::from_utf8(bytes).ok()
}

/// Convert an optional JSON string into the SQLite `Value` pair used by
/// `INSERT` statements: `(zstd_blob, original_text)`. The text value is
/// always stored as well so debugging tools that don't know about the
/// compressed column can still read it via `sqlite3` CLI.
pub fn encode_for_insert(s: Option<&str>) -> (Value, Value) {
    let text_value = s.map(|t| Value::Text(t.to_string())).unwrap_or(Value::Null);
    let blob_value = s.and_then(compress).map(Value::Blob).unwrap_or(Value::Null);
    (blob_value, text_value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let original = r#"{"sessionID":"abc","tokens":{"input":100,"output":50}}"#;
        let compressed = compress(original).expect("compress");
        let decompressed = decompress(&compressed).expect("decompress");
        assert_eq!(decompressed, original);
        // For a small payload, zstd's framing may add a few bytes of
        // overhead. The compression wins on realistic JSONL rows
        // (>=1 KB), so we don't assert `compressed.len() < original.len()`
        // here.
    }

    #[test]
    fn empty_input() {
        assert!(compress("").is_none());
        assert_eq!(encode_for_insert(None), (Value::Null, Value::Null));
        assert_eq!(
            encode_for_insert(Some("")),
            (Value::Null, Value::Text(String::new()))
        );
    }
}
