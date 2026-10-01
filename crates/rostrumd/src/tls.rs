//! The API's self-signed certificate: generated once, kept forever.
//!
//! A phone trusts this certificate by the SHA-256 of its DER, carried in the
//! pairing link. Regenerating it would silently unpair every phone, so it is
//! only ever generated when there is none, and a half-present identity is an
//! error to fix by hand rather than something to paper over.
//!
//! `cert.pem` is the commit point. The key is written first; a key with no
//! certificate beside it can only be the remains of an interrupted first run
//! (no phone can have pinned a certificate that was never written), so it is
//! replaced. A certificate with no key is refused.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use rostrum_remote::CertFingerprint;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject};

use crate::fsutil::{ensure_private_dir, write_private};

pub const CERT_FILE: &str = "cert.pem";
pub const KEY_FILE: &str = "key.pem";

/// The certificate and key the API serves, and the fingerprint phones pin.
pub struct TlsIdentity {
    cert: CertificateDer<'static>,
    key: PrivateKeyDer<'static>,
    fingerprint: CertFingerprint,
}

/// Whether [`TlsIdentity::load_or_create`] found an identity or made one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Provenance {
    Loaded,
    Generated,
}

#[derive(Debug, thiserror::Error)]
pub enum TlsError {
    #[error("could not read {}", path.display())]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not write {}", path.display())]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{} does not hold a PEM {what}: {reason}", path.display())]
    Pem {
        path: PathBuf,
        what: &'static str,
        reason: String,
    },
    #[error(
        "{} exists but {} does not; restore the key, or delete {} to generate a new identity (every phone will need to pair again)",
        cert.display(), key.display(), dir.display()
    )]
    MissingKey {
        cert: PathBuf,
        key: PathBuf,
        dir: PathBuf,
    },
    #[error("could not generate a certificate: {0}")]
    Generate(#[from] rcgen::Error),
    #[error("the certificate and key do not make a usable TLS configuration: {0}")]
    Config(#[from] rustls::Error),
}

impl TlsIdentity {
    /// Load `<dir>/cert.pem` and `<dir>/key.pem`, or generate both if there is
    /// no certificate yet.
    pub fn load_or_create(dir: &Path, machine: &str) -> Result<(Self, Provenance), TlsError> {
        let cert_path = dir.join(CERT_FILE);
        let key_path = dir.join(KEY_FILE);
        match (cert_path.exists(), key_path.exists()) {
            (true, true) => Ok((Self::load(&cert_path, &key_path)?, Provenance::Loaded)),
            (true, false) => Err(TlsError::MissingKey {
                cert: cert_path,
                key: key_path,
                dir: dir.to_path_buf(),
            }),
            (false, _) => Ok((
                Self::generate(dir, &cert_path, &key_path, machine)?,
                Provenance::Generated,
            )),
        }
    }

    fn load(cert_path: &Path, key_path: &Path) -> Result<Self, TlsError> {
        let read = |path: &Path| {
            std::fs::read(path).map_err(|source| TlsError::Read {
                path: path.to_path_buf(),
                source,
            })
        };
        let cert_pem = read(cert_path)?;
        let key_pem = read(key_path)?;
        let cert = CertificateDer::from_pem_slice(&cert_pem).map_err(|err| TlsError::Pem {
            path: cert_path.to_path_buf(),
            what: "certificate",
            reason: err.to_string(),
        })?;
        let key = PrivateKeyDer::from_pem_slice(&key_pem).map_err(|err| TlsError::Pem {
            path: key_path.to_path_buf(),
            what: "private key",
            reason: err.to_string(),
        })?;
        let identity = Self::from_parts(cert, key);
        // Prove the pair belongs together now, not at the first handshake.
        identity.server_config()?;
        Ok(identity)
    }

    fn generate(
        dir: &Path,
        cert_path: &Path,
        key_path: &Path,
        machine: &str,
    ) -> Result<Self, TlsError> {
        let mut params =
            rcgen::CertificateParams::new(vec!["rostrumd".to_string(), "localhost".to_string()])?;
        params
            .distinguished_name
            .push(rcgen::DnType::CommonName, format!("rostrumd on {machine}"));
        let key_pair = rcgen::KeyPair::generate()?;
        let cert = params.self_signed(&key_pair)?;

        let write = |path: &Path, text: &str| {
            write_private(path, text.as_bytes()).map_err(|source| TlsError::Write {
                path: path.to_path_buf(),
                source,
            })
        };
        ensure_private_dir(dir).map_err(|source| TlsError::Write {
            path: dir.to_path_buf(),
            source,
        })?;
        // Key first: the certificate's presence is what marks the pair
        // complete.
        write(key_path, &key_pair.serialize_pem())?;
        write(cert_path, &cert.pem())?;

        let key =
            PrivateKeyDer::try_from(key_pair.serialize_der()).map_err(|reason| TlsError::Pem {
                path: key_path.to_path_buf(),
                what: "private key",
                reason: reason.to_string(),
            })?;
        Ok(Self::from_parts(cert.der().clone(), key))
    }

    fn from_parts(cert: CertificateDer<'static>, key: PrivateKeyDer<'static>) -> Self {
        let fingerprint = CertFingerprint::of_der(cert.as_ref());
        Self {
            cert,
            key,
            fingerprint,
        }
    }

    pub fn fingerprint(&self) -> CertFingerprint {
        self.fingerprint
    }

    pub fn cert_der(&self) -> &CertificateDer<'static> {
        &self.cert
    }

    /// A server configuration using the `ring` provider explicitly, so nothing
    /// depends on which provider happens to be the process default.
    pub fn server_config(&self) -> Result<rustls::ServerConfig, TlsError> {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let config = rustls::ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()?
            .with_no_client_auth()
            .with_single_cert(vec![self.cert.clone()], self.key.clone_key())?;
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;
    use crate::fsutil::ScratchDir;

    #[test]
    fn a_generated_identity_is_reused_with_the_same_fingerprint() {
        let scratch = ScratchDir::new("tls-reuse");
        let dir = scratch.join("tls");
        let (first, provenance) = TlsIdentity::load_or_create(&dir, "desk").expect("generate");
        assert_eq!(provenance, Provenance::Generated);
        let (second, provenance) = TlsIdentity::load_or_create(&dir, "desk").expect("load");
        assert_eq!(provenance, Provenance::Loaded);
        assert_eq!(first.fingerprint(), second.fingerprint());
        assert_eq!(
            first.fingerprint(),
            CertFingerprint::of_der(second.cert_der().as_ref())
        );
        second.server_config().expect("usable");
    }

    #[test]
    fn the_key_is_private_and_the_directory_owner_only() {
        let scratch = ScratchDir::new("tls-mode");
        let dir = scratch.join("tls");
        TlsIdentity::load_or_create(&dir, "desk").expect("generate");
        let mode = |path: &Path| {
            std::fs::metadata(path)
                .expect("metadata")
                .permissions()
                .mode()
                & 0o777
        };
        assert_eq!(mode(&dir.join(KEY_FILE)), 0o600);
        assert_eq!(mode(&dir), 0o700);
    }

    #[test]
    fn a_certificate_without_its_key_is_refused_not_regenerated() {
        let scratch = ScratchDir::new("tls-nokey");
        let dir = scratch.join("tls");
        TlsIdentity::load_or_create(&dir, "desk").expect("generate");
        let before = std::fs::read(dir.join(CERT_FILE)).expect("cert");
        std::fs::remove_file(dir.join(KEY_FILE)).expect("remove key");
        assert!(matches!(
            TlsIdentity::load_or_create(&dir, "desk"),
            Err(TlsError::MissingKey { .. })
        ));
        assert_eq!(std::fs::read(dir.join(CERT_FILE)).expect("cert"), before);
    }

    #[test]
    fn a_key_left_by_an_interrupted_first_run_is_replaced() {
        let scratch = ScratchDir::new("tls-orphan");
        let dir = scratch.join("tls");
        std::fs::create_dir_all(&dir).expect("dir");
        std::fs::write(dir.join(KEY_FILE), "partial").expect("orphan key");
        let (_, provenance) = TlsIdentity::load_or_create(&dir, "desk").expect("generate");
        assert_eq!(provenance, Provenance::Generated);
        TlsIdentity::load_or_create(&dir, "desk").expect("now loads");
    }

    #[test]
    fn a_corrupt_certificate_is_an_error() {
        let scratch = ScratchDir::new("tls-corrupt");
        let dir = scratch.join("tls");
        TlsIdentity::load_or_create(&dir, "desk").expect("generate");
        std::fs::write(dir.join(CERT_FILE), "not pem").expect("corrupt");
        assert!(matches!(
            TlsIdentity::load_or_create(&dir, "desk"),
            Err(TlsError::Pem {
                what: "certificate",
                ..
            })
        ));
    }
}
