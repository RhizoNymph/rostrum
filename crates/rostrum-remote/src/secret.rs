//! Secrets that cross the wire: device tokens, device ids, the GitHub token.
//!
//! Every type here redacts itself in `Debug`, so a stray `{:?}` in a log line
//! cannot leak one. The server never stores a [`DeviceToken`], only its
//! [`TokenHash`].

use std::fmt;

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SecretError {
    #[error("a device token is 32 bytes of base64url")]
    Token,
    #[error("a device id is 32 hex characters")]
    DeviceId,
    #[error("a token hash is 64 hex characters")]
    Hash,
}

/// The bearer credential a paired phone presents: 32 random bytes.
#[derive(Clone, PartialEq, Eq)]
pub struct DeviceToken(String);

impl DeviceToken {
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(URL_SAFE_NO_PAD.encode(bytes))
    }

    pub fn parse(text: &str) -> Result<Self, SecretError> {
        let bytes = URL_SAFE_NO_PAD
            .decode(text)
            .map_err(|_| SecretError::Token)?;
        let bytes: [u8; 32] = bytes.try_into().map_err(|_| SecretError::Token)?;
        Ok(Self::from_bytes(bytes))
    }

    /// The token as it goes into an `Authorization` header. Named so that every
    /// use is a deliberate one.
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// What the server keeps instead of the token.
    pub fn hash(&self) -> TokenHash {
        TokenHash(Sha256::digest(self.0.as_bytes()).into())
    }
}

impl fmt::Debug for DeviceToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("DeviceToken(redacted)")
    }
}

impl Serialize for DeviceToken {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for DeviceToken {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).map_err(serde::de::Error::custom)
    }
}

/// SHA-256 of a [`DeviceToken`], stored server-side and compared in constant
/// time.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct TokenHash([u8; 32]);

impl TokenHash {
    pub fn ct_eq(&self, other: &Self) -> bool {
        self.0
            .iter()
            .zip(other.0.iter())
            .fold(0u8, |acc, (a, b)| acc | (a ^ b))
            == 0
    }

    pub fn to_hex(&self) -> String {
        self.0.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    pub fn from_hex(text: &str) -> Result<Self, SecretError> {
        let bytes = decode_hex(text).ok_or(SecretError::Hash)?;
        let bytes: [u8; 32] = bytes.try_into().map_err(|_| SecretError::Hash)?;
        Ok(Self(bytes))
    }
}

impl fmt::Debug for TokenHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TokenHash({}…)", &self.to_hex()[..8])
    }
}

impl Serialize for TokenHash {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for TokenHash {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::from_hex(&text).map_err(serde::de::Error::custom)
    }
}

/// A paired device's public identifier: 16 random bytes as hex. Not a secret —
/// it names the device on the desktop page so it can be revoked.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DeviceId(String);

impl DeviceId {
    pub fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
    }

    pub fn parse(text: &str) -> Result<Self, SecretError> {
        let bytes = decode_hex(text).ok_or(SecretError::DeviceId)?;
        let bytes: [u8; 16] = bytes.try_into().map_err(|_| SecretError::DeviceId)?;
        Ok(Self::from_bytes(bytes))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for DeviceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Serialize for DeviceId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for DeviceId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).map_err(serde::de::Error::custom)
    }
}

/// A GitHub token handed from the desktop to a paired phone.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct GitHubToken(String);

impl GitHubToken {
    pub fn new(raw: impl Into<String>) -> Self {
        Self(raw.into())
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for GitHubToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("GitHubToken(redacted)")
    }
}

fn decode_hex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|ix| u8::from_str_radix(text.get(ix..ix + 2)?, 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_token_round_trips_through_its_text() {
        let token = DeviceToken::from_bytes([7; 32]);
        assert_eq!(token.expose().len(), 43);
        assert_eq!(DeviceToken::parse(token.expose()), Ok(token));
    }

    #[test]
    fn a_short_or_garbled_token_is_rejected() {
        assert_eq!(DeviceToken::parse("abc"), Err(SecretError::Token));
        assert_eq!(DeviceToken::parse("!!!"), Err(SecretError::Token));
    }

    #[test]
    fn the_hash_is_stable_and_distinguishes_tokens() {
        let a = DeviceToken::from_bytes([1; 32]);
        let b = DeviceToken::from_bytes([2; 32]);
        assert!(a.hash().ct_eq(&a.clone().hash()));
        assert!(!a.hash().ct_eq(&b.hash()));
        assert_eq!(TokenHash::from_hex(&a.hash().to_hex()), Ok(a.hash()));
    }

    #[test]
    fn secrets_are_redacted_in_debug_output() {
        let token = DeviceToken::from_bytes([9; 32]);
        assert!(!format!("{token:?}").contains(token.expose()));
        let github = GitHubToken::new("gho_secret");
        assert!(!format!("{github:?}").contains("gho_secret"));
    }

    #[test]
    fn device_ids_are_32_hex_characters() {
        let id = DeviceId::from_bytes([0xab; 16]);
        assert_eq!(id.as_str().len(), 32);
        assert_eq!(DeviceId::parse(id.as_str()), Ok(id));
        assert_eq!(DeviceId::parse("xyz"), Err(SecretError::DeviceId));
        assert_eq!(DeviceId::parse("abcd"), Err(SecretError::DeviceId));
    }

    #[test]
    fn tokens_serialise_as_their_text() {
        let token = DeviceToken::from_bytes([3; 32]);
        let json = serde_json::to_string(&token).expect("serialises");
        assert_eq!(json, format!("\"{}\"", token.expose()));
        let back: DeviceToken = serde_json::from_str(&json).expect("parses");
        assert_eq!(back, token);
    }
}
