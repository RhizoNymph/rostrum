//! The coordinator with a fake job runner: ordering, one-at-a-time, busy
//! rejection, release, handoff records and shutdown.

use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use rostrum_core::{PrNumber, RepoId};
use rostrum_git::{Autostash, BranchName};
use rostrum_local::{LocalJob, LocalResult};
use rostrum_remote::{JobOutcome, LocalOpKind, PrKey, PrRef, SyncEntryState, SyncRun};
use tokio::sync::Semaphore;

use super::*;
use crate::fsutil::ScratchDir;

/// Records what ran, and optionally waits for a permit before each job.
#[derive(Default)]
struct Recorder {
    order: Mutex<Vec<u32>>,
    running: AtomicUsize,
    max_running: AtomicUsize,
}

fn runner(recorder: Arc<Recorder>, gate: Option<Arc<Semaphore>>, result: LocalResult) -> JobRunner {
    Arc::new(move |job: LocalJob| {
        let recorder = recorder.clone();
        let gate = gate.clone();
        let result = result.clone();
        Box::pin(async move {
            let now = recorder.running.fetch_add(1, Ordering::SeqCst) + 1;
            recorder.max_running.fetch_max(now, Ordering::SeqCst);
            recorder.order.lock().expect("lock").push(job.pr.number.0);
            match gate {
                Some(gate) => gate.acquire().await.expect("gate").forget(),
                None => tokio::time::sleep(Duration::from_millis(5)).await,
            }
            recorder.running.fetch_sub(1, Ordering::SeqCst);
            result
        })
    })
}

fn jobs(scratch: &ScratchDir, runner: JobRunner) -> Jobs {
    let book = HandoffBook::load(scratch.join("handoffs.json")).expect("empty");
    Jobs::spawn(runner, book)
}

fn key(name: &str) -> CloneKey {
    CloneKey::of_path(Path::new(&format!("/nonexistent/rostrumd-test/{name}")))
}

fn pr(n: u32) -> PrRef {
    PrRef {
        key: PrKey {
            repo: RepoId::new("owner", "repo"),
            number: PrNumber(n),
        },
        title: format!("PR {n}"),
        url: format!("https://github.com/owner/repo/pull/{n}"),
        body: String::new(),
        head_ref: format!("feat-{n}"),
        base_ref: "main".into(),
    }
}

fn on_clone(n: u32, clone: &str) -> PlannedEntry {
    PlannedEntry {
        pr: pr(n),
        target: EntryTarget::Clone {
            key: key(clone),
            path: PathBuf::from(format!("/nonexistent/rostrumd-test/{clone}")),
            head: BranchName::new(format!("feat-{n}")).expect("branch"),
            base: BranchName::new("main").expect("branch"),
        },
    }
}

fn plan(entries: Vec<PlannedEntry>) -> SyncPlan {
    SyncPlan {
        op: LocalOpKind::PullRebase,
        autostash: Autostash::Disabled,
        handler: None,
        entries,
    }
}

