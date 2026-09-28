//! The client against a real TLS listener with a self-signed certificate:
//! pinning, host fallback, probing, and error mapping.

use std::sync::Arc;

use rostrum_remote::{
    CertFingerprint, Endpoint, Host, PairingCode,
    client::{ClientError, RemoteClient, probe},
    pairing::PairRequest,
};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
use tokio_rustls::TlsAcceptor;

/// A one-route-at-a-time HTTPS server: answers by path, forever.
async fn serve() -> (u16, CertFingerprint) {
    let certified = rcgen::generate_simple_self_signed(vec!["rostrumd".to_string()])
        .expect("self-signed certificate");
    let cert_der: CertificateDer<'static> = certified.cert.der().clone();
    let fingerprint = CertFingerprint::of_der(cert_der.as_ref());
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
        certified.signing_key.serialize_der(),
    ));
    let config = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .expect("protocol versions")
    .with_no_client_auth()
    .with_single_cert(vec![cert_der], key)
    .expect("server config");
    let acceptor = TlsAcceptor::from(Arc::new(config));

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let port = listener.local_addr().expect("addr").port();
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                let Ok(mut tls) = acceptor.accept(stream).await else {
                    return;
                };
                let mut buf = vec![0u8; 16 * 1024];
                let mut read = 0;
                loop {
                    let Ok(n) = tls.read(&mut buf[read..]).await else {
                        return;
                    };
                    if n == 0 {
                        return;
                    }
                    read += n;
                    if buf[..read].windows(4).any(|w| w == b"\r\n\r\n") {
                        break;
                    }
                }
                let head = String::from_utf8_lossy(&buf[..read]).to_string();
                let path = head.split_whitespace().nth(1).unwrap_or("/").to_string();
                let (status, body) = respond(&path, &head);
                let response = format!(
                    "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = tls.write_all(response.as_bytes()).await;
                let _ = tls.shutdown().await;
            });
        }
    });
    (port, fingerprint)
}

fn respond(path: &str, head: &str) -> (&'static str, String) {
    match path {
        "/api/v1/hello" => (
            "200 OK",
            r#"{"machine":"test-desk","api_version":1}"#.to_string(),
        ),
        "/api/v1/machine" if !head.to_ascii_lowercase().contains("authorization: bearer") => (
            "401 Unauthorized",
            r#"{"code":"unauthorized","message":"no token"}"#.to_string(),
        ),
        "/api/v1/config" => (
            "200 OK",
            r#"{"repos":[{"owner":"RhizoNymph","name":"rostrum"}],"prs_per_repo":30,"hide_drafts":false,"hide_empty_repos":true,"authors":["RhizoNymph"],"include_involved":true,"autostash":false}"#.to_string(),
        ),
        "/api/v1/pair" => (
            "410 Gone",
            r#"{"code":"pairing_code_expired","message":"that code has expired"}"#.to_string(),
        ),
        _ => (
            "404 Not Found",
            r#"{"code":"not_found","message":"no such route"}"#.to_string(),
        ),
    }
}

fn host(text: &str) -> Host {
    text.parse().expect("host")
}

#[tokio::test]
async fn a_pinned_client_talks_to_the_certificate_it_was_given() {
    let (port, fingerprint) = serve().await;
    let endpoint = Endpoint::new(vec![host("127.0.0.1")], port, fingerprint).expect("endpoint");
    let client = RemoteClient::new(endpoint, None).expect("client");
    let hello = client.hello().await.expect("hello");
    assert_eq!(hello.machine, "test-desk");
    assert_eq!(hello.api_version, 1);
}

#[tokio::test]
async fn a_different_certificate_is_refused() {
    let (port, _) = serve().await;
    let wrong = CertFingerprint::of_der(b"some other certificate");
    let endpoint = Endpoint::new(vec![host("127.0.0.1")], port, wrong).expect("endpoint");
    let client = RemoteClient::new(endpoint, None).expect("client");
    let err = client.hello().await.expect_err("must refuse");
    assert!(
        matches!(err, ClientError::CertificateMismatch { .. }),
        "got {err:?}"
    );
}

#[tokio::test]
async fn an_unreachable_address_falls_through_to_the_next() {
    let (port, fingerprint) = serve().await;
    // The listener is bound to 127.0.0.1 only, so 127.0.0.2 refuses.
    let endpoint = Endpoint::new(
        vec![host("127.0.0.2"), host("127.0.0.1")],
        port,
        fingerprint,
    )
    .expect("endpoint");
    let client = RemoteClient::new(endpoint, None).expect("client");
    client.hello().await.expect("second host answers");
    assert_eq!(client.current_host(), &host("127.0.0.1"));
}

#[tokio::test]
async fn no_reachable_address_names_every_failure() {
    let endpoint = Endpoint::new(
        vec![host("127.0.0.2"), host("127.0.0.3")],
        9,
        CertFingerprint::of_der(b"x"),
    )
    .expect("endpoint");
    let client = RemoteClient::new(endpoint, None).expect("client");
    let err = client.hello().await.expect_err("nothing listens");
    let ClientError::Unreachable { failures } = err else {
        panic!("expected unreachable, got {err:?}");
    };
    assert_eq!(failures.len(), 2);
}

#[tokio::test]
async fn a_probe_reports_the_certificate_it_saw() {
    let (port, fingerprint) = serve().await;
    let found = probe(&[host("127.0.0.2"), host("127.0.0.1")], port)
        .await
        .expect("probe");
    assert_eq!(found.fingerprint, fingerprint);
    assert_eq!(found.host, host("127.0.0.1"));
    assert_eq!(found.hello.machine, "test-desk");
}

#[tokio::test]
async fn a_401_means_the_device_is_no_longer_paired() {
    let (port, fingerprint) = serve().await;
    let endpoint = Endpoint::new(vec![host("127.0.0.1")], port, fingerprint).expect("endpoint");
    let client = RemoteClient::new(endpoint, None).expect("client");
    let err = client.machine().await.expect_err("no token");
    assert!(matches!(err, ClientError::Unauthorized), "got {err:?}");
}

#[tokio::test]
async fn api_errors_arrive_with_their_code() {
    let (port, fingerprint) = serve().await;
    let endpoint = Endpoint::new(vec![host("127.0.0.1")], port, fingerprint).expect("endpoint");
    let client = RemoteClient::new(endpoint, None).expect("client");
    let err = client
        .pair(&PairRequest {
            code: PairingCode::parse("K7QX-M2PD").expect("code"),
            device_name: "test phone".into(),
        })
        .await
        .expect_err("expired");
    let ClientError::Api(api) = err else {
        panic!("expected an API error, got {err:?}");
    };
    assert_eq!(api.code, rostrum_remote::ApiErrorCode::PairingCodeExpired);
    assert_eq!(api.message, "that code has expired");
}

#[tokio::test]
async fn the_desktop_config_arrives_typed() {
    let (port, fingerprint) = serve().await;
    let endpoint = Endpoint::new(vec![host("127.0.0.1")], port, fingerprint).expect("endpoint");
    let client = RemoteClient::new(endpoint, None).expect("client");
    let config = client.config().await.expect("config");
    assert_eq!(
        config.repos,
        vec![rostrum_core::RepoId::new("RhizoNymph", "rostrum")]
    );
    assert_eq!(config.prs_per_repo, 30);
    assert_eq!(config.authors[0].as_str(), "rhizonymph");
    assert!(config.include_involved);
}
