//! Running both routers on their listeners, and stopping them.

use std::{future::Future, io, net::SocketAddr, pin::Pin, sync::Arc, task::Poll, time::Duration};

use axum::Router;
use axum_server::{Handle, tls_rustls::RustlsConfig};
use tokio::{sync::watch, task::JoinHandle};

use crate::{api, daemon::Daemon, error::StartupError, web};

type ServeTask = JoinHandle<io::Result<()>>;

/// The running servers.
pub struct Servers {
    tasks: Vec<(&'static str, ServeTask)>,
    tls_handles: Vec<Handle<SocketAddr>>,
    stop_http: watch::Sender<bool>,
}

/// Serve the page on every `http` listener and the API on every `https`
/// listener.
pub fn start(
    daemon: &Daemon,
    tls: Arc<rustls::ServerConfig>,
    http: Vec<std::net::TcpListener>,
    https: Vec<std::net::TcpListener>,
) -> Result<Servers, StartupError> {
    let (stop_http, stop) = watch::channel(false);
    let page = web::router(daemon.clone());
    let api = api::router(daemon.clone());
    let tls = RustlsConfig::from_config(tls);
    let mut tasks = Vec::new();
    let mut tls_handles = Vec::new();
    for listener in http {
        let task = serve_http(listener, page.clone(), stop.clone()).map_err(|source| {
            StartupError::Serve {
                server: "page",
                source,
            }
        })?;
        tasks.push(("page", task));
    }
    for listener in https {
        let handle = Handle::new();
        let task =
            serve_https(listener, api.clone(), tls.clone(), handle.clone()).map_err(|source| {
                StartupError::Serve {
                    server: "API",
                    source,
                }
            })?;
        tls_handles.push(handle);
        tasks.push(("API", task));
    }
    Ok(Servers {
        tasks,
        tls_handles,
        stop_http,
    })
}

/// Plain HTTP with the peer address available to handlers.
pub fn serve_http(
    listener: std::net::TcpListener,
    router: Router,
    mut stop: watch::Receiver<bool>,
) -> io::Result<ServeTask> {
    let listener = tokio::net::TcpListener::from_std(listener)?;
    Ok(tokio::spawn(async move {
        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(async move {
            let _ = stop.wait_for(|stopping| *stopping).await;
        })
        .await
    }))
}

/// HTTPS over rustls with the peer address available to handlers.
pub fn serve_https(
    listener: std::net::TcpListener,
    router: Router,
    tls: RustlsConfig,
    handle: Handle<SocketAddr>,
) -> io::Result<ServeTask> {
    let server = axum_server::from_tcp_rustls(listener, tls)?.handle(handle);
    Ok(tokio::spawn(server.serve(
        router.into_make_service_with_connect_info::<SocketAddr>(),
    )))
}

impl Servers {
    /// Stop accepting, give in-flight requests `grace` to finish, then stop.
    pub async fn shutdown(self, grace: Duration) {
        let _ = self.stop_http.send(true);
        for handle in &self.tls_handles {
            handle.graceful_shutdown(Some(grace));
        }
        for (server, task) in self.tasks {
            match tokio::time::timeout(grace + Duration::from_secs(1), task).await {
                Ok(Ok(Ok(()))) => {}
                Ok(Ok(Err(error))) => {
                    tracing::warn!(server, %error, "server stopped with an error")
                }
                Ok(Err(error)) => tracing::warn!(server, %error, "server task failed"),
                Err(_) => tracing::warn!(server, "server did not stop in time"),
            }
        }
    }

    /// Wait until any server stops on its own — which only happens on an
    /// error, since stopping is otherwise [`Servers::shutdown`]'s job.
    pub async fn any_stopped(&mut self) -> (&'static str, String) {
        if self.tasks.is_empty() {
            return std::future::pending().await;
        }
        let (index, result) = std::future::poll_fn(|cx| {
            for (index, (_, task)) in self.tasks.iter_mut().enumerate() {
                if let Poll::Ready(result) = Pin::new(task).poll(cx) {
                    return Poll::Ready((index, result));
                }
            }
            Poll::Pending
        })
        .await;
        let (server, _) = self.tasks.remove(index);
        let reason = match result {
            Ok(Ok(())) => "stopped".to_string(),
            Ok(Err(error)) => error.to_string(),
            Err(error) => error.to_string(),
        };
        (server, reason)
    }
}
