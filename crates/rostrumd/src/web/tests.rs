//! The page server against a daemon on scratch directories: the page in both
//! variants, the gate, code generation, devices, and the APK download.

use axum::http::{Method, StatusCode, header};
use rostrum_remote::{ApiError, ApiErrorCode, PairingOffer};

use super::{CONTENT_SECURITY_POLICY, CodeOffer};
use crate::{
    net::NetworkView,
    registry::DeviceView,
    testkit::{Kit, Options, PAGE_HOST, fingerprint, json, page_request, send, send_full},
};

const SHA: &str = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08";

async fn page(kit: &Kit, from: &str, host: &str) -> String {
    let (status, headers, body) =
        send_full(&kit.web(), page_request(Method::GET, "/", from, host, None)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        headers[header::CONTENT_TYPE]
            .to_str()
            .expect("ascii")
            .starts_with("text/html")
    );
    String::from_utf8(body.to_vec()).expect("utf-8")
}

async fn issue(
    kit: &Kit,
    from: &str,
    host: &str,
    origin: Option<&str>,
) -> (StatusCode, axum::body::Bytes) {
    send(
        &kit.web(),
        page_request(Method::POST, "/pairing-codes", from, host, origin),
    )
    .await
}

fn publish_apk(kit: &Kit, bytes: &[u8]) {
    let dir = kit.scratch.join("apk");
    std::fs::write(dir.join("rostrum.apk"), bytes).expect("apk");
    std::fs::write(
        dir.join("rostrum.apk.json"),
        format!(
            r#"{{"version_name":"0.3.0","version_code":12,"built_at":"2026-09-28T12:00:00Z","sha256":"{SHA}","size":{}}}"#,
            bytes.len()
        ),
    )
    .expect("meta");
}

// --- the page ---------------------------------------------------------------

#[tokio::test]
async fn a_lan_visitor_gets_the_download_and_is_sent_to_the_tailnet_to_pair() {
    let kit = Kit::new("web-visitor");
    let html = page(&kit, "192.168.0.50", "192.168.0.111:8484").await;
    assert!(html.contains("No build published yet"));
    assert!(html.contains("android/scripts/publish-apk.sh"));
    assert!(html.contains("can only be generated from this computer or over the tailnet"));
    assert!(html.contains("http://test-desk.tail.example:8484/"));
    assert!(html.contains("http://100.64.0.10:8484/"));
    assert!(
        !html.contains("id=\"generate\""),
        "no pairing for a visitor"
    );
    assert!(
        !html.contains("id=\"device-list\""),
        "no device list for a visitor"
    );
    assert!(!html.contains(&fingerprint().short()));
}

#[tokio::test]
async fn a_visitor_without_a_tailnet_is_pointed_at_localhost() {
    let kit = Kit::with(
        "web-visitor-no-tailnet",
        Options {
            view: NetworkView {
                lan: vec!["192.168.0.111".parse().expect("ip")],
                tailnet: None,
            },
            ..Options::default()
        },
    );
    let html = page(&kit, "192.168.0.50", "192.168.0.111:8484").await;
    assert!(html.contains("http://localhost:8484/"));
    assert!(!html.contains("tail.example"));
}

#[tokio::test]
async fn this_computer_gets_pairing_and_the_device_list() {
    let kit = Kit::new("web-admin");
    kit.pair("Pixel 8").await;
    let html = page(&kit, "127.0.0.1", PAGE_HOST).await;
    assert!(html.contains("id=\"generate\""));
    assert!(html.contains("Generate pairing code"));
    assert!(html.contains("<h2 id=\"devices-title\">Paired devices</h2>"));
    assert!(html.contains("Pixel 8"));
    assert!(html.contains("data-revoke="));
    assert!(html.contains(&fingerprint().short()));
    assert!(!html.contains("can only be generated from this computer"));
    assert!(html.contains("id=\"no-devices\" hidden"));
}

#[tokio::test]
async fn the_tailnet_gets_pairing_too() {
    let kit = Kit::new("web-admin-tailnet");
    let html = page(&kit, "100.64.0.20", "test-desk.tail.example:8484").await;
    assert!(html.contains("id=\"generate\""));
    assert!(html.contains("No phones paired yet."));
}

