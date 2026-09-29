//! The page's markup, from a [`PageModel`]. Pure: the same model is always
//! the same page, so every variant is a string in a test.

use std::fmt::Write;

use rostrum_remote::CertFingerprint;

use super::format::{escape, group_hex, human_size, utc};
use crate::{
    registry::DeviceView,
    web::{
        apk::{ApkListing, PUBLISH_COMMAND},
        gate::Refusal,
    },
};

const CSS: &str = include_str!("page.css");
const JS: &str = include_str!("page.js");

const DOWNLOAD_ICON: &str = r#"<svg aria-hidden="true" width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 3v12"/><path d="m7 10 5 5 5-5"/><path d="M5 21h14"/></svg>"#;

pub struct PageModel<'a> {
    pub machine: &'a str,
    pub version: &'a str,
    pub apk: &'a ApkListing,
    pub pairing: &'a PairingPanel,
}

/// The pairing half of the page: everything for a request that may
/// administer, an explanation for one that may not.
pub enum PairingPanel {
    Admin {
        devices: Vec<DeviceView>,
        fingerprint: CertFingerprint,
    },
    Visitor {
        refusal: Refusal,
        tailnet_urls: Vec<String>,
        http_port: u16,
    },
}

pub fn render(model: &PageModel) -> String {
    let machine = escape(model.machine);
    let mut html = String::with_capacity(24 * 1024);
    let _ = write!(
        html,
        r##"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover">
<meta name="color-scheme" content="dark">
<meta name="theme-color" content="#0f1115">
<meta name="referrer" content="no-referrer">
<title>Rostrum · {machine}</title>
<style>{CSS}</style>
</head>
<body>
<main class="wrap">
<header class="top">
<div class="logo" aria-hidden="true">R</div>
<div><h1>Rostrum</h1><p class="muted small">on <strong>{machine}</strong></p></div>
</header>
"##
    );
    download_card(&mut html, model.apk);
    match model.pairing {
        PairingPanel::Admin {
            devices,
            fingerprint,
        } => {
            pair_card(&mut html, fingerprint);
            devices_card(&mut html, devices);
        }
        PairingPanel::Visitor {
            refusal,
            tailnet_urls,
            http_port,
        } => visitor_card(&mut html, *refusal, tailnet_urls, *http_port),
    }
    let _ = write!(
        html,
        r#"<footer class="muted small">rostrumd {version} · <span class="mono">{machine}</span></footer>
</main>
<script>{JS}</script>
</body>
</html>
"#,
        version = escape(model.version),
    );
    html
}

fn download_card(html: &mut String, apk: &ApkListing) {
    html.push_str(
        r#"<section class="card" aria-labelledby="download-title">
<h2 id="download-title">Get the Android app</h2>
"#,
    );
    let button = |html: &mut String| {
        let _ = writeln!(
            html,
            r#"<a class="btn btn-primary btn-big" href="/rostrum.apk" download="{name}">{DOWNLOAD_ICON}<span>Download for Android</span></a>"#,
            name = escape(&apk.download_name()),
        );
    };
    const HINT: &str = r#"<p class="hint">The first time, Android asks to allow installs from this browser — allow it, then tap Install.</p>
"#;
    match apk {
        ApkListing::Published { meta, .. } => {
            button(html);
            let _ = write!(
                html,
                r#"<dl class="meta">
<div><dt>Version</dt><dd>{name} <span class="muted">({code})</span></dd></div>
<div><dt>Size</dt><dd>{size}</dd></div>
<div><dt>Built</dt><dd><time datetime="{built}" data-rel>{built_utc}</time></dd></div>
<div class="full"><dt>SHA-256</dt><dd class="mono break small" id="apk-sha256">{sha}</dd></div>
</dl>
{HINT}"#,
                name = escape(&meta.version_name),
                code = meta.version_code,
                size = human_size(meta.size),
                built = meta.built_at.to_rfc3339(),
                built_utc = utc(&meta.built_at),
                sha = escape(&meta.sha256.to_string()),
            );
        }
        ApkListing::Unlabelled { size, problem, .. } => {
            button(html);
            let _ = write!(
                html,
                r#"<dl class="meta"><div><dt>Size</dt><dd>{size}</dd></div></dl>
<p class="warn small">This build has no usable description ({problem}). Re-run <code>{PUBLISH_COMMAND}</code> to publish it properly.</p>
{HINT}"#,
                size = human_size(*size),
                problem = escape(problem),
            );
        }
        ApkListing::Missing => {
            let _ = write!(
                html,
                r#"<div class="empty">
<p><strong>No build published yet.</strong></p>
<p class="muted small">Build and publish one from the rostrum repository with <code>{PUBLISH_COMMAND}</code>, then reload this page.</p>
</div>
"#
            );
        }
    }
    html.push_str("</section>\n");
}

