//! What both servers share: the error body, the JSON extractor, the peer
//! address, and request logging.

mod failure;
mod log;
mod peer;

pub use failure::{ApiFailure, ApiJson};
pub use log::log_request;
pub use peer::{Peer, peer_ip};
