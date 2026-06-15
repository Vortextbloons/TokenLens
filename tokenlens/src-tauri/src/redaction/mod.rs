//! Secret redaction. Strips obvious API keys, tokens, private keys, etc.
//! before persisting raw JSON. Conservative — when in doubt, redact.

use regex::Regex;
use std::sync::OnceLock;

struct Patterns {
    #[allow(dead_code)]
    raw: Vec<(&'static str, &'static str)>,
    compiled: Vec<(&'static str, Regex)>,
}

static PATTERNS: OnceLock<Patterns> = OnceLock::new();

fn get_patterns() -> &'static Patterns {
    PATTERNS.get_or_init(|| {
        // Order matters: more specific patterns first.
        let raw: Vec<(&'static str, &'static str)> = vec![
            // OpenAI
            ("openai_key", r"sk-[A-Za-z0-9_\-]{20,}"),
            ("openai_proj_key", r"sk-proj-[A-Za-z0-9_\-]{20,}"),
            // Anthropic
            ("anthropic_key", r"sk-ant-[A-Za-z0-9_\-]{20,}"),
            // Google Gemini / PaLM
            ("google_key", r"AIza[A-Za-z0-9_\-]{35}"),
            // GitHub
            ("github_pat", r"ghp_[A-Za-z0-9]{36}"),
            ("github_oauth", r"gho_[A-Za-z0-9]{36}"),
            ("github_user", r"ghu_[A-Za-z0-9]{36}"),
            ("github_server", r"ghs_[A-Za-z0-9]{36}"),
            ("github_refresh", r"ghr_[A-Za-z0-9]{36}"),
            ("github_fine", r"github_pat_[A-Za-z0-9_]{82}"),
            // AWS
            ("aws_access_key", r"AKIA[0-9A-Z]{16}"),
            ("aws_secret_key", r#"(?i)aws(.{0,20})?(secret|sk).{0,5}['"][0-9a-zA-Z/+=]{40}['"]"#),
            // Slack
            ("slack_token", r"xox[abprs]-[A-Za-z0-9-]{10,}"),
            // Stripe
            ("stripe_key", r"sk_(?:live|test)_[A-Za-z0-9]{24,}"),
            ("stripe_restricted", r"rk_(?:live|test)_[A-Za-z0-9]{24,}"),
            // HuggingFace
            ("hf_token", r"hf_[A-Za-z0-9]{20,}"),
            // JWT (heuristic)
            ("jwt", r"eyJ[A-Za-z0-9_\-]{10,}\.eyJ[A-Za-z0-9_\-]{10,}\.[A-Za-z0-9_\-]{10,}"),
            // Private key blocks
            ("private_key", r"-----BEGIN [A-Z ]+PRIVATE KEY-----[\s\S]*?-----END [A-Z ]+PRIVATE KEY-----"),
            // Bearer tokens
            ("bearer", r"(?i)bearer\s+[A-Za-z0-9._\-]{20,}"),
            // Generic env-var style secrets
            (
                "env_secret",
                r#"(?im)\b(api[_-]?key|secret|access[_-]?token|auth[_-]?token|password|passwd|pwd|token|private[_-]?key)\b\s*[:=]\s*['"]?([A-Za-z0-9._\-/+=]{16,})['"]?"#,
            ),
        ];
        let compiled = raw
            .iter()
            .filter_map(|(name, pat)| Regex::new(pat).ok().map(|r| (*name, r)))
            .collect();
        Patterns {
            raw,
            compiled,
        }
    })
}

/// Redact obvious secrets in `input`, replacing with `[REDACTED:NAME]`.
pub fn redact(input: &str) -> String {
    let p = get_patterns();
    let mut out = input.to_string();
    for (name, re) in &p.compiled {
        out = re.replace_all(&out, &format!("[REDACTED:{name}]")).into_owned();
    }
    out
}

/// True if the input contains anything that looks like a secret.
pub fn contains_secret(input: &str) -> bool {
    let p = get_patterns();
    p.compiled.iter().any(|(_, re)| re.is_match(input))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_openai() {
        let s = "Hello sk-abcdefghijklmnopqrstuvwxyz1234567890 end";
        let r = redact(s);
        assert!(r.contains("[REDACTED:openai_key]"));
    }

    #[test]
    fn redacts_github_pat() {
        let s = "Token ghp_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let r = redact(s);
        assert!(r.contains("[REDACTED:github_pat]"));
    }

    #[test]
    fn redacts_private_key() {
        let s = "-----BEGIN RSA PRIVATE KEY-----\nABC\n-----END RSA PRIVATE KEY-----";
        let r = redact(s);
        assert!(r.contains("[REDACTED:private_key]"));
    }

    #[test]
    fn redacts_jwt() {
        let s = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NSJ9.abcdefghijklmnop";
        let r = redact(s);
        assert!(r.contains("[REDACTED:jwt]"));
    }

    #[test]
    fn no_op_on_clean() {
        let s = "This is a perfectly normal log message with no secrets.";
        let r = redact(s);
        assert_eq!(r, s);
    }
}
