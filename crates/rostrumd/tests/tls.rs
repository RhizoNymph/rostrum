//! The phone's real client (`rostrum_remote::client::RemoteClient`) against
//! the daemon's real servers over TLS on loopback: the pairing link from the
//! page, pinning, pairing, authenticated calls, and unpairing.

mod common;

use common::{Harness, handover};
use rostrum_core::{LoginKey, RepoId};
use rostrum_remote::{
    API_VERSION, ApiErrorCode, CertFingerprint, Endpoint, PairRequest, PairingCode, PairingOffer,
    client::{ClientError, RemoteClient, probe},
};

#[tokio::test]
async fn a_phone_pairs_from_the_page_link_and_uses_the_api_until_it_unpairs() {
    let harness = Harness::start(
        "it-tls-pair",
        None,
        Some(serde_json::json!({
            "repos": ["RhizoNymph/rostrum", "not a repo", "rust-lang/rust"],
            "prs_per_repo": 30,
            "hide_drafts": true,
            "authors": ["Ada-Lin"],
            "autostash": true,
            "clones": {"RhizoNymph/rostrum": "/home/secret/rostrum"}
        })),
    )
    .await;

    // The page's code carries a link the phone can parse.
    let offer_json = harness.issue_code().await;
    let offer = PairingOffer::from_uri(&offer_json.uri).expect("link parses");
    assert_eq!(offer.machine, "it-desk");
    assert_eq!(offer.endpoint.fingerprint(), harness.fingerprint);
    assert_eq!(offer.endpoint.port(), harness.api.port());

    // Pinned to that fingerprint, the client reaches the API.
    let pairing = RemoteClient::new(offer.endpoint.clone(), None).expect("client");
    let hello = pairing.hello().await.expect("hello");
    assert_eq!(hello.machine, "it-desk");
    assert_eq!(hello.api_version, API_VERSION);

    let response = pairing
        .pair(&PairRequest {
            code: offer.code.clone(),
            device_name: "Integration Phone".into(),
            replaces: None,
        })
        .await
        .expect("pairs");
    assert_eq!(response.machine.name, "it-desk");
    assert_eq!(
        response.github.as_ref().map(|g| g.token.expose()),
        Some("gho_integration")
    );

    // The same code does not pair twice.
    let again = pairing
        .pair(&PairRequest {
            code: offer.code,
            device_name: "Second".into(),
            replaces: None,
        })
        .await
        .expect_err("single use");
    assert!(
        matches!(&again, ClientError::Api(error) if error.code == ApiErrorCode::PairingCodeInvalid),
        "{again:?}"
    );

    let client = RemoteClient::new(offer.endpoint, Some(response.token.clone())).expect("client");
    let machine = client.machine().await.expect("machine");
    assert_eq!(machine, response.machine);
    assert!(client.handoffs().await.expect("handoffs").is_empty());
    assert_eq!(client.sync_all().await.expect("sync-all"), None);
    assert_eq!(client.github_token().await.expect("github"), handover());
    let config = client.config().await.expect("config");
    assert_eq!(
        config.repos,
        vec![
            RepoId::new("RhizoNymph", "rostrum"),
            RepoId::new("rust-lang", "rust")
        ]
    );
    assert_eq!(config.prs_per_repo, 30);
    assert!(config.hide_drafts);
    assert_eq!(config.authors, vec![LoginKey::new("ada-lin")]);
    assert!(config.autostash);

    client.unpair().await.expect("unpair");
    let err = client.machine().await.expect_err("revoked");
    assert!(matches!(err, ClientError::Unauthorized), "{err:?}");
    let err = client.config().await.expect_err("revoked");
    assert!(matches!(err, ClientError::Unauthorized), "{err:?}");
    assert!(
        harness
            .daemon
            .registry
            .devices()
            .await
            .expect("devices")
            .is_empty()
    );

    harness.stop().await;
}

#[tokio::test]
async fn a_probe_sees_the_fingerprint_the_page_shows() {
    let harness = Harness::start("it-tls-probe", None, None).await;
    let found = probe(&["127.0.0.1".parse().expect("host")], harness.api.port())
        .await
        .expect("probe");
    assert_eq!(found.fingerprint, harness.fingerprint);
    assert_eq!(
        found.fingerprint.short(),
        harness.issue_code().await.fingerprint_short
    );
    harness.stop().await;
}

