use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::error::{PreviewError, PreviewResult};

/// Opaque 128-bit random capability token for a single preview server instance.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct PreviewToken([u8; 16]);

impl PreviewToken {
    /// Draws 128 bits from the OS random source.
    pub fn generate() -> PreviewResult<Self> {
        let mut bytes = [0u8; 16];
        getrandom::fill(&mut bytes).map_err(|e| PreviewError::Entropy(e.to_string()))?;
        Ok(Self(bytes))
    }

    /// Parses a 32-character hexadecimal token.
    pub fn parse(value: impl AsRef<str>) -> PreviewResult<Self> {
        let s = value.as_ref();
        if s.len() != 32 {
            return Err(PreviewError::InvalidToken(s.to_string()));
        }
        let mut bytes = [0u8; 16];
        for (i, chunk) in s.as_bytes().chunks_exact(2).enumerate() {
            let hi = hex_val(chunk[0]).ok_or_else(|| PreviewError::InvalidToken(s.to_string()))?;
            let lo = hex_val(chunk[1]).ok_or_else(|| PreviewError::InvalidToken(s.to_string()))?;
            bytes[i] = (hi << 4) | lo;
        }
        Ok(Self(bytes))
    }
}

impl fmt::Display for PreviewToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for b in self.0 {
            write!(f, "{b:02x}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for PreviewToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("PreviewToken")
            .field(&self.to_string())
            .finish()
    }
}

impl FromStr for PreviewToken {
    type Err = PreviewError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

impl Serialize for PreviewToken {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for PreviewToken {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Self::parse(s).map_err(serde::de::Error::custom)
    }
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_is_128_bit_hex_and_unique() {
        let a = PreviewToken::generate().unwrap();
        let b = PreviewToken::generate().unwrap();
        assert_eq!(a.to_string().len(), 32);
        assert!(a.to_string().chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
        assert_eq!(a, a);
    }

    #[test]
    fn parse_roundtrips_display() {
        let token = PreviewToken::generate().unwrap();
        let parsed = PreviewToken::parse(token.to_string()).unwrap();
        assert_eq!(token, parsed);
        assert_eq!(token, token.to_string().parse::<PreviewToken>().unwrap());
    }

    #[test]
    fn parse_rejects_invalid() {
        assert!(PreviewToken::parse("").is_err());
        assert!(PreviewToken::parse("abc").is_err());
        assert!(PreviewToken::parse("g".repeat(32)).is_err());
        assert!(PreviewToken::parse("0".repeat(31)).is_err());
        assert!(PreviewToken::parse("0".repeat(33)).is_err());
    }

    #[test]
    fn serde_roundtrip() {
        let token = PreviewToken::generate().unwrap();
        let json = serde_json::to_string(&token).unwrap();
        assert_eq!(json, format!("\"{token}\""));
        let back: PreviewToken = serde_json::from_str(&json).unwrap();
        assert_eq!(back, token);
    }
}
