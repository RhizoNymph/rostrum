//! Stack actions from the phone, end to end: the core against a TLS
//! stand-in for the paired desktop's stack routes, with the GitHub stand-in
//! behind the feed (for the local checks and the refresh after a job ends).

mod support;

use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
};

use chrono::Utc;
use rostrum_config::Config;
use rostrum_core::{PrNumber, RefName, RepoId, StackNumber};
use rostrum_ffi::{
    RemoteErrorCode, RostrumCore, RostrumError,
    stack_actions::{
        StackEligibility, StackJobKind, StackJobResult, StackJobState, StackMergeMethod,
        StackPlanCheck, StackPlanRequest, StackRewrite,
    },
};
use rostrum_remote::{
    ArrangeStackRequest, CertFingerprint, DeviceToken, Endpoint, RewriteBranch, StackJobId,
    StackJobKind as WireKind, StackJobResult as WireResult, StackJobState as WireState,
    StackJobStatus, StackRewritePlan,
};
use serde_json::json;
use support::{
    Handler, Log, Request, Scratch,
    github::{FakeGitHub, Pr, World},
    serve,
};

const TOKEN: [u8; 32] = [9; 32];

fn token() -> DeviceToken {
    DeviceToken::from_bytes(TOKEN)
}

fn repo() -> RepoId {
    RepoId::new("octo", "repo")
}

fn json<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(value).expect("serialises")
}

fn job(id: u64, kind: WireKind, state: WireState) -> StackJobStatus {
    let finished = !matches!(state, WireState::Running { .. });
    StackJobStatus {
        id: StackJobId(id),
        repo: repo(),
        kind,
        started_at: Utc::now(),
        finished_at: finished.then(Utc::now),
        state,
    }
}

fn refusal(status: u16, code: &str, message: &str) -> (u16, String) {
    (
        status,
        json!({"code": code, "message": message}).to_string(),
    )
}

/// The desktop's stack routes. Arranging #1,#4 rewrites `topic-4`; job 1
/// (arrange) runs once then finishes; make finishes at once; merging is
/// refused as busy; unknown jobs are not found.
fn desktop(polls: Arc<AtomicU32>) -> Handler {
    Arc::new(move |request: &Request| {
        if request.bearer() != Some(token().expose().to_ascii_lowercase().as_str()) {
            return refusal(401, "unauthorized", "unknown device");
        }
        let body: serde_json::Value = serde_json::from_str(&request.body).unwrap_or_default();
        match (request.method.as_str(), request.path.as_str()) {
            ("POST", "/api/v1/stacks/plan") => {
                let rewrites = if body["kind"] == "arrange" {
                    vec![RewriteBranch {
                        number: PrNumber(4),
                        branch: RefName::new("topic-4").expect("ref"),
                    }]
                } else {
                    Vec::new()
                };
                (200, json(&StackRewritePlan { rewrites }))
            }
            ("POST", "/api/v1/stacks/arrange") => {
                let request: ArrangeStackRequest =
                    serde_json::from_value(body).expect("arrange body");
                if request.confirm_rewrite != vec![RefName::new("topic-4").expect("ref")] {
                    return refusal(
                        409,
                        "rewrite_not_confirmed",
                        "this would rewrite `topic-4`, but the request confirmed no branches",
                    );
                }
                (
                    200,
                    json(&job(
                        1,
                        WireKind::Arrange,
                        WireState::Running { progress: None },
                    )),
                )
            }
            ("GET", "/api/v1/stacks/jobs/1") => {
                let state = if polls.fetch_add(1, Ordering::SeqCst) == 0 {
                    WireState::Running {
                        progress: Some("Rebasing #4 (1/1)…".into()),
                    }
                } else {
                    WireState::Done {
                        result: WireResult::Stacked {
                            rewritten: vec![PrNumber(4)],
                            tracked: true,
                        },
                        detail: "Stacked #1, #4 on main".into(),
                    }
                };
                (200, json(&job(1, WireKind::Arrange, state)))
            }
            ("POST", "/api/v1/stacks/make") => (
                200,
                json(&job(
                    2,
                    WireKind::Make,
                    WireState::Done {
                        result: WireResult::Stacked {
                            rewritten: vec![],
                            tracked: false,
                        },
                        detail: "Stacked #3, #4".into(),
                    },
                )),
            ),
            ("POST", "/api/v1/stacks/extend") => (
                200,
                json(&job(
                    3,
                    WireKind::Extend,
                    WireState::Failed {
                        pushed: vec![PrNumber(3)],
                        detail: "the lease on topic-3 was rejected".into(),
                    },
                )),
            ),
            ("POST", "/api/v1/stacks/merge") => {
                refusal(409, "busy", "another job is running on this clone")
            }
            ("POST", "/api/v1/stacks/unstack") => (
                200,
                json(&job(
                    4,
                    WireKind::Unstack,
                    WireState::Done {
                        result: WireResult::Unstacked {
                            stack: StackNumber::new(7).expect("n"),
                        },
                        detail: "Stack 7 dissolved".into(),
                    },
                )),
            ),
            _ => refusal(404, "not_found", "no such job"),
        }
    })
}

