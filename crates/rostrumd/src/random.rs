//! Secrets from the operating system's random source.
//!
//! `rostrum-remote` deliberately has no random source of its own; the
//! constructors there take bytes, and this is where the bytes come from.

use rostrum_remote::{DeviceId, DeviceToken, PairingCode};

#[derive(Debug, thiserror::Error)]
#[error("the operating system's random source failed: {0}")]
pub struct RandomError(getrandom::Error);

fn bytes<const N: usize>() -> Result<[u8; N], RandomError> {
    let mut buf = [0u8; N];
    getrandom::fill(&mut buf).map_err(RandomError)?;
    Ok(buf)
}

/// 32 random bytes: the bearer credential a paired phone presents.
pub fn device_token() -> Result<DeviceToken, RandomError> {
    bytes::<32>().map(DeviceToken::from_bytes)
}

/// 16 random bytes: a paired device's public name for revoking it.
pub fn device_id() -> Result<DeviceId, RandomError> {
    bytes::<16>().map(DeviceId::from_bytes)
}

/// 40 random bits as eight Crockford symbols.
pub fn pairing_code() -> Result<PairingCode, RandomError> {
    bytes::<5>().map(PairingCode::from_entropy)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_are_well_formed_and_do_not_repeat() {
        let a = device_token().expect("token");
        let b = device_token().expect("token");
        assert_ne!(a, b);
        assert_eq!(DeviceToken::parse(a.expose()), Ok(a));

        let id = device_id().expect("id");
        assert_eq!(id.as_str().len(), 32);

        let codes: std::collections::HashSet<_> = (0..64)
            .map(|_| pairing_code().expect("code").as_str().to_string())
            .collect();
        assert!(codes.len() > 60, "codes should essentially never repeat");
    }
}