/// Poll the coordinator until `done` holds for the latest run.
async fn wait_for(jobs: &Jobs, done: impl Fn(&SyncRun) -> bool) -> SyncRun {
    for _ in 0..2000 {
        if let Some(run) = jobs.latest_sync().await.expect("latest")
            && done(&run)
        {
            return run;
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    panic!("the sync run never reached the expected state");
}

fn entry_done(run: &SyncRun, index: usize) -> bool {
    matches!(run.entries[index].state, SyncEntryState::Done { .. })
}

#[tokio::test]
async fn sync_all_runs_entries_one_at_a_time_in_request_order() {
    let scratch = ScratchDir::new("jobs-order");
    let recorder = Arc::new(Recorder::default());
    let jobs = jobs(
        &scratch,
        runner(recorder.clone(), None, LocalResult::Completed),
    );

    let started = jobs
        .start_sync(plan(vec![
            on_clone(5, "a"),
            PlannedEntry {
                pr: pr(6),
                target: EntryTarget::NotConfigured,
            },
            on_clone(3, "b"),
            PlannedEntry {
                pr: pr(9),
                target: EntryTarget::Invalid {
                    reason: "bad branch".into(),
                },
            },
            on_clone(1, "a"),
            on_clone(8, "b"),
        ]))
        .await
        .expect("starts");
    assert_eq!(started.entries.len(), 6);
    assert!(
        started
            .entries
            .iter()
            .all(|entry| entry.state == SyncEntryState::Pending)
    );
    assert_eq!(started.finished_at, None);

    let finished = wait_for(&jobs, SyncRun::is_finished).await;
    assert_eq!(finished.id, started.id);
    assert_eq!(*recorder.order.lock().expect("lock"), vec![5, 3, 1, 8]);
    assert_eq!(recorder.max_running.load(Ordering::SeqCst), 1);

    let outcomes: Vec<JobOutcome> = finished
        .entries
        .iter()
        .map(|entry| match &entry.state {
            SyncEntryState::Done { outcome } => outcome.clone(),
            other => panic!("not done: {other:?}"),
        })
        .collect();
    assert_eq!(
        outcomes,
        vec![
            JobOutcome::Completed,
            JobOutcome::NotConfigured,
            JobOutcome::Completed,
            JobOutcome::Failed {
                reason: "bad branch".into()
            },
            JobOutcome::Completed,
            JobOutcome::Completed,
        ]
    );
    let summary = finished.summary();
    assert_eq!(
        (summary.updated, summary.skipped, summary.failed),
        (4, 1, 1)
    );

    // Every clone is free again.
    drop(jobs.acquire(key("a")).await.expect("a is free"));
    drop(jobs.acquire(key("b")).await.expect("b is free"));
}

#[tokio::test]
async fn a_running_sync_holds_its_clones_until_their_last_entry() {
    let scratch = ScratchDir::new("jobs-busy");
    let recorder = Arc::new(Recorder::default());
    let gate = Arc::new(Semaphore::new(0));
    let jobs = jobs(
        &scratch,
        runner(recorder.clone(), Some(gate.clone()), LocalResult::UpToDate),
    );

    jobs.start_sync(plan(vec![
        on_clone(1, "a"),
        on_clone(2, "b"),
        on_clone(3, "a"),
    ]))
    .await
    .expect("starts");

    let busy = |result: Result<Lease, JobsError>| match result {
        Err(JobsError::Busy(busy)) => busy,
        Ok(_) => panic!("expected busy, got a lease"),
        Err(other) => panic!("expected busy, got {other}"),
    };
    assert_eq!(busy(jobs.acquire(key("a")).await), Busy::SyncAll);
    assert_eq!(busy(jobs.acquire(key("b")).await), Busy::SyncAll);
    drop(
        jobs.acquire(key("c"))
            .await
            .expect("an untouched clone is free"),
    );
    assert!(matches!(
        jobs.start_sync(plan(vec![on_clone(4, "c")])).await,
        Err(JobsError::Busy(Busy::SyncRunning))
    ));

    // First entry (a) finishes; a still has entry 3 to go.
    gate.add_permits(1);
    wait_for(&jobs, |run| entry_done(run, 0)).await;
    assert_eq!(busy(jobs.acquire(key("a")).await), Busy::SyncAll);

    // Second entry (b) finishes; b has nothing left and is released.
    gate.add_permits(1);
    wait_for(&jobs, |run| entry_done(run, 1)).await;
    drop(
        jobs.acquire(key("b"))
            .await
            .expect("b released after its last entry"),
    );
    assert_eq!(busy(jobs.acquire(key("a")).await), Busy::SyncAll);

    gate.add_permits(1);
    wait_for(&jobs, SyncRun::is_finished).await;
    drop(jobs.acquire(key("a")).await.expect("a released at the end"));
    assert_eq!(recorder.max_running.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn a_held_clone_refuses_a_second_job_and_a_sync() {
    let scratch = ScratchDir::new("jobs-single");
    let jobs = jobs(
        &scratch,
        runner(Arc::default(), None, LocalResult::Completed),
    );
    let lease = jobs.acquire(key("a")).await.expect("free");
    assert!(matches!(
        jobs.acquire(key("a")).await,
        Err(JobsError::Busy(Busy::Job))
    ));
    assert!(matches!(
        jobs.start_sync(plan(vec![on_clone(1, "b"), on_clone(2, "a")]))
            .await,
        Err(JobsError::Busy(Busy::Job))
    ));
    // The refused sync reserved nothing.
    drop(jobs.acquire(key("b")).await.expect("b untouched"));
    drop(lease);
    drop(jobs.acquire(key("a")).await.expect("released on drop"));
}

#[tokio::test]
async fn exclusive_work_finishes_even_when_the_caller_gives_up() {
    let scratch = ScratchDir::new("jobs-detached");
    let jobs = jobs(
        &scratch,
        runner(Arc::default(), None, LocalResult::Completed),
    );
    let (release, wait) = tokio::sync::oneshot::channel::<()>();
    let (finished_tx, finished) = tokio::sync::oneshot::channel::<()>();
    let caller = jobs.exclusive(key("a"), async move {
        let _ = wait.await;
        let _ = finished_tx.send(());
    });
    // The caller times out (the phone hung up) while the work is running.
    assert!(
        tokio::time::timeout(Duration::from_millis(20), caller)
            .await
            .is_err()
    );
    assert!(matches!(
        jobs.acquire(key("a")).await,
        Err(JobsError::Busy(Busy::Job))
    ));
    release.send(()).expect("work still waiting");
    finished.await.expect("the work ran to the end");
    // The lease goes with the work, not with the request.
    for _ in 0..500 {
        if let Ok(lease) = jobs.acquire(key("a")).await {
            drop(lease);
            return;
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    panic!("the lease was never released");
}

#[tokio::test]
async fn a_handed_off_job_is_recorded() {
    let scratch = ScratchDir::new("jobs-handoff");
    let jobs = jobs(
        &scratch,
        runner(
            Arc::default(),
            None,
            LocalResult::HandedOff {
                session: "rostrum-owner-repo-4".into(),
            },
        ),
    );
    let job = LocalJob {
        clone: scratch.path().to_path_buf(),
        branch: BranchName::new("feat-4").expect("branch"),
        base: BranchName::new("main").expect("branch"),
        op: rostrum_local::LocalOp::RebaseBase,
        autostash: Autostash::Disabled,
        handler: None,
        pr: crate::convert::pr_meta(&pr(4)),
    };
    let result = jobs.run_job(key("a"), job, pr(4).key).await.expect("runs");
    assert!(matches!(result, LocalResult::HandedOff { .. }));
    let records = jobs.handoff_records().await.expect("records");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].session, "rostrum-owner-repo-4");
    assert_eq!(records[0].key, pr(4).key);
    assert_eq!(records[0].head_ref, "feat-4");
}

#[tokio::test]
async fn shutdown_refuses_new_work_and_waits_for_running_work() {
    let scratch = ScratchDir::new("jobs-shutdown");
    let jobs = jobs(
        &scratch,
        runner(Arc::default(), None, LocalResult::Completed),
    );
    let lease = jobs.acquire(key("a")).await.expect("free");
    let stopper = {
        let jobs = jobs.clone();
        tokio::spawn(async move { jobs.shutdown().await })
    };
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert!(!stopper.is_finished(), "running work holds shutdown");
    assert!(matches!(
        jobs.acquire(key("b")).await,
        Err(JobsError::Busy(Busy::ShuttingDown))
    ));
    drop(lease);
    tokio::time::timeout(Duration::from_secs(2), stopper)
        .await
        .expect("shutdown completes")
        .expect("joins");
}

#[tokio::test]
async fn shutdown_stops_a_sync_between_entries() {
    let scratch = ScratchDir::new("jobs-shutdown-sync");
    let recorder = Arc::new(Recorder::default());
    let gate = Arc::new(Semaphore::new(0));
    let jobs = jobs(
        &scratch,
        runner(recorder.clone(), Some(gate.clone()), LocalResult::Completed),
    );
    jobs.start_sync(plan(vec![
        on_clone(1, "a"),
        on_clone(2, "b"),
        on_clone(3, "c"),
    ]))
    .await
    .expect("starts");
    wait_for(&jobs, |run| run.entries[0].state == SyncEntryState::Running).await;

    let stopper = {
        let jobs = jobs.clone();
        tokio::spawn(async move { jobs.shutdown().await })
    };
    // Only let the first entry finish once the coordinator is stopping.
    loop {
        match jobs.acquire(key("z")).await {
            Err(JobsError::Busy(Busy::ShuttingDown)) => break,
            Ok(lease) => drop(lease),
            Err(other) => panic!("unexpected {other}"),
        }
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    gate.add_permits(1);
    tokio::time::timeout(Duration::from_secs(2), stopper)
        .await
        .expect("shutdown completes")
        .expect("joins");

    let run = jobs.latest_sync().await.expect("latest").expect("a run");
    assert!(run.is_finished());
    assert!(entry_done(&run, 0));
    assert_eq!(run.entries[1].state, SyncEntryState::Pending);
    assert_eq!(run.entries[2].state, SyncEntryState::Pending);
    assert_eq!(*recorder.order.lock().expect("lock"), vec![1]);
}

#[tokio::test]
async fn an_empty_sync_finishes_immediately() {
    let scratch = ScratchDir::new("jobs-empty");
    let jobs = jobs(
        &scratch,
        runner(Arc::default(), None, LocalResult::Completed),
    );
    assert_eq!(jobs.latest_sync().await.expect("latest"), None);
    jobs.start_sync(plan(Vec::new())).await.expect("starts");
    let run = wait_for(&jobs, SyncRun::is_finished).await;
    assert!(run.entries.is_empty());
    assert_eq!(run.summary().describe(true), "nothing to do");
}