/// `octo/repo` on GitHub: stack 7 = #1 → #2 on main; #3 chained off #2;
/// #4 on main.
fn github() -> World {
    let mut two = Pr::new(2, "alice");
    two.base = "topic-1".into();
    let mut three = Pr::new(3, "alice");
    three.base = "topic-2".into();
    World {
        viewer: "me".into(),
        repos: vec![(
            "octo/repo".into(),
            vec![Pr::new(1, "alice"), two, three, Pr::new(4, "bob")],
        )],
        stacks: Some(json!([
            {"number": 7, "base": {"ref": "main"}, "open": true,
             "pull_requests": [{"number": 1}, {"number": 2}]}
        ])),
        ..Default::default()
    }
}

struct Setup {
    core: Arc<RostrumCore>,
    github: FakeGitHub,
    desktop: Log,
    _scratch: Scratch,
}

async fn setup(tag: &str) -> Setup {
    let github = FakeGitHub::start(github()).await;
    let (port, fingerprint, desktop_log) = serve(desktop(Arc::new(AtomicU32::new(0)))).await;
    let scratch = Scratch::new(tag);
    Config {
        repos: vec!["octo/repo".into()],
        ..Default::default()
    }
    .save_to(&scratch.dir.join("config.json"))
    .expect("config");
    let core = RostrumCore::open_with_github_api(
        scratch.path(),
        github.graphql_url.clone(),
        github.rest_base.clone(),
    )
    .await
    .expect("open");
    core.set_github_token(Some("ghp_test".into()))
        .await
        .expect("token");
    core.refresh_feed().await.expect("refresh");
    core.set_remote(
        json(&endpoint(port, fingerprint)),
        token().expose().to_string(),
    )
    .await
    .expect("remote");
    Setup {
        core,
        github,
        desktop: desktop_log,
        _scratch: scratch,
    }
}

fn endpoint(port: u16, fingerprint: CertFingerprint) -> Endpoint {
    Endpoint::new(vec!["127.0.0.1".parse().expect("host")], port, fingerprint).expect("endpoint")
}

fn feed_reads(github: &FakeGitHub) -> usize {
    github.graphql_calls("pullRequests(states: OPEN").len()
}

#[tokio::test]
async fn the_cached_feed_answers_eligibility_without_the_desktop() {
    let setup = setup("stack-local").await;
    let core = &setup.core;

    let candidates = core
        .stack_candidates("octo/repo".into(), 7)
        .await
        .expect("candidates");
    assert_eq!(
        candidates
            .iter()
            .map(|candidate| (candidate.number, candidate.eligibility.clone()))
            .collect::<Vec<_>>(),
        vec![
            (3, StackEligibility::Eligible { chained: true }),
            (4, StackEligibility::Eligible { chained: false }),
        ]
    );
    assert!(matches!(
        core.stack_candidates("octo/repo".into(), 8).await,
        Err(RostrumError::InvalidInput { .. })
    ));

    let extend = core
        .check_stack_plan(StackPlanRequest::Extend {
            repo: "octo/repo".into(),
            stack: 7,
            prs: vec![4],
        })
        .await
        .expect("check");
    assert_eq!(
        extend,
        StackPlanCheck::Valid {
            rewrites: vec![StackRewrite {
                number: 4,
                branch: "topic-4".into()
            }]
        }
    );
    let taken = core
        .check_stack_plan(StackPlanRequest::Arrange {
            repo: "octo/repo".into(),
            prs: vec![1, 4],
            trunk: "main".into(),
        })
        .await
        .expect("check");
    assert!(
        matches!(taken, StackPlanCheck::Invalid { ref reason } if reason.contains("stack 7")),
        "{taken:?}"
    );
    assert!(setup.desktop.requests().is_empty(), "no desktop call");

    // Cheap mistakes never reach the desktop either.
    assert!(matches!(
        core.make_stack("octo/repo".into(), vec![3], "main".into())
            .await,
        Err(RostrumError::InvalidInput { .. })
    ));
    assert!(matches!(
        core.arrange_stack("octo/repo".into(), vec![3, 3], "main".into(), vec![])
            .await,
        Err(RostrumError::InvalidInput { .. })
    ));
    assert!(matches!(
        core.unstack("octo/repo".into(), 0).await,
        Err(RostrumError::InvalidInput { .. })
    ));
    assert!(setup.desktop.requests().is_empty());
}

