//! `sync-all`: start a run across many pull requests, and poll it.

use std::{collections::HashMap, path::PathBuf};

use axum::{Json, extract::State};
use rostrum_remote::{SyncAllRequest, SyncRun};

use super::auth::AuthedDevice;
use crate::{
    convert,
    daemon::Daemon,
    http::{ApiFailure, ApiJson},
    jobs::{CloneKey, EntryTarget, PlannedEntry, SyncPlan},
};

/// Resolve every pull request against the config as it is now, then hand the
/// plan to the coordinator. Answers at once with the run as it starts.
pub async fn start(
    State(daemon): State<Daemon>,
    _device: AuthedDevice,
    ApiJson(request): ApiJson<SyncAllRequest>,
) -> Result<Json<SyncRun>, ApiFailure> {
    let config = daemon.rostrum_config.load();
    let mut keys: HashMap<PathBuf, CloneKey> = HashMap::new();
    let mut entries = Vec::with_capacity(request.prs.len());
    for pr in request.prs {
        let target = match config.local_path(&pr.key.repo) {
            None => EntryTarget::NotConfigured,
            Some(path) => match (convert::branch(&pr.head_ref), convert::branch(&pr.base_ref)) {
                (Ok(head), Ok(base)) => {
                    let key = match keys.get(&path) {
                        Some(key) => key.clone(),
                        None => {
                            let key = CloneKey::resolve(&path).await;
                            keys.insert(path.clone(), key.clone());
                            key
                        }
                    };
                    EntryTarget::Clone {
                        key,
                        path,
                        head,
                        base,
                    }
                }
                (Err(reason), _) | (_, Err(reason)) => EntryTarget::Invalid { reason },
            },
        };
        entries.push(PlannedEntry { pr, target });
    }
    let plan = SyncPlan {
        op: request.op,
        autostash: convert::autostash(request.autostash),
        handler: config.conflict_handler.clone(),
        entries,
    };
    Ok(Json(daemon.jobs.start_sync(plan).await?))
}

pub async fn latest(
    State(daemon): State<Daemon>,
    _device: AuthedDevice,
) -> Result<Json<Option<SyncRun>>, ApiFailure> {
    Ok(Json(daemon.jobs.latest_sync().await?))
}
