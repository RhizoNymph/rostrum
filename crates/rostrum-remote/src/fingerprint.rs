//! The SHA-256 of the desktop's certificate: the only thing the phone trusts.
//!
//! `rostrumd` serves its API with a self-signed certificate. No certificate
//! authority vouches for it, and none needs to: the pairing link carries the
//! fingerprint, and the phone accepts that one certificate and nothing else.

use std::fmt;

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct CertFingerprint([u8; 32]);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FingerprintError {
    #[error("the fingerprint is not valid base64url")]
    Encoding,
    #[error("a fingerprint is 32 bytes, not {0}")]
    Length(usize),
}

impl CertFingerprint {
    /// Fingerprint a DER-encoded certificate.
    pub fn of_der(der: &[u8]) -> Self {
        Self(Sha256::digest(der).into())
    }

    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// The form the pairing link carries.
    pub fn to_base64url(&self) -> String {
        URL_SAFE_NO_PAD.encode(self.0)
    }

    pub fn from_base64url(text: &str) -> Result<Self, FingerprintError> {
        let bytes = URL_SAFE_NO_PAD
            .decode(text)
            .map_err(|_| FingerprintError::Encoding)?;
        let len = bytes.len();
        let bytes: [u8; 32] = bytes
            .try_into()
            .map_err(|_| FingerprintError::Length(len))?;
        Ok(Self(bytes))
    }

    /// Full lowercase hex, for logs and the desktop page.
    pub fn to_hex(&self) -> String {
        self.0.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    /// The first six bytes as three groups, `4F2A · 91C0 · 7E3B`: short enough
    /// to compare by eye between the phone and the desktop page.
    pub fn short(&self) -> String {
        self.0[..6]
            .chunks(2)
            .map(|pair| format!("{:02X}{:02X}", pair[0], pair[1]))
            .collect::<Vec<_>>()
            .join(" \u{00b7} ")
    }
}

impl fmt::Debug for CertFingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CertFingerprint({})", self.to_hex())
    }
}

impl Serialize for CertFingerprint {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_base64url())
    }
}

impl<'de> Deserialize<'de> for CertFingerprint {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::from_base64url(&text).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// FIPS 180-2's "abc" vector: proves this is SHA-256 over the raw bytes.
    #[test]
    fn fingerprints_are_sha256_of_the_der() {
        let fp = CertFingerprint::of_der(b"abc");
        assert_eq!(
            fp.to_hex(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn base64url_round_trips() {
        let fp = CertFingerprint::of_der(b"certificate");
        let text = fp.to_base64url();
        assert_eq!(text.len(), 43, "32 bytes unpadded");
        assert!(!text.contains('+') && !text.contains('/') && !text.contains('='));
        assert_eq!(CertFingerprint::from_base64url(&text), Ok(fp));
    }

    #[test]
    fn a_truncated_or_garbled_fingerprint_is_rejected() {
        assert_eq!(
            CertFingerprint::from_base64url("AAAA"),
            Err(FingerprintError::Length(3))
        );
        assert_eq!(
            CertFingerprint::from_base64url("not base64 at all!"),
            Err(FingerprintError::Encoding)
        );
    }

    #[test]
    fn the_short_form_is_three_groups_of_the_leading_bytes() {
        let fp = CertFingerprint::from_bytes([
            0x4f, 0x2a, 0x91, 0xc0, 0x7e, 0x3b, 0xff, 0xff, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ]);
        assert_eq!(fp.short(), "4F2A \u{00b7} 91C0 \u{00b7} 7E3B");
    }

    #[test]
    fn serde_uses_base64url() {
        let fp = CertFingerprint::of_der(b"x");
        let json = serde_json::to_string(&fp).expect("serialises");
        assert_eq!(json, format!("\"{}\"", fp.to_base64url()));
        let back: CertFingerprint = serde_json::from_str(&json).expect("parses");
        assert_eq!(back, fp);
    }
}
