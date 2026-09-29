//! `GET /`: one self-contained page — inline CSS and JS, no external
//! requests — in rostrum's dark palette, mobile first.
//!
//! Rendered on the server from a [`PageModel`], so the download card, the
//! device list and the gated-or-not pairing panel are all there before any
//! script runs; the script only adds code generation, the countdown, relative
//! times and revoking.

mod format;
mod render;

use axum::{
    extract::State,
    response::{Html, IntoResponse, Response},
};

pub use format::{escape, group_hex, human_size};
pub use render::{PageModel, PairingPanel, render};

use super::{apk, gate::Access};
use crate::{daemon::Daemon, http::ApiFailure};

pub async fn index(State(daemon): State<Daemon>, access: Access) -> Response {
    let apk = apk::inspect(&daemon.apk_dir);
    let pairing = match access {
        Access::Admin => match daemon.registry.devices().await {
            Ok(devices) => PairingPanel::Admin {
                devices,
                fingerprint: daemon.fingerprint,
            },
            Err(error) => {
                return ApiFailure::internal("could not list devices", &error).into_response();
            }
        },
        Access::Visitor(refusal) => PairingPanel::Visitor {
            refusal,
            tailnet_urls: daemon.network_view().tailnet_page_urls(daemon.http_port),
            http_port: daemon.http_port,
        },
    };
    Html(render(&PageModel {
        machine: &daemon.machine_name,
        version: crate::VERSION,
        apk: &apk,
        pairing: &pairing,
    }))
    .into_response()
}
