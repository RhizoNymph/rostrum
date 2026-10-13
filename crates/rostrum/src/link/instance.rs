//! One rostrum window per user session, as far as links are concerned.
//!
//! The first rostrum to start listens on a Unix socket in the user's runtime
//! directory. A later `rostrum <link>` — what the desktop's
//! `x-scheme-handler/rostrum` runs — connects, writes the link as one line,
//! waits for the answer and exits; the running window opens the link. When
//! nothing answers, the later process becomes the window itself.
//!
//! Plain blocking std sockets on a dedicated thread: the listener spends its
//! life in `accept`, and the hand-off runs before any runtime exists. The
//! listener's only way into the app is the channel it is given.
//!
//! The wire is one line each way:
//!
//! ```text
//! → rostrum://pr?repo=octo%2Frepo&number=42\n
//! ← ok\n                     the window took it
//! ← refused <reason>\n       it did not (malformed, or the window is closing)
//! ```

use std::{
    fs,
    io::{self, BufRead, BufReader, Read, Write},
    os::unix::{
        fs::{DirBuilderExt, PermissionsExt},
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    thread,
    time::Duration,
};

use rostrum_core::RostrumLink;

/// The longest line either side reads, newline included. A link is capped
/// well below this by `rostrum_core::link::MAX_LINK_LEN`.
const MAX_LINE: u64 = 4096;

/// How long a hand-off waits on a running window before giving up on it.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(3);

/// How long the listener waits on a connected client's line.
const CLIENT_TIMEOUT: Duration = Duration::from_secs(2);

/// Where the socket lives: `$XDG_RUNTIME_DIR/rostrum/desktop.sock` (private
/// to the user, cleared at logout), else under the cache directory. `None`
/// when neither is known, and then links are not shared between processes.
pub fn socket_path() -> Option<PathBuf> {
    dirs::runtime_dir()
        .or_else(dirs::cache_dir)
        .map(|base| base.join("rostrum").join("desktop.sock"))
}

/// What a hand-off achieved.
#[derive(Debug, PartialEq, Eq)]
pub enum HandOff {
    /// A running window took the link.
    Delivered,
    /// No window is listening (no socket, or a stale one).
    NoInstance,
}

#[derive(Debug, thiserror::Error)]
pub enum HandOffError {
    #[error("the running rostrum refused the link: {0}")]
    Refused(String),
    #[error("the running rostrum did not answer: {0}")]
    NoAnswer(#[source] io::Error),
    #[error("could not reach the running rostrum at {path}: {source}")]
    Connect {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

/// Give `link` to the window listening at `path`, if there is one.
pub fn hand_off(path: &Path, link: &RostrumLink) -> Result<HandOff, HandOffError> {
    let mut stream = match UnixStream::connect(path) {
        Ok(stream) => stream,
        Err(error) if is_absent(&error) => return Ok(HandOff::NoInstance),
        Err(source) => {
            return Err(HandOffError::Connect {
                path: path.to_path_buf(),
                source,
            });
        }
    };
    let answer = send_line(&mut stream, &link.to_string()).map_err(HandOffError::NoAnswer)?;
    match answer.as_str() {
        "ok" => Ok(HandOff::Delivered),
        other => Err(HandOffError::Refused(
            other.strip_prefix("refused ").unwrap_or(other).to_string(),
        )),
    }
}

/// Write `line`, then read the answer line.
fn send_line(stream: &mut UnixStream, line: &str) -> io::Result<String> {
    stream.set_read_timeout(Some(ANSWER_TIMEOUT))?;
    stream.set_write_timeout(Some(ANSWER_TIMEOUT))?;
    stream.write_all(format!("{line}\n").as_bytes())?;
    stream.flush()?;
    read_line(stream)
}

/// The outcome of trying to become the listening window.
#[derive(Debug)]
pub enum Claim {
    /// This process listens; serve it.
    Listening(Listener),
    /// Another window already listens and keeps doing so.
    AlreadyRunning,
}

/// Listen at `path`, unless a live window already does. A socket file
/// nothing answers on is left over from a window that died; it is replaced.
///
/// Two processes claiming in the same instant can both find the path stale;
/// the later one then replaces the earlier's socket and the earlier window
/// stops receiving links. Starting two windows at once is rare enough to
/// leave it at that.
pub fn claim(path: &Path) -> io::Result<Claim> {
    match UnixStream::connect(path) {
        Ok(_) => return Ok(Claim::AlreadyRunning),
        Err(error) if error.kind() == io::ErrorKind::ConnectionRefused => {
            tracing::info!(path = %path.display(), "replacing a stale rostrum socket");
            fs::remove_file(path)?;
        }
        Err(error) if is_absent(&error) => {}
        Err(error) => return Err(error),
    }
    if let Some(parent) = path.parent() {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(parent)?;
    }
    let listener = UnixListener::bind(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(Claim::Listening(Listener {
        listener,
        path: path.to_path_buf(),
    }))
}

/// A bound socket, not yet serving.
#[derive(Debug)]
pub struct Listener {
    listener: UnixListener,
    path: PathBuf,
}

impl Listener {
    /// Serve on a thread of its own until `sink` reports the app is gone.
    /// `sink` gets every well-formed link and answers whether it was taken.
    pub fn serve(self, sink: impl Fn(RostrumLink) -> bool + Send + 'static) -> io::Result<()> {
        let path = self.path.clone();
        thread::Builder::new()
            .name("rostrum-links".into())
            .spawn(move || {
                tracing::info!(path = %path.display(), "listening for rostrum links");
                for stream in self.listener.incoming() {
                    let mut stream = match stream {
                        Ok(stream) => stream,
                        Err(error) => {
                            tracing::warn!(%error, "could not accept a link connection");
                            continue;
                        }
                    };
                    match serve_one(&mut stream, &sink) {
                        Ok(Served::Open) => {}
                        Ok(Served::Closed) => {
                            tracing::info!("the window is gone; no longer listening for links");
                            return;
                        }
                        // `claim` probing from a second window connects
                        // and hangs up without a word.
                        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => {
                            tracing::debug!("a link connection closed without a link");
                        }
                        Err(error) => tracing::warn!(%error, "a link connection failed"),
                    }
                }
            })
            .map(|_| ())
    }
}

/// Whether the app still takes links after one connection.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Served {
    Open,
    Closed,
}

/// Read one line from a client, hand a valid link to `sink`, answer.
pub(crate) fn serve_one(
    stream: &mut UnixStream,
    sink: &impl Fn(RostrumLink) -> bool,
) -> io::Result<Served> {
    stream.set_read_timeout(Some(CLIENT_TIMEOUT))?;
    stream.set_write_timeout(Some(CLIENT_TIMEOUT))?;
    let line = read_line(stream)?;
    let (answer, served) = match RostrumLink::parse(&line) {
        Ok(link) => {
            tracing::info!(%link, "link handed over by another rostrum");
            if sink(link) {
                ("ok".to_string(), Served::Open)
            } else {
                ("refused rostrum is closing".to_string(), Served::Closed)
            }
        }
        Err(error) => {
            tracing::warn!(%error, "refused a malformed link");
            (format!("refused {error}"), Served::Open)
        }
    };
    stream.write_all(format!("{answer}\n").as_bytes())?;
    stream.flush()?;
    Ok(served)
}

/// One line, at most [`MAX_LINE`] bytes, without its newline. A longer line
/// is an error rather than a truncated link.
fn read_line(stream: &mut UnixStream) -> io::Result<String> {
    let mut line = String::new();
    BufReader::new(Read::by_ref(stream).take(MAX_LINE)).read_line(&mut line)?;
    match line.strip_suffix('\n') {
        Some(complete) => Ok(complete.to_string()),
        None if line.len() as u64 >= MAX_LINE => {
            Err(io::Error::new(io::ErrorKind::InvalidData, "line too long"))
        }
        None => Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "the connection closed mid-line",
        )),
    }
}

/// No listener: the path does not exist, or nothing is bound to it.
fn is_absent(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
    )
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{
            Mutex,
            atomic::{AtomicUsize, Ordering},
            mpsc,
        },
        time::Duration,
    };

    use super::*;

    /// A fresh directory under the system temp dir (or `/tmp` when that is
    /// too deep: a socket path is limited to about 100 bytes), removed on
    /// drop.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let temp = std::env::temp_dir();
            let base = if temp.as_os_str().len() > 48 {
                PathBuf::from("/tmp")
            } else {
                temp
            };
            let dir = base.join(format!(
                "rl-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&dir).expect("scratch dir");
            Self(dir)
        }

        fn socket(&self) -> PathBuf {
            self.0.join("nested").join("desktop.sock")
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn link() -> RostrumLink {
        RostrumLink::parse("rostrum://pr?repo=octo%2Frepo&number=42").expect("link")
    }

    /// Serve at `path`, forwarding every link into the returned receiver.
    fn listening(path: &Path) -> mpsc::Receiver<RostrumLink> {
        let Claim::Listening(listener) = claim(path).expect("claim") else {
            panic!("expected to listen");
        };
        let (tx, rx) = mpsc::channel();
        let tx = Mutex::new(tx);
        listener
            .serve(move |link| tx.lock().expect("sender").send(link).is_ok())
            .expect("serve");
        rx
    }

    #[test]
    fn with_no_socket_there_is_no_instance() {
        let scratch = Scratch::new();
        assert_eq!(
            hand_off(&scratch.socket(), &link()).expect("hand off"),
            HandOff::NoInstance
        );
    }

    #[test]
    fn a_running_window_takes_the_link() {
        let scratch = Scratch::new();
        let received = listening(&scratch.socket());
        assert_eq!(
            hand_off(&scratch.socket(), &link()).expect("hand off"),
            HandOff::Delivered
        );
        assert_eq!(
            received.recv_timeout(Duration::from_secs(5)).expect("link"),
            link()
        );
    }

    #[test]
    fn a_second_window_does_not_take_over_a_live_socket() {
        let scratch = Scratch::new();
        let received = listening(&scratch.socket());
        assert!(matches!(
            claim(&scratch.socket()).expect("claim"),
            Claim::AlreadyRunning
        ));
        // The first window still gets links.
        hand_off(&scratch.socket(), &link()).expect("hand off");
        assert_eq!(
            received.recv_timeout(Duration::from_secs(5)).expect("link"),
            link()
        );
    }

    #[test]
    fn a_stale_socket_is_no_instance_and_is_replaced_by_the_next_window() {
        let scratch = Scratch::new();
        let path = scratch.socket();
        fs::create_dir_all(path.parent().expect("parent")).expect("dir");
        // Bound and dropped: the file stays, nothing listens.
        drop(UnixListener::bind(&path).expect("bind"));
        assert!(path.exists());
        assert_eq!(
            hand_off(&path, &link()).expect("hand off"),
            HandOff::NoInstance
        );
        let received = listening(&path);
        assert_eq!(
            hand_off(&path, &link()).expect("hand off"),
            HandOff::Delivered
        );
        assert_eq!(
            received.recv_timeout(Duration::from_secs(5)).expect("link"),
            link()
        );
    }

    #[test]
    fn the_socket_and_its_directory_are_private() {
        let scratch = Scratch::new();
        let path = scratch.socket();
        let _received = listening(&path);
        let mode = |p: &Path| fs::metadata(p).expect("metadata").permissions().mode() & 0o777;
        assert_eq!(mode(&path), 0o600);
        assert_eq!(mode(path.parent().expect("parent")), 0o700);
    }

    /// Run `serve_one` against a client that writes `sent`, and return what
    /// the client read back and what reached the sink.
    fn exchange(sent: &[u8], taking: bool) -> (String, Served, Vec<RostrumLink>) {
        let (mut client, mut server) = UnixStream::pair().expect("pair");
        client.write_all(sent).expect("write");
        client
            .shutdown(std::net::Shutdown::Write)
            .expect("shutdown");
        let seen = Mutex::new(Vec::new());
        let served = serve_one(&mut server, &|link| {
            seen.lock().expect("seen").push(link);
            taking
        })
        .expect("served");
        let mut answer = String::new();
        client.read_to_string(&mut answer).expect("answer");
        (answer, served, seen.into_inner().expect("seen"))
    }

    #[test]
    fn a_well_formed_line_reaches_the_sink_and_is_acknowledged() {
        let (answer, served, seen) = exchange(b"rostrum://issue?repo=octo/repo&number=7\n", true);
        assert_eq!(answer, "ok\n");
        assert_eq!(served, Served::Open);
        assert_eq!(
            seen,
            vec![RostrumLink::parse("rostrum://issue?repo=octo/repo&number=7").expect("link")]
        );
    }

    #[test]
    fn a_malformed_line_is_refused_with_the_reason_and_never_reaches_the_sink() {
        let (answer, served, seen) = exchange(b"rostrum://pr?repo=octo&number=1\n", true);
        assert!(answer.starts_with("refused "), "{answer}");
        assert!(answer.contains("octo"), "{answer}");
        assert_eq!(served, Served::Open);
        assert!(seen.is_empty());
    }

    #[test]
    fn a_closing_window_refuses_and_stops_listening() {
        let (answer, served, _) = exchange(b"rostrum://pr?repo=octo/repo&number=1\n", false);
        assert_eq!(answer, "refused rostrum is closing\n");
        assert_eq!(served, Served::Closed);
    }

    #[test]
    fn an_overlong_or_unterminated_line_is_an_error_not_a_link() {
        let (mut client, mut server) = UnixStream::pair().expect("pair");
        let long = vec![b'x'; MAX_LINE as usize + 10];
        let writer = thread::spawn(move || {
            let _ = client.write_all(&long);
            client
        });
        let sink = |_: RostrumLink| -> bool { panic!("nothing should reach the sink") };
        assert_eq!(
            serve_one(&mut server, &sink).map_err(|e| e.kind()),
            Err(io::ErrorKind::InvalidData)
        );
        drop(writer.join());

        let (mut client, mut server) = UnixStream::pair().expect("pair");
        client
            .write_all(b"rostrum://pr?repo=a/b&number=1")
            .expect("write");
        client
            .shutdown(std::net::Shutdown::Write)
            .expect("shutdown");
        assert_eq!(
            serve_one(&mut server, &sink).map_err(|e| e.kind()),
            Err(io::ErrorKind::UnexpectedEof)
        );
    }

    #[test]
    fn a_refusal_comes_back_to_the_launching_process() {
        let scratch = Scratch::new();
        let path = scratch.socket();
        let Claim::Listening(listener) = claim(&path).expect("claim") else {
            panic!("expected to listen");
        };
        listener.serve(|_| false).expect("serve");
        let Err(HandOffError::Refused(reason)) = hand_off(&path, &link()) else {
            panic!("expected a refusal");
        };
        assert_eq!(reason, "rostrum is closing");
    }
}
