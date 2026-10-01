//! The writer: every SQLite write, in the order the state changed.
//!
//! Writes are enqueued from inside the state actor, so the queue's order is
//! the order the state changed in, and one task drains it — two refreshes of
//! the same repository can never land on disk newest-first. A write whose
//! durability the caller must wait for (a draft) carries an acknowledgement.

use rostrum_core::{
    Baseline, Conversation, Issue, IssueDetail, PrNumber, PullRequest, RepoId, RepoMeta, Stack,
};
use rostrum_db::Db;
use rostrum_github::{DraftComment, PullRequestFile};
use tokio::sync::{mpsc, oneshot};

use crate::error::RostrumError;

/// One thing to write.
pub(crate) enum Write {
    PullRequests {
        repo: RepoId,
        prs: Vec<PullRequest>,
    },
    Conversation {
        repo: RepoId,
        number: PrNumber,
        conversation: Conversation,
    },
    Files {
        repo: RepoId,
        number: PrNumber,
        head_sha: String,
        files: Vec<PullRequestFile>,
    },
    /// The whole pending review of one pull request; empty clears it.
    Drafts {
        repo: RepoId,
        number: PrNumber,
        head_sha: String,
        drafts: Vec<DraftComment>,
    },
    Baseline(Baseline),
    Issues {
        repo: RepoId,
        issues: Vec<Issue>,
    },
    IssueDetail {
        repo: RepoId,
        detail: Box<IssueDetail>,
    },
    Stacks {
        repo: RepoId,
        stacks: Vec<Stack>,
    },
    RepoMeta {
        repo: RepoId,
        meta: RepoMeta,
    },
}

/// Resolves once the write it was issued for is on disk (or failed).
pub(crate) type Ack = oneshot::Receiver<Result<(), RostrumError>>;

struct Envelope {
    write: Write,
    done: Option<oneshot::Sender<Result<(), RostrumError>>>,
}

#[derive(Clone)]
pub(crate) struct Writer {
    tx: mpsc::UnboundedSender<Envelope>,
}

impl Writer {
    /// Start the writer on the current Tokio runtime. It stops when every
    /// handle is dropped, after draining what was queued.
    pub(crate) fn spawn(db: Db) -> Self {
        let (tx, mut rx) = mpsc::unbounded_channel::<Envelope>();
        tokio::spawn(async move {
            while let Some(envelope) = rx.recv().await {
                let result = perform(&db, envelope.write).await;
                if let Err(error) = &result {
                    tracing::warn!(%error, "local write failed");
                }
                if let Some(done) = envelope.done {
                    let _ = done.send(result);
                }
            }
            tracing::debug!("writer stopped");
        });
        Self { tx }
    }

    /// Queue a write whose failure is only logged: cache, which the next
    /// refresh rewrites anyway.
    pub(crate) fn send(&self, write: Write) {
        if self.tx.send(Envelope { write, done: None }).is_err() {
            tracing::warn!("writer has stopped; dropping a cache write");
        }
    }

    /// Queue a write the caller will wait for.
    pub(crate) fn send_acked(&self, write: Write) -> Ack {
        let (done, ack) = oneshot::channel();
        if let Err(mpsc::error::SendError(envelope)) = self.tx.send(Envelope {
            write,
            done: Some(done),
        }) && let Some(done) = envelope.done
        {
            let _ = done.send(Err(RostrumError::internal("the writer has stopped")));
        }
        ack
    }
}

/// Wait for an acknowledged write.
pub(crate) async fn settled(ack: Ack) -> Result<(), RostrumError> {
    ack.await
        .map_err(|_| RostrumError::internal("the writer stopped before writing"))?
}

async fn perform(db: &Db, write: Write) -> Result<(), RostrumError> {
    match write {
        Write::PullRequests { repo, prs } => db.save_pull_requests(&repo, &prs).await,
        Write::Conversation {
            repo,
            number,
            conversation,
        } => db.save_conversation(&repo, number, &conversation).await,
        Write::Files {
            repo,
            number,
            head_sha,
            files,
        } => {
            db.save_pull_request_files(&repo, number, &head_sha, &files)
                .await
        }
        Write::Drafts {
            repo,
            number,
            head_sha,
            drafts,
        } => {
            if drafts.is_empty() {
                db.clear_drafts(&repo, number).await
            } else {
                db.save_drafts(&repo, number, &head_sha, &drafts).await
            }
        }
        Write::Baseline(baseline) => db.save_baseline(&baseline).await,
        Write::Issues { repo, issues } => db.save_issues(&repo, &issues).await,
        Write::IssueDetail { repo, detail } => db.save_issue_detail(&repo, &detail).await,
        Write::Stacks { repo, stacks } => db.save_stacks(&repo, &stacks).await,
        Write::RepoMeta { repo, meta } => db.save_repo_meta(&repo, &meta).await,
    }
    .map_err(RostrumError::from)
}
