//! A real daemon on loopback: both servers on ephemeral ports, a generated
//! TLS identity, fake GitHub and tmux, and a runner of the caller's choosing.

#![allow(dead_code)]

use std::{net::SocketAddr, path::Path, sync::Arc, time::Duration};

use rostrum_core::{RepoId, RepoState};
use rostrum_local::{LocalJob, LocalResult};
use rostrum_remote::{
    CertFingerprint, Endpoint, GitHubHandover, GitHubToken, PairRequest, PairResponse,
    PairingOffer, client::RemoteClient,
};
use rostrum_stack::{GhOutput, GhRunner, GhStackCommand, StackOpError};
use rostrumd::{
    Daemon, DaemonParts, TlsIdentity,
    boxed::BoxFuture,
    fsutil::ScratchDir,
    github::HandoverSource,
    jobs::{JobRunner, live_runner},
    net::{NetworkView, listen},
    rostrum_config::RostrumConfig,
    server::{self, Servers},
    stacks::{GhStackOps, RepoSnapshots, SnapshotError, StackOps},
    tmux::{SessionLister, TmuxError, TmuxSession},
    web::CodeOffer,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::watch,
};

pub struct FixedHandover(pub Option<GitHubHandover>);

impl HandoverSource for FixedHandover {
    fn handover(&self) -> BoxFuture<'_, Option<GitHubHandover>> {
        let answer = self.0.clone();
        Box::pin(async move { answer })
    }
}

pub struct NoSessions;

impl SessionLister for NoSessions {
    fn list(&self) -> BoxFuture<'_, Result<Vec<TmuxSession>, TmuxError>> {
        Box::pin(async { Ok(Vec::new()) })
    }
}

pub fn handover() -> GitHubHandover {
    GitHubHandover {
        token: GitHubToken::new("gho_integration"),
        host: "github.com".into(),
        source: "gh auth token on it-desk".into(),
    }
}

pub fn answering(result: LocalResult) -> JobRunner {
    Arc::new(move |_job: LocalJob| {
        let result = result.clone();
        Box::pin(async move { result })
    })
}

pub struct Harness {
    pub scratch: ScratchDir,
    pub daemon: Daemon,
    pub page: SocketAddr,
    pub api: SocketAddr,
    pub fingerprint: CertFingerprint,
    servers: Option<Servers>,
    _network: watch::Sender<NetworkView>,
}

/// Stack backends for a harness: none by default (a snapshot with no pull
/// requests, and operations that must not be reached).
pub struct StackSetup {
    pub ops: Arc<dyn StackOps>,
    pub snapshots: Arc<dyn RepoSnapshots>,
    pub scratch_dir: Option<std::path::PathBuf>,
}

impl Default for StackSetup {
    fn default() -> Self {
        Self {
            ops: Arc::new(GhStackOps(Arc::new(UnreachableGh))),
            snapshots: Arc::new(FixedSnapshots(RepoState::new(RepoId::new("o", "r")))),
            scratch_dir: None,
        }
    }
}

/// A `gh` that fails the test if anything runs it.
pub struct UnreachableGh;

impl GhRunner for UnreachableGh {
    async fn run(
        &self,
        _cwd: &Path,
        _repo: &RepoId,
        command: &GhStackCommand,
    ) -> Result<GhOutput, StackOpError> {
        panic!("no gh expected, got {command}");
    }
}

/// Every snapshot is this state, relabelled for the repository asked about.
pub struct FixedSnapshots(pub RepoState);

impl RepoSnapshots for FixedSnapshots {
    fn snapshot<'a>(&'a self, repo: &'a RepoId) -> BoxFuture<'a, Result<RepoState, SnapshotError>> {
        let mut state = self.0.clone();
        state.id = repo.clone();
        Box::pin(async move { Ok(state) })
    }
}

impl Harness {
    /// Start both servers. `rostrum_config` is written as rostrum's
    /// `config.json` when given.
    pub async fn start(
        tag: &str,
        runner: Option<JobRunner>,
        rostrum_config: Option<serde_json::Value>,
    ) -> Self {
        Self::start_with(tag, runner, rostrum_config, StackSetup::default()).await
    }

