//! The phone's HTTPS client for `rostrumd`.
//!
//! Trust is a single certificate fingerprint from the pairing link: no CA
//! roots, no hostname check — the desktop is reached by LAN IP, tailnet IP, or
//! MagicDNS name, and the same self-signed certificate answers on all of them.
//!
//! A desktop has several addresses and which one works depends on where the
//! phone is. Each call tries the endpoint's hosts in order, starting from the
//! one that last worked, and moves on only when a host cannot be *connected*
//! to. Once a request has been sent it is never retried elsewhere: a job that
//! timed out on one address may still be running, and running it twice would
//! rebase twice.

use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use reqwest::{Method, StatusCode};
use rustls::{
    DigitallySignedStruct, SignatureScheme,
    client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    crypto::{CryptoProvider, verify_tls12_signature, verify_tls13_signature},
    pki_types::{CertificateDer, ServerName, UnixTime},
};
use serde::{Serialize, de::DeserializeOwned};

use crate::{
    api::{
        AbortRequest, ApiError, ApiErrorCode, DesktopConfig, HandoffSession, JobOutcome,
        JobRequest, LocalStatus, LocalStatusRequest, MachineInfo, SyncAllRequest, SyncRun,
    },
    fingerprint::CertFingerprint,
    host::Host,
    pairing::{Endpoint, GitHubHandover, Hello, PairRequest, PairResponse},
    routes,
    secret::DeviceToken,
    stack::{
        ArrangeStackRequest, ExtendStackRequest, MakeStackRequest, MergeStackRequest, StackJobId,
        StackJobStatus, StackPlanRequest, StackRewritePlan, UnstackRequest,
    },
};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(4);
/// Reads and small writes.
const QUICK: Duration = Duration::from_secs(30);
/// A local job fetches and rebases; every git step is bounded on the desktop,
/// so this only has to outlast the sum of them.
const JOB: Duration = Duration::from_secs(15 * 60);

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("the desktop did not answer at any of its addresses ({})", describe(.failures))]
    Unreachable { failures: Vec<HostFailure> },
    #[error("{host} presented a different certificate than the one this phone paired with")]
    CertificateMismatch { host: Host },
    #[error("this phone is no longer paired with the desktop")]
    Unauthorized,
    #[error("{0}")]
    Api(ApiError),
    #[error("the desktop took too long to answer")]
    Timeout,
    #[error("unexpected response from the desktop: {0}")]
    Protocol(String),
    #[error("could not set up TLS: {0}")]
    Tls(String),
}

#[derive(Debug, Clone)]
pub struct HostFailure {
    pub host: Host,
    pub reason: String,
}

fn describe(failures: &[HostFailure]) -> String {
    failures
        .iter()
        .map(|failure| format!("{}: {}", failure.host, failure.reason))
        .collect::<Vec<_>>()
        .join("; ")
}

/// A paired (or pairing) connection to one desktop.
pub struct RemoteClient {
    http: reqwest::Client,
    endpoint: Endpoint,
    token: Option<DeviceToken>,
    /// Index into `endpoint.hosts()` of the host that last answered.
    preferred: AtomicUsize,
}

