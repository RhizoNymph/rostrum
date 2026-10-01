//! Shared fixtures for the integration tests: a scratch data directory, a
//! seeded cache, and a desktop stand-in serving the remote protocol over a
//! self-signed TLS certificate.

#![allow(dead_code)]

pub mod github;

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use chrono::{DateTime, Utc};
use rostrum_config::Config;
use rostrum_core::{MergeStateStatus, Mergeable, NodeId, PrNumber, PullRequest, RepoId, User};
use rostrum_db::Db;
use rostrum_github::PullRequestFile;
use rostrum_remote::CertFingerprint;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
use tokio_rustls::TlsAcceptor;

/// A directory of its own under cargo's per-target scratch space, removed
/// when dropped.
pub struct Scratch {
    pub dir: PathBuf,
}

impl Scratch {
    pub fn new(tag: &str) -> Self {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let dir =
            Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("rostrum-ffi-{tag}-{unique}"));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        Self { dir }
    }

    pub fn path(&self) -> String {
        self.dir.to_string_lossy().into_owned()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

pub fn at(secs: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(secs, 0).expect("valid time")
}

pub fn repo() -> RepoId {
    RepoId::new("octo", "repo")
}

/// An open pull request in `octo/repo`.
pub fn pull(number: u32, head_sha: &str) -> PullRequest {
    PullRequest {
        number: PrNumber(number),
        node_id: NodeId(format!("PR_{number}")),
        title: format!("Pull request {number}"),
        url: format!("https://github.com/octo/repo/pull/{number}"),
        is_draft: false,
        created_at: at(1_700_000_000),
        updated_at: at(1_700_000_000 + i64::from(number)),
        author: Some(User {
            login: format!("author{number}"),
            avatar_url: None,
        }),
        head_ref: format!("topic-{number}"),
        head_sha: head_sha.into(),
        base_ref: "main".into(),
        additions: 3,
        deletions: 1,
        changed_files: 1,
        mergeable: Mergeable::Mergeable,
        merge_state: MergeStateStatus::Clean,
        review_decision: None,
        assignees: Vec::new(),
        review_requests: Vec::new(),
        labels: Vec::new(),
        comment_count: 0,
        checks: None,
        base_divergence: None,
        is_cross_repository: false,
    }
}

/// One file: context line 10, old line 11 replaced by new line 11, context
/// line 12 — then a second hunk adding line 40.
pub fn files() -> Vec<PullRequestFile> {
    vec![PullRequestFile {
        filename: "src/lib.rs".into(),
        previous_filename: None,
        status: "modified".into(),
        additions: 2,
        deletions: 1,
        patch: Some(
            "@@ -10,3 +10,3 @@ fn main() {\n ctx\n-let a = 1;\n+let a = 2;\n end\n@@ -39,1 +39,2 @@\n tail\n+added\n"
                .into(),
        ),
    }]
}

/// Write a config watching only `octo/repo` and a cache holding `prs` (and,
/// if given, the files of pull request 1 at `head`), as a previous session
/// would have left them.
pub async fn seed(dir: &Path, prs: &[PullRequest], files_at: Option<&str>) {
    Config {
        repos: vec!["octo/repo".into()],
        ..Default::default()
    }
    .save_to(&dir.join("config.json"))
    .expect("config");
    let db = Db::open(&dir.join("cache.db")).await.expect("db");
    db.save_pull_requests(&repo(), prs).await.expect("prs");
    if let Some(head) = files_at {
        db.save_pull_request_files(&repo(), PrNumber(1), head, &files())
            .await
            .expect("files");
    }
    db.close().await;
}

// --- a desktop stand-in ------------------------------------------------------

/// One request the stand-in received.
#[derive(Clone, Debug)]
pub struct Request {
    pub method: String,
    pub path: String,
    /// Lowercased header block.
    pub headers: String,
    pub body: String,
}

impl Request {
    pub fn bearer(&self) -> Option<&str> {
        self.headers
            .lines()
            .find_map(|line| line.strip_prefix("authorization: bearer "))
            .map(str::trim)
    }
}

pub type Handler = Arc<dyn Fn(&Request) -> (u16, String) + Send + Sync>;

/// Everything the stand-in received, for assertions.
#[derive(Clone, Default)]
pub struct Log(pub Arc<Mutex<Vec<Request>>>);

impl Log {
    pub fn requests(&self) -> Vec<Request> {
        self.0.lock().expect("log").clone()
    }

    pub fn last(&self, path: &str) -> Option<Request> {
        self.requests()
            .into_iter()
            .rev()
            .find(|request| request.path == path)
    }
}

/// Serve `handler` over HTTPS on 127.0.0.1 with a fresh self-signed
/// certificate. Returns the port, the certificate's fingerprint, and the log.
pub async fn serve(handler: Handler) -> (u16, CertFingerprint, Log) {
    let certified = rcgen::generate_simple_self_signed(vec!["rostrumd".to_string()])
        .expect("self-signed certificate");
    let cert_der: CertificateDer<'static> = certified.cert.der().clone();
    let fingerprint = CertFingerprint::of_der(cert_der.as_ref());
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
        certified.signing_key.serialize_der(),
    ));
    let config = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .expect("protocol versions")
    .with_no_client_auth()
    .with_single_cert(vec![cert_der], key)
    .expect("server config");
    let acceptor = TlsAcceptor::from(Arc::new(config));
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let log = Log::default();
    let recorded = log.clone();

    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            let acceptor = acceptor.clone();
            let handler = handler.clone();
            let recorded = recorded.clone();
            tokio::spawn(async move {
                let Ok(mut tls) = acceptor.accept(stream).await else {
                    return;
                };
                let Some(request) = read_request(&mut tls).await else {
                    return;
                };
                let (status, body) = handler(&request);
                recorded.0.lock().expect("log").push(request);
                let reason = match status {
                    200 => "OK",
                    401 => "Unauthorized",
                    403 => "Forbidden",
                    404 => "Not Found",
                    410 => "Gone",
                    _ => "Status",
                };
                let response = format!(
                    "HTTP/1.1 {status} {reason}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = tls.write_all(response.as_bytes()).await;
                let _ = tls.shutdown().await;
            });
        }
    });
    (port, fingerprint, log)
}

