//! Profiles end to end: the registry's persistence, ordering and active
//! switching, one core per profile and full isolation between them, and
//! pairing desktops into profiles against a TLS stand-in.

mod support;

use std::{sync::Arc, time::Duration};

use rostrum_core::RepoId;
use rostrum_ffi::{ProfileRegistry, RostrumError, profiles::ProfileKind, remote::RemoteStatus};
use rostrum_remote::{
    CertFingerprint, DeviceId, DeviceToken, Endpoint, PairingCode, PairingOffer,
    api::{CloneInfo, MachineInfo},
    pairing::PairResponse,
};
use support::{Handler, Request, Scratch, assert_no_secret_on_disk, serve};

const CODE: &str = "K7QXM2PD";

fn issued_token() -> DeviceToken {
    DeviceToken::from_bytes([7; 32])
}

fn machine(name: &str) -> MachineInfo {
    MachineInfo {
        name: name.into(),
        version: "0.1.0".into(),
        api_version: 1,
        clones: vec![CloneInfo {
            repo: RepoId::new("octo", "repo"),
            path: "/code/octo/repo".into(),
        }],
        handler_configured: false,
        autostash: false,
    }
}

/// A desktop called `name`: pairs with [`CODE`], serves its machine info to
/// the issued token, and forgets the device on request.
fn desktop(name: &'static str) -> Handler {
    Arc::new(move |request: &Request| {
        match (request.method.as_str(), request.path.as_str()) {
            ("GET", "/api/v1/hello") => {
                return (200, format!(r#"{{"machine":"{name}","api_version":1}}"#));
            }
            ("POST", "/api/v1/pair") => {
                let body: serde_json::Value =
                    serde_json::from_str(&request.body).expect("pair body");
                if body["code"] != CODE {
                    return (
                        403,
                        r#"{"code":"pairing_code_invalid","message":"that code is not valid"}"#
                            .into(),
                    );
                }
                return (
                    200,
                    serde_json::to_string(&PairResponse {
                        device: DeviceId::from_bytes([1; 16]),
                        token: issued_token(),
                        machine: machine(name),
                        github: None,
                    })
                    .expect("json"),
                );
            }
            _ => {}
        }
        if request.bearer() != Some(issued_token().expose().to_ascii_lowercase().as_str()) {
            return (401, r#"{"code":"unauthorized","message":"no"}"#.into());
        }
        match (request.method.as_str(), request.path.as_str()) {
            ("GET", "/api/v1/machine") => {
                (200, serde_json::to_string(&machine(name)).expect("json"))
            }
            ("DELETE", "/api/v1/device") => (200, "null".into()),
            _ => (404, r#"{"code":"not_found","message":"no"}"#.into()),
        }
    })
}

fn link(port: u16, fingerprint: CertFingerprint, code: &str) -> String {
    PairingOffer {
        machine: "ignored".into(),
        endpoint: Endpoint::new(vec!["127.0.0.1".parse().expect("host")], port, fingerprint)
            .expect("endpoint"),
        code: PairingCode::parse(code).expect("code"),
    }
    .to_uri()
}

fn registry(scratch: &Scratch) -> Arc<ProfileRegistry> {
    ProfileRegistry::open(scratch.path()).expect("open registry")
}

fn profile_dirs(scratch: &Scratch) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(scratch.dir.join("profiles"))
        .expect("profiles dir")
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// Distinct millisecond timestamps for ordering.
async fn tick() {
    tokio::time::sleep(Duration::from_millis(3)).await;
}

fn not_found(id: &str) -> RostrumError {
    RostrumError::ProfileNotFound { id: id.into() }
}

#[tokio::test]
async fn the_registry_persists_orders_and_switches() {
    let scratch = Scratch::new("registry");
    let registry = registry(&scratch);
    assert!(registry.profiles().is_empty());
    assert_eq!(registry.active_profile(), None);

    let work = registry
        .create_token_profile(" Work ".into())
        .await
        .expect("work");
    tick().await;
    let home = registry
        .create_token_profile("Home".into())
        .await
        .expect("home");
    assert_eq!(work.label, "Work");
    assert_eq!(work.kind, ProfileKind::TokenOnly);
    assert_eq!(work.id.len(), 16);
    assert_ne!(work.id, home.id);
    // Newest first.
    let ids = |registry: &ProfileRegistry| {
        registry
            .profiles()
            .into_iter()
            .map(|profile| profile.id)
            .collect::<Vec<_>>()
    };
    assert_eq!(ids(&registry), vec![home.id.clone(), work.id.clone()]);

    tick().await;
    let active = registry
        .set_active_profile(work.id.clone())
        .expect("activate");
    assert!(active.last_used_ms > home.last_used_ms);
    assert_eq!(registry.active_profile(), Some(work.id.clone()));
    // Using a profile moves it to the front.
    assert_eq!(ids(&registry), vec![work.id.clone(), home.id.clone()]);

    let renamed = registry
        .rename_profile(home.id.clone(), "  Laptop ".into())
        .expect("rename");
    assert_eq!(renamed.label, "Laptop");
    assert!(matches!(
        registry.rename_profile(home.id.clone(), " ".into()),
        Err(RostrumError::InvalidInput { .. })
    ));
    let login = registry
        .set_profile_login(home.id.clone(), Some(" octocat ".into()))
        .expect("login");
    assert_eq!(login.github_login.as_deref(), Some("octocat"));
    assert!(matches!(
        registry.create_token_profile("  ".into()).await,
        Err(RostrumError::InvalidInput { .. })
    ));

    // A second open of the same root reads back exactly this.
    let before = registry.profiles();
    drop(registry);
    let reopened = ProfileRegistry::open(scratch.path()).expect("reopen");
    assert_eq!(reopened.profiles(), before);
    assert_eq!(reopened.active_profile(), Some(work.id.clone()));
    assert_eq!(reopened.profiles()[1].label, "Laptop");
    assert_eq!(
        reopened.profiles()[1].github_login.as_deref(),
        Some("octocat")
    );
    let cleared = reopened
        .set_profile_login(home.id.clone(), Some("  ".into()))
        .expect("clear login");
    assert_eq!(cleared.github_login, None);

    // An unknown id is reported as such, everywhere.
    let missing = "0123456789abcdef";
    assert_eq!(
        reopened.set_active_profile(missing.into()),
        Err(not_found(missing))
    );
    assert_eq!(
        reopened.rename_profile(missing.into(), "x".into()),
        Err(not_found(missing))
    );
    assert_eq!(
        reopened.set_profile_login(missing.into(), None),
        Err(not_found(missing))
    );
    assert_eq!(
        reopened.core(missing.into()).await.map(|_| ()),
        Err(not_found(missing))
    );
    assert_eq!(
        reopened.remove_profile(missing.into()).await,
        Err(not_found(missing))
    );
}

#[tokio::test]
async fn each_profile_has_one_core_and_they_are_isolated() {
    let scratch = Scratch::new("isolation");
    let registry = registry(&scratch);
    let a = registry.create_token_profile("A".into()).await.expect("a");
    let b = registry.create_token_profile("B".into()).await.expect("b");

    let core_a = registry.core(a.id.clone()).await.expect("core a");
    let again = registry.core(a.id.clone()).await.expect("core a again");
    assert!(
        Arc::ptr_eq(&core_a, &again),
        "the same id returns the same core"
    );
    let core_b = registry.core(b.id.clone()).await.expect("core b");
    assert!(!Arc::ptr_eq(&core_a, &core_b));

    core_a.add_repo("octo/only-a".into()).await.expect("add");
    core_a.set_refresh_interval(900).await.expect("interval");
    core_b
        .set_github_token(Some("ghp_b".into()))
        .await
        .expect("token");

    let settings_a = core_a.settings().await.expect("a");
    let settings_b = core_b.settings().await.expect("b");
    assert!(settings_a.repos.contains(&"octo/only-a".to_string()));
    assert!(!settings_b.repos.contains(&"octo/only-a".to_string()));
    assert_eq!(settings_a.refresh_interval_secs, 900);
    assert_ne!(settings_b.refresh_interval_secs, 900);
    assert!(matches!(
        core_a.github_status().await,
        rostrum_ffi::session::GitHubStatus::NoToken
    ));

    // Each profile's data lives in its own directory.
    assert_eq!(profile_dirs(&scratch), {
        let mut ids = vec![a.id.clone(), b.id.clone()];
        ids.sort();
        ids
    });
    drop((core_a, core_b, again));
    drop(registry);
    let reopened = ProfileRegistry::open(scratch.path()).expect("reopen");
    let settings_a = reopened
        .core(a.id.clone())
        .await
        .expect("a")
        .settings()
        .await
        .expect("a");
    let settings_b = reopened
        .core(b.id.clone())
        .await
        .expect("b")
        .settings()
        .await
        .expect("b");
    assert!(settings_a.repos.contains(&"octo/only-a".to_string()));
    assert!(!settings_b.repos.contains(&"octo/only-a".to_string()));
    // No secret reached the registry or a profile's files.
    drop(reopened);
    assert_no_secret_on_disk(&scratch.dir, "ghp_b");
}

#[tokio::test]
async fn removing_a_profile_deletes_its_data_and_unsets_active() {
    let scratch = Scratch::new("remove");
    let registry = registry(&scratch);
    let keep = registry
        .create_token_profile("Keep".into())
        .await
        .expect("keep");
    let gone = registry
        .create_token_profile("Gone".into())
        .await
        .expect("gone");
    registry
        .set_active_profile(gone.id.clone())
        .expect("activate");
    let core = registry.core(gone.id.clone()).await.expect("core");
    core.add_repo("octo/gone".into()).await.expect("add");
    drop(core);
    assert!(scratch.dir.join("profiles").join(&gone.id).exists());

    registry
        .remove_profile(gone.id.clone())
        .await
        .expect("remove");
    assert!(!scratch.dir.join("profiles").join(&gone.id).exists());
    assert_eq!(registry.active_profile(), None);
    assert_eq!(
        registry
            .profiles()
            .into_iter()
            .map(|profile| profile.id)
            .collect::<Vec<_>>(),
        vec![keep.id.clone()]
    );
    assert_eq!(
        registry.core(gone.id.clone()).await.map(|_| ()),
        Err(not_found(&gone.id))
    );
    // Removing a profile that is not active leaves the active one alone.
    registry
        .set_active_profile(keep.id.clone())
        .expect("activate");
    let other = registry
        .create_token_profile("Other".into())
        .await
        .expect("other");
    registry.remove_profile(other.id).await.expect("remove");
    assert_eq!(registry.active_profile(), Some(keep.id.clone()));

    drop(registry);
    let reopened = ProfileRegistry::open(scratch.path()).expect("reopen");
    assert_eq!(reopened.profiles().len(), 1);
    assert_eq!(profile_dirs(&scratch), Vec::<String>::new());
}

#[tokio::test]
async fn an_unregistered_profile_directory_is_cleaned_up_on_open() {
    let scratch = Scratch::new("orphans");
    let registry = registry(&scratch);
    let kept = registry
        .create_token_profile("Kept".into())
        .await
        .expect("kept");
    registry.core(kept.id.clone()).await.expect("core");
    drop(registry);
    // A crash between creating a directory and registering it.
    std::fs::create_dir_all(scratch.dir.join("profiles").join("0123456789abcdef")).expect("orphan");
    std::fs::create_dir_all(scratch.dir.join("profiles").join("not-an-id")).expect("other");

    let _reopened = ProfileRegistry::open(scratch.path()).expect("reopen");
    let mut expected = vec![kept.id, "not-an-id".to_string()];
    expected.sort();
    assert_eq!(profile_dirs(&scratch), expected);
}

#[tokio::test]
async fn pairing_creates_a_desktop_profile_and_reuses_it_for_the_same_certificate() {
    let scratch = Scratch::new("pairing");
    let registry = registry(&scratch);
    let token_profile = registry
        .create_token_profile("Token".into())
        .await
        .expect("token");
    registry
        .set_active_profile(token_profile.id.clone())
        .expect("active");
    let (port, fingerprint, log) = serve(desktop("desk-one")).await;

    let paired = registry
        .pair_desktop_with_link(link(port, fingerprint, CODE), "Pixel".into())
        .await
        .expect("pairs");
    assert!(paired.created);
    assert_eq!(paired.profile.label, "desk-one");
    assert_eq!(
        paired.profile.kind,
        ProfileKind::Desktop {
            machine: "desk-one".into(),
            fingerprint_short: fingerprint.short()
        }
    );
    assert_eq!(paired.pairing.device_token, issued_token().expose());
    // Pairing does not change the active profile.
    assert_eq!(registry.active_profile(), Some(token_profile.id.clone()));

    // The profile's core has this desktop as its remote; the other does not.
    let core = registry
        .core(paired.profile.id.clone())
        .await
        .expect("core");
    assert!(matches!(
        core.remote_status().await.expect("status"),
        RemoteStatus::Paired { port: p, .. } if p == port
    ));
    assert_eq!(core.machine_info().await.expect("machine").name, "desk-one");
    let other = registry.core(token_profile.id.clone()).await.expect("core");
    assert_eq!(
        other.remote_status().await.expect("status"),
        RemoteStatus::NotPaired
    );

    // The same desktop again, by link and by address: the same profile.
    let again = registry
        .pair_desktop_with_link(link(port, fingerprint, CODE), "Pixel".into())
        .await
        .expect("re-pairs");
    assert!(!again.created);
    assert_eq!(again.profile.id, paired.profile.id);
    registry
        .rename_profile(paired.profile.id.clone(), "My desk".into())
        .expect("rename");
    let manual = registry
        .pair_desktop_manual(
            "127.0.0.1".into(),
            port,
            fingerprint.to_base64url(),
            "k7qx-m2pd".into(),
            "Pixel".into(),
        )
        .await
        .expect("re-pairs manually");
    assert!(!manual.created);
    assert_eq!(manual.profile.id, paired.profile.id);
    // A re-pairing keeps the name the user gave it.
    assert_eq!(manual.profile.label, "My desk");
    assert_eq!(registry.profiles().len(), 2);

    // A second desktop is a second profile.
    let (port_two, fingerprint_two, _) = serve(desktop("desk-two")).await;
    let second = registry
        .pair_desktop_with_link(link(port_two, fingerprint_two, CODE), "Pixel".into())
        .await
        .expect("pairs the second");
    assert!(second.created);
    assert_ne!(second.profile.id, paired.profile.id);
    assert_eq!(registry.profiles().len(), 3);

    // Removing a paired profile unpairs it on its desktop.
    registry
        .remove_profile(paired.profile.id.clone())
        .await
        .expect("remove");
    assert!(log.last("/api/v1/device").is_some());
    assert_eq!(registry.profiles().len(), 2);

    // The device token never reached the registry file.
    drop((core, other));
    drop(registry);
    assert_no_secret_on_disk(&scratch.dir, issued_token().expose());
}

#[tokio::test]
async fn a_failed_pairing_leaves_no_profile_behind() {
    let scratch = Scratch::new("failed-pairing");
    let registry = registry(&scratch);
    let (port, fingerprint, _) = serve(desktop("desk")).await;

    let wrong_code = registry
        .pair_desktop_with_link(link(port, fingerprint, "AAAA-AAAA"), "Pixel".into())
        .await;
    assert!(matches!(wrong_code, Err(RostrumError::RemoteApi { .. })));
    let wrong_certificate = registry
        .pair_desktop_with_link(
            link(port, CertFingerprint::of_der(b"someone else"), CODE),
            "Pixel".into(),
        )
        .await;
    assert!(matches!(
        wrong_certificate,
        Err(RostrumError::CertificateMismatch { .. })
    ));
    assert!(matches!(
        registry
            .pair_desktop_with_link("not a link".into(), "Pixel".into())
            .await,
        Err(RostrumError::InvalidInput { .. })
    ));

    assert!(registry.profiles().is_empty());
    assert_eq!(profile_dirs(&scratch), Vec::<String>::new());
    drop(registry);
    assert!(
        ProfileRegistry::open(scratch.path())
            .expect("reopen")
            .profiles()
            .is_empty()
    );
}

#[tokio::test]
async fn removing_a_profile_whose_desktop_is_gone_still_succeeds() {
    let scratch = Scratch::new("remove-unreachable");
    let registry = registry(&scratch);
    let profile = registry
        .create_token_profile("Desk".into())
        .await
        .expect("profile");
    let core = registry.core(profile.id.clone()).await.expect("core");
    // A remote that nothing answers on.
    let port = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        listener.local_addr().expect("addr").port()
    };
    let endpoint = Endpoint::new(
        vec!["127.0.0.1".parse().expect("host")],
        port,
        CertFingerprint::of_der(b"x"),
    )
    .expect("endpoint");
    core.set_remote(
        serde_json::to_string(&endpoint).expect("json"),
        issued_token().expose().to_string(),
    )
    .await
    .expect("remote");
    drop(core);

    registry
        .remove_profile(profile.id.clone())
        .await
        .expect("removed despite the desktop being unreachable");
    assert!(registry.profiles().is_empty());
}

#[tokio::test]
async fn the_pair_screen_reads_links_and_probes_before_any_profile_exists() {
    let scratch = Scratch::new("pre-pairing");
    let registry = registry(&scratch);
    let (port, fingerprint, _) = serve(desktop("desk-one")).await;

    let preview = registry
        .parse_pairing_link(link(port, fingerprint, CODE))
        .expect("preview");
    assert_eq!(preview.hosts, vec!["127.0.0.1"]);
    assert_eq!(preview.port, port);
    assert_eq!(preview.code, "K7QX-M2PD");
    assert_eq!(preview.fingerprint_short, fingerprint.short());
    assert!(matches!(
        registry.parse_pairing_link("https://example.com".into()),
        Err(RostrumError::InvalidInput { .. })
    ));

    let probe = registry
        .probe_desktop("127.0.0.1".into(), port)
        .await
        .expect("probe");
    assert_eq!(probe.machine, "desk-one");
    assert!(probe.compatible);
    assert_eq!(probe.fingerprint, fingerprint.to_base64url());
    assert_eq!(probe.fingerprint_short, fingerprint.short());
    assert!(matches!(
        registry.probe_desktop("not a host!".into(), port).await,
        Err(RostrumError::InvalidInput { .. })
    ));
    assert!(matches!(
        registry.probe_desktop("127.0.0.1".into(), 0).await,
        Err(RostrumError::InvalidInput { .. })
    ));

    // Neither made a profile or wrote anything.
    assert!(registry.profiles().is_empty());
    assert_eq!(profile_dirs(&scratch), Vec::<String>::new());
    assert!(!scratch.dir.join("profiles.json").exists());
}
