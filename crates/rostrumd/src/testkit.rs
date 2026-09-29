//! A daemon on scratch directories with fake backends, and request helpers,
//! for the router tests.

use std::{
    net::{IpAddr, SocketAddr},
    path::PathBuf,
    sync::Arc,
    time::Duration,
};

use axum::{
    Router,
    body::{Body, Bytes},
    extract::ConnectInfo,
    http::{HeaderMap, Method, Request, StatusCode, header},
};
use rostrum_local::{LocalJob, LocalResult};
use rostrum_remote::{
    CertFingerprint, DeviceToken, GitHubHandover, GitHubToken, PairRequest, PairResponse,
    PairingCode, routes,
};
use serde::{Serialize, de::DeserializeOwned};
use tokio::sync::watch;
use tower::ServiceExt;

use crate::{
    Daemon, DaemonParts,
    boxed::BoxFuture,
    fsutil::ScratchDir,
    github::HandoverSource,
    jobs::JobRunner,
    net::{NetworkView, tailscale::Tailnet},
    rostrum_config::RostrumConfig,
    tmux::{SessionLister, TmuxError, TmuxSession},
};

pub const HTTP_PORT: u16 = 8484;
pub const HTTPS_PORT: u16 = 8485;
pub const PAGE_HOST: &str = "127.0.0.1:8484";

pub struct FixedHandover(pub Option<GitHubHandover>);

impl HandoverSource for FixedHandover {
    fn handover(&self) -> BoxFuture<'_, Option<GitHubHandover>> {
        let answer = self.0.clone();
        Box::pin(async move { answer })
    }
}

pub struct FixedSessions(pub Vec<TmuxSession>);

impl SessionLister for FixedSessions {
    fn list(&self) -> BoxFuture<'_, Result<Vec<TmuxSession>, TmuxError>> {
        let sessions = self.0.clone();
        Box::pin(async move { Ok(sessions) })
    }
}

/// A runner that answers `result` for every job without touching git.
pub fn answering(result: LocalResult) -> JobRunner {
    Arc::new(move |_job: LocalJob| {
        let result = result.clone();
        Box::pin(async move { result })
    })
}

pub fn handover() -> GitHubHandover {
    GitHubHandover {
        token: GitHubToken::new("gho_testtoken"),
        host: "github.com".into(),
        source: "gh auth token on test-desk".into(),
    }
}

/// This machine's addresses, as the tests pretend them to be.
pub fn view() -> NetworkView {
    NetworkView {
        lan: vec!["192.168.0.111".parse().expect("ip")],
        tailnet: Some(Tailnet {
            ipv4: vec!["100.64.0.10".parse().expect("ip")],
            ipv6: vec!["fd7a:115c:a1e0::a".parse().expect("ip")],
            dns_name: Some("test-desk.tail.example".into()),
        }),
    }
}

pub struct Options {
    pub runner: JobRunner,
    pub github: Option<GitHubHandover>,
    pub sessions: Vec<TmuxSession>,
    pub view: NetworkView,
    pub code_ttl: Duration,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            runner: answering(LocalResult::Completed),
            github: None,
            sessions: Vec::new(),
            view: view(),
            code_ttl: Duration::from_secs(300),
        }
    }
}

pub struct Kit {
    pub scratch: ScratchDir,
    pub daemon: Daemon,
    /// Keeps the daemon's view of the network alive (and settable).
    _network: watch::Sender<NetworkView>,
}

impl Kit {
    pub fn new(tag: &str) -> Self {
        Self::with(tag, Options::default())
    }

    pub fn with(tag: &str, options: Options) -> Self {
        let scratch = ScratchDir::new(tag);
        std::fs::create_dir_all(scratch.join("apk")).expect("apk dir");
        let (network, receiver) = watch::channel(options.view);
        let daemon = Daemon::start(DaemonParts {
            machine_name: "test-desk".into(),
            hostname: "test-desk".into(),
            http_port: HTTP_PORT,
            https_port: HTTPS_PORT,
            fingerprint: fingerprint(),
            apk_dir: scratch.join("apk"),
            rostrum_config: RostrumConfig::at(scratch.join("config.json")),
            code_ttl: options.code_ttl,
            devices_file: scratch.join("state/devices.json"),
            handoffs_file: scratch.join("state/handoffs.json"),
            runner: options.runner,
            github: Arc::new(FixedHandover(options.github)),
            tmux: Arc::new(FixedSessions(options.sessions)),
            network: receiver,
        })
        .expect("daemon starts");
        Self {
            scratch,
            daemon,
            _network: network,
        }
    }