#[tokio::test]
async fn the_page_is_self_contained() {
    let kit = Kit::new("web-self-contained");
    let html = page(&kit, "127.0.0.1", PAGE_HOST).await;
    assert!(html.starts_with("<!doctype html>"));
    assert!(html.contains("<style>") && html.contains("<script>"));
    assert!(
        html.contains("[hidden] { display: none !important; }"),
        "a class's display must not override the hidden attribute"
    );
    assert!(html.contains("class=\"offer\" id=\"offer\" hidden"));
    for external in ["<link", "src=\"http", "href=\"https://", "@import", "url("] {
        assert!(
            !html.contains(external),
            "the page must not load {external}"
        );
    }
    for colour in [
        "#0f1115", "#161920", "#272b36", "#e4e7ee", "#9aa2b4", "#5b9dff", "#3fb950", "#f85149",
    ] {
        assert!(html.contains(colour), "palette colour {colour}");
    }
}

#[tokio::test]
async fn a_published_apk_is_described() {
    let kit = Kit::new("web-apk-page");
    publish_apk(&kit, &[7u8; 12_400]);
    let html = page(&kit, "192.168.0.50", "192.168.0.111:8484").await;
    assert!(html.contains("Download for Android"));
    assert!(html.contains("href=\"/rostrum.apk\""));
    assert!(html.contains("download=\"rostrum-0.3.0.apk\""));
    assert!(html.contains("0.3.0"));
    assert!(html.contains("(12)"));
    assert!(html.contains("12.4 kB"));
    assert!(html.contains(SHA));
    assert!(html.contains("datetime=\"2026-09-28T12:00:00+00:00\""));
    assert!(html.contains("allow installs from this browser"));
    assert!(!html.contains("No build published yet"));
}

#[tokio::test]
async fn device_names_are_escaped() {
    let kit = Kit::new("web-escape");
    kit.pair("<script>alert('x')</script>").await;
    let html = page(&kit, "127.0.0.1", PAGE_HOST).await;
    assert!(!html.contains("<script>alert"));
    assert!(html.contains("&lt;script&gt;alert(&#39;x&#39;)&lt;/script&gt;"));
}

