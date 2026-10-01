//! Stack jobs end to end: the phone's real `RemoteClient` over TLS, the
//! daemon's real routes, `rostrum-stack`'s real pipeline and real git against
//! a scratch origin and clone — with `rostrum-stack`'s own recording `gh`
//! double, so no `gh stack` command ever runs for real.

mod common;
#[path = "../../rostrum-stack/tests/support/mod.rs"]
mod support;

use std::{sync::Arc, time::Duration};

use common::{FixedSnapshots, Harness, StackSetup};
use rostrum_core::{LoadState, PrNumber, RefName, RepoState};
use rostrum_remote::{
    ApiErrorCode, ArrangeStackRequest, MakeStackRequest, StackJobId, StackJobResult, StackJobState,
    StackJobStatus, StackPlanRequest, UnstackRequest,
    client::{ClientError, RemoteClient},
};
use rostrumd::stacks::GhStackOps;
use support::{FakeGh, Fixture, init_argv, link_argv, pull, repo_id, view_argv};

fn r(name: &str) -> RefName {
    RefName::new(name).expect("valid")
}

fn n(numbers: &[u32]) -> Vec<PrNumber> {
    numbers.iter().copied().map(PrNumber).collect()
}

/// The daemon, configured with `fixture`'s clone of `octo/repo`, answering
/// snapshots with `prs` and running stack jobs with `gh`.
async fn harness(
    tag: &str,
    fixture: &Fixture,
    prs: Vec<rostrum_core::PullRequest>,
    gh: Arc<FakeGh>,
) -> Harness {
    Harness::start_with(
        tag,
        None,
        Some(serde_json::json!({
            "clones": { "octo/repo": fixture.clone().display().to_string() }
        })),
        StackSetup {
            ops: Arc::new(GhStackOps(gh)),
            snapshots: Arc::new(FixedSnapshots(RepoState {
                prs,
                load: LoadState::Idle,
                ..RepoState::new(repo_id())
            })),
            scratch_dir: Some(fixture.scratch()),
        },
    )
    .await
}

async fn finished(client: &RemoteClient, id: StackJobId) -> StackJobStatus {
    for _ in 0..600 {
        let status = client.stack_job(id).await.expect("poll");
        if status.is_finished() {
            return status;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("stack job {id} never finished");
}

#[tokio::test]
async fn a_phone_arranges_a_stack_after_confirming_exactly_the_planned_rewrite() {
    let fixture = Fixture::new("rd-arrange", &[("a", "main"), ("b", "a"), ("c", "main")]);
    let (a_before, b_before) = (fixture.origin("a"), fixture.origin("b"));
    let gh = Arc::new(FakeGh::default());
    let prs = vec![
        pull(1, "a", "main"),
        pull(2, "b", "a"),
        pull(3, "c", "main"),
    ];
    let harness = harness("rd-arrange-daemon", &fixture, prs, gh.clone()).await;
    let (client, _) = harness.pair("phone").await;

    // 1. The dry run names exactly c.
    let plan = client
        .plan_stack_rewrite(&StackPlanRequest::Arrange {
            repo: repo_id(),
            prs: n(&[1, 2, 3]),
            trunk: r("main"),
        })
        .await
        .expect("plan");
    assert_eq!(plan.confirm_rewrite(), vec![r("c")]);

    // 2. Confirming something else is refused, and nothing runs.
    let refused = client
        .arrange_stack(&ArrangeStackRequest {
            repo: repo_id(),
            prs: n(&[1, 2, 3]),
            trunk: r("main"),
            confirm_rewrite: vec![r("b"), r("c")],
        })
        .await
        .expect_err("over-confirmed");
    assert!(
        matches!(&refused, ClientError::Api(error) if error.code == ApiErrorCode::RewriteNotConfirmed),
        "{refused:?}"
    );
    assert!(gh.argvs().is_empty());
    assert!(!fixture.origin_is_ancestor("b", "c"), "nothing pushed");

    // 3. Confirming the plan runs it; the job can be polled to its end.
    let started = client
        .arrange_stack(&ArrangeStackRequest {
            repo: repo_id(),
            prs: n(&[1, 2, 3]),
            trunk: r("main"),
            confirm_rewrite: plan.confirm_rewrite(),
        })
        .await
        .expect("starts");
    let done = finished(&client, started.id).await;
    let StackJobState::Done { result, detail } = done.state else {
        panic!("expected done, got {:?}", done.state);
    };
    assert_eq!(
        result,
        StackJobResult::Stacked {
            rewritten: n(&[3]),
            tracked: true
        }
    );
    assert!(
        detail.contains("1 branch(es) rebased and pushed"),
        "{detail}"
    );

    // Only c moved — onto b — and the bottom two are untouched.
    assert_eq!(
        (fixture.origin("a"), fixture.origin("b")),
        (a_before, b_before)
    );
    assert!(fixture.origin_is_ancestor("b", "c"));
    assert_eq!(
        gh.argvs(),
        vec![
            link_argv(&["1", "2", "3"]),
            init_argv(&["a", "b", "c"]),
            view_argv()
        ]
    );
    assert_eq!(fixture.worktree_count(), 1, "scratch worktrees are removed");

    harness.stop().await;
}

#[tokio::test]
async fn a_phone_makes_a_chain_into_a_stack_and_the_clone_is_busy_meanwhile_only() {
    let fixture = Fixture::new("rd-make", &[("a", "main"), ("b", "a")]);
    let (a_before, b_before) = (fixture.origin("a"), fixture.origin("b"));
    let gh = Arc::new(FakeGh::default());
    let prs = vec![pull(1, "a", "main"), pull(2, "b", "a")];
    let harness = harness("rd-make-daemon", &fixture, prs, gh.clone()).await;
    let (client, _) = harness.pair("phone").await;

    let started = client
        .make_stack(&MakeStackRequest {
            repo: repo_id(),
            prs: n(&[1, 2]),
            trunk: r("main"),
        })
        .await
        .expect("starts");
    let done = finished(&client, started.id).await;
    assert!(
        matches!(
            &done.state,
            StackJobState::Done {
                result: StackJobResult::Stacked { rewritten, .. },
                ..
            } if rewritten.is_empty()
        ),
        "{:?}",
        done.state
    );
    assert_eq!(
        (fixture.origin("a"), fixture.origin("b")),
        (a_before, b_before)
    );
    assert_eq!(gh.argvs()[0], link_argv(&["1", "2"]));

    // Finished, the clone is free again: a further request is validated
    // (stack 3 does not exist) rather than refused as busy.
    let err = client
        .unstack(&UnstackRequest {
            repo: repo_id(),
            stack: rostrum_core::StackNumber::new(3).expect("non-zero"),
        })
        .await
        .expect_err("no such stack");
    assert!(
        matches!(&err, ClientError::Api(error) if error.code == ApiErrorCode::NotFound),
        "{err:?}"
    );

    harness.stop().await;
}

#[tokio::test]
async fn an_unpaired_phone_cannot_reach_the_stack_routes() {
    let harness = Harness::start("rd-stack-401", None, None).await;
    let client = RemoteClient::new(harness.endpoint(), None).expect("client");
    let err = client
        .make_stack(&MakeStackRequest {
            repo: repo_id(),
            prs: n(&[1, 2]),
            trunk: r("main"),
        })
        .await
        .expect_err("no token");
    assert!(matches!(err, ClientError::Unauthorized), "{err:?}");
    let err = client.stack_job(StackJobId(1)).await.expect_err("no token");
    assert!(matches!(err, ClientError::Unauthorized), "{err:?}");
    harness.stop().await;
}
