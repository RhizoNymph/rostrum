//! The daemon's record of the conflicts it handed to a tmux session.
//!
//! tmux knows a session's name and when it started; only the daemon knows
//! which pull request and worktree it was for. That association is kept here,
//! keyed by session name (one per pull request, so the record stays as small
//! as the set of pull requests), and persisted in `<state_dir>/handoffs.json`
//! so a restart does not forget it.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use rostrum_git::{BranchName, Repo};
use rostrum_remote::PrKey;
use serde::{Deserialize, Serialize};

use crate::state_file::{StoreError, read_json, write_json};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandoffRecord {
    pub session: String,
    pub key: PrKey,
    pub head_ref: String,
    /// Where the stopped operation is, when it could be found.
    pub worktree: Option<String>,
    pub started_at: DateTime<Utc>,
}

impl HandoffRecord {
    /// Describe a handoff that just happened, finding the worktree `branch` is
    /// checked out in under `clone`.
    pub async fn describe(
        session: String,
        key: PrKey,
        head_ref: String,
        clone: &Path,
        branch: &BranchName,
    ) -> Self {
        let worktree = match Repo::open(clone).await {
            Ok(repo) => match repo.worktree_for(branch).await {
                Ok(found) => found.map(|repo| repo.root().display().to_string()),
                Err(error) => {
                    tracing::debug!(%error, "could not find the handed-off worktree");
                    None
                }
            },
            Err(error) => {
                tracing::debug!(%error, "could not open the clone of a handed-off pull request");
                None
            }
        };
        Self {
            session,
            key,
            head_ref,
            worktree,
            started_at: Utc::now(),
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct HandoffsFile {
    handoffs: Vec<HandoffRecord>,
}

#[derive(Debug)]
pub struct HandoffBook {
    path: PathBuf,
    records: Vec<HandoffRecord>,
}

impl HandoffBook {
    pub fn load(path: PathBuf) -> Result<Self, StoreError> {
        let records = read_json::<HandoffsFile>(&path)?
            .map(|file| file.handoffs)
            .unwrap_or_default();
        Ok(Self { path, records })
    }

    pub fn records(&self) -> &[HandoffRecord] {
        &self.records
    }

    /// Record a handoff, replacing any earlier one for the same session.
    pub fn upsert(&mut self, record: HandoffRecord) -> Result<(), StoreError> {
        let mut next: Vec<HandoffRecord> = self
            .records
            .iter()
            .filter(|existing| existing.session != record.session)
            .cloned()
            .collect();
        next.push(record);
        write_json(
            &self.path,
            &HandoffsFile {
                handoffs: next.clone(),
            },
        )?;
        self.records = next;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use rostrum_core::{PrNumber, RepoId};

    use super::*;
    use crate::fsutil::ScratchDir;

    fn record(session: &str, worktree: &str) -> HandoffRecord {
        HandoffRecord {
            session: session.into(),
            key: PrKey {
                repo: RepoId::new("o", "r"),
                number: PrNumber(1),
            },
            head_ref: "feat".into(),
            worktree: Some(worktree.into()),
            started_at: Utc::now(),
        }
    }

    #[test]
    fn records_persist_and_a_session_is_recorded_once() {
        let scratch = ScratchDir::new("handoffs");
        let path = scratch.join("handoffs.json");
        let mut book = HandoffBook::load(path.clone()).expect("empty");
        book.upsert(record("rostrum-o-r-1", "/a")).expect("upsert");
        book.upsert(record("rostrum-o-r-2", "/b")).expect("upsert");
        book.upsert(record("rostrum-o-r-1", "/c")).expect("replace");

        let reloaded = HandoffBook::load(path).expect("load");
        assert_eq!(reloaded.records().len(), 2);
        let first = reloaded
            .records()
            .iter()
            .find(|r| r.session == "rostrum-o-r-1")
            .expect("present");
        assert_eq!(first.worktree.as_deref(), Some("/c"));
    }

    #[tokio::test]
    async fn describing_a_handoff_outside_a_repository_has_no_worktree() {
        let scratch = ScratchDir::new("handoffs-describe");
        let record = HandoffRecord::describe(
            "rostrum-o-r-1".into(),
            PrKey {
                repo: RepoId::new("o", "r"),
                number: PrNumber(1),
            },
            "feat".into(),
            scratch.path(),
            &BranchName::new("feat").expect("branch"),
        )
        .await;
        assert_eq!(record.worktree, None);
        assert_eq!(record.head_ref, "feat");
    }
}