#[tokio::test]
async fn responses_carry_the_hardening_headers() {
    let kit = Kit::new("web-headers");
    let (_, headers, _) = send_full(
        &kit.web(),
        page_request(Method::GET, "/", "127.0.0.1", PAGE_HOST, None),
    )
    .await;
    assert_eq!(
        headers[header::CONTENT_SECURITY_POLICY],
        CONTENT_SECURITY_POLICY
    );
    assert_eq!(headers[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
    assert_eq!(headers[header::CACHE_CONTROL], "no-store");
    assert_eq!(headers[header::X_FRAME_OPTIONS], "DENY");
}

#[tokio::test]
async fn an_unknown_page_is_404() {
    let kit = Kit::new("web-404");
    let (status, body) = send(
        &kit.web(),
        page_request(Method::GET, "/nope", "127.0.0.1", PAGE_HOST, None),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(json::<ApiError>(&body).code, ApiErrorCode::NotFound);
}

// --- pairing codes ----------------------------------------------------------

#[tokio::test]
async fn a_code_comes_with_a_link_that_parses_and_a_qr_code() {
    let kit = Kit::new("web-code");
    let (status, body) = issue(&kit, "127.0.0.1", PAGE_HOST, Some("http://127.0.0.1:8484")).await;
    assert_eq!(status, StatusCode::OK);
    let offer: CodeOffer = json(&body);

    assert_eq!(offer.code.len(), 9);
    assert_eq!(&offer.code[4..5], "-");
    let parsed = PairingOffer::from_uri(&offer.uri).expect("the link parses");
    assert_eq!(parsed.code.to_string(), offer.code);
    assert_eq!(parsed.machine, "test-desk");
    assert_eq!(parsed.endpoint.port(), 8485);
    assert_eq!(parsed.endpoint.fingerprint(), fingerprint());
    let hosts: Vec<String> = parsed
        .endpoint
        .hosts()
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(
        hosts,
        vec![
            "192.168.0.111",
            "100.64.0.10",
            "fd7a:115c:a1e0::a",
            "test-desk.tail.example"
        ]
    );
    assert_eq!(offer.hosts, hosts);
    assert!(offer.qr_svg.starts_with("<svg "));
    assert_eq!(offer.fingerprint_short, fingerprint().short());
    assert_eq!(offer.fingerprint_hex, fingerprint().to_hex());
    let ttl = offer.expires_at - chrono::Utc::now();
    assert!(ttl > chrono::TimeDelta::seconds(290) && ttl <= chrono::TimeDelta::seconds(300));

    // And it pairs.
    let paired = kit
        .daemon
        .registry
        .pair(
            parsed.code,
            "phone".into(),
            None,
            "192.168.0.50".parse().expect("ip"),
        )
        .await;
    assert!(paired.is_ok());
}

#[tokio::test]
async fn codes_are_only_for_this_computer_and_the_tailnet() {
    let kit = Kit::new("web-gate");
    for from in [
        "127.0.0.1",
        "127.9.9.9",
        "::1",
        "::ffff:127.0.0.1",
        "100.64.0.20",
        "::ffff:100.100.1.1",
        "fd7a:115c:a1e0::5",
    ] {
        let (status, _) = issue(&kit, from, PAGE_HOST, None).await;
        assert_eq!(status, StatusCode::OK, "{from} may generate codes");
    }
    for from in [
        "192.168.0.50",
        "192.168.0.111",
        "10.0.0.8",
        "172.17.0.2",
        "::ffff:192.168.0.50",
        "fe80::1",
        "2001:db8::1",
        "100.128.0.1",
    ] {
        let (status, body) = issue(&kit, from, PAGE_HOST, None).await;
        assert_eq!(
            status,
            StatusCode::FORBIDDEN,
            "{from} may not generate codes"
        );
        assert_eq!(json::<ApiError>(&body).code, ApiErrorCode::Forbidden);
    }
}

#[tokio::test]
async fn a_cross_site_post_is_refused_and_a_same_origin_one_accepted() {
    let kit = Kit::new("web-origin");
    let (status, _) = issue(&kit, "127.0.0.1", PAGE_HOST, Some("http://evil.example")).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = issue(&kit, "127.0.0.1", PAGE_HOST, Some("null")).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = issue(&kit, "127.0.0.1", PAGE_HOST, Some("http://127.0.0.1:9999")).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = issue(&kit, "127.0.0.1", PAGE_HOST, Some("http://127.0.0.1:8484")).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = issue(&kit, "127.0.0.1", PAGE_HOST, None).await;
    assert_eq!(status, StatusCode::OK, "no Origin (curl) is allowed");
}

#[tokio::test]
async fn a_rebound_name_is_refused_even_from_loopback() {
    let kit = Kit::new("web-rebind");
    let (status, body) = issue(
        &kit,
        "127.0.0.1",
        "evil.example:8484",
        Some("http://evil.example:8484"),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(json::<ApiError>(&body).code, ApiErrorCode::Forbidden);
    let html = page(&kit, "127.0.0.1", "evil.example:8484").await;
    assert!(!html.contains("id=\"generate\""));

    for host in [
        "localhost:8484",
        "test-desk:8484",
        "test-desk.tail.example:8484",
        "[::1]:8484",
    ] {
        let (status, _) = issue(&kit, "127.0.0.1", host, None).await;
        assert_eq!(status, StatusCode::OK, "{host}");
    }
}

#[tokio::test]
async fn with_no_address_to_advertise_no_code_is_issued() {
    let kit = Kit::with(
        "web-no-hosts",
        Options {
            view: NetworkView::default(),
            ..Options::default()
        },
    );
    let (status, body) = issue(&kit, "127.0.0.1", PAGE_HOST, None).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(
        json::<ApiError>(&body)
            .message
            .contains("no LAN or tailnet address")
    );
}

// --- devices ----------------------------------------------------------------

#[tokio::test]
async fn devices_are_listed_without_their_hashes_for_admins_only() {
    let kit = Kit::new("web-devices");
    let paired = kit.pair("Pixel").await;
    let (status, body) = send(
        &kit.web(),
        page_request(Method::GET, "/devices", "127.0.0.1", PAGE_HOST, None),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let devices: Vec<DeviceView> = json(&body);
    assert_eq!(devices.len(), 1);
    assert_eq!(devices[0].id, paired.device);
    let text = String::from_utf8_lossy(&body);
    assert!(!text.contains("token_hash"));
    assert!(!text.contains(&paired.token.hash().to_hex()));

    let (status, _) = send(
        &kit.web(),
        page_request(
            Method::GET,
            "/devices",
            "192.168.0.50",
            "192.168.0.111:8484",
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn revoking_from_the_page() {
    let kit = Kit::new("web-revoke");
    let paired = kit.pair("Pixel").await;
    let path = format!("/devices/{}/revoke", paired.device);
    let web = kit.web();

    let (status, _) = send(
        &web,
        page_request(
            Method::POST,
            &path,
            "192.168.0.50",
            "192.168.0.111:8484",
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "not from the LAN");
    let (status, _) = send(
        &web,
        page_request(
            Method::POST,
            &path,
            "127.0.0.1",
            PAGE_HOST,
            Some("http://evil.example"),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "not cross-site");

    let (status, body) = send(
        &web,
        page_request(
            Method::POST,
            &path,
            "127.0.0.1",
            PAGE_HOST,
            Some("http://127.0.0.1:8484"),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(&body[..], b"null");
    assert!(
        kit.daemon
            .registry
            .devices()
            .await
            .expect("devices")
            .is_empty()
    );

    let (status, _) = send(
        &web,
        page_request(Method::POST, &path, "127.0.0.1", PAGE_HOST, None),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "already revoked");

    let (status, _) = send(
        &web,
        page_request(
            Method::POST,
            "/devices/xyz/revoke",
            "127.0.0.1",
            PAGE_HOST,
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

// --- the APK ----------------------------------------------------------------

#[tokio::test]
async fn the_apk_downloads_with_its_type_name_and_length() {
    let kit = Kit::new("web-apk");
    let bytes: Vec<u8> = (0..50_000u32).map(|n| (n % 251) as u8).collect();
    publish_apk(&kit, &bytes);
    let (status, headers, body) = send_full(
        &kit.web(),
        page_request(
            Method::GET,
            "/rostrum.apk",
            "192.168.0.50",
            "192.168.0.111:8484",
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        headers[header::CONTENT_TYPE],
        "application/vnd.android.package-archive"
    );
    assert_eq!(
        headers[header::CONTENT_DISPOSITION],
        "attachment; filename=\"rostrum-0.3.0.apk\""
    );
    assert_eq!(headers[header::CONTENT_LENGTH], "50000");
    assert_eq!(&body[..], &bytes[..]);
}

#[tokio::test]
async fn an_unlabelled_apk_still_downloads_under_a_plain_name() {
    let kit = Kit::new("web-apk-unlabelled");
    std::fs::write(kit.scratch.join("apk/rostrum.apk"), b"PK").expect("apk");
    let (status, headers, _) = send_full(
        &kit.web(),
        page_request(
            Method::GET,
            "/rostrum.apk",
            "192.168.0.50",
            "192.168.0.111:8484",
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        headers[header::CONTENT_DISPOSITION],
        "attachment; filename=\"rostrum.apk\""
    );
    let html = page(&kit, "192.168.0.50", "192.168.0.111:8484").await;
    assert!(html.contains("no usable description"));
}

#[tokio::test]
async fn no_apk_is_a_404() {
    let kit = Kit::new("web-apk-404");
    let (status, body) = send(
        &kit.web(),
        page_request(
            Method::GET,
            "/rostrum.apk",
            "192.168.0.50",
            "192.168.0.111:8484",
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let error: ApiError = json(&body);
    assert_eq!(error.code, ApiErrorCode::NotFound);
    assert!(error.message.contains("publish-apk.sh"));
}
