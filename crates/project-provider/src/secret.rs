//! Redaction-safe secret handling.
//!
//! [`SecretString`] is a newtype over a credential string that never prints its
//! value through `Debug`/`Display` and is dropped immediately after use by the
//! caller. [`redact_credentials`] is a defensive second layer that scrubs
//! high-signal credential shapes from any logged string; it is a
//! belt-and-suspenders measure, not the primary defense.

use std::fmt;

/// A credential string that must never appear in logs, DTOs, or URLs.
///
/// There is intentionally no `Clone`: a credential should be held once, passed
/// by reference to the connector, and dropped. `Debug`/`Display` render a
/// fixed redacted marker.
pub struct SecretString {
    inner: String,
}

impl SecretString {
    pub fn new(value: String) -> Self {
        Self { inner: value }
    }

    /// Access the value for the single connect request. The caller must drop
    /// the `SecretString` immediately after the request is built.
    pub fn expose(&self) -> &str {
        &self.inner
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretString([REDACTED])")
    }
}

impl fmt::Display for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[REDACTED]")
    }
}

impl PartialEq for SecretString {
    fn eq(&self, other: &Self) -> bool {
        self.inner == other.inner
    }
}

impl Eq for SecretString {}

fn is_token_delimiter(c: char) -> bool {
    c.is_whitespace()
        || matches!(
            c,
            '"' | '\'' | ',' | ';' | '(' | ')' | '[' | ']' | '{' | '}' | '\\'
        )
}

/// Scrubs high-signal credential shapes from `text`: `sk-…`, `AIza…`, `gsk_…`,
/// and `Bearer …` tokens are replaced with a fixed marker. Non-credential text
/// is preserved byte-for-byte (character-for-character).
pub fn redact_credentials(text: &str) -> String {
    const PATTERNS: [(&str, &str); 4] = [
        ("sk-", "sk-[REDACTED]"),
        ("AIza", "AIza[REDACTED]"),
        ("gsk_", "gsk_[REDACTED]"),
        ("Bearer ", "Bearer [REDACTED]"),
    ];

    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        let remaining: String = chars[i..].iter().collect();
        let mut matched = false;
        for (needle, replacement) in PATTERNS {
            if remaining.starts_with(needle) {
                out.push_str(replacement);
                i += needle.chars().count();
                while i < chars.len() && !is_token_delimiter(chars[i]) {
                    i += 1;
                }
                matched = true;
                break;
            }
        }
        if !matched {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_and_display_are_redacted() {
        let secret = SecretString::new("sk-test-123".into());
        let debug = format!("{secret:?}");
        let display = format!("{secret}");
        assert!(!debug.contains("sk-test-123"));
        assert!(!display.contains("sk-test-123"));
        assert_eq!(debug, "SecretString([REDACTED])");
        assert_eq!(display, "[REDACTED]");
    }

    #[test]
    fn expose_returns_value() {
        let secret = SecretString::new("AIza-test".into());
        assert_eq!(secret.expose(), "AIza-test");
        assert!(!secret.is_empty());
        assert!(SecretString::new(String::new()).is_empty());
    }

    #[test]
    fn equality_compares_value() {
        assert_eq!(SecretString::new("x".into()), SecretString::new("x".into()));
        assert_ne!(SecretString::new("x".into()), SecretString::new("y".into()));
    }

    #[test]
    fn redact_credentials_scrubs_common_shapes() {
        let input =
            "key=sk-abcDEF123 token=gsk_lmnop Bearer eyJhbGci0 AIzaSyAZ09 secret and plain text";
        let out = redact_credentials(input);
        assert!(!out.contains("sk-abcDEF123"));
        assert!(!out.contains("gsk_lmnop"));
        assert!(!out.contains("eyJhbGci0"));
        assert!(!out.contains("AIzaSyAZ09"));
        assert!(out.contains("sk-[REDACTED]"));
        assert!(out.contains("gsk_[REDACTED]"));
        assert!(out.contains("Bearer [REDACTED]"));
        assert!(out.contains("AIza[REDACTED]"));
        assert!(out.contains("plain text"));
    }

    #[test]
    fn redact_credentials_leaves_plain_text_alone() {
        let input = "no secrets here, just words and 123 numbers";
        assert_eq!(redact_credentials(input), input);
    }
}
