//! The desktop half of the core, end to end: pairing by link and by address,
//! then every desktop call, against a stand-in serving the real protocol over
//! a self-signed certificate — the same pinning, fallback and error mapping
//! the phone gets.

mod support;

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use rostrum_core::{LoginKey, RepoId};
use rostrum_ffi::{
    RemoteErrorCode, RostrumCore, RostrumError,
    feed::{FeedObserver, FeedSnapshot},
    remote::{JobOutcome, LocalOp, LocalStatus, RemoteStatus, SyncAllOp, SyncEntryState},
    session::GitHubStatus,
    types::ColorRole,
};
use rostrum_remote::{
    CertFingerprint, DeviceId, DeviceToken, Endpoint, GitHubToken, PairingCode, PairingOffer,
    api::{
        CloneInfo, DesktopConfig, JobOutcome as WireOutcome, JobRequest, LocalBranchStatus,
        LocalStatus as WireStatus, LocalStatusRequest, MachineInfo, SyncAllRequest, SyncEntry,
        SyncEntryState as WireEntryState, SyncRun,
    },
    pairing::{GitHubHandover, PairResponse},
};
use support::{Handler, Request, Scratch, assert_no_secret_on_disk, pull, seed, serve};

const ISSUED: [u8; 32] = [7; 32];
const CODE: &str = "K7QXM2PD";

fn issued_token() -> DeviceToken {
    DeviceToken::from_bytes(ISSUED)
}

fn machine() -> MachineInfo {
    MachineInfo {
        name: "test-desk".into(),
        version: "0.1.0".into(),
        api_version: 1,
        clones: vec![CloneInfo {
            repo: RepoId::new("octo", "repo"),
            path: "/code/octo/repo".into(),
        }],
        handler_configured: true,
        autostash: false,
    }
}

fn json<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(value).expect("serialises")
}

fn unauthorized() -> (u16, String) {
    (
        401,
        r#"{"code":"unauthorized","message":"unknown device"}"#.into(),
    )
}

/// The shareable part of the stand-in desktop's config.
fn desktop_config() -> DesktopConfig {
    DesktopConfig {
        repos: vec![
            RepoId::new("zed-industries", "zed"),
            RepoId::new("octo", "repo"),
        ],
        prs_per_repo: 30,
        hide_drafts: true,
        hide_empty_repos: false,
        authors: vec![LoginKey::new("Alice")],
        include_involved: true,
        autostash: true,
    }
}

/// The desktop's side of the protocol, faithful enough to check what the
/// phone sends: pairing checks the code, everything else the bearer token.
fn desktop(api_version: u32) -> Handler {
    desktop_serving(api_version, Arc::new(Mutex::new(desktop_config())))
}

