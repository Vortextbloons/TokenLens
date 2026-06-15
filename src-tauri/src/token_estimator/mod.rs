//! Local token estimation. Two modes:
//! - `chars4`: simple chars/4 heuristic. Fast and language-agnostic.
//! - `tiktoken`: model-specific BPE (Phase 4+). Not implemented yet, but the
//!   hook is here so we can drop it in later.

/// Estimate tokens from text using the configured mode.
///
/// `mode` is one of: "off", "chars4", "tiktoken".
/// `model_hint` lets the future tiktoken path pick a BPE.
pub fn estimate_tokens(text: &str, mode: &str, _model_hint: Option<&str>) -> i64 {
    if text.is_empty() {
        return 0;
    }
    match mode {
        "off" => 0,
        "chars4" => chars_div4(text),
        "tiktoken" => {
            // Fallback to chars/4 for now. Real implementation lands in Phase 4.
            chars_div4(text)
        }
        _ => chars_div4(text),
    }
}

fn chars_div4(s: &str) -> i64 {
    // Count unicode scalar values, not bytes, so emoji/CJK don't bloat.
    let count = s.chars().count() as i64;
    (count + 3) / 4
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty() {
        assert_eq!(estimate_tokens("", "chars4", None), 0);
    }

    #[test]
    fn ascii_4_chars() {
        assert_eq!(estimate_tokens("abcd", "chars4", None), 1);
        assert_eq!(estimate_tokens("abcdefgh", "chars4", None), 2);
    }

    #[test]
    fn unicode_counted_as_chars() {
        // 4 emoji = 4 chars => 1 token
        let s = "🚀🚀🚀🚀";
        assert_eq!(estimate_tokens(s, "chars4", None), 1);
    }

    #[test]
    fn off_mode_zero() {
        assert_eq!(estimate_tokens("hello world", "off", None), 0);
    }
}
