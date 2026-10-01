//! The stack routes against a daemon on scratch directories, with a fixed
//! GitHub snapshot and recording stack operations: auth, validation, the
//! rewrite confirmation, the job lifecycle and the busy rule.

use std::{sync::Arc, time::Duration};

use axum::{
    body::Bytes,
    http::{Method, StatusCode},
};
use rostrum_core::{PrNumber, RefName, RepoId, StackNumber};
use rostrum_remote::{
    ApiError, ApiErrorCode, ArrangeStackRequest, DeviceToken, ExtendStackRequest, JobRequest,
    LocalOpKind, MakeStackRequest, MergeStackRequest, PrKey, PrRef, StackJobId, StackJobKind,
    StackJobResult, StackJobState, StackJobStatus, StackMergeMethod, StackPlanRequest,
    StackRewritePlan, UnstackRequest, routes,
};
use rostrum_stack::MergeMethod;
use tokio::sync::Semaphore;

use crate::testkit::{
    FakeStackOps, Kit, Options, StackCall, api_request, github_stack, json, pull, repo_state, send,
};

const NO_BODY: Option<&()> = None;

fn repo() -> RepoId {
    RepoId::new("owner", "repo")
}

fn r(name: &str) -> RefName {
    RefName::new(name).expect("valid")
}

fn n(numbers: &[u32]) -> Vec<PrNumber> {
    numbers.iter().copied().map(PrNumber).collect()
}

fn seven() -> StackNumber {
    StackNumber::new(7).expect("non-zero")
}

/// #1 a←main and #2 b←a chain; #3 c←main does not; #4 d←c builds on it.
/// Stack 7 is #10 x←main ← #11 y←x; #12 z←y chains off its top, #13 w←main
/// does not.
fn snapshot() -> rostrum_core::RepoState {
    repo_state(
        vec![
            pull(1, "a", "main"),
            pull(2, "b", "a"),
            pull(3, "c", "main"),
            pull(4, "d", "c"),
            pull(10, "x", "main"),
            pull(11, "y", "x"),
            pull(12, "z", "y"),
            pull(13, "w", "main"),
        ],
        vec![github_stack(7, &[10, 11])],
    )
}

fn kit_with(tag: &str, stacks: Arc<FakeStackOps>) -> Kit {
    let kit = Kit::with(
        tag,
        Options {
            stacks,
            snapshot: snapshot(),
            ..Options::default()
        },
    );
    kit.configure_clone();
    kit
}

fn kit(tag: &str) -> Kit {
    kit_with(tag, Arc::default())
}

async fn post<B: serde::Serialize>(
    kit: &Kit,
    path: &str,
    token: Option<&DeviceToken>,
    body: &B,
) -> (StatusCode, Bytes) {
    send(
        &kit.api(),
        api_request(Method::POST, path, "100.64.0.20", token, Some(body)),
    )
    .await
}

async fn get_job(kit: &Kit, token: &DeviceToken, id: StackJobId) -> (StatusCode, Bytes) {
    send(
        &kit.api(),
        api_request(
            Method::GET,
            &routes::stack_job(id),
            "100.64.0.20",
            Some(token),
            NO_BODY,
        ),
    )
    .await
}