fn pair_card(html: &mut String, fingerprint: &CertFingerprint) {
    let _ = write!(
        html,
        r##"<section class="card" aria-labelledby="pair-title">
<h2 id="pair-title">Pair a phone</h2>
<p class="muted small">Generate a one-time code, then scan the QR code with the phone's camera — or open this page on the phone and tap <em>Open in Rostrum</em>.</p>
<button class="btn btn-primary" id="generate" type="button">Generate pairing code</button>
<p class="error small" id="generate-error" role="alert" hidden></p>
<p class="success" id="paired" role="status" hidden></p>
<div class="offer" id="offer" hidden>
<div class="code" id="code" aria-live="polite"></div>
<div class="expiry" id="expiry" role="timer"></div>
<div class="qr" id="qr"></div>
<a class="btn" id="open" href="#">Open in Rostrum</a>
<details class="hosts"><summary class="small">Addresses in this code</summary><ul class="plain" id="hosts"></ul></details>
</div>
<div class="fp">
<div class="fp-row"><span class="muted small">Certificate fingerprint</span><span class="mono fp-short" id="fp-short">{short}</span></div>
<details><summary class="small">Full SHA-256</summary><p class="mono break small" id="fp-hex">{full}</p></details>
<p class="muted small">Typing a code by hand instead? Check the phone shows this same fingerprint before it pairs.</p>
</div>
</section>
"##,
        short = escape(&fingerprint.short()),
        full = group_hex(&fingerprint.to_hex()),
    );
}

fn devices_card(html: &mut String, devices: &[DeviceView]) {
    html.push_str(
        r#"<section class="card" aria-labelledby="devices-title">
<h2 id="devices-title">Paired devices</h2>
<ul class="devices" id="device-list">
"#,
    );
    for device in devices {
        let _ = writeln!(
            html,
            r#"<li class="device"><div class="device-main"><div class="device-name">{name}</div><div class="muted small">Paired <time datetime="{paired}" data-rel>{paired_utc}</time> · last seen <time datetime="{seen}" data-rel>{seen_utc}</time> · <span class="mono">{ip}</span></div></div><button type="button" class="btn btn-danger btn-small" data-revoke="{id}" data-name="{name}">Revoke</button></li>"#,
            name = escape(&device.name),
            paired = device.paired_at.to_rfc3339(),
            paired_utc = utc(&device.paired_at),
            seen = device.last_seen.to_rfc3339(),
            seen_utc = utc(&device.last_seen),
            ip = escape(&device.last_ip.to_string()),
            id = escape(device.id.as_str()),
        );
    }
    let _ = write!(
        html,
        r#"</ul>
<p class="muted small" id="no-devices"{hidden}>No phones paired yet.</p>
</section>
"#,
        hidden = if devices.is_empty() { "" } else { " hidden" },
    );
}

fn visitor_card(html: &mut String, refusal: Refusal, tailnet_urls: &[String], http_port: u16) {
    html.push_str(
        r#"<section class="card" aria-labelledby="pair-title">
<h2 id="pair-title">Pair a phone</h2>
<p>Pairing codes can only be generated from this computer or over the tailnet.</p>
"#,
    );
    if refusal != Refusal::NotLocal {
        let _ = writeln!(
            html,
            r#"<p class="warn small">{}.</p>"#,
            escape(&refusal.to_string())
        );
    }
    if tailnet_urls.is_empty() {
        let _ = writeln!(
            html,
            r#"<p class="muted small">This computer is not on a tailnet right now, so pair from the computer itself at <code>http://localhost:{http_port}/</code>.</p>"#
        );
    } else {
        html.push_str(
            r#"<p class="muted small">Open this page over the tailnet to pair:</p>
<ul class="plain links">
"#,
        );
        for url in tailnet_urls {
            let url = escape(url);
            let _ = writeln!(
                html,
                r#"<li><a class="mono break" href="{url}">{url}</a></li>"#
            );
        }
        html.push_str("</ul>\n");
    }
    html.push_str("</section>\n");
}