/// [`desktop`], answering `/api/v1/config` with whatever `config` holds at
/// the time of the request.
fn desktop_serving(api_version: u32, config: Arc<Mutex<DesktopConfig>>) -> Handler {
    Arc::new(move |request: &Request| {
        match (request.method.as_str(), request.path.as_str()) {
            ("GET", "/api/v1/hello") => {
                return (
                    200,
                    format!(r#"{{"machine":"test-desk","api_version":{api_version}}}"#),
                );
            }
            ("POST", "/api/v1/pair") => {
                let body: serde_json::Value =
                    serde_json::from_str(&request.body).expect("pair body");
                if body["code"] != CODE {
                    return (
                        403,
                        r#"{"code":"pairing_code_invalid","message":"that code is not valid"}"#
                            .into(),
                    );
                }
                return (
                    200,
                    json(&PairResponse {
                        device: DeviceId::from_bytes([1; 16]),
                        token: issued_token(),
                        machine: machine(),
                        github: Some(GitHubHandover {
                            token: GitHubToken::new("ghp_handed_over"),
                            host: "github.com".into(),
                            source: "gh auth token".into(),
                        }),
                    }),
                );
            }
            _ => {}
        }
        if request.bearer() != Some(issued_token().expose().to_ascii_lowercase().as_str()) {
            return unauthorized();
        }
        match (request.method.as_str(), request.path.as_str()) {
            ("GET", "/api/v1/machine") => (200, json(&machine())),
            ("GET", "/api/v1/config") => (200, json(&*config.lock().expect("config"))),
            ("GET", "/api/v1/github-token") => (
                200,
                json(&GitHubHandover {
                    token: GitHubToken::new("ghp_fresh"),
                    host: "github.com".into(),
                    source: "gh auth token".into(),
                }),
            ),
            ("POST", "/api/v1/local/status") => {
                let request: LocalStatusRequest =
                    serde_json::from_str(&request.body).expect("status body");
                (
                    200,
                    json(&WireStatus::CheckedOut {
                        branch: LocalBranchStatus {
                            worktree: "/code/octo/repo-wt".into(),
                            branch: request.head_ref,
                            ahead: 1,
                            behind: 0,
                            fetched: true,
                            blocker: None,
                            in_progress: None,
                            handoff: None,
                        },
                    }),
                )
            }
            ("POST", "/api/v1/local/job") => {
                let job: JobRequest = serde_json::from_str(&request.body).expect("job body");
                (
                    200,
                    json(&WireOutcome::HandedOff {
                        session: format!("rostrum-{}", job.pr.key.number.0),
                    }),
                )
            }
            ("POST", "/api/v1/local/abort") => (200, "null".into()),
            ("POST", "/api/v1/sync-all") => {
                let sync: SyncAllRequest = serde_json::from_str(&request.body).expect("sync body");
                (
                    200,
                    json(&SyncRun {
                        id: 1,
                        op: sync.op,
                        started_at: support::at(1_800_000_000),
                        finished_at: None,
                        entries: sync
                            .prs
                            .into_iter()
                            .map(|pr| SyncEntry {
                                key: pr.key,
                                head_ref: pr.head_ref,
                                state: WireEntryState::Pending,
                            })
                            .collect(),
                    }),
                )
            }
            ("GET", "/api/v1/sync-all") => (200, "null".into()),
            ("GET", "/api/v1/handoffs") => (200, "[]".into()),
            ("DELETE", "/api/v1/device") => (200, "null".into()),
            _ => (
                404,
                r#"{"code":"not_found","message":"no such route"}"#.into(),
            ),
        }
    })
}

fn endpoint(port: u16, fingerprint: CertFingerprint) -> Endpoint {
    Endpoint::new(vec!["127.0.0.1".parse().expect("host")], port, fingerprint).expect("endpoint")
}

fn link(port: u16, fingerprint: CertFingerprint) -> String {
    PairingOffer {
        machine: "test-desk".into(),
        endpoint: endpoint(port, fingerprint),
        code: PairingCode::parse(CODE).expect("code"),
    }
    .to_uri()
}

/// A core over a cache holding one pull request, #7, as a previous session
/// left it.
async fn core(scratch: &Scratch) -> Arc<RostrumCore> {
    seed(&scratch.dir, &[pull(7, "head7")], None).await;
    let core = RostrumCore::open(scratch.path()).await.expect("open");
    core.cached_feed().await.expect("cached feed");
    core
}

#[tokio::test]
async fn pairing_by_link_then_every_desktop_call() {
    let scratch = Scratch::new("pair-link");
    let core = core(&scratch).await;
    let (port, fingerprint, log) = serve(desktop(1)).await;
    let uri = link(port, fingerprint);

    let preview = core.parse_pairing_link(uri.clone()).expect("preview");
    assert_eq!(preview.machine, "test-desk");
    assert_eq!(preview.hosts, vec!["127.0.0.1"]);
    assert_eq!(preview.code, "K7QX-M2PD");
    assert_eq!(preview.fingerprint_short, fingerprint.short());

    let paired = core
        .pair_with_link(uri, "Pixel 9".into())
        .await
        .expect("pairs");
    assert_eq!(paired.device_token, issued_token().expose());
    assert_eq!(paired.machine.clones[0].repo, "octo/repo");
    assert_eq!(
        paired.github.as_ref().map(|github| github.token.as_str()),
        Some("ghp_handed_over")
    );
    let saved: Endpoint = serde_json::from_str(&paired.endpoint).expect("endpoint json");
    assert_eq!(saved, endpoint(port, fingerprint));
    let pair_request = log.last("/api/v1/pair").expect("pair request");
    assert!(pair_request.body.contains(CODE), "{}", pair_request.body);
    assert!(pair_request.body.contains("Pixel 9"));
    // The handed-over token was applied because none was set.
    assert_eq!(core.github_status().await, GitHubStatus::Unverified);
    assert!(matches!(
        core.remote_status().await.expect("status"),
        RemoteStatus::Paired { port: p, .. } if p == port
    ));

    let info = core.machine_info().await.expect("machine");
    assert_eq!(info.name, "test-desk");
    assert!(info.handler_configured);

    let status = core
        .local_status("octo/repo".into(), 7)
        .await
        .expect("local status");
    let LocalStatus::CheckedOut { branch } = status else {
        panic!("expected a checked-out branch, got {status:?}");
    };
    assert_eq!(branch.branch, "topic-7");
    assert_eq!(branch.ahead, 1);

    let job = core
        .run_local_job("octo/repo".into(), 7, LocalOp::MergeBase, true)
        .await
        .expect("job");
    assert_eq!(
        job.outcome,
        JobOutcome::HandedOff {
            session: "rostrum-7".into(),
            attach_command: "tmux attach -t =rostrum-7".into()
        }
    );
    assert_eq!(job.chip.map(|chip| chip.role), Some(ColorRole::Accent));
    let sent: JobRequest =
        serde_json::from_str(&log.last("/api/v1/local/job").expect("job request").body)
            .expect("job body");
    assert_eq!(sent.pr.key.number.0, 7);
    assert_eq!(sent.pr.head_ref, "topic-7");
    assert_eq!(sent.pr.base_ref, "main");
    assert_eq!(sent.pr.title, "Pull request 7");
    assert!(sent.autostash);
    assert_eq!(
        serde_json::to_value(sent.op).expect("op"),
        serde_json::json!("merge_base")
    );

    let run = core
        .start_sync_all(SyncAllOp::RebaseBase, false)
        .await
        .expect("sync all");
    assert_eq!(run.op, LocalOp::RebaseBase);
    assert_eq!(run.entries.len(), 1);
    assert_eq!(run.entries[0].number, 7);
    assert_eq!(run.entries[0].state, SyncEntryState::Pending);
    assert_eq!(run.progress_text, "0/1\u{2026}");
    assert_eq!(core.sync_all_status().await.expect("sync status"), None);
    assert!(core.handoffs().await.expect("handoffs").is_empty());

    core.abort_local("octo/repo".into(), 7)
        .await
        .expect("abort");

    let fresh = core
        .refresh_github_token_from_desktop()
        .await
        .expect("fresh token");
    assert_eq!(fresh.token, "ghp_fresh");

    core.unpair().await.expect("unpair");
    assert_eq!(
        core.remote_status().await.expect("status"),
        RemoteStatus::NotPaired
    );
    assert_eq!(core.machine_info().await, Err(RostrumError::NotPaired));
    // Every authenticated call carried the issued token.
    assert!(
        log.requests()
            .iter()
            .filter(|request| !request.path.ends_with("/hello") && !request.path.ends_with("/pair"))
            .all(|request| request.bearer().is_some())
    );

    // None of the secrets that passed through reached the disk.
    drop(core);
    for secret in [issued_token().expose(), "ghp_handed_over", "ghp_fresh"] {
        assert_no_secret_on_disk(&scratch.dir, secret);
    }
}

#[tokio::test]
async fn a_saved_pairing_reconnects_and_failures_are_typed() {
    let scratch = Scratch::new("reconnect");
    let core = core(&scratch).await;
    let (port, fingerprint, _) = serve(desktop(1)).await;
    let saved = serde_json::to_string(&endpoint(port, fingerprint)).expect("json");

    core.set_remote(saved.clone(), issued_token().expose().to_string())
        .await
        .expect("set remote");
    assert_eq!(
        core.machine_info().await.expect("machine").name,
        "test-desk"
    );

    // A token the desktop no longer knows: the device was revoked there.
    let revoked = DeviceToken::from_bytes([9; 32]);
    core.set_remote(saved, revoked.expose().to_string())
        .await
        .expect("set remote");
    assert_eq!(core.machine_info().await, Err(RostrumError::DeviceRevoked));
    assert_eq!(
        core.desktop_config().await,
        Err(RostrumError::DeviceRevoked)
    );
    assert_eq!(
        core.copy_desktop_config().await,
        Err(RostrumError::DeviceRevoked)
    );

    // The desktop answering with a different certificate than was paired.
    let other =
        serde_json::to_string(&endpoint(port, CertFingerprint::of_der(b"another"))).expect("json");
    core.set_remote(other, issued_token().expose().to_string())
        .await
        .expect("set remote");
    assert_eq!(
        core.machine_info().await,
        Err(RostrumError::CertificateMismatch {
            host: "127.0.0.1".into()
        })
    );

    assert!(matches!(
        core.set_remote(
            "not an endpoint".into(),
            issued_token().expose().to_string()
        )
        .await,
        Err(RostrumError::InvalidInput { .. })
    ));
    assert!(matches!(
        core.set_remote(
            serde_json::to_string(&endpoint(port, fingerprint)).expect("json"),
            "short".into()
        )
        .await,
        Err(RostrumError::InvalidInput { .. })
    ));
}

#[tokio::test]
async fn manual_pairing_pins_the_probed_certificate() {
    let scratch = Scratch::new("pair-manual");
    let core = core(&scratch).await;
    let (port, fingerprint, _) = serve(desktop(1)).await;

    let probe = core
        .probe_desktop("127.0.0.1".into(), port)
        .await
        .expect("probe");
    assert_eq!(probe.machine, "test-desk");
    assert!(probe.compatible);
    assert_eq!(probe.fingerprint, fingerprint.to_base64url());
    assert_eq!(probe.fingerprint_short, fingerprint.short());

    let wrong = core
        .pair_manual(
            "127.0.0.1".into(),
            port,
            probe.fingerprint.clone(),
            "AAAA-AAAA".into(),
            "Pixel".into(),
        )
        .await;
    assert!(matches!(
        wrong,
        Err(RostrumError::RemoteApi {
            code: RemoteErrorCode::PairingCodeInvalid,
            ..
        })
    ));

    // Typed by hand: lowercase, with the separator.
    let paired = core
        .pair_manual(
            "127.0.0.1".into(),
            port,
            probe.fingerprint,
            "k7qx-m2pd".into(),
            "Pixel".into(),
        )
        .await
        .expect("pairs");
    assert_eq!(paired.device_token, issued_token().expose());

    assert!(matches!(
        core.pair_manual(
            "127.0.0.1".into(),
            port,
            "not base64!".into(),
            CODE.into(),
            "Pixel".into()
        )
        .await,
        Err(RostrumError::InvalidInput { .. })
    ));
    assert!(matches!(
        core.probe_desktop("not a host!".into(), port).await,
        Err(RostrumError::InvalidInput { .. })
    ));
}

#[tokio::test]
async fn an_incompatible_desktop_is_refused_before_pairing() {
    let scratch = Scratch::new("incompatible");
    let core = core(&scratch).await;
    let (port, fingerprint, log) = serve(desktop(99)).await;

    let result = core
        .pair_with_link(link(port, fingerprint), "Pixel".into())
        .await;
    assert_eq!(
        result,
        Err(RostrumError::IncompatibleDesktop {
            desktop: 99,
            supported: 1
        })
    );
    assert!(log.last("/api/v1/pair").is_none());
    assert_eq!(
        core.remote_status().await.expect("status"),
        RemoteStatus::NotPaired
    );
}

#[tokio::test]
async fn an_unreachable_desktop_is_reported() {
    let scratch = Scratch::new("unreachable");
    let core = core(&scratch).await;
    // A port nothing listens on any more.
    let port = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        listener.local_addr().expect("addr").port()
    };
    let saved =
        serde_json::to_string(&endpoint(port, CertFingerprint::of_der(b"x"))).expect("json");
    core.set_remote(saved, issued_token().expose().to_string())
        .await
        .expect("set remote");
    assert!(matches!(
        core.machine_info().await,
        Err(RostrumError::DesktopUnreachable { .. })
    ));
}

