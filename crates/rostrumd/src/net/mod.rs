//! Everything about addresses: who is asking, what this computer can be
//! reached at, and the sockets the servers listen on.
//!
//! - [`client`]: whether a peer is this computer, the tailnet, or anyone else.
//! - [`request_host`]: the `Host` and `Origin` checks on the page server's
//!   privileged routes.
//! - [`interfaces`] and [`tailscale`]: the raw material for the addresses a
//!   pairing link advertises, combined in [`view`].
//! - [`listen`]: binding `0.0.0.0` and `::` on one port without colliding.

pub mod client;
pub mod interfaces;
pub mod listen;
pub mod request_host;
pub mod tailscale;
pub mod view;

pub use client::ClientClass;
pub use view::NetworkView;
