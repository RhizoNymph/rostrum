//! The API router against a daemon on scratch directories, one request at a
//! time with `tower::ServiceExt::oneshot`.

use std::{sync::Arc, time::Duration};

use axum::http::{Method, StatusCode};
use rostrum_core::{LoginKey, PrNumber, RepoId};
use rostrum_local::LocalResult;
use rostrum_remote::{
    API_VERSION, AbortRequest, ApiError, ApiErrorCode, DesktopConfig, DeviceToken, GitHubHandover,
    HandoffSession, Hello, JobOutcome, JobRequest, LocalOpKind, LocalStatus, LocalStatusRequest,
    MachineInfo, PairRequest, PrKey, PrRef, SyncAllRequest, SyncEntryState, SyncRun, routes,
};
use tokio::sync::Semaphore;

use crate::{
    jobs::{CloneKey, JobRunner},
    registry::DeviceView,
    testkit::{Kit, Options, answering, api_request, handover, json, send, stranger_code},
    tmux::TmuxSession,
};

const NO_BODY: Option<&()> = None;

fn key(n: u32) -> PrKey {
    PrKey {
        repo: RepoId::new("owner", "repo"),
        number: PrNumber(n),
    }
}

fn pr(n: u32) -> PrRef {
    PrRef {
        key: key(n),
        title: format!("PR {n}"),
        url: format!("https://github.com/owner/repo/pull/{n}"),
        body: String::new(),
        head_ref: format!("feat-{n}"),
        base_ref: "main".into(),
    }
}

fn error(body: &axum::body::Bytes) -> ApiError {
    json(body)
}

async fn get(
    kit: &Kit,
    path: &str,
    token: Option<&DeviceToken>,
) -> (StatusCode, axum::body::Bytes) {
    send(
        &kit.api(),
        api_request(Method::GET, path, "192.168.0.50", token, NO_BODY),
    )
    .await
}

async fn post<B: serde::Serialize>(
    kit: &Kit,
    path: &str,
    token: &DeviceToken,
    body: &B,
) -> (StatusCode, axum::body::Bytes) {
    send(
        &kit.api(),
        api_request(Method::POST, path, "192.168.0.50", Some(token), Some(body)),
    )
    .await
}

// --- hello, fallback --------------------------------------------------------

#[tokio::test]
async fn hello_is_open_and_names_the_machine() {
    let kit = Kit::new("api-hello");
    let (status, body) = get(&kit, routes::HELLO, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        json::<Hello>(&body),
        Hello {
            machine: "test-desk".into(),
            api_version: API_VERSION
        }
    );
}

