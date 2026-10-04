//! Sharing settings with the paired desktop, both ways, against a TLS
//! stand-in that keeps a revision like `rostrumd`: the preview with its
//! revision and both differences, a push applied at the previewed revision,
//! a push refused because the desktop changed in between, and copying the
//! desktop's sorts, trunks and issues per repository.

mod support;

use std::sync::{Arc, Mutex};

use rostrum_core::branches::TrunkName;
use rostrum_core::{ItemSortKey as CoreItem, LoginKey, RepoId, RepoSortKey as CoreRepo, Sort};
use rostrum_ffi::{
    RostrumCore, RostrumError,
    remote::{ConfigField, ConfigPushResult},
    sort::{ItemSortKey, RepoSortKey, SortDirection},
};
use rostrum_remote::{
    CertFingerprint, ConfigConflict, ConfigPush, ConfigRevision, DeviceToken, Endpoint,
    RevisedConfig,
    api::{ApiErrorCode, DesktopConfig, MachineInfo, RepoTrunks},
};
use support::{Handler, Log, Request, Scratch, seed, serve};

const TOKEN: [u8; 32] = [5; 32];

fn token() -> DeviceToken {
    DeviceToken::from_bytes(TOKEN)
}

/// The desktop's shareable settings and how many times they changed.
struct Desk {
    config: DesktopConfig,
    revision: u32,
}

impl Desk {
    fn revised(&self) -> RevisedConfig {
        RevisedConfig {
            config: self.config.clone(),
            revision: ConfigRevision(format!("r{}", self.revision)),
        }
    }
}

fn desk() -> Desk {
    Desk {
        config: DesktopConfig {
            repos: vec![
                RepoId::new("octo", "repo"),
                RepoId::new("zed-industries", "zed"),
            ],
            prs_per_repo: 30,
            hide_drafts: true,
            hide_empty_repos: true,
            authors: vec![LoginKey::new("alice")],
            include_involved: false,
            autostash: true,
            issues_per_repo: Some(10),
            repo_sort: Some(Sort::new(CoreRepo::Stars)),
            item_sort: Some(Sort::new(CoreItem::Title)),
            trunks: Some(vec![RepoTrunks {
                repo: RepoId::new("octo", "repo"),
                trunks: vec![TrunkName::parse("develop").expect("trunk")],
            }]),
        },
        revision: 1,
    }
}

fn json<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(value).expect("serialises")
}