#[tokio::test]
async fn a_client_pinned_to_another_certificate_is_refused() {
    let harness = Harness::start("it-tls-mismatch", None, None).await;
    let wrong = Endpoint::new(
        vec!["127.0.0.1".parse().expect("host")],
        harness.api.port(),
        CertFingerprint::of_der(b"some other certificate"),
    )
    .expect("endpoint");
    let err = RemoteClient::new(wrong, None)
        .expect("client")
        .hello()
        .await
        .expect_err("pinned elsewhere");
    assert!(
        matches!(err, ClientError::CertificateMismatch { .. }),
        "{err:?}"
    );
    harness.stop().await;
}

#[tokio::test]
async fn a_code_nobody_issued_is_refused() {
    let harness = Harness::start("it-tls-stranger", None, None).await;
    let client = RemoteClient::new(harness.endpoint(), None).expect("client");
    let err = client
        .pair(&PairRequest {
            code: PairingCode::parse("ZZZZ-ZZZZ").expect("code"),
            device_name: "x".into(),
            replaces: None,
        })
        .await
        .expect_err("never issued");
    assert!(
        matches!(&err, ClientError::Api(error) if error.code == ApiErrorCode::PairingCodeInvalid),
        "{err:?}"
    );
    harness.stop().await;
}

#[tokio::test]
async fn the_page_refuses_a_cross_site_code_request_over_a_real_socket() {
    let harness = Harness::start("it-tls-origin", None, None).await;
    let (status, body) = common::http_request(
        harness.page,
        "POST",
        "/pairing-codes",
        &[("Origin", "http://evil.example")],
    )
    .await;
    assert_eq!(status, 403, "{body}");
    harness.stop().await;
}

#[tokio::test]
async fn a_phone_pushes_its_settings_and_a_stale_push_is_turned_back() {
    use rostrum_remote::{ConfigPush, ConfigPushOutcome, diff};

    let harness = Harness::start(
        "it-tls-push",
        None,
        Some(serde_json::json!({
            "repos": ["RhizoNymph/rostrum"],
            "refresh_secs": 45,
            "clones": {"RhizoNymph/rostrum": "/home/secret/rostrum"},
            "kept_unknown": [1, 2]
        })),
    )
    .await;
    let (client, _) = harness.pair("phone").await;

    // Preview: the phone's settings against the desktop's.
    let current = client.config_with_revision().await.expect("config");
    let mut mine = current.config.clone();
    mine.repos.push(RepoId::new("rust-lang", "rust"));
    mine.hide_drafts = true;
    let changes = diff(&current.config, &mine);
    assert_eq!(changes.len(), 2);

    let pushed = client
        .push_config(&ConfigPush {
            config: mine.clone(),
            base: Some(current.revision.clone()),
        })
        .await
        .expect("push");
    let ConfigPushOutcome::Applied(applied) = pushed else {
        panic!("expected applied, got {pushed:?}");
    };
    assert_eq!(applied.config.repos, mine.repos);
    assert!(diff(&applied.config, &mine).is_empty());
    // An older phone's plain read sees the new settings too.
    assert_eq!(client.config().await.expect("config").repos, mine.repos);

    // A second push still based on the old revision is turned back with the
    // current settings, and writes nothing.
    let before = std::fs::read(harness.scratch.join("config.json")).expect("read");
    let mut stale = mine.clone();
    stale.prs_per_repo = 7;
    let outcome = client
        .push_config(&ConfigPush {
            config: stale,
            base: Some(current.revision),
        })
        .await
        .expect("answers");
    assert_eq!(outcome, ConfigPushOutcome::Changed(applied));
    assert_eq!(
        std::fs::read(harness.scratch.join("config.json")).expect("read"),
        before
    );

    let on_disk: serde_json::Value = serde_json::from_slice(&before).expect("json");
    assert_eq!(on_disk["refresh_secs"], 45);
    assert_eq!(
        on_disk["clones"],
        serde_json::json!({"RhizoNymph/rostrum": "/home/secret/rostrum"})
    );
    assert_eq!(on_disk["kept_unknown"], serde_json::json!([1, 2]));

    // Unpaired, a push is refused.
    client.unpair().await.expect("unpair");
    let err = client
        .push_config(&ConfigPush {
            config: mine,
            base: None,
        })
        .await
        .expect_err("revoked");
    assert!(matches!(err, ClientError::Unauthorized), "{err:?}");
    harness.stop().await;
}