#[tokio::test]
async fn an_unknown_route_is_a_json_404() {
    let kit = Kit::new("api-404");
    let (status, body) = get(&kit, "/api/v1/nope", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(error(&body).code, ApiErrorCode::NotFound);
}

// --- bearer auth ------------------------------------------------------------

#[tokio::test]
async fn a_missing_garbage_or_unknown_token_is_401() {
    let kit = Kit::new("api-401");
    let api = kit.api();
    let unknown = DeviceToken::from_bytes([42; 32]);
    let requests = [
        api_request(Method::GET, routes::MACHINE, "192.168.0.50", None, NO_BODY),
        api_request(
            Method::GET,
            routes::MACHINE,
            "192.168.0.50",
            Some(&unknown),
            NO_BODY,
        ),
        axum::http::Request::builder()
            .uri(routes::MACHINE)
            .header("authorization", "Bearer garbage")
            .body(axum::body::Body::empty())
            .expect("request"),
        axum::http::Request::builder()
            .uri(routes::MACHINE)
            .header("authorization", format!("Basic {}", unknown.expose()))
            .body(axum::body::Body::empty())
            .expect("request"),
    ];
    for request in requests {
        let (status, body) = send(&api, request).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(error(&body).code, ApiErrorCode::Unauthorized);
    }
}

#[tokio::test]
async fn every_authenticated_route_refuses_without_a_token() {
    let kit = Kit::new("api-401-all");
    let api = kit.api();
    for (method, path) in [
        (Method::GET, routes::MACHINE),
        (Method::GET, routes::CONFIG),
        (Method::GET, routes::GITHUB_TOKEN),
        (Method::POST, routes::LOCAL_STATUS),
        (Method::POST, routes::LOCAL_JOB),
        (Method::POST, routes::LOCAL_ABORT),
        (Method::POST, routes::SYNC_ALL),
        (Method::GET, routes::SYNC_ALL),
        (Method::GET, routes::HANDOFFS),
        (Method::DELETE, routes::DEVICE),
    ] {
        let (status, _) = send(
            &api,
            api_request(
                method.clone(),
                path,
                "192.168.0.50",
                None,
                Some(&serde_json::json!({})),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{method} {path}");
    }
}

#[tokio::test]
async fn a_revoked_device_is_401() {
    let kit = Kit::new("api-revoked");
    let paired = kit.pair("Pixel").await;
    let (status, _) = get(&kit, routes::MACHINE, Some(&paired.token)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        kit.daemon
            .registry
            .revoke(paired.device.clone())
            .await
            .expect("revoke")
    );
    let (status, body) = get(&kit, routes::MACHINE, Some(&paired.token)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(error(&body).code, ApiErrorCode::Unauthorized);
}

#[tokio::test]
async fn an_authenticated_request_records_where_the_device_was_seen() {
    let kit = Kit::new("api-last-ip");
    let paired = kit.pair("Pixel").await;
    let (status, _) = send(
        &kit.api(),
        api_request(
            Method::GET,
            routes::MACHINE,
            "::ffff:100.64.0.77",
            Some(&paired.token),
            NO_BODY,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let devices: Vec<DeviceView> = kit.daemon.registry.devices().await.expect("devices");
    assert_eq!(
        devices[0].last_ip,
        "100.64.0.77".parse::<std::net::IpAddr>().expect("ip")
    );
}

// --- config -----------------------------------------------------------------

#[tokio::test]
async fn the_config_route_is_401_without_a_token() {
    let kit = Kit::new("api-config-401");
    let (status, body) = get(&kit, routes::CONFIG, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(error(&body).code, ApiErrorCode::Unauthorized);
}

#[tokio::test]
async fn a_paired_phone_gets_the_copyable_part_of_the_desktop_config() {
    let kit = Kit::new("api-config");
    kit.write_rostrum_config(&serde_json::json!({
        "repos": [
            "RhizoNymph/rostrum",
            "not a repo",
            "https://github.com/zed-industries/zed",
            "RhizoNymph/rostrum",
            "a/b/c",
            "rust-lang/rust"
        ],
        "refresh_secs": 15,
        "prs_per_repo": 40,
        "notifications": true,
        "hide_empty_repos": false,
        "hide_drafts": true,
        "authors": ["Ada-Lin", "ada-lin", "   ", "RhizoNymph"],
        "include_involved": true,
        "autostash": true,
        "clones": {"RhizoNymph/rostrum": "/home/secret/rostrum"},
        "conflict_handler": {"command": "API_KEY=hunter2 claude {context}"}
    }));
    let paired = kit.pair("Pixel").await;
    let (status, body) = get(&kit, routes::CONFIG, Some(&paired.token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        json::<DesktopConfig>(&body),
        DesktopConfig {
            repos: vec![
                RepoId::new("RhizoNymph", "rostrum"),
                RepoId::new("zed-industries", "zed"),
                RepoId::new("rust-lang", "rust"),
            ],
            prs_per_repo: 40,
            hide_drafts: true,
            hide_empty_repos: false,
            authors: vec![LoginKey::new("ada-lin"), LoginKey::new("rhizonymph")],
            include_involved: true,
            autostash: true,
        }
    );
    // The machine-specific and personal-habit fields never leave the desktop.
    let text = String::from_utf8_lossy(&body);
    for absent in [
        "clones",
        "/home/secret",
        "conflict_handler",
        "hunter2",
        "refresh_secs",
        "notifications",
    ] {
        assert!(
            !text.contains(absent),
            "`{absent}` must not be sent: {text}"
        );
    }
}

#[tokio::test]
async fn a_missing_desktop_config_is_sent_as_the_defaults() {
    let kit = Kit::new("api-config-default");
    let paired = kit.pair("Pixel").await;
    let (status, body) = get(&kit, routes::CONFIG, Some(&paired.token)).await;
    assert_eq!(status, StatusCode::OK);
    let config: DesktopConfig = json(&body);
    let defaults = rostrum_config::Config::default();
    assert_eq!(config.repos.len(), defaults.repos.len());
    assert_eq!(config.prs_per_repo, defaults.prs_per_repo);
    assert!(config.authors.is_empty());
}

// --- pairing ----------------------------------------------------------------

#[tokio::test]
async fn pairing_returns_a_working_token_the_machine_and_no_github_without_one() {
    let kit = Kit::new("api-pair");
    let clone = kit.configure_clone();
    let paired = kit.pair("  Pixel 8  ").await;
    assert_eq!(paired.machine.name, "test-desk");
    assert_eq!(paired.machine.api_version, API_VERSION);
    assert_eq!(paired.machine.clones.len(), 1);
    assert_eq!(paired.machine.clones[0].path, clone.display().to_string());
    assert!(paired.github.is_none());

    let (status, body) = get(&kit, routes::MACHINE, Some(&paired.token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json::<MachineInfo>(&body), paired.machine);

    let devices = kit.daemon.registry.devices().await.expect("devices");
    assert_eq!(devices.len(), 1);
    assert_eq!(devices[0].name, "Pixel 8");
    assert_eq!(devices[0].id, paired.device);
}

#[tokio::test]
async fn pairing_hands_over_the_github_token_when_there_is_one() {
    let kit = Kit::with(
        "api-pair-github",
        Options {
            github: Some(handover()),
            ..Options::default()
        },
    );
    let paired = kit.pair("Pixel").await;
    let github = paired.github.expect("handover");
    assert_eq!(github.token.expose(), "gho_testtoken");
    assert_eq!(github.host, "github.com");

    let (status, body) = get(&kit, routes::GITHUB_TOKEN, Some(&paired.token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json::<GitHubHandover>(&body), handover());
}

#[tokio::test]
async fn the_github_token_route_is_404_without_a_token() {
    let kit = Kit::new("api-github-404");
    let paired = kit.pair("Pixel").await;
    let (status, body) = get(&kit, routes::GITHUB_TOKEN, Some(&paired.token)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(error(&body).code, ApiErrorCode::NotFound);
}

#[tokio::test]
async fn a_code_pairs_only_once() {
    let kit = Kit::new("api-pair-once");
    let code = kit.daemon.registry.issue_code().await.expect("code").code;
    let request = PairRequest {
        code,
        device_name: "a".into(),
    };
    let api = kit.api();
    let pair = || {
        send(
            &api,
            api_request(
                Method::POST,
                routes::PAIR,
                "192.168.0.50",
                None,
                Some(&request),
            ),
        )
    };
    assert_eq!(pair().await.0, StatusCode::OK);
    let (status, body) = pair().await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(error(&body).code, ApiErrorCode::PairingCodeInvalid);
}

#[tokio::test]
async fn a_malformed_code_is_a_bad_request() {
    let kit = Kit::new("api-pair-malformed");
    let (status, body) = send(
        &kit.api(),
        api_request(
            Method::POST,
            routes::PAIR,
            "192.168.0.50",
            None,
            Some(&serde_json::json!({"code": "UUUU", "device_name": "x"})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error(&body).code, ApiErrorCode::BadRequest);
}

#[tokio::test]
async fn repeated_failures_from_one_address_are_rate_limited() {
    let kit = Kit::new("api-pair-throttle");
    let api = kit.api();
    let attempt = |code, from: &'static str| {
        api_request(
            Method::POST,
            routes::PAIR,
            from,
            None,
            Some(&PairRequest {
                code,
                device_name: "x".into(),
            }),
        )
    };
    for _ in 0..crate::registry::codes::FAILURE_LIMIT {
        let (status, _) = send(&api, attempt(stranger_code(), "192.168.0.66")).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }
    let good = kit.daemon.registry.issue_code().await.expect("code").code;
    let (status, body) = send(&api, attempt(good.clone(), "192.168.0.66")).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(error(&body).code, ApiErrorCode::RateLimited);
    // Someone else can still use it.
    let (status, _) = send(&api, attempt(good, "192.168.0.67")).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test(start_paused = true)]
async fn an_expired_code_is_410() {
    let kit = Kit::with(
        "api-pair-expired",
        Options {
            code_ttl: Duration::from_secs(60),
            ..Options::default()
        },
    );
    let code = kit.daemon.registry.issue_code().await.expect("code").code;
    tokio::time::advance(Duration::from_secs(61)).await;
    let (status, body) = send(
        &kit.api(),
        api_request(
            Method::POST,
            routes::PAIR,
            "192.168.0.50",
            None,
            Some(&PairRequest {
                code,
                device_name: "late".into(),
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::GONE);
    assert_eq!(error(&body).code, ApiErrorCode::PairingCodeExpired);
}

// --- local/status -----------------------------------------------------------

#[tokio::test]
async fn status_of_a_repository_without_a_clone_is_not_configured() {
    let kit = Kit::new("api-status-nc");
    let paired = kit.pair("Pixel").await;
    let (status, body) = post(
        &kit,
        routes::LOCAL_STATUS,
        &paired.token,
        &LocalStatusRequest {
            key: key(1),
            head_ref: "feat".into(),
        },
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json::<LocalStatus>(&body), LocalStatus::NotConfigured);
}

#[tokio::test]
async fn an_invalid_branch_is_a_bad_request_on_every_local_route() {
    let kit = Kit::new("api-branch");
    kit.configure_clone();
    let paired = kit.pair("Pixel").await;
    let evil = "--upload-pack=touch /tmp/x";
    let (status, body) = post(
        &kit,
        routes::LOCAL_STATUS,
        &paired.token,
        &LocalStatusRequest {
            key: key(1),
            head_ref: evil.into(),
        },
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error(&body).code, ApiErrorCode::BadRequest);

    let mut bad_base = pr(1);
    bad_base.base_ref = "a..b".into();
    let (status, _) = post(
        &kit,
        routes::LOCAL_JOB,
        &paired.token,
        &JobRequest {
            pr: bad_base,
            op: LocalOpKind::MergeBase,
            autostash: false,
        },
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, _) = post(
        &kit,
        routes::LOCAL_ABORT,
        &paired.token,
        &AbortRequest {
            key: key(1),
            head_ref: evil.into(),
        },
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn a_body_that_does_not_parse_is_a_bad_request() {
    let kit = Kit::new("api-bad-body");
    let paired = kit.pair("Pixel").await;
    let (status, body) = post(
        &kit,
        routes::LOCAL_JOB,
        &paired.token,
        &serde_json::json!({"nope": true}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error(&body).code, ApiErrorCode::BadRequest);
}

// --- local/job --------------------------------------------------------------

fn job(n: u32) -> JobRequest {
    JobRequest {
        pr: pr(n),
        op: LocalOpKind::PullRebase,
        autostash: true,
    }
}

#[tokio::test]
async fn a_job_on_a_repository_without_a_clone_is_not_configured() {
    let kit = Kit::new("api-job-nc");
    let paired = kit.pair("Pixel").await;
    let (status, body) = post(&kit, routes::LOCAL_JOB, &paired.token, &job(1)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json::<JobOutcome>(&body), JobOutcome::NotConfigured);
}

#[tokio::test]
async fn a_job_runs_and_its_result_is_mapped() {
    let kit = Kit::with(
        "api-job",
        Options {
            runner: answering(LocalResult::Refused("the worktree is dirty".into())),
            ..Options::default()
        },
    );
    kit.configure_clone();
    let paired = kit.pair("Pixel").await;
    let (status, body) = post(&kit, routes::LOCAL_JOB, &paired.token, &job(1)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        json::<JobOutcome>(&body),
        JobOutcome::Refused {
            reason: "the worktree is dirty".into()
        }
    );
}

#[tokio::test]
async fn a_job_on_a_busy_clone_is_409() {
    let kit = Kit::new("api-job-busy");
    let clone = kit.configure_clone();
    let paired = kit.pair("Pixel").await;
    let lease = kit
        .daemon
        .jobs
        .acquire(CloneKey::resolve(&clone).await)
        .await
        .expect("free");
    for (path, body) in [
        (
            routes::LOCAL_JOB,
            serde_json::to_value(job(1)).expect("json"),
        ),
        (
            routes::LOCAL_STATUS,
            serde_json::json!({"key": key(1), "head_ref": "feat-1"}),
        ),
        (
            routes::LOCAL_ABORT,
            serde_json::json!({"key": key(1), "head_ref": "feat-1"}),
        ),
    ] {
        let (status, response) = post(&kit, path, &paired.token, &body).await;
        assert_eq!(status, StatusCode::CONFLICT, "{path}");
        assert_eq!(error(&response).code, ApiErrorCode::Busy);
    }
    drop(lease);
    let (status, _) = post(&kit, routes::LOCAL_JOB, &paired.token, &job(1)).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn aborting_where_no_clone_is_configured_is_404() {
    let kit = Kit::new("api-abort-nc");
    let paired = kit.pair("Pixel").await;
    let (status, body) = post(
        &kit,
        routes::LOCAL_ABORT,
        &paired.token,
        &AbortRequest {
            key: key(1),
            head_ref: "feat-1".into(),
        },
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(error(&body).code, ApiErrorCode::NotFound);
}

// --- sync-all ---------------------------------------------------------------

async fn latest(kit: &Kit, token: &DeviceToken) -> Option<SyncRun> {
    let (status, body) = get(kit, routes::SYNC_ALL, Some(token)).await;
    assert_eq!(status, StatusCode::OK);
    json(&body)
}

#[tokio::test]
async fn sync_all_starts_a_run_that_can_be_polled_to_the_end() {
    let kit = Kit::new("api-sync");
    kit.configure_clone();
    let paired = kit.pair("Pixel").await;
    assert_eq!(latest(&kit, &paired.token).await, None);

    let mut elsewhere = pr(3);
    elsewhere.key.repo = RepoId::new("someone", "else");
    let mut odd = pr(4);
    odd.head_ref = "bad..name".into();
    let (status, body) = post(
        &kit,
        routes::SYNC_ALL,
        &paired.token,
        &SyncAllRequest {
            op: LocalOpKind::RebaseBase,
            autostash: false,
            prs: vec![pr(1), elsewhere, pr(2), odd],
        },
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let started: SyncRun = json(&body);
    assert_eq!(started.op, LocalOpKind::RebaseBase);
    assert_eq!(started.entries.len(), 4);

    let mut finished = None;
    for _ in 0..500 {
        if let Some(run) = latest(&kit, &paired.token).await
            && run.is_finished()
        {
            finished = Some(run);
            break;
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    let finished = finished.expect("the run finishes");
    assert_eq!(finished.id, started.id);
    let outcomes: Vec<&JobOutcome> = finished
        .entries
        .iter()
        .map(|entry| match &entry.state {
            SyncEntryState::Done { outcome } => outcome,
            other => panic!("not done: {other:?}"),
        })
        .collect();
    assert_eq!(outcomes[0], &JobOutcome::Completed);
    assert_eq!(outcomes[1], &JobOutcome::NotConfigured);
    assert_eq!(outcomes[2], &JobOutcome::Completed);
    assert!(matches!(outcomes[3], JobOutcome::Failed { .. }));
}

#[tokio::test]
async fn a_second_sync_while_one_runs_is_409_and_so_is_a_job_on_its_clone() {
    let gate = Arc::new(Semaphore::new(0));
    let runner: JobRunner = {
        let gate = gate.clone();
        Arc::new(move |_job| {
            let gate = gate.clone();
            Box::pin(async move {
                gate.acquire().await.expect("gate").forget();
                LocalResult::UpToDate
            })
        })
    };
    let kit = Kit::with(
        "api-sync-busy",
        Options {
            runner,
            ..Options::default()
        },
    );
    kit.configure_clone();
    let paired = kit.pair("Pixel").await;
    let request = SyncAllRequest {
        op: LocalOpKind::PullRebase,
        autostash: false,
        prs: vec![pr(1), pr(2)],
    };
    assert_eq!(
        post(&kit, routes::SYNC_ALL, &paired.token, &request)
            .await
            .0,
        StatusCode::OK
    );
    let (status, body) = post(&kit, routes::SYNC_ALL, &paired.token, &request).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error(&body).code, ApiErrorCode::Busy);
    let (status, _) = post(&kit, routes::LOCAL_JOB, &paired.token, &job(5)).await;
    assert_eq!(status, StatusCode::CONFLICT);
    gate.add_permits(2);
}

// --- handoffs, device -------------------------------------------------------

#[tokio::test]
async fn handoffs_list_rostrum_sessions_joined_with_what_the_daemon_recorded() {
    let kit = Kit::with(
        "api-handoffs",
        Options {
            runner: answering(LocalResult::HandedOff {
                session: "rostrum-owner-repo-1".into(),
            }),
            sessions: vec![
                TmuxSession {
                    name: "rostrum-owner-repo-1".into(),
                    created: chrono::DateTime::from_timestamp(1_759_000_000, 0),
                },
                TmuxSession {
                    name: "rostrum-other-thing-9".into(),
                    created: chrono::DateTime::from_timestamp(1_758_000_000, 0),
                },
                TmuxSession {
                    name: "personal".into(),
                    created: None,
                },
            ],
            ..Options::default()
        },
    );
    kit.configure_clone();
    let paired = kit.pair("Pixel").await;
    let (status, body) = post(&kit, routes::LOCAL_JOB, &paired.token, &job(1)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        json::<JobOutcome>(&body),
        JobOutcome::HandedOff {
            session: "rostrum-owner-repo-1".into()
        }
    );

    let (status, body) = get(&kit, routes::HANDOFFS, Some(&paired.token)).await;
    assert_eq!(status, StatusCode::OK);
    let sessions: Vec<HandoffSession> = json(&body);
    assert_eq!(sessions.len(), 2);
    assert_eq!(sessions[0].session, "rostrum-owner-repo-1");
    assert_eq!(sessions[0].key, Some(key(1)));
    assert_eq!(sessions[0].head_ref.as_deref(), Some("feat-1"));
    assert_eq!(sessions[1].session, "rostrum-other-thing-9");
    assert_eq!(sessions[1].key, None);
}

#[tokio::test]
async fn no_tmux_sessions_is_an_empty_list() {
    let kit = Kit::new("api-handoffs-empty");
    let paired = kit.pair("Pixel").await;
    let (status, body) = get(&kit, routes::HANDOFFS, Some(&paired.token)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(json::<Vec<HandoffSession>>(&body).is_empty());
}

#[tokio::test]
async fn deleting_the_device_forgets_the_caller() {
    let kit = Kit::new("api-forget");
    let keep = kit.pair("keep").await;
    let gone = kit.pair("gone").await;
    let (status, body) = send(
        &kit.api(),
        api_request(
            Method::DELETE,
            routes::DEVICE,
            "192.168.0.50",
            Some(&gone.token),
            NO_BODY,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(&body[..], b"null");
    assert_eq!(
        get(&kit, routes::MACHINE, Some(&gone.token)).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        get(&kit, routes::MACHINE, Some(&keep.token)).await.0,
        StatusCode::OK
    );
    let devices = kit.daemon.registry.devices().await.expect("devices");
    assert_eq!(devices.len(), 1);
    assert_eq!(devices[0].name, "keep");
}
