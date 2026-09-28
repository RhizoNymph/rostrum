//! Listening on every configured address without double-binding.
//!
//! On Linux an IPv6 socket bound to `::` also accepts IPv4 by default
//! (`net.ipv6.bindv6only = 0`), so binding `0.0.0.0` and then `::` on one port
//! fails with "address in use" — or, in the other order, the IPv4 listener
//! never sees a connection. Every IPv6 listener here sets `IPV6_V6ONLY`, so
//! `0.0.0.0` and `::` are two sockets for two families, independent of the
//! sysctl.

use std::{
    io,
    net::{IpAddr, SocketAddr, TcpListener},
};

use socket2::{Domain, Protocol, Socket, Type};

const BACKLOG: i32 = 1024;
/// `EAFNOSUPPORT` on Linux: the kernel has IPv6 disabled.
const EAFNOSUPPORT: i32 = 97;

#[derive(Debug, thiserror::Error)]
pub enum ListenError {
    #[error("could not listen on {addr}")]
    Bind {
        addr: SocketAddr,
        #[source]
        source: io::Error,
    },
    #[error("none of the `bind` addresses could be listened on")]
    Nothing,
}

/// Bind one listener per address on `port`. An address this machine does not
/// have, or a family its kernel does not support, is skipped with a warning;
/// anything else — above all "address in use" — is fatal, because running on
/// half the configured addresses would look like a network problem.
///
/// With `port` 0 the first listener picks a free port and the rest share it.
pub fn bind_all(addrs: &[IpAddr], port: u16) -> Result<Vec<TcpListener>, ListenError> {
    let mut listeners = Vec::with_capacity(addrs.len());
    let mut port = port;
    for ip in addrs {
        let addr = SocketAddr::new(*ip, port);
        match bind_one(addr) {
            Ok(listener) => {
                if port == 0 {
                    port = listener
                        .local_addr()
                        .map_err(|source| ListenError::Bind { addr, source })?
                        .port();
                }
                listeners.push(listener);
            }
            Err(source) if skippable(&source) => {
                tracing::warn!(%addr, error = %source, "skipping a listen address this machine cannot use");
            }
            Err(source) => return Err(ListenError::Bind { addr, source }),
        }
    }
    if listeners.is_empty() {
        return Err(ListenError::Nothing);
    }
    Ok(listeners)
}

fn skippable(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::AddrNotAvailable || error.raw_os_error() == Some(EAFNOSUPPORT)
}

/// One non-blocking listener, IPv6-only when the address is IPv6.
pub fn bind_one(addr: SocketAddr) -> io::Result<TcpListener> {
    let socket = Socket::new(Domain::for_address(addr), Type::STREAM, Some(Protocol::TCP))?;
    if addr.is_ipv6() {
        socket.set_only_v6(true)?;
    }
    // Lets a restarted daemon rebind while old connections sit in TIME_WAIT.
    socket.set_reuse_address(true)?;
    socket.bind(&addr.into())?;
    socket.listen(BACKLOG)?;
    socket.set_nonblocking(true)?;
    Ok(socket.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_v4_and_v6_wildcards_share_a_port() {
        let listeners = bind_all(
            &["::".parse().expect("ip"), "0.0.0.0".parse().expect("ip")],
            0,
        )
        .expect("both bind");
        assert_eq!(listeners.len(), 2);
        let ports: Vec<u16> = listeners
            .iter()
            .map(|listener| listener.local_addr().expect("addr").port())
            .collect();
        assert_eq!(ports[0], ports[1]);
    }

    #[test]
    fn loopback_v4_and_v6_share_a_port() {
        let listeners = bind_all(
            &["127.0.0.1".parse().expect("ip"), "::1".parse().expect("ip")],
            0,
        )
        .expect("both bind");
        let port = listeners[0].local_addr().expect("addr").port();
        assert_eq!(listeners[1].local_addr().expect("addr").port(), port);
        // Both families accept on that port.
        std::net::TcpStream::connect(("127.0.0.1", port)).expect("v4 connects");
        std::net::TcpStream::connect(("::1", port)).expect("v6 connects");
    }

    #[test]
    fn a_port_in_use_is_fatal() {
        let held = bind_one("127.0.0.1:0".parse().expect("addr")).expect("bind");
        let port = held.local_addr().expect("addr").port();
        let err = bind_all(&["127.0.0.1".parse().expect("ip")], port).expect_err("in use");
        assert!(matches!(err, ListenError::Bind { .. }));
    }

    #[test]
    fn an_address_this_machine_lacks_is_skipped() {
        // TEST-NET-3 is never assigned to a local interface.
        let listeners = bind_all(
            &[
                "203.0.113.7".parse().expect("ip"),
                "127.0.0.1".parse().expect("ip"),
            ],
            0,
        )
        .expect("loopback still binds");
        assert_eq!(listeners.len(), 1);
    }

    #[test]
    fn nothing_bindable_is_an_error() {
        assert!(matches!(
            bind_all(&["203.0.113.7".parse().expect("ip")], 0),
            Err(ListenError::Nothing)
        ));
    }
}
