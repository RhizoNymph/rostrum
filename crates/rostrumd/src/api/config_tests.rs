//! `PUT /api/v1/config` through the router: auth, validation, the stale-base
//! 409, and what changes in the file.

use axum::http::{Method, StatusCode};
use rostrum_config::Config;
use rostrum_core::{LoginKey, RepoId};
use rostrum_remote::{
    ApiError, ApiErrorCode, ConfigConflict, ConfigPush, DesktopConfig, DeviceToken, RevisedConfig,
    routes,
};
use serde_json::json;

use crate::testkit::{Kit, api_request, json, send};

fn seed(kit: &Kit) {
    kit.write_rostrum_config(&json!({
        "repos": ["old/one"],
        "prs_per_repo": 25,
        "refresh_secs": 45,
        "notifications": true,
        "clones": {"old/one": "/home/secret/one"},
        "conflict_handler": {"command": "claude {context}"},
        "kept_unknown": {"x": 1},
    }));
}

fn proposed() -> DesktopConfig {
    DesktopConfig {
        repos: vec![RepoId::new("phone", "one"), RepoId::new("phone", "two")],
        prs_per_repo: 50,
        hide_drafts: true,
        hide_empty_repos: false,
        authors: vec![LoginKey::new("ada")],
        include_involved: true,
        autostash: false,
        issues_per_repo: None,
        repo_sort: None,
        item_sort: None,
        trunks: None,
    }
}

async fn get_config(kit: &Kit, token: &DeviceToken) -> RevisedConfig {
    let (status, body) = send(
        &kit.api(),
        api_request(
            Method::GET,
            routes::CONFIG,
            "100.64.0.20",
            Some(token),
            None::<&()>,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    json(&body)
}

async fn put(
    kit: &Kit,
    token: Option<&DeviceToken>,
    push: &ConfigPush,
) -> (StatusCode, axum::body::Bytes) {
    send(
        &kit.api(),
        api_request(
            Method::PUT,
            routes::CONFIG,
            "100.64.0.20",
            token,
            Some(push),
        ),
    )
    .await
}

fn on_disk(kit: &Kit) -> serde_json::Value {
    serde_json::from_slice(&std::fs::read(kit.scratch.join("config.json")).expect("read"))
        .expect("json")
}

#[tokio::test]
async fn a_push_without_a_token_is_401_and_writes_nothing() {
    let kit = Kit::new("config-push-401");
    seed(&kit);
    let before = on_disk(&kit);
    for token in [None, Some(&DeviceToken::from_bytes([3; 32]))] {
        let (status, body) = put(
            &kit,
            token,
            &ConfigPush {
                config: proposed(),
                base: None,
            },
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(json::<ApiError>(&body).code, ApiErrorCode::Unauthorized);
    }
    assert_eq!(on_disk(&kit), before);
}

#[tokio::test]
async fn get_returns_a_revision_that_a_push_can_build_on() {
    let kit = Kit::new("config-push-apply");
    seed(&kit);
    let token = kit.pair("Pixel").await.token;
    let current = get_config(&kit, &token).await;
    assert_eq!(current.config.repos, vec![RepoId::new("old", "one")]);

    let (status, body) = put(
        &kit,
        Some(&token),
        &ConfigPush {
            config: proposed(),
            base: Some(current.revision.clone()),
        },
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let applied: RevisedConfig = json(&body);
    assert_ne!(applied.revision, current.revision);
    assert_eq!(applied.config.repos, proposed().repos);
    assert_eq!(
        get_config(&kit, &token).await,
        applied,
        "GET reads what was written"
    );

    // Only the shareable keys changed; the machine's and unknown ones survive.
    let after = on_disk(&kit);
    assert_eq!(after["repos"], json!(["phone/one", "phone/two"]));
    assert_eq!(after["prs_per_repo"], 50);
    assert_eq!(after["refresh_secs"], 45);
    assert_eq!(after["notifications"], true);
    assert_eq!(after["clones"], json!({"old/one": "/home/secret/one"}));
    assert_eq!(
        after["conflict_handler"],
        json!({"command": "claude {context}"})
    );
    assert_eq!(after["kept_unknown"], json!({"x": 1}));
}

#[tokio::test]
async fn a_stale_base_is_409_with_the_current_settings_and_writes_nothing() {
    let kit = Kit::new("config-push-stale");
    seed(&kit);
    let token = kit.pair("Pixel").await.token;
    let seen = get_config(&kit, &token).await;

    // The desktop's user changes something after the phone previewed.
    let mut changed: Config = serde_json::from_value(on_disk(&kit)).expect("config");
    changed.hide_drafts = true;
    let mut document = on_disk(&kit);
    rostrum_config::overlay_shared(&mut document, &changed.shared());
    rostrum_config::document::write_atomic(&kit.scratch.join("config.json"), &document)
        .expect("desktop write");
    let before = on_disk(&kit);

    let (status, body) = put(
        &kit,
        Some(&token),
        &ConfigPush {
            config: proposed(),
            base: Some(seen.revision.clone()),
        },
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    let conflict: ConfigConflict = json(&body);
    assert_eq!(conflict.code, ApiErrorCode::ConfigChanged);
    assert!(
        conflict.current.config.hide_drafts,
        "the desktop's change is reported"
    );
    assert_ne!(conflict.current.revision, seen.revision);
    assert_eq!(on_disk(&kit), before, "nothing written");

    // Pushing on the reported revision goes through.
    let (status, _) = put(
        &kit,
        Some(&token),
        &ConfigPush {
            config: proposed(),
            base: Some(conflict.current.revision),
        },
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn invalid_settings_are_400_and_write_nothing() {
    let kit = Kit::new("config-push-invalid");
    seed(&kit);
    let token = kit.pair("Pixel").await.token;
    let before = on_disk(&kit);

    let mut zero = proposed();
    zero.prs_per_repo = 0;
    let mut login = proposed();
    login.authors = vec![LoginKey::new("not a login")];
    let mut dup = proposed();
    dup.repos.push(RepoId::new("phone", "one"));
    for config in [zero, login, dup] {
        let (status, body) = put(&kit, Some(&token), &ConfigPush { config, base: None }).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(json::<ApiError>(&body).code, ApiErrorCode::BadRequest);
    }
    // A body that is not a DesktopConfig at all.
    let (status, _) = send(
        &kit.api(),
        api_request(
            Method::PUT,
            routes::CONFIG,
            "100.64.0.20",
            Some(&token),
            Some(&json!({"repos": "nope"})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(on_disk(&kit), before);
}
