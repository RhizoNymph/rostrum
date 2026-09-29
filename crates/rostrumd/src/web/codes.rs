//! `POST /pairing-codes`: a one-time code, its link, and the link's QR code.

use axum::{Json, extract::State};
use chrono::{DateTime, Utc};
use rostrum_remote::{ApiErrorCode, Endpoint, EndpointError, Host, PairingOffer};
use serde::{Deserialize, Serialize};

use super::{gate::Admin, qr};
use crate::{
    daemon::Daemon,
    http::{ApiFailure, Peer},
};

/// What the page shows for a fresh code.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeOffer {
    /// `XXXX-XXXX`.
    pub code: String,
    /// `rostrum://pair?…`, for the "Open in Rostrum" link.
    pub uri: String,
    /// The same link as an inline `<svg>` element.
    pub qr_svg: String,
    pub expires_at: DateTime<Utc>,
    /// `4F2A · 91C0 · 7E3B`, for comparing by eye.
    pub fingerprint_short: String,
    pub fingerprint_hex: String,
    /// The addresses in the link, in the order the phone tries them.
    pub hosts: Vec<String>,
}

pub async fn issue(
    State(daemon): State<Daemon>,
    Admin(class): Admin,
    Peer(ip): Peer,
) -> Result<Json<CodeOffer>, ApiFailure> {
    let hosts = daemon.network_view().advertised_hosts();
    // Checked before a code is minted, so a failure wastes nothing.
    let endpoint = Endpoint::new(hosts.clone(), daemon.https_port, daemon.fingerprint).map_err(
        |error| match error {
            EndpointError::NoHosts => ApiFailure::new(
                ApiErrorCode::Internal,
                "this computer has no LAN or tailnet address a phone could reach; connect it to a network and try again",
            ),
            EndpointError::Port => {
                ApiFailure::new(ApiErrorCode::Internal, "the API port is not configured")
            }
        },
    )?;
    let issued = daemon
        .registry
        .issue_code()
        .await
        .map_err(|error| ApiFailure::internal("could not issue a pairing code", &error))?;
    let offer = PairingOffer {
        machine: daemon.machine_name.clone(),
        endpoint,
        code: issued.code.clone(),
    };
    let uri = offer.to_uri();
    let qr_svg = qr::svg(&uri)
        .map_err(|error| ApiFailure::internal("could not draw the QR code", &error))?;
    tracing::info!(
        client = %ip,
        via = ?class,
        hosts = hosts.len(),
        expires_at = %issued.expires_at,
        "issued a pairing code"
    );
    Ok(Json(CodeOffer {
        code: issued.code.to_string(),
        uri,
        qr_svg,
        expires_at: issued.expires_at,
        fingerprint_short: daemon.fingerprint.short(),
        fingerprint_hex: daemon.fingerprint.to_hex(),
        hosts: hosts.iter().map(Host::to_string).collect(),
    }))
}