    /// [`Harness::start`] with stack backends.
    pub async fn start_with(
        tag: &str,
        runner: Option<JobRunner>,
        rostrum_config: Option<serde_json::Value>,
        stacks: StackSetup,
    ) -> Self {
        let scratch = ScratchDir::new(tag);
        if let Some(config) = rostrum_config {
            std::fs::write(
                scratch.join("config.json"),
                serde_json::to_vec(&config).expect("json"),
            )
            .expect("config");
        }
        let (identity, _) =
            TlsIdentity::load_or_create(&scratch.join("tls"), "it-desk").expect("identity");
        let fingerprint = identity.fingerprint();

        let http = listen::bind_one("127.0.0.1:0".parse().expect("addr")).expect("http");
        let https = listen::bind_one("127.0.0.1:0".parse().expect("addr")).expect("https");
        let page = http.local_addr().expect("addr");
        let api = https.local_addr().expect("addr");

        let (network, receiver) = watch::channel(NetworkView {
            lan: vec!["127.0.0.1".parse().expect("ip")],
            tailnet: None,
        });
        let daemon = Daemon::start(DaemonParts {
            machine_name: "it-desk".into(),
            hostname: "it-desk".into(),
            http_port: page.port(),
            https_port: api.port(),
            fingerprint,
            apk_dir: scratch.join("apk"),
            rostrum_config: RostrumConfig::at(scratch.join("config.json")),
            code_ttl: Duration::from_secs(300),
            devices_file: scratch.join("state/devices.json"),
            handoffs_file: scratch.join("state/handoffs.json"),
            runner: runner.unwrap_or_else(live_runner),
            github: Arc::new(FixedHandover(Some(handover()))),
            tmux: Arc::new(NoSessions),
            network: receiver,
            stack_ops: stacks.ops,
            snapshots: stacks.snapshots,
            stack_scratch_dir: stacks
                .scratch_dir
                .unwrap_or_else(|| scratch.join("stack-worktrees")),
        })
        .expect("daemon");
        let servers = server::start(
            &daemon,
            Arc::new(identity.server_config().expect("tls config")),
            vec![http],
            vec![https],
        )
        .expect("servers");
        Self {
            scratch,
            daemon,
            page,
            api,
            fingerprint,
            servers: Some(servers),
            _network: network,
        }
    }

    /// `POST /pairing-codes` over a real socket, as the page's script does.
    pub async fn issue_code(&self) -> CodeOffer {
        let (status, body) = http_request(
            self.page,
            "POST",
            "/pairing-codes",
            &[("Origin", &format!("http://{}", self.page))],
        )
        .await;
        assert_eq!(status, 200, "{body}");
        serde_json::from_str(&body).expect("offer")
    }

    /// Pair through the page and the API, returning a client holding the
    /// device token.
    pub async fn pair(&self, name: &str) -> (RemoteClient, PairResponse) {
        let offer = PairingOffer::from_uri(&self.issue_code().await.uri).expect("link parses");
        let pairing = RemoteClient::new(offer.endpoint.clone(), None).expect("client");
        let response = pairing
            .pair(&PairRequest {
                code: offer.code,
                device_name: name.into(),
                replaces: None,
            })
            .await
            .expect("pairs");
        let client =
            RemoteClient::new(offer.endpoint, Some(response.token.clone())).expect("client");
        (client, response)
    }

    pub fn endpoint(&self) -> Endpoint {
        Endpoint::new(
            vec!["127.0.0.1".parse().expect("host")],
            self.api.port(),
            self.fingerprint,
        )
        .expect("endpoint")
    }

    pub async fn stop(mut self) {
        if let Some(servers) = self.servers.take() {
            servers.shutdown(Duration::from_secs(2)).await;
        }
        self.daemon.jobs.shutdown().await;
    }
}

/// A minimal HTTP/1.1 exchange: status code and body.
pub async fn http_request(
    addr: SocketAddr,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
) -> (u16, String) {
    let mut stream = tokio::net::TcpStream::connect(addr).await.expect("connect");
    let mut request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\nContent-Length: 0\r\n"
    );
    for (name, value) in headers {
        request.push_str(&format!("{name}: {value}\r\n"));
    }
    request.push_str("\r\n");
    stream.write_all(request.as_bytes()).await.expect("write");
    let mut response = Vec::new();
    stream.read_to_end(&mut response).await.expect("read");
    let text = String::from_utf8_lossy(&response).to_string();
    let (head, body) = text.split_once("\r\n\r\n").expect("a response");
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .expect("status");
    // Bodies here are small JSON; a chunked one is de-chunked crudely.
    let body = if head
        .to_ascii_lowercase()
        .contains("transfer-encoding: chunked")
    {
        dechunk(body)
    } else {
        body.to_string()
    };
    (status, body)
}

fn dechunk(mut body: &str) -> String {
    let mut out = String::new();
    while let Some((size, rest)) = body.split_once("\r\n") {
        let Ok(size) = usize::from_str_radix(size.trim(), 16) else {
            break;
        };
        if size == 0 {
            break;
        }
        out.push_str(&rest[..size]);
        body = &rest[size + 2..];
    }
    out
}

/// Run `git` in `dir`, panicking on failure.
pub fn git(dir: &Path, args: &[&str]) {
    let status = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .status()
        .expect("git runs");
    assert!(status.success(), "git {args:?} failed in {}", dir.display());
}