pub async fn read_request<S: AsyncReadExt + Unpin>(stream: &mut S) -> Option<Request> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    let header_end = loop {
        let n = stream.read(&mut chunk).await.ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break end + 4;
        }
    };
    let head = String::from_utf8_lossy(&buf[..header_end]).to_string();
    let lowered = head.to_ascii_lowercase();
    let length: usize = lowered
        .lines()
        .find_map(|line| line.strip_prefix("content-length:"))
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(0);
    while buf.len() < header_end + length {
        let n = stream.read(&mut chunk).await.ok()?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    let mut words = head.split_whitespace();
    Some(Request {
        method: words.next()?.to_string(),
        path: words.next()?.to_string(),
        headers: lowered,
        body: String::from_utf8_lossy(&buf[header_end..(header_end + length).min(buf.len())])
            .to_string(),
    })
}

/// Fail if any file under `dir` contains `secret`: the core must never write
/// a token to disk.
pub fn assert_no_secret_on_disk(dir: &Path, secret: &str) {
    let mut pending = vec![dir.to_path_buf()];
    while let Some(next) = pending.pop() {
        for entry in std::fs::read_dir(&next).expect("read dir") {
            let path = entry.expect("entry").path();
            if path.is_dir() {
                pending.push(path);
            } else {
                let bytes = std::fs::read(&path).expect("read file");
                assert!(
                    !bytes
                        .windows(secret.len())
                        .any(|window| window == secret.as_bytes()),
                    "{} contains a secret",
                    path.display()
                );
            }
        }
    }
}