    pub fn api(&self) -> Router {
        crate::api::router(self.daemon.clone())
    }

    pub fn web(&self) -> Router {
        crate::web::router(self.daemon.clone())
    }

    /// rostrum's `config.json`.
    pub fn write_rostrum_config(&self, json: &serde_json::Value) {
        std::fs::write(
            self.scratch.join("config.json"),
            serde_json::to_vec(json).expect("json"),
        )
        .expect("write config");
    }

    /// A directory configured as `owner/repo`'s clone (not a git repository:
    /// the fake runner never looks).
    pub fn configure_clone(&self) -> PathBuf {
        let clone = self.scratch.join("clone");
        std::fs::create_dir_all(&clone).expect("clone dir");
        self.write_rostrum_config(&serde_json::json!({
            "clones": { "owner/repo": clone.display().to_string() }
        }));
        clone
    }

    /// Pair a device over the API with a freshly issued code.
    pub async fn pair(&self, name: &str) -> PairResponse {
        let code = self.daemon.registry.issue_code().await.expect("code").code;
        let (status, body) = send(
            &self.api(),
            api_request(
                Method::POST,
                routes::PAIR,
                "192.168.0.50",
                None,
                Some(&PairRequest {
                    code,
                    device_name: name.into(),
                    replaces: None,
                }),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
        serde_json::from_slice(&body).expect("pair response")
    }
}

pub fn fingerprint() -> CertFingerprint {
    CertFingerprint::of_der(b"rostrumd test certificate")
}

fn with_peer(mut request: Request<Body>, from: &str) -> Request<Body> {
    let ip: IpAddr = from.parse().expect("peer ip");
    request
        .extensions_mut()
        .insert(ConnectInfo(SocketAddr::new(ip, 50_000)));
    request
}

/// An API request from `from`, with a bearer token and a JSON body if given.
pub fn api_request<B: Serialize>(
    method: Method,
    path: &str,
    from: &str,
    token: Option<&DeviceToken>,
    body: Option<&B>,
) -> Request<Body> {
    let mut builder = Request::builder().method(method).uri(path);
    if let Some(token) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {}", token.expose()));
    }
    let body = match body {
        Some(body) => {
            builder = builder.header(header::CONTENT_TYPE, "application/json");
            Body::from(serde_json::to_vec(body).expect("json"))
        }
        None => Body::empty(),
    };
    with_peer(builder.body(body).expect("request"), from)
}

/// A page-server request from `from`, addressed to `host`, with an `Origin`
/// if given.
pub fn page_request(
    method: Method,
    path: &str,
    from: &str,
    host: &str,
    origin: Option<&str>,
) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header(header::HOST, host);
    if let Some(origin) = origin {
        builder = builder.header(header::ORIGIN, origin);
    }
    with_peer(builder.body(Body::empty()).expect("request"), from)
}

pub async fn send(router: &Router, request: Request<Body>) -> (StatusCode, Bytes) {
    let (status, _, body) = send_full(router, request).await;
    (status, body)
}

pub async fn send_full(router: &Router, request: Request<Body>) -> (StatusCode, HeaderMap, Bytes) {
    let response = router.clone().oneshot(request).await.expect("infallible");
    let status = response.status();
    let headers = response.headers().clone();
    let body = axum::body::to_bytes(response.into_body(), 64 * 1024 * 1024)
        .await
        .expect("body");
    (status, headers, body)
}

pub fn json<T: DeserializeOwned>(body: &Bytes) -> T {
    serde_json::from_slice(body)
        .unwrap_or_else(|err| panic!("{err}: {}", String::from_utf8_lossy(body)))
}

/// A pairing code nobody issued.
pub fn stranger_code() -> PairingCode {
    PairingCode::parse("ZZZZ-ZZZZ").expect("code")
}