#[tokio::test]
async fn local_calls_need_a_desktop_and_a_known_pull_request() {
    let scratch = Scratch::new("local-guards");
    let core = core(&scratch).await;
    assert_eq!(
        core.run_local_job("octo/repo".into(), 7, LocalOp::PullRebase, false)
            .await,
        Err(RostrumError::NotPaired)
    );

    let (port, fingerprint, _) = serve(desktop(1)).await;
    core.pair_with_link(link(port, fingerprint), "Pixel".into())
        .await
        .expect("pairs");
    assert_eq!(
        core.local_status("octo/repo".into(), 99).await,
        Err(RostrumError::UnknownPullRequest {
            repo: "octo/repo".into(),
            number: 99
        })
    );
    assert!(matches!(
        core.pair_with_link("https://example.com".into(), "Pixel".into())
            .await,
        Err(RostrumError::InvalidInput { .. })
    ));
    assert!(matches!(
        core.pair_with_link(link(port, fingerprint), "   ".into())
            .await,
        Err(RostrumError::InvalidInput { .. })
    ));
}

#[derive(Default)]
struct Snapshots(Mutex<Vec<FeedSnapshot>>);

impl FeedObserver for Snapshots {
    fn feed_changed(&self, snapshot: FeedSnapshot) {
        if let Ok(mut all) = self.0.lock() {
            all.push(snapshot);
        }
    }
}

