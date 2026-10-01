//! Pair with a *running* rostrumd the way a phone does, use the API, unpair,
//! and check the daemon's journal for the secrets that crossed the wire.
//!
//! ```sh
//! cargo run -p rostrumd --example pair_live            # page on 127.0.0.1:8484
//! cargo run -p rostrumd --example pair_live -- 127.0.0.1:9000
//! ```
//!
//! Prints no secret: not the pairing code, not the device token, not the
//! GitHub token. The device it pairs is unpaired before it exits.

use std::{net::SocketAddr, process::ExitCode};

use rostrum_remote::{
    PairRequest, PairingOffer,
    client::{ClientError, RemoteClient},
};
use rostrumd::web::CodeOffer;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::main]
async fn main() -> ExitCode {
    let page: SocketAddr = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:8484".into())
        .parse()
        .expect("a socket address for the page server");
    match run(page).await {
        Ok(()) => {
            println!("\nall checks passed");
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("\nFAILED: {message}");
            ExitCode::FAILURE
        }
    }
}

async fn run(page: SocketAddr) -> Result<(), String> {
    // 1. The page issues a code, as its button does.
    let (status, body) = http(page, "POST", "/pairing-codes").await?;
    if status != 200 {
        return Err(format!("POST /pairing-codes answered {status}: {body}"));
    }
    let offer: CodeOffer = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    let parsed = PairingOffer::from_uri(&offer.uri).map_err(|e| format!("link: {e}"))?;
    println!("code issued; link parses:");
    println!("  machine      {}", parsed.machine);
    println!(
        "  hosts        {}",
        parsed
            .endpoint
            .hosts()
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    );
    println!("  port         {}", parsed.endpoint.port());
    println!("  fingerprint  {}", parsed.endpoint.fingerprint().short());
    println!("  expires at   {}", offer.expires_at);
    println!("  qr svg       {} bytes", offer.qr_svg.len());

    // 2. The phone pairs over the pinned connection.
    let pairing = RemoteClient::new(parsed.endpoint.clone(), None).map_err(|e| e.to_string())?;
    let hello = pairing.hello().await.map_err(|e| format!("hello: {e}"))?;
    println!("\nhello via {}: {hello:?}", pairing.current_host());
    let paired = pairing
        .pair(&PairRequest {
            code: parsed.code.clone(),
            device_name: "rostrumd live check".into(),
            replaces: None,
        })
        .await
        .map_err(|e| format!("pair: {e}"))?;
    println!(
        "paired as device {} on {} (api {}, {} clones, handler configured: {})",
        paired.device,
        paired.machine.name,
        paired.machine.api_version,
        paired.machine.clones.len(),
        paired.machine.handler_configured
    );
    match &paired.github {
        Some(github) => println!(
            "github token handed over ({} from {})",
            github.host, github.source
        ),
        None => println!("no github token on the desktop"),
    }

    // 3. Authenticated calls.
    let client = RemoteClient::new(parsed.endpoint.clone(), Some(paired.token.clone()))
        .map_err(|e| e.to_string())?;
    let machine = client
        .machine()
        .await
        .map_err(|e| format!("machine: {e}"))?;
    println!(
        "\nGET /machine: {} rostrumd {}",
        machine.name, machine.version
    );
    let handoffs = client
        .handoffs()
        .await
        .map_err(|e| format!("handoffs: {e}"))?;
    println!(
        "GET /handoffs: {} session(s) {:?}",
        handoffs.len(),
        handoffs
            .iter()
            .map(|s| s.session.as_str())
            .collect::<Vec<_>>()
    );
    let latest = client
        .sync_all()
        .await
        .map_err(|e| format!("sync-all: {e}"))?;
    println!(
        "GET /sync-all: {}",
        if latest.is_some() { "a run" } else { "null" }
    );
    let github = match client.github_token().await {
        Ok(handover) => Some(handover),
        Err(ClientError::Api(error)) => {
            println!("GET /github-token: {:?} ({})", error.code, error.message);
            None
        }
        Err(other) => return Err(format!("github-token: {other}")),
    };
    if let Some(handover) = &github {
        println!(
            "GET /github-token: {} from {}",
            handover.host, handover.source
        );
    }
    let (status, devices) = http(page, "GET", "/devices").await?;
    if status != 200 || !devices.contains(paired.device.as_str()) {
        return Err(format!("the page does not list the new device ({status})"));
    }
    println!("the page lists the device");

    // 4. Unpair, and the token stops working.
    client.unpair().await.map_err(|e| format!("unpair: {e}"))?;
    match client.machine().await {
        Err(ClientError::Unauthorized) => println!("\nunpaired; the token is refused (401)"),
        other => return Err(format!("after unpairing, /machine gave {other:?}")),
    }
    let (_, devices) = http(page, "GET", "/devices").await?;
    if devices.contains(paired.device.as_str()) {
        return Err("the device is still listed after unpairing".into());
    }
    println!("the page no longer lists it");

    // 5. Nothing secret in the journal.
    let journal = std::process::Command::new("journalctl")
        .args([
            "--user",
            "-u",
            "rostrumd",
            "-n",
            "500",
            "--no-pager",
            "-o",
            "cat",
        ])
        .output()
        .map_err(|e| format!("journalctl: {e}"))?;
    let journal = String::from_utf8_lossy(&journal.stdout);
    let mut secrets = vec![
        ("pairing code", parsed.code.as_str().to_string()),
        ("pairing code (grouped)", parsed.code.to_string()),
        ("device token", paired.token.expose().to_string()),
        ("token hash", paired.token.hash().to_hex()),
    ];
    if let Some(github) = paired.github.as_ref().or(github.as_ref()) {
        secrets.push(("github token", github.token.expose().to_string()));
    }
    for (what, secret) in &secrets {
        if journal.contains(secret.as_str()) {
            return Err(format!("the journal contains the {what}"));
        }
    }
    println!(
        "journal checked ({} lines): no {} found",
        journal.lines().count(),
        secrets
            .iter()
            .map(|(what, _)| *what)
            .collect::<Vec<_>>()
            .join(", ")
    );
    Ok(())
}

/// A minimal HTTP/1.1 exchange with the page server, as a same-origin page
/// script would make it.
async fn http(addr: SocketAddr, method: &str, path: &str) -> Result<(u16, String), String> {
    let mut stream = tokio::net::TcpStream::connect(addr)
        .await
        .map_err(|e| format!("connect {addr}: {e}"))?;
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nOrigin: http://{addr}\r\nConnection: close\r\nContent-Length: 0\r\n\r\n"
    );
    stream
        .write_all(request.as_bytes())
        .await
        .map_err(|e| e.to_string())?;
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .await
        .map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&response).to_string();
    let (head, body) = text.split_once("\r\n\r\n").ok_or("no HTTP response")?;
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .ok_or("no status")?;
    Ok((status, body.to_string()))
}