#[tokio::test]
async fn arranging_confirms_the_planned_rewrite_and_polls_to_the_end() {
    let setup = setup("stack-arrange").await;
    let core = &setup.core;

    let plan = core
        .plan_stack_rewrite(StackPlanRequest::Arrange {
            repo: "octo/repo".into(),
            prs: vec![1, 4],
            trunk: "main".into(),
        })
        .await
        .expect("plan");
    assert!(plan.needs_rewrite);
    assert_eq!(
        plan.rewrites,
        vec![StackRewrite {
            number: 4,
            branch: "topic-4".into()
        }]
    );

    // Confirming nothing is refused, typed, with the branches to confirm.
    let refused = core
        .arrange_stack("octo/repo".into(), vec![1, 4], "main".into(), vec![])
        .await;
    let Err(RostrumError::RewriteNotConfirmed { branches, reason }) = refused else {
        panic!("expected RewriteNotConfirmed, got {refused:?}");
    };
    assert_eq!(branches, plan.rewrites);
    assert!(reason.contains("topic-4"), "{reason}");

    let confirm: Vec<String> = plan.rewrites.iter().map(|r| r.branch.clone()).collect();
    let started = core
        .arrange_stack("octo/repo".into(), vec![1, 4], "main".into(), confirm)
        .await
        .expect("arrange");
    assert_eq!(started.id, 1);
    assert_eq!(started.kind, StackJobKind::Arrange);
    assert!(!started.finished);
    let sent: serde_json::Value = serde_json::from_str(
        &setup
            .desktop
            .last("/api/v1/stacks/arrange")
            .expect("sent")
            .body,
    )
    .expect("json");
    assert_eq!(sent["prs"], json!([1, 4]));
    assert_eq!(sent["confirm_rewrite"], json!(["topic-4"]));

    let reads = feed_reads(&setup.github);
    let running = core.stack_job(1).await.expect("poll");
    assert_eq!(
        running.state,
        StackJobState::Running {
            progress: Some("Rebasing #4 (1/1)…".into())
        }
    );
    assert_eq!(feed_reads(&setup.github), reads, "no refresh while running");

    let done = core.stack_job(1).await.expect("poll");
    assert!(done.finished);
    assert!(done.finished_at.is_some());
    assert_eq!(
        done.state,
        StackJobState::Done {
            result: StackJobResult::Stacked {
                rewritten: vec![4],
                tracked: true
            },
            detail: "Stacked #1, #4 on main".into()
        }
    );
    assert_eq!(
        feed_reads(&setup.github),
        reads + 1,
        "refreshed once it ended"
    );
    core.stack_job(1).await.expect("poll again");
    assert_eq!(feed_reads(&setup.github), reads + 1, "and only once");
}

#[tokio::test]
async fn every_other_action_maps_its_outcome() {
    let setup = setup("stack-actions").await;
    let core = &setup.core;
    let reads = feed_reads(&setup.github);

    let made = core
        .make_stack("octo/repo".into(), vec![3, 4], "main".into())
        .await
        .expect("make");
    assert!(made.finished);
    assert_eq!(made.kind, StackJobKind::Make);
    assert_eq!(
        feed_reads(&setup.github),
        reads + 1,
        "a finished start refreshes"
    );

    let extended = core
        .extend_stack("octo/repo".into(), 7, vec![3], vec![])
        .await
        .expect("extend");
    assert_eq!(
        extended.state,
        StackJobState::Failed {
            pushed: vec![3],
            detail: "the lease on topic-3 was rejected".into()
        }
    );
    let sent: serde_json::Value = serde_json::from_str(
        &setup
            .desktop
            .last("/api/v1/stacks/extend")
            .expect("sent")
            .body,
    )
    .expect("json");
    assert_eq!(sent["stack"], 7);

    let busy = core
        .merge_stack("octo/repo".into(), 7, StackMergeMethod::Squash)
        .await;
    assert!(
        matches!(
            busy,
            Err(RostrumError::RemoteApi {
                code: RemoteErrorCode::Busy,
                ..
            })
        ),
        "{busy:?}"
    );
    let sent: serde_json::Value = serde_json::from_str(
        &setup
            .desktop
            .last("/api/v1/stacks/merge")
            .expect("sent")
            .body,
    )
    .expect("json");
    assert_eq!(sent["method"], "squash");

    let unstacked = core.unstack("octo/repo".into(), 7).await.expect("unstack");
    assert_eq!(
        unstacked.state,
        StackJobState::Done {
            result: StackJobResult::Unstacked { stack: 7 },
            detail: "Stack 7 dissolved".into()
        }
    );

    assert!(matches!(
        core.stack_job(99).await,
        Err(RostrumError::RemoteApi {
            code: RemoteErrorCode::NotFound,
            ..
        })
    ));
}

#[tokio::test]
async fn stack_actions_need_a_paired_desktop() {
    let github = FakeGitHub::start(github()).await;
    let scratch = Scratch::new("stack-unpaired");
    Config {
        repos: vec!["octo/repo".into()],
        ..Default::default()
    }
    .save_to(&scratch.dir.join("config.json"))
    .expect("config");
    let core = RostrumCore::open_with_github_api(
        scratch.path(),
        github.graphql_url.clone(),
        github.rest_base.clone(),
    )
    .await
    .expect("open");
    assert!(matches!(
        core.unstack("octo/repo".into(), 7).await,
        Err(RostrumError::NotPaired)
    ));
    assert!(matches!(
        core.stack_job(1).await,
        Err(RostrumError::NotPaired)
    ));
}