impl RemoteClient {
    /// `token` is `None` only while pairing.
    pub fn new(endpoint: Endpoint, token: Option<DeviceToken>) -> Result<Self, ClientError> {
        let verifier = PinnedVerifier::new(Some(endpoint.fingerprint()));
        Ok(Self {
            http: http_client(verifier)?,
            endpoint,
            token,
            preferred: AtomicUsize::new(0),
        })
    }

    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }

    /// The host that answered most recently.
    pub fn current_host(&self) -> &Host {
        let hosts = self.endpoint.hosts();
        &hosts[self.preferred.load(Ordering::Relaxed) % hosts.len()]
    }

    pub async fn hello(&self) -> Result<Hello, ClientError> {
        self.call(Method::GET, routes::HELLO, None::<&()>, QUICK)
            .await
    }

    pub async fn pair(&self, request: &PairRequest) -> Result<PairResponse, ClientError> {
        self.call(Method::POST, routes::PAIR, Some(request), QUICK)
            .await
    }

    pub async fn machine(&self) -> Result<MachineInfo, ClientError> {
        self.call(Method::GET, routes::MACHINE, None::<&()>, QUICK)
            .await
    }

    /// The desktop's repositories and feed preferences, for copying.
    pub async fn config(&self) -> Result<DesktopConfig, ClientError> {
        self.call(Method::GET, routes::CONFIG, None::<&()>, QUICK)
            .await
    }

    pub async fn github_token(&self) -> Result<GitHubHandover, ClientError> {
        self.call(Method::GET, routes::GITHUB_TOKEN, None::<&()>, QUICK)
            .await
    }

    pub async fn local_status(
        &self,
        request: &LocalStatusRequest,
    ) -> Result<LocalStatus, ClientError> {
        // Fetches first, so it gets the job budget's slack, not QUICK's.
        self.call(Method::POST, routes::LOCAL_STATUS, Some(request), JOB)
            .await
    }

    pub async fn run_job(&self, request: &JobRequest) -> Result<JobOutcome, ClientError> {
        self.call(Method::POST, routes::LOCAL_JOB, Some(request), JOB)
            .await
    }

    pub async fn abort(&self, request: &AbortRequest) -> Result<(), ClientError> {
        self.call(Method::POST, routes::LOCAL_ABORT, Some(request), JOB)
            .await
    }

    pub async fn start_sync_all(&self, request: &SyncAllRequest) -> Result<SyncRun, ClientError> {
        self.call(Method::POST, routes::SYNC_ALL, Some(request), QUICK)
            .await
    }

    pub async fn sync_all(&self) -> Result<Option<SyncRun>, ClientError> {
        self.call(Method::GET, routes::SYNC_ALL, None::<&()>, QUICK)
            .await
    }

    pub async fn handoffs(&self) -> Result<Vec<HandoffSession>, ClientError> {
        self.call(Method::GET, routes::HANDOFFS, None::<&()>, QUICK)
            .await
    }

    /// Which branches an arrangement or extension would rebase and
    /// force-push. Show them, then send exactly these as `confirm_rewrite`.
    pub async fn plan_stack_rewrite(
        &self,
        request: &StackPlanRequest,
    ) -> Result<StackRewritePlan, ClientError> {
        self.call(Method::POST, routes::STACK_PLAN, Some(request), QUICK)
            .await
    }

    /// Start making a chain that already chains into a stack.
    pub async fn make_stack(
        &self,
        request: &MakeStackRequest,
    ) -> Result<StackJobStatus, ClientError> {
        self.call(Method::POST, routes::STACK_MAKE, Some(request), QUICK)
            .await
    }

    /// Start arranging pull requests into a stack.
    pub async fn arrange_stack(
        &self,
        request: &ArrangeStackRequest,
    ) -> Result<StackJobStatus, ClientError> {
        self.call(Method::POST, routes::STACK_ARRANGE, Some(request), QUICK)
            .await
    }

    /// Start adding pull requests to the top of a stack.
    pub async fn extend_stack(
        &self,
        request: &ExtendStackRequest,
    ) -> Result<StackJobStatus, ClientError> {
        self.call(Method::POST, routes::STACK_EXTEND, Some(request), QUICK)
            .await
    }

    /// Start merging a whole stack.
    pub async fn merge_stack(
        &self,
        request: &MergeStackRequest,
    ) -> Result<StackJobStatus, ClientError> {
        self.call(Method::POST, routes::STACK_MERGE, Some(request), QUICK)
            .await
    }

    /// Start dissolving a stack.
    pub async fn unstack(&self, request: &UnstackRequest) -> Result<StackJobStatus, ClientError> {
        self.call(Method::POST, routes::STACK_UNSTACK, Some(request), QUICK)
            .await
    }

    /// Poll a stack job.
    pub async fn stack_job(&self, id: StackJobId) -> Result<StackJobStatus, ClientError> {
        self.call(Method::GET, &routes::stack_job(id), None::<&()>, QUICK)
            .await
    }

    /// Forget this device on the desktop.
    pub async fn unpair(&self) -> Result<(), ClientError> {
        self.call(Method::DELETE, routes::DEVICE, None::<&()>, QUICK)
            .await
    }

    async fn call<B: Serialize + ?Sized, T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<&B>,
        timeout: Duration,
    ) -> Result<T, ClientError> {
        let hosts = self.endpoint.hosts();
        let start = self.preferred.load(Ordering::Relaxed);
        let mut failures = Vec::new();
        let mut mismatch = None;

        for offset in 0..hosts.len() {
            let ix = (start + offset) % hosts.len();
            let host = &hosts[ix];
            let url = format!("https://{}{path}", host.authority(self.endpoint.port()));
            let mut request = self.http.request(method.clone(), &url).timeout(timeout);
            if let Some(token) = &self.token {
                request = request.bearer_auth(token.expose());
            }
            if let Some(body) = body {
                request = request.json(body);
            }

            match request.send().await {
                Ok(response) => {
                    self.preferred.store(ix, Ordering::Relaxed);
                    return decode(response).await;
                }
                Err(err) if is_certificate_mismatch(&err) => {
                    tracing::warn!(%host, "certificate does not match the paired fingerprint");
                    mismatch.get_or_insert_with(|| host.clone());
                    failures.push(HostFailure {
                        host: host.clone(),
                        reason: "wrong certificate".into(),
                    });
                }
                Err(err) if err.is_connect() => {
                    tracing::debug!(%host, error = %err, "could not connect; trying the next address");
                    failures.push(HostFailure {
                        host: host.clone(),
                        reason: root_cause(&err),
                    });
                }
                Err(err) if err.is_timeout() => return Err(ClientError::Timeout),
                Err(err) => return Err(ClientError::Protocol(root_cause(&err))),
            }
        }

        match mismatch {
            Some(host) => Err(ClientError::CertificateMismatch { host }),
            None => Err(ClientError::Unreachable { failures }),
        }
    }
}