/// `GET` and `PUT /api/v1/config` as `rostrumd` answers them.
fn desktop(state: Arc<Mutex<Desk>>) -> Handler {
    Arc::new(move |request: &Request| {
        if request.bearer() != Some(token().expose().to_ascii_lowercase().as_str()) {
            return (401, r#"{"code":"unauthorized","message":"no"}"#.into());
        }
        let mut desk = state.lock().expect("desk");
        match (request.method.as_str(), request.path.as_str()) {
            ("GET", "/api/v1/machine") => (
                200,
                json(&MachineInfo {
                    name: "test-desk".into(),
                    version: "0.1.0".into(),
                    api_version: 1,
                    clones: vec![],
                    handler_configured: false,
                    autostash: false,
                }),
            ),
            ("GET", "/api/v1/config") => (200, json(&desk.revised())),
            ("PUT", "/api/v1/config") => {
                let push: ConfigPush = serde_json::from_str(&request.body).expect("push body");
                if push.base != Some(desk.revised().revision) {
                    return (
                        409,
                        json(&ConfigConflict {
                            code: ApiErrorCode::ConfigChanged,
                            message: "the desktop's settings changed".into(),
                            current: desk.revised(),
                        }),
                    );
                }
                let current = desk.config.clone();
                desk.config = DesktopConfig {
                    issues_per_repo: push.config.issues_per_repo.or(current.issues_per_repo),
                    repo_sort: push.config.repo_sort.or(current.repo_sort),
                    item_sort: push.config.item_sort.or(current.item_sort),
                    trunks: push.config.trunks.clone().or(current.trunks),
                    ..push.config
                };
                desk.revision += 1;
                (200, json(&desk.revised()))
            }
            _ => (404, r#"{"code":"not_found","message":"no"}"#.into()),
        }
    })
}

fn endpoint(port: u16, fingerprint: CertFingerprint) -> String {
    json(
        &Endpoint::new(vec!["127.0.0.1".parse().expect("host")], port, fingerprint)
            .expect("endpoint"),
    )
}

/// A phone watching `octo/repo` only, with settings of its own, paired with
/// the stand-in.
async fn setup(tag: &str) -> (Arc<RostrumCore>, Arc<Mutex<Desk>>, Log, Scratch) {
    let scratch = Scratch::new(tag);
    seed(&scratch.dir, &[], None).await;
    let core = RostrumCore::open(scratch.path()).await.expect("open");
    core.set_prs_per_repo(40).await.expect("prs");
    core.set_issues_per_repo(15).await.expect("issues");
    core.set_item_sort(ItemSortKey::Author, Some(SortDirection::Descending))
        .await
        .expect("sort");
    core.set_trunks("octo/repo".into(), Some(vec!["release".into()]))
        .await
        .expect("trunks");
    let state = Arc::new(Mutex::new(desk()));
    let (port, fingerprint, log) = serve(desktop(state.clone())).await;
    core.set_remote(endpoint(port, fingerprint), token().expose().to_string())
        .await
        .expect("remote");
    (core, state, log, scratch)
}

fn fields(changes: &[rostrum_ffi::remote::ConfigChange]) -> Vec<ConfigField> {
    changes.iter().map(|change| change.field).collect()
}

#[tokio::test]
async fn the_preview_carries_the_revision_and_both_differences() {
    let (core, _state, _log, _scratch) = setup("config-preview").await;
    let preview = core.desktop_config().await.expect("preview");
    assert_eq!(preview.revision, "r1");
    assert_eq!(preview.issues_per_repo, Some(10));
    // What pushing this phone's settings would change on the desktop.
    assert_eq!(
        fields(&preview.push_changes),
        vec![
            ConfigField::Repos,
            ConfigField::PrsPerRepo,
            ConfigField::IssuesPerRepo,
            ConfigField::HideDrafts,
            ConfigField::Authors,
            ConfigField::Autostash,
            ConfigField::RepoSort,
            ConfigField::ItemSort,
            ConfigField::Trunks,
        ]
    );
    let prs = &preview.push_changes[1];
    assert_eq!((prs.before.as_str(), prs.after.as_str()), ("30", "40"));
    assert_eq!(prs.label, "Pull requests per repository");
    // And what copying the desktop's would change here: the same fields, the
    // other way round.
    assert_eq!(fields(&preview.copy_changes), fields(&preview.push_changes));
    assert_eq!(
        (
            preview.copy_changes[1].before.as_str(),
            preview.copy_changes[1].after.as_str()
        ),
        ("40", "30")
    );
}

#[tokio::test]
async fn a_push_at_the_previewed_revision_is_applied() {
    let (core, state, log, _scratch) = setup("config-push").await;
    let preview = core.desktop_config().await.expect("preview");
    let result = core
        .push_config_to_desktop(preview.revision.clone())
        .await
        .expect("push");
    let ConfigPushResult::Applied { desktop } = result else {
        panic!("expected applied, got {result:?}");
    };
    assert_eq!(desktop.revision, "r2");
    assert!(
        desktop.push_changes.is_empty(),
        "{:?}",
        desktop.push_changes
    );
    assert!(!desktop.changes_anything);

    let sent: serde_json::Value =
        serde_json::from_str(&log.last("/api/v1/config").expect("put").body).expect("json");
    assert_eq!(sent["base"], "r1");
    assert_eq!(sent["prs_per_repo"], 40);
    assert_eq!(sent["issues_per_repo"], 15);
    assert_eq!(
        sent["repos"],
        serde_json::json!([{"owner": "octo", "name": "repo"}])
    );
    let desk = state.lock().expect("desk");
    assert_eq!(desk.config.repos, vec![RepoId::new("octo", "repo")]);
    assert_eq!(
        desk.config.item_sort,
        Some(Sort::with_direction(
            CoreItem::Author,
            rostrum_core::SortDirection::Descending
        ))
    );
    assert_eq!(
        desk.config.trunks,
        Some(vec![RepoTrunks {
            repo: RepoId::new("octo", "repo"),
            trunks: vec![TrunkName::parse("release").expect("trunk")],
        }])
    );
}

#[tokio::test]
async fn a_push_after_the_desktop_changed_is_refused_with_the_new_difference() {
    let (core, state, log, _scratch) = setup("config-changed").await;
    let preview = core.desktop_config().await.expect("preview");
    // Someone edits the desktop's settings while the phone shows the preview.
    {
        let mut desk = state.lock().expect("desk");
        desk.config.prs_per_repo = 40;
        desk.revision += 1;
    }
    let result = core
        .push_config_to_desktop(preview.revision.clone())
        .await
        .expect("push");
    let ConfigPushResult::Changed { desktop } = result else {
        panic!("expected changed, got {result:?}");
    };
    assert_eq!(desktop.revision, "r2");
    // Their edit already matches the phone: one change fewer to push.
    assert!(!fields(&desktop.push_changes).contains(&ConfigField::PrsPerRepo));
    assert_eq!(desktop.push_changes.len(), preview.push_changes.len() - 1);
    assert_eq!(state.lock().expect("desk").revision, 2, "nothing written");

    let retried = core
        .push_config_to_desktop(desktop.revision.clone())
        .await
        .expect("retry");
    assert!(matches!(retried, ConfigPushResult::Applied { .. }));
    assert_eq!(
        log.requests()
            .iter()
            .filter(|request| request.method == "PUT")
            .count(),
        2
    );
}

#[tokio::test]
async fn copying_takes_the_desktops_sorts_trunks_and_issue_count() {
    let (core, _state, _log, _scratch) = setup("config-copy-new").await;
    let settings = core.copy_desktop_config().await.expect("copy");
    assert_eq!(settings.issues_per_repo, 10);
    assert_eq!(settings.prs_per_repo, 30);
    let sort = core.sort_settings().await.expect("sort");
    assert_eq!(sort.repo_key, RepoSortKey::Stars);
    assert_eq!(sort.item_key, ItemSortKey::Title);
    let trunks = core.trunks("octo/repo".into()).await.expect("trunks");
    assert!(!trunks.detected);
    assert_eq!(trunks.configured, vec!["develop"]);
    let again = core.desktop_config().await.expect("preview");
    assert!(again.copy_changes.is_empty(), "{:?}", again.copy_changes);
    assert!(!again.changes_anything);
}

#[tokio::test]
async fn pushing_needs_a_paired_desktop_and_a_revision() {
    let scratch = Scratch::new("config-push-unpaired");
    seed(&scratch.dir, &[], None).await;
    let core = RostrumCore::open(scratch.path()).await.expect("open");
    assert_eq!(
        core.push_config_to_desktop("r1".into()).await,
        Err(RostrumError::NotPaired)
    );
    let (core, _state, log, _scratch) = setup("config-push-blank").await;
    assert!(matches!(
        core.push_config_to_desktop("  ".into()).await,
        Err(RostrumError::InvalidInput { .. })
    ));
    assert!(log.requests().iter().all(|request| request.method != "PUT"));
}
