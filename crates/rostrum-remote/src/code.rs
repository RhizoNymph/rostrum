//! Pairing codes: eight Crockford base32 symbols, shown as `XXXX-XXXX`.
//!
//! Forty bits from the server's random source. A code lives for minutes, is
//! consumed by its first successful use, and the server throttles failed
//! attempts, so guessing one is not a practical attack; what the code has to be
//! is easy to read aloud and type on a phone keyboard. Crockford's alphabet
//! drops `I`, `L`, `O` and `U`, and parsing folds the look-alikes back, so a
//! code typed from a screen with `O` for `0` still works.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// A pairing code, always exactly [`PairingCode::LEN`] symbols of the alphabet.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct PairingCode([u8; PairingCode::LEN]);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PairingCodeError {
    #[error("a pairing code has {expected} characters, not {found}")]
    Length { expected: usize, found: usize },
    #[error("`{0}` is not a character a pairing code can contain")]
    Symbol(char),
}

impl PairingCode {
    pub const LEN: usize = 8;

    /// Encode forty random bits. The caller supplies the entropy so this crate
    /// needs no random source of its own.
    pub fn from_entropy(bytes: [u8; 5]) -> Self {
        let bits = bytes
            .iter()
            .fold(0u64, |acc, &byte| (acc << 8) | u64::from(byte));
        let mut symbols = [0u8; Self::LEN];
        for (ix, symbol) in symbols.iter_mut().enumerate() {
            let shift = 5 * (Self::LEN - 1 - ix);
            *symbol = ALPHABET[((bits >> shift) & 0x1f) as usize];
        }
        Self(symbols)
    }

    /// Parse what a person typed: case-insensitive, separators ignored, and
    /// `O`→`0`, `I`/`L`→`1` folded as Crockford specifies.
    pub fn parse(input: &str) -> Result<Self, PairingCodeError> {
        let mut symbols = Vec::with_capacity(Self::LEN);
        for ch in input.chars() {
            if ch == '-' || ch == ' ' || ch == '\u{00b7}' {
                continue;
            }
            let folded = match ch.to_ascii_uppercase() {
                'O' => '0',
                'I' | 'L' => '1',
                other => other,
            };
            if !folded.is_ascii() || !ALPHABET.contains(&(folded as u8)) {
                return Err(PairingCodeError::Symbol(ch));
            }
            symbols.push(folded as u8);
        }
        let found = symbols.len();
        let symbols: [u8; Self::LEN] =
            symbols.try_into().map_err(|_| PairingCodeError::Length {
                expected: Self::LEN,
                found,
            })?;
        Ok(Self(symbols))
    }

    /// The compact form, as it travels: `K7QXM2PD`.
    pub fn as_str(&self) -> &str {
        // Every byte is drawn from ALPHABET, which is ASCII.
        std::str::from_utf8(&self.0).unwrap_or_default()
    }

    /// Compare without an early exit, so response timing says nothing about
    /// how much of a guess was right.
    pub fn ct_eq(&self, other: &Self) -> bool {
        self.0
            .iter()
            .zip(other.0.iter())
            .fold(0u8, |acc, (a, b)| acc | (a ^ b))
            == 0
    }
}

/// The grouped form a person reads: `K7QX-M2PD`.
impl fmt::Display for PairingCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = self.as_str();
        write!(f, "{}-{}", &text[..4], &text[4..])
    }
}

/// Redacted: a live code in a log line is a credential in a log line.
impl fmt::Debug for PairingCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PairingCode(****-****)")
    }
}

impl Serialize for PairingCode {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for PairingCode {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entropy_encodes_to_eight_alphabet_symbols() {
        let code = PairingCode::from_entropy([0xff, 0x00, 0xa5, 0x5a, 0x01]);
        assert_eq!(code.as_str().len(), PairingCode::LEN);
        assert!(code.as_str().bytes().all(|b| ALPHABET.contains(&b)));
    }

    #[test]
    fn the_encoding_is_the_big_endian_five_bit_split() {
        assert_eq!(PairingCode::from_entropy([0; 5]).as_str(), "00000000");
        assert_eq!(PairingCode::from_entropy([0xff; 5]).as_str(), "ZZZZZZZZ");
        // 0x0000000001 → the last symbol is 1.
        assert_eq!(
            PairingCode::from_entropy([0, 0, 0, 0, 1]).as_str(),
            "00000001"
        );
    }

    #[test]
    fn different_entropy_gives_different_codes() {
        let a = PairingCode::from_entropy([1, 2, 3, 4, 5]);
        let b = PairingCode::from_entropy([1, 2, 3, 4, 6]);
        assert_ne!(a, b);
        assert!(!a.ct_eq(&b));
        assert!(a.ct_eq(&a.clone()));
    }

    #[test]
    fn a_code_round_trips_through_its_display_form() {
        let code = PairingCode::from_entropy([9, 8, 7, 6, 5]);
        let shown = code.to_string();
        assert_eq!(shown.len(), 9);
        assert_eq!(&shown[4..5], "-");
        assert_eq!(PairingCode::parse(&shown), Ok(code));
    }

    #[test]
    fn parsing_forgives_case_separators_and_look_alikes() {
        let code = PairingCode::parse("K7QX-M2PD").expect("valid");
        assert_eq!(PairingCode::parse("k7qx m2pd"), Ok(code.clone()));
        assert_eq!(PairingCode::parse("k7qxm2pd"), Ok(code));
        assert_eq!(
            PairingCode::parse("OOOO-IIII").expect("folds").as_str(),
            "00001111"
        );
        assert_eq!(
            PairingCode::parse("llll-0000").expect("folds").as_str(),
            "11110000"
        );
    }

    #[test]
    fn parsing_rejects_symbols_outside_the_alphabet() {
        assert_eq!(
            PairingCode::parse("ABCD-EFGU"),
            Err(PairingCodeError::Symbol('U'))
        );
        assert_eq!(
            PairingCode::parse("ABCD-EF!H"),
            Err(PairingCodeError::Symbol('!'))
        );
        assert!(matches!(
            PairingCode::parse("ABCD-EFGé"),
            Err(PairingCodeError::Symbol('é'))
        ));
    }

    #[test]
    fn parsing_rejects_the_wrong_length() {
        assert_eq!(
            PairingCode::parse("ABC"),
            Err(PairingCodeError::Length {
                expected: 8,
                found: 3
            })
        );
        assert_eq!(
            PairingCode::parse("ABCD-EFGH-J"),
            Err(PairingCodeError::Length {
                expected: 8,
                found: 9
            })
        );
    }

    #[test]
    fn debug_output_never_shows_the_code() {
        let code = PairingCode::parse("K7QX-M2PD").expect("valid");
        assert!(!format!("{code:?}").contains("K7QX"));
    }

    #[test]
    fn serde_uses_the_compact_form() {
        let code = PairingCode::parse("K7QX-M2PD").expect("valid");
        let json = serde_json::to_string(&code).expect("serialises");
        assert_eq!(json, "\"K7QXM2PD\"");
        let back: PairingCode = serde_json::from_str("\"k7qx-m2pd\"").expect("parses");
        assert_eq!(back, code);
        assert!(serde_json::from_str::<PairingCode>("\"nope\"").is_err());
    }
}