async fn finished(kit: &Kit, token: &DeviceToken, id: StackJobId) -> StackJobStatus {
    for _ in 0..500 {
        let (status, body) = get_job(kit, token, id).await;
        assert_eq!(status, StatusCode::OK);
        let job: StackJobStatus = json(&body);
        if job.is_finished() {
            return job;
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    panic!("stack job {id} never finished");
}

fn error(body: &Bytes) -> ApiError {
    json(body)
}

fn arrange(prs: &[u32], confirm: &[&str]) -> ArrangeStackRequest {
    ArrangeStackRequest {
        repo: repo(),
        prs: n(prs),
        trunk: r("main"),
        confirm_rewrite: confirm.iter().map(|b| r(b)).collect(),
    }
}

fn extend(prs: &[u32], confirm: &[&str]) -> ExtendStackRequest {
    ExtendStackRequest {
        repo: repo(),
        stack: seven(),
        prs: n(prs),
        confirm_rewrite: confirm.iter().map(|b| r(b)).collect(),
    }
}

// --- auth -------------------------------------------------------------------

#[tokio::test]
async fn every_stack_route_is_401_without_a_token() {
    let kit = kit("stack-401");
    let body = serde_json::json!({});
    for path in [
        routes::STACK_PLAN,
        routes::STACK_MAKE,
        routes::STACK_ARRANGE,
        routes::STACK_EXTEND,
        routes::STACK_MERGE,
        routes::STACK_UNSTACK,
    ] {
        let (status, response) = post(&kit, path, None, &body).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{path}");
        assert_eq!(error(&response).code, ApiErrorCode::Unauthorized);
    }
    let (status, _) = send(
        &kit.api(),
        api_request(
            Method::GET,
            &routes::stack_job(StackJobId(1)),
            "100.64.0.20",
            None,
            NO_BODY,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let unknown = DeviceToken::from_bytes([8; 32]);
    let (status, _) = post(&kit, routes::STACK_MAKE, Some(&unknown), &body).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(kit.stacks.calls().is_empty());
}

// --- the dry run ---------------------------------------------------------------

#[tokio::test]
async fn the_plan_names_the_branches_a_rewrite_would_touch() {
    let kit = kit("stack-plan");
    let token = kit.pair("Pixel").await.token;

    let (status, body) = post(
        &kit,
        routes::STACK_PLAN,
        Some(&token),
        &StackPlanRequest::Arrange {
            repo: repo(),
            prs: n(&[1, 2, 3, 4]),
            trunk: r("main"),
        },
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let plan: StackRewritePlan = json(&body);
    assert_eq!(plan.confirm_rewrite(), vec![r("c"), r("d")]);
    assert_eq!(plan.rewrites[0].number, PrNumber(3));

    let (status, body) = post(
        &kit,
        routes::STACK_PLAN,
        Some(&token),
        &StackPlanRequest::Extend {
            repo: repo(),
            stack: seven(),
            prs: n(&[12]),
        },
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(!json::<StackRewritePlan>(&body).needs_rewrite());

    let (status, body) = post(
        &kit,
        routes::STACK_PLAN,
        Some(&token),
        &StackPlanRequest::Arrange {
            repo: repo(),
            prs: n(&[1, 99]),
            trunk: r("main"),
        },
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        error(&body).message.contains("#99"),
        "{}",
        error(&body).message
    );
    assert!(kit.stacks.calls().is_empty(), "a dry run runs nothing");
}

// --- validation and the rewrite confirmation --------------------------------

#[tokio::test]
async fn arranging_with_the_wrong_confirmation_is_refused_and_runs_nothing() {
    let kit = kit("stack-confirm");
    let token = kit.pair("Pixel").await.token;
    for confirm in [&[][..], &["c"][..], &["b", "c", "d"][..], &["d", "e"][..]] {
        let (status, body) = post(
            &kit,
            routes::STACK_ARRANGE,
            Some(&token),
            &arrange(&[1, 2, 3, 4], confirm),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT, "{confirm:?}");
        let error = error(&body);
        assert_eq!(error.code, ApiErrorCode::RewriteNotConfirmed);
        assert!(error.message.contains("`c`, `d`"), "{}", error.message);
    }
    let (status, body) = post(
        &kit,
        routes::STACK_EXTEND,
        Some(&token),
        &extend(&[13], &[]),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error(&body).code, ApiErrorCode::RewriteNotConfirmed);
    assert!(kit.stacks.calls().is_empty());
}

#[tokio::test]
async fn making_a_chain_that_needs_a_rewrite_is_refused() {
    let kit = kit("stack-make-rewrite");
    let token = kit.pair("Pixel").await.token;
    let (status, body) = post(
        &kit,
        routes::STACK_MAKE,
        Some(&token),
        &MakeStackRequest {
            repo: repo(),
            prs: n(&[1, 3]),
            trunk: r("main"),
        },
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error(&body).code, ApiErrorCode::RewriteNotConfirmed);
    assert!(kit.stacks.calls().is_empty());
}

#[tokio::test]
async fn invalid_plans_are_bad_requests_and_unknown_things_are_404() {
    let kit = kit("stack-invalid");
    let token = kit.pair("Pixel").await.token;
    // A closed (here: unknown) pull request.
    let (status, _) = post(
        &kit,
        routes::STACK_ARRANGE,
        Some(&token),
        &arrange(&[1, 50], &[]),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    // Already in stack 7.
    let (status, body) = post(
        &kit,
        routes::STACK_ARRANGE,
        Some(&token),
        &arrange(&[10, 1], &[]),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(error(&body).message.contains("stack 7"));
    // A stack the repository does not have.
    let eight = StackNumber::new(8).expect("non-zero");
    for path in [routes::STACK_MERGE, routes::STACK_UNSTACK] {
        let body = serde_json::json!({"repo": repo(), "stack": eight, "method": "merge"});
        let (status, response) = post(&kit, path, Some(&token), &body).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}");
        assert_eq!(error(&response).code, ApiErrorCode::NotFound);
    }
    // A repository with no clone.
    let mut elsewhere = arrange(&[1, 2], &[]);
    elsewhere.repo = RepoId::new("someone", "else");
    let (status, body) = post(&kit, routes::STACK_ARRANGE, Some(&token), &elsewhere).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(error(&body).message.contains("someone/else"));
    // A body that does not parse (stack 0 is never a stack).
    let (status, _) = post(
        &kit,
        routes::STACK_UNSTACK,
        Some(&token),
        &serde_json::json!({"repo": repo(), "stack": 0}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(kit.stacks.calls().is_empty());
}

// --- jobs ----------------------------------------------------------------------

#[tokio::test]
async fn an_arrangement_runs_as_a_job_that_can_be_polled_to_its_end() {
    let kit = kit("stack-arrange");
    let token = kit.pair("Pixel").await.token;
    let (status, body) = post(
        &kit,
        routes::STACK_ARRANGE,
        Some(&token),
        &arrange(&[1, 2, 3, 4], &["d", "c"]),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let started: StackJobStatus = json(&body);
    assert_eq!(started.kind, StackJobKind::Arrange);
    assert_eq!(started.repo, repo());

    let done = finished(&kit, &token, started.id).await;
    assert!(done.finished_at.is_some());
    let StackJobState::Done { result, detail } = done.state else {
        panic!("done, got {:?}", done.state);
    };
    assert_eq!(
        result,
        StackJobResult::Stacked {
            rewritten: n(&[3, 4]),
            tracked: true
        }
    );
    assert!(detail.contains("Stack created"), "{detail}");
    assert_eq!(
        kit.stacks.calls(),
        vec![StackCall::Make {
            prs: vec![1, 2, 3, 4],
            trunk: "main".into(),
            clone: kit.scratch.join("clone"),
        }]
    );
}

#[tokio::test]
async fn make_extend_merge_and_unstack_each_reach_the_operation() {
    let kit = kit("stack-ops");
    let token = kit.pair("Pixel").await.token;
    let clone = kit.scratch.join("clone");

    let (status, body) = post(
        &kit,
        routes::STACK_MAKE,
        Some(&token),
        &MakeStackRequest {
            repo: repo(),
            prs: n(&[1, 2]),
            trunk: r("main"),
        },
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let made = finished(&kit, &token, json::<StackJobStatus>(&body).id).await;
    assert!(matches!(
        made.state,
        StackJobState::Done {
            result: StackJobResult::Stacked { ref rewritten, .. },
            ..
        } if rewritten.is_empty()
    ));

    let (status, body) = post(
        &kit,
        routes::STACK_EXTEND,
        Some(&token),
        &extend(&[13], &["w"]),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let extended = finished(&kit, &token, json::<StackJobStatus>(&body).id).await;
    assert!(matches!(
        extended.state,
        StackJobState::Done {
            result: StackJobResult::Extended { stack, .. },
            ..
        } if stack == seven()
    ));

    let (status, body) = post(
        &kit,
        routes::STACK_MERGE,
        Some(&token),
        &MergeStackRequest {
            repo: repo(),
            stack: seven(),
            method: StackMergeMethod::Squash,
        },
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let merged = finished(&kit, &token, json::<StackJobStatus>(&body).id).await;
    assert_eq!(merged.kind, StackJobKind::Merge);
    assert!(matches!(
        merged.state,
        StackJobState::Done { ref detail, .. } if detail == "Stack 7 merged: ✓ Merged"
    ));

    let (status, body) = post(
        &kit,
        routes::STACK_UNSTACK,
        Some(&token),
        &UnstackRequest {
            repo: repo(),
            stack: seven(),
        },
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let unstacked = finished(&kit, &token, json::<StackJobStatus>(&body).id).await;
    assert!(matches!(
        unstacked.state,
        StackJobState::Done {
            result: StackJobResult::Unstacked { .. },
            ..
        }
    ));

    assert_eq!(
        kit.stacks.calls(),
        vec![
            StackCall::Make {
                prs: vec![1, 2],
                trunk: "main".into(),
                clone: clone.clone()
            },
            StackCall::Extend {
                stack: 7,
                prs: vec![13]
            },
            StackCall::Merge {
                stack: 7,
                method: MergeMethod::Squash,
                cwd: clone.clone()
            },
            StackCall::Unstack { stack: 7, clone },
        ]
    );
}

#[tokio::test]
async fn an_unknown_or_malformed_job_id() {
    let kit = kit("stack-job-404");
    let token = kit.pair("Pixel").await.token;
    let (status, body) = get_job(&kit, &token, StackJobId(41)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(error(&body).code, ApiErrorCode::NotFound);
    let (status, _) = send(
        &kit.api(),
        api_request(
            Method::GET,
            "/api/v1/stacks/jobs/forty-one",
            "100.64.0.20",
            Some(&token),
            NO_BODY,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn a_running_stack_job_holds_its_clone_and_reports_progress() {
    let gate = Arc::new(Semaphore::new(0));
    let kit = kit_with("stack-busy", Arc::new(FakeStackOps::gated(gate.clone())));
    let token = kit.pair("Pixel").await.token;

    let (status, body) = post(
        &kit,
        routes::STACK_ARRANGE,
        Some(&token),
        &arrange(&[1, 2], &[]),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let id = json::<StackJobStatus>(&body).id;

    // Progress arrives while it runs.
    let mut progress = None;
    for _ in 0..500 {
        let (_, body) = get_job(&kit, &token, id).await;
        if let StackJobState::Running {
            progress: Some(step),
        } = json::<StackJobStatus>(&body).state
        {
            progress = Some(step);
            break;
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    assert_eq!(progress.as_deref(), Some("Fetching branches…"));

    // A second stack job, and a local job, on the same clone are refused.
    let (status, body) = post(
        &kit,
        routes::STACK_UNSTACK,
        Some(&token),
        &UnstackRequest {
            repo: repo(),
            stack: seven(),
        },
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error(&body).code, ApiErrorCode::Busy);
    let local = JobRequest {
        pr: PrRef {
            key: PrKey {
                repo: repo(),
                number: PrNumber(1),
            },
            title: "PR 1".into(),
            url: String::new(),
            body: String::new(),
            head_ref: "a".into(),
            base_ref: "main".into(),
        },
        op: LocalOpKind::PullRebase,
        autostash: false,
    };
    let (status, _) = post(&kit, routes::LOCAL_JOB, Some(&token), &local).await;
    assert_eq!(status, StatusCode::CONFLICT);

    gate.add_permits(1);
    let done = finished(&kit, &token, id).await;
    assert!(matches!(done.state, StackJobState::Done { .. }));

    // Released with the job.
    gate.add_permits(1);
    let (status, _) = post(
        &kit,
        routes::STACK_UNSTACK,
        Some(&token),
        &UnstackRequest {
            repo: repo(),
            stack: seven(),
        },
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn a_busy_clone_is_refused_before_github_is_asked() {
    let kit = kit("stack-busy-first");
    let token = kit.pair("Pixel").await.token;
    let lease = kit
        .daemon
        .jobs
        .acquire(crate::jobs::CloneKey::resolve(&kit.scratch.join("clone")).await)
        .await
        .expect("free");
    // Even an invalid request is answered "busy": the clone is checked first.
    let (status, body) = post(
        &kit,
        routes::STACK_ARRANGE,
        Some(&token),
        &arrange(&[1, 99], &[]),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error(&body).code, ApiErrorCode::Busy);
    drop(lease);
}
