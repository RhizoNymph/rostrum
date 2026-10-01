//! `run_extend_job` end to end against real git, with the recording `gh`
//! double: adding pull requests to the top of stack 7, which is #1 (`a`, on
//! `main`) ← #2 (`b`, on `a`).

mod support;

use rostrum_core::{
    ExtendPlan, LoadState, PrNumber, PullRequest, RefName, RepoState, Stack, StackMembers,
    StackNumber, plan_extend,
};
use rostrum_git::PushRejection;
use rostrum_stack::{
    ExtendJob, LocalTracking, Progress, StackOpError, StackOutcome, run_extend_job,
};
use support::*;

fn seven() -> StackNumber {
    StackNumber::new(7).expect("non-zero")
}

/// The plan for adding `order` to stack 7, given the open pull requests
/// beyond #1 and #2.
fn extend(extra: Vec<PullRequest>, order: &[u32]) -> ExtendPlan {
    let mut prs = vec![pull(1, "a", "main"), pull(2, "b", "a")];
    prs.extend(extra);
    let state = RepoState {
        prs,
        stacks: vec![Stack {
            repo: repo_id(),
            number: Some(seven()),
            trunk: RefName::new("main").expect("valid"),
            members: StackMembers::new(vec![PrNumber(1), PrNumber(2)]).expect("valid"),
        }],
        load: LoadState::Idle,
        ..RepoState::new(repo_id())
    };
    let order: Vec<PrNumber> = order.iter().copied().map(PrNumber).collect();
    plan_extend(&state, seven(), &order).expect("valid extension")
}

fn job(fixture: &Fixture, plan: ExtendPlan) -> ExtendJob {
    ExtendJob {
        clone: fixture.clone(),
        plan,
        handler: None,
        scratch_dir: fixture.scratch(),
    }
}

fn extend_argv(numbers: &[&str]) -> Vec<String> {
    let mut argv = vec!["stack".to_string(), "link".into(), "7".into()];
    argv.extend(numbers.iter().map(|n| n.to_string()));
    argv
}

fn existing(fixture: &Fixture) -> (String, String) {
    (fixture.origin("a"), fixture.origin("b"))
}

#[tokio::test]
async fn additions_already_on_the_top_are_linked_without_a_rewrite() {
    let fixture = Fixture::new(
        "ext-chained",
        &[("a", "main"), ("b", "a"), ("c", "b"), ("d", "c")],
    );
    let before = (existing(&fixture), fixture.origin("c"), fixture.origin("d"));
    let plan = extend(vec![pull(3, "c", "b"), pull(4, "d", "c")], &[3, 4]);
    assert!(!plan.needs_rewrite());
    let gh = FakeGh::default();

    let outcome = run_extend_job(job(&fixture, plan), &gh, &Progress::none())
        .await
        .expect("runs");

    let StackOutcome::Extended { stack, report } = outcome else {
        panic!("expected an extension, got {outcome:?}");
    };
    assert_eq!(stack, seven());
    assert!(report.rewritten.is_empty());
    assert!(matches!(report.local, LocalTracking::Skipped(_)));
    assert_eq!(
        (existing(&fixture), fixture.origin("c"), fixture.origin("d")),
        before,
        "nothing pushed"
    );
    assert_eq!(gh.argvs(), vec![extend_argv(&["3", "4"])]);
}

#[tokio::test]
async fn unchained_additions_are_rebased_onto_the_top_and_lease_pushed() {
    let fixture = Fixture::new(
        "ext-rebase",
        &[("a", "main"), ("b", "a"), ("c", "main"), ("d", "main")],
    );
    let before = existing(&fixture);
    let plan = extend(vec![pull(3, "c", "main"), pull(4, "d", "main")], &[3, 4]);
    let names: Vec<&str> = plan.rewrites().iter().map(|m| m.head.as_str()).collect();
    assert_eq!(names, vec!["c", "d"]);
    let gh = FakeGh::default();

    let outcome = run_extend_job(job(&fixture, plan), &gh, &Progress::none())
        .await
        .expect("runs");

    let StackOutcome::Extended { report, .. } = outcome else {
        panic!("expected an extension, got {outcome:?}");
    };
    assert_eq!(report.rewritten, vec![PrNumber(3), PrNumber(4)]);
    assert_eq!(
        existing(&fixture),
        before,
        "the stack's own members are untouched"
    );
    assert!(fixture.origin_is_ancestor("b", "c"));
    assert!(fixture.origin_is_ancestor("c", "d"));
    assert_eq!(
        output(
            &fixture.path("origin.git"),
            &["rev-list", "--count", "b..c"]
        ),
        "1",
        "only c's own commit sits on the top"
    );
    assert_eq!(gh.argvs(), vec![extend_argv(&["3", "4"])]);
    assert_eq!(fixture.worktree_count(), 1, "scratch worktrees are removed");
}

