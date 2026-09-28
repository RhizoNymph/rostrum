//! The phone's real client (`rostrum_remote::client::RemoteClient`) against
//! the daemon's real servers over TLS on loopback: the pairing link from the
//! page, pinning, pairing, authenticated calls, and unpairing.

mod common;

use common::{Harness, handover};
use rostrum_remote::{
    API_VERSION, ApiErrorCode, CertFingerprint, Endpoint, PairRequest, PairingCode, PairingOffer,
    client::{ClientError, RemoteClient, probe},
};

#[tokio::test]
async fn a_phone_pairs_from_the_page_link_and_uses_the_api_until_it_unpairs() {
    let harness = Harness::start("it-tls-pair", None, None).await;

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

    client.unpair().await.expect("unpair");
    let err = client.machine().await.expect_err("revoked");
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