fn repo_names(snapshot: &FeedSnapshot) -> Vec<String> {
    snapshot
        .repos
        .iter()
        .map(|section| section.repo.clone())
        .collect()
}

#[tokio::test]
async fn the_desktops_config_needs_a_paired_desktop() {
    let scratch = Scratch::new("config-unpaired");
    let core = core(&scratch).await;
    assert_eq!(core.desktop_config().await, Err(RostrumError::NotPaired));
    assert_eq!(
        core.copy_desktop_config().await,
        Err(RostrumError::NotPaired)
    );
    // Nothing changed on the phone.
    assert_eq!(
        core.settings().await.expect("settings").repos,
        vec!["octo/repo"]
    );
}

#[tokio::test]
async fn copying_the_desktops_config_end_to_end() {
    let scratch = Scratch::new("config-copy");
    let core = core(&scratch).await;
    // The phone's own habits, which copying must leave alone.
    core.set_refresh_interval(600).await.expect("interval");
    core.set_notifications(true, false)
        .await
        .expect("notifications");
    core.add_repo("rust-lang/rust".into()).await.expect("add");
    core.set_query("needle".into()).await.expect("query");

    let served = Arc::new(Mutex::new(desktop_config()));
    let (port, fingerprint, log) = serve(desktop_serving(1, served.clone())).await;
    core.pair_with_link(link(port, fingerprint), "Pixel".into())
        .await
        .expect("pairs");
    let observer = Arc::new(Snapshots::default());
    core.set_feed_observer(Some(observer.clone()))
        .await
        .expect("observe");

    let preview = core.desktop_config().await.expect("preview");
    assert_eq!(preview.machine, "test-desk");
    assert_eq!(preview.repos, vec!["zed-industries/zed", "octo/repo"]);
    assert_eq!(preview.added, vec!["zed-industries/zed"]);
    assert_eq!(preview.removed, vec!["rust-lang/rust"]);
    assert_eq!(preview.prs_per_repo, 30);
    assert!(preview.hide_drafts && !preview.hide_empty_repos);
    assert_eq!(preview.authors, vec!["alice"]);
    assert!(preview.include_involved && preview.autostash);
    assert!(preview.changes_anything);

    // The desktop changes after the preview: copying applies what it says
    // now, not what the preview showed.
    served.lock().expect("config").repos = vec![RepoId::new("zed-industries", "zed")];
    let settings = core.copy_desktop_config().await.expect("copy");
    assert_eq!(settings.repos, vec!["zed-industries/zed"]);
    assert_eq!(settings.prs_per_repo, 30);
    assert!(settings.feed.hide_drafts && !settings.feed.hide_empty_repos);
    assert_eq!(settings.feed.authors, vec!["alice"]);
    assert!(settings.feed.include_involved && settings.autostash);
    assert_eq!(settings.refresh_interval_secs, 600);
    assert!(settings.notify_new_pull_requests && !settings.notify_review_requests);
    assert_eq!(
        log.requests()
            .iter()
            .filter(|request| request.path == "/api/v1/config")
            .count(),
        2
    );

    // The feed follows: dropped repositories and their pull requests are
    // gone, the search box is untouched.
    let feed = core.cached_feed().await.expect("feed");
    assert_eq!(repo_names(&feed), vec!["zed-industries/zed"]);
    assert_eq!(feed.query, "needle");
    assert!(feed.preferences.hide_drafts);
    assert_eq!(
        core.local_status("octo/repo".into(), 7).await,
        Err(RostrumError::UnknownPullRequest {
            repo: "octo/repo".into(),
            number: 7
        })
    );
    let mut notified = false;
    for _ in 0..200 {
        notified = observer
            .0
            .lock()
            .expect("lock")
            .iter()
            .any(|snapshot| repo_names(snapshot) == vec!["zed-industries/zed"]);
        if notified {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(notified, "the observer saw the copied feed");

    // Copying again would change nothing.
    let again = core.desktop_config().await.expect("preview");
    assert!(again.added.is_empty() && again.removed.is_empty());
    assert!(!again.changes_anything);
    drop(core);

    // The copy was written to disk.
    let reopened = RostrumCore::open(scratch.path()).await.expect("reopen");
    let settings = reopened.settings().await.expect("settings");
    assert_eq!(settings.repos, vec!["zed-industries/zed"]);
    assert_eq!(settings.prs_per_repo, 30);
    assert_eq!(settings.feed.authors, vec!["alice"]);
    assert!(settings.autostash);
    assert_eq!(settings.refresh_interval_secs, 600);
    let feed = reopened.cached_feed().await.expect("feed");
    assert_eq!(repo_names(&feed), vec!["zed-industries/zed"]);
    assert_eq!(feed.query, "");
}