#[tokio::test]
async fn a_chained_addition_below_a_rewrite_is_left_alone() {
    let fixture = Fixture::new(
        "ext-mixed",
        &[("a", "main"), ("b", "a"), ("c", "b"), ("d", "main")],
    );
    let c_before = fixture.origin("c");
    let plan = extend(vec![pull(3, "c", "b"), pull(4, "d", "main")], &[3, 4]);
    let gh = FakeGh::default();

    let outcome = run_extend_job(job(&fixture, plan), &gh, &Progress::none())
        .await
        .expect("runs");
    let StackOutcome::Extended { report, .. } = outcome else {
        panic!("expected an extension, got {outcome:?}");
    };
    assert_eq!(report.rewritten, vec![PrNumber(4)]);
    assert_eq!(fixture.origin("c"), c_before);
    assert!(fixture.origin_is_ancestor("c", "d"));
}

#[tokio::test]
async fn a_refused_lease_stops_before_linking() {
    let fixture = Fixture::new(
        "ext-lease",
        &[("a", "main"), ("b", "a"), ("c", "main"), ("d", "main")],
    );
    git(
        &fixture.root,
        &["clone", "-q", "--bare", "origin.git", "pushed.git"],
    );
    fixture.advance(
        fixture.path("pushed.git").to_str().expect("utf-8"),
        "d",
        "late.txt",
        "late\n",
    );
    let theirs = output(&fixture.path("pushed.git"), &["rev-parse", "d"]);
    git(
        &fixture.clone(),
        &[
            "config",
            "remote.origin.pushurl",
            fixture.path("pushed.git").to_str().expect("utf-8"),
        ],
    );
    let gh = FakeGh::default();
    let plan = extend(vec![pull(3, "c", "main"), pull(4, "d", "main")], &[3, 4]);

    let outcome = run_extend_job(job(&fixture, plan), &gh, &Progress::none())
        .await
        .expect("runs");
    assert_eq!(
        outcome,
        StackOutcome::PushRejected {
            pushed: vec![PrNumber(3)],
            number: PrNumber(4),
            reason: PushRejection::StaleLease,
        }
    );
    assert_eq!(
        output(&fixture.path("pushed.git"), &["rev-parse", "d"]),
        theirs
    );
    assert!(gh.commands().is_empty());
}

#[tokio::test]
async fn a_rerun_after_a_failed_link_resumes_without_rewriting() {
    let fixture = Fixture::new("ext-resume", &[("a", "main"), ("b", "a"), ("c", "main")]);
    let plan = || extend(vec![pull(3, "c", "main")], &[3]);

    let failing = FakeGh::failing_link();
    let outcome = run_extend_job(job(&fixture, plan()), &failing, &Progress::none())
        .await
        .expect("runs");
    let StackOutcome::LinkFailed { pushed, .. } = outcome else {
        panic!("expected a link failure, got {outcome:?}");
    };
    assert_eq!(pushed, vec![PrNumber(3)]);
    let rewritten = fixture.origin("c");

    // GitHub still says #3 targets main (the link never ran), so the plan
    // still asks for a rewrite; the branch is already on the top, so none
    // happens.
    let gh = FakeGh::default();
    let outcome = run_extend_job(job(&fixture, plan()), &gh, &Progress::none())
        .await
        .expect("runs");
    let StackOutcome::Extended { report, .. } = outcome else {
        panic!("expected an extension, got {outcome:?}");
    };
    assert!(report.rewritten.is_empty());
    assert_eq!(fixture.origin("c"), rewritten);
    assert_eq!(gh.argvs(), vec![extend_argv(&["3"])]);
}

#[tokio::test]
async fn a_conflict_aborts_and_leaves_everything_as_it_was() {
    let fixture = Fixture::new("ext-conflict", &[("a", "main"), ("b", "a"), ("c", "main")]);
    let bare = fixture.path("origin.git");
    fixture.advance(
        bare.to_str().expect("utf-8"),
        "b",
        "shared.txt",
        "one\nB\nthree\n",
    );
    fixture.advance(
        bare.to_str().expect("utf-8"),
        "c",
        "shared.txt",
        "one\nC\nthree\n",
    );
    let before = (existing(&fixture), fixture.origin("c"));
    let gh = FakeGh::default();

    let outcome = run_extend_job(
        job(&fixture, extend(vec![pull(3, "c", "main")], &[3])),
        &gh,
        &Progress::none(),
    )
    .await
    .expect("runs");
    assert!(
        matches!(outcome, StackOutcome::Conflicted { number, .. } if number == PrNumber(3)),
        "{outcome:?}"
    );
    assert_eq!((existing(&fixture), fixture.origin("c")), before);
    assert!(gh.commands().is_empty());
    assert_eq!(fixture.worktree_count(), 1);
}

#[tokio::test]
async fn a_top_branch_missing_on_the_remote_names_the_top() {
    let fixture = Fixture::new("ext-no-top", &[("a", "main"), ("c", "main")]);
    let err = run_extend_job(
        job(&fixture, extend(vec![pull(3, "c", "main")], &[3])),
        &FakeGh::default(),
        &Progress::none(),
    )
    .await
    .expect_err("fails");
    assert!(
        matches!(&err, StackOpError::MissingOnRemote { branch, number } if branch == "b" && *number == PrNumber(2)),
        "{err:?}"
    );
}