/// What an unpaired phone learns from a host before trusting it: used when a
/// pairing code is typed by hand and there is no link to carry a fingerprint.
/// The caller shows [`CertFingerprint::short`] for comparison with the desktop
/// page before pairing against it.
#[derive(Debug, Clone)]
pub struct Probe {
    pub host: Host,
    pub fingerprint: CertFingerprint,
    pub hello: Hello,
}

pub async fn probe(hosts: &[Host], port: u16) -> Result<Probe, ClientError> {
    let mut failures = Vec::new();
    for host in hosts {
        let verifier = PinnedVerifier::new(None);
        let seen = verifier.seen.clone();
        let http = http_client(verifier)?;
        let url = format!("https://{}{}", host.authority(port), routes::HELLO);
        match http.get(&url).timeout(QUICK).send().await {
            Ok(response) => {
                let hello: Hello = decode(response).await?;
                let fingerprint = seen
                    .lock()
                    .ok()
                    .and_then(|guard| *guard)
                    .ok_or_else(|| ClientError::Tls("no certificate was presented".into()))?;
                return Ok(Probe {
                    host: host.clone(),
                    fingerprint,
                    hello,
                });
            }
            Err(err) => failures.push(HostFailure {
                host: host.clone(),
                reason: root_cause(&err),
            }),
        }
    }
    Err(ClientError::Unreachable { failures })
}

async fn decode<T: DeserializeOwned>(response: reqwest::Response) -> Result<T, ClientError> {
    let status = response.status();
    let bytes = response
        .bytes()
        .await
        .map_err(|err| ClientError::Protocol(root_cause(&err)))?;
    if status.is_success() {
        return serde_json::from_slice(&bytes).map_err(|err| {
            ClientError::Protocol(format!("could not read the {status} response: {err}"))
        });
    }
    let error = serde_json::from_slice::<ApiError>(&bytes).unwrap_or_else(|_| {
        ApiError::new(
            ApiErrorCode::Internal,
            format!("{status}: {}", String::from_utf8_lossy(&bytes)),
        )
    });
    if status == StatusCode::UNAUTHORIZED || error.code == ApiErrorCode::Unauthorized {
        return Err(ClientError::Unauthorized);
    }
    Err(ClientError::Api(error))
}

fn http_client(verifier: PinnedVerifier) -> Result<reqwest::Client, ClientError> {
    let provider = verifier.provider.clone();
    let config = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|err| ClientError::Tls(err.to_string()))?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(verifier))
        .with_no_client_auth();
    reqwest::Client::builder()
        .use_preconfigured_tls(config)
        .https_only(true)
        .connect_timeout(CONNECT_TIMEOUT)
        .build()
        .map_err(|err| ClientError::Tls(root_cause(&err)))
}

fn is_certificate_mismatch(err: &reqwest::Error) -> bool {
    let mut current: Option<&(dyn std::error::Error + 'static)> = Some(err);
    while let Some(error) = current {
        if matches!(
            error.downcast_ref::<rustls::Error>(),
            Some(rustls::Error::InvalidCertificate(_))
        ) {
            return true;
        }
        // The TLS error reaches reqwest wrapped in `io::Error`s, possibly
        // nested, and `io::Error::source()` skips the wrapped error to return
        // *its* source — so each layer is unwrapped with `get_ref` instead.
        current = match error.downcast_ref::<std::io::Error>() {
            Some(io) => io
                .get_ref()
                .map(|inner| inner as &(dyn std::error::Error + 'static)),
            None => error.source(),
        };
    }
    false
}

/// The innermost error's message: reqwest's own is usually "error sending
/// request", which says nothing.
fn root_cause(err: &(dyn std::error::Error + 'static)) -> String {
    let mut current = err;
    while let Some(next) = current.source() {
        current = next;
    }
    current.to_string()
}

/// Accepts exactly one certificate, by SHA-256 of its DER — or, with no
/// expectation, any certificate, recording what it saw (for [`probe`]).
///
/// Signatures are still verified with the provider's algorithms: pinning
/// replaces the chain-of-trust check, not the proof that the peer holds the
/// certificate's key.
#[derive(Debug)]
struct PinnedVerifier {
    expected: Option<CertFingerprint>,
    seen: Arc<Mutex<Option<CertFingerprint>>>,
    provider: Arc<CryptoProvider>,
}

impl PinnedVerifier {
    fn new(expected: Option<CertFingerprint>) -> Self {
        Self {
            expected,
            seen: Arc::new(Mutex::new(None)),
            provider: Arc::new(rustls::crypto::ring::default_provider()),
        }
    }
}

impl ServerCertVerifier for PinnedVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        let actual = CertFingerprint::of_der(end_entity.as_ref());
        if let Ok(mut seen) = self.seen.lock() {
            *seen = Some(actual);
        }
        match self.expected {
            None => Ok(ServerCertVerified::assertion()),
            Some(expected) if expected == actual => Ok(ServerCertVerified::assertion()),
            Some(_) => Err(rustls::Error::InvalidCertificate(
                rustls::CertificateError::ApplicationVerificationFailure,
            )),
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}
