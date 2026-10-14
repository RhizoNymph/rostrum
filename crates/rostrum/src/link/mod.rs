//! Opening a `rostrum://pr` or `rostrum://issue` link on the desktop.
//!
//! `rostrum <link>` is what the `x-scheme-handler/rostrum` desktop entry
//! runs. At startup ([`start`]) a link is either handed to the rostrum
//! window already running (and this process exits), or this process becomes
//! the window and opens it once GitHub auth has resolved. A plain `rostrum`
//! with no link starts a window as before, and listens for links only when
//! no other window does. The link grammar itself lives in
//! `rostrum_core::link`; what the window does with one is
//! `Workspace::open_link`.

pub mod instance;
pub mod linked;

use std::{ffi::OsString, path::Path};

use rostrum_core::{LinkError, RostrumLink};

use instance::{Claim, HandOff, HandOffError, Listener};

/// What the command line asked for.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ArgsError {
    #[error("rostrum can't open this link: {0}")]
    Link(#[from] LinkError),
    #[error("the link is not valid UTF-8")]
    NotUtf8,
    #[error("expected at most one link, got {0} arguments")]
    TooMany(usize),
}

/// The link on the command line, if any. The desktop entry's `%u` expands
/// to nothing when the launcher has no URL, so no argument is an ordinary
/// start.
pub fn link_from_args(
    args: impl IntoIterator<Item = OsString>,
) -> Result<Option<RostrumLink>, ArgsError> {
    let args: Vec<OsString> = args.into_iter().collect();
    match args.as_slice() {
        [] => Ok(None),
        [arg] => {
            let text = arg.to_str().ok_or(ArgsError::NotUtf8)?;
            Ok(Some(RostrumLink::parse(text)?))
        }
        more => Err(ArgsError::TooMany(more.len())),
    }
}

/// How this process goes on after looking for a running window.
#[derive(Debug)]
pub enum Startup {
    /// The running window took the link; exit successfully.
    HandedOff,
    /// A window is running but would not take the link; say why and exit
    /// with failure rather than open a second window behind its back.
    Failed(HandOffError),
    /// Open a window. `initial` is opened once auth resolves; `listener`,
    /// when present, is served so later links reach this window.
    Run {
        initial: Option<RostrumLink>,
        listener: Option<Listener>,
    },
}

/// Decide, given the link (if any) and where the socket lives (if anywhere).
pub fn start(link: Option<RostrumLink>, socket: Option<&Path>) -> Startup {
    let Some(socket) = socket else {
        tracing::warn!("no runtime or cache directory; links will not reach a running window");
        return Startup::Run {
            initial: link,
            listener: None,
        };
    };
    if let Some(link) = &link {
        match instance::hand_off(socket, link) {
            Ok(HandOff::Delivered) => return Startup::HandedOff,
            Ok(HandOff::NoInstance) => {}
            Err(error) => return Startup::Failed(error),
        }
    }
    let listener = match instance::claim(socket) {
        Ok(Claim::Listening(listener)) => Some(listener),
        Ok(Claim::AlreadyRunning) => {
            tracing::info!("another rostrum window receives links; this one will not");
            None
        }
        Err(error) => {
            tracing::warn!(%error, path = %socket.display(), "could not listen for links");
            None
        }
    };
    Startup::Run {
        initial: link,
        listener,
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        sync::{
            Mutex,
            atomic::{AtomicUsize, Ordering},
            mpsc,
        },
        time::Duration,
    };

    use super::*;

    fn args(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    fn link() -> RostrumLink {
        RostrumLink::parse("rostrum://issue?repo=octo%2Frepo&number=7").expect("link")
    }

    #[test]
    fn no_argument_is_an_ordinary_start() {
        assert_eq!(link_from_args(args(&[])), Ok(None));
    }

    #[test]
    fn one_argument_is_the_link() {
        assert_eq!(
            link_from_args(args(&["rostrum://issue?repo=octo%2Frepo&number=7"])),
            Ok(Some(link()))
        );
    }

    #[test]
    fn a_malformed_link_or_extra_arguments_are_refused() {
        assert_eq!(
            link_from_args(args(&["rostrum://pr?repo=octo&number=1"])),
            Err(ArgsError::Link(LinkError::InvalidRepo("octo".into())))
        );
        assert_eq!(
            link_from_args(args(&["rostrum://pr?repo=a/b&number=1", "again"])),
            Err(ArgsError::TooMany(2))
        );
    }

    #[test]
    fn a_link_that_is_not_utf8_is_refused() {
        use std::os::unix::ffi::OsStringExt;
        assert_eq!(
            link_from_args([OsString::from_vec(vec![b'r', 0xFF])]),
            Err(ArgsError::NotUtf8)
        );
    }

    /// A socket path in a fresh directory, short enough to bind.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let dir = PathBuf::from("/tmp").join(format!(
                "rs-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&dir).expect("scratch");
            Self(dir)
        }

        fn socket(&self) -> PathBuf {
            self.0.join("desktop.sock")
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// Start as a window would and serve, forwarding links to the receiver.
    fn first_window(socket: &Path) -> mpsc::Receiver<RostrumLink> {
        let Startup::Run {
            listener: Some(listener),
            ..
        } = start(None, Some(socket))
        else {
            panic!("the first window should listen");
        };
        let (tx, rx) = mpsc::channel();
        let tx = Mutex::new(tx);
        listener
            .serve(move |link| tx.lock().expect("sender").send(link).is_ok())
            .expect("serve");
        rx
    }

    #[test]
    fn the_first_start_with_a_link_opens_it_and_listens() {
        let scratch = Scratch::new();
        let Startup::Run {
            initial,
            listener: Some(_),
        } = start(Some(link()), Some(&scratch.socket()))
        else {
            panic!("expected a listening window");
        };
        assert_eq!(initial, Some(link()));
    }

    #[test]
    fn a_later_start_with_a_link_hands_it_over_and_exits() {
        let scratch = Scratch::new();
        let received = first_window(&scratch.socket());
        assert!(matches!(
            start(Some(link()), Some(&scratch.socket())),
            Startup::HandedOff
        ));
        assert_eq!(
            received.recv_timeout(Duration::from_secs(5)).expect("link"),
            link()
        );
    }

    #[test]
    fn a_later_start_without_a_link_opens_a_window_that_does_not_listen() {
        let scratch = Scratch::new();
        let _received = first_window(&scratch.socket());
        assert!(matches!(
            start(None, Some(&scratch.socket())),
            Startup::Run {
                initial: None,
                listener: None
            }
        ));
    }

    #[test]
    fn a_window_that_refuses_fails_the_launch_instead_of_opening_another() {
        let scratch = Scratch::new();
        let Startup::Run {
            listener: Some(listener),
            ..
        } = start(None, Some(&scratch.socket()))
        else {
            panic!("expected to listen");
        };
        listener.serve(|_| false).expect("serve");
        assert!(matches!(
            start(Some(link()), Some(&scratch.socket())),
            Startup::Failed(HandOffError::Refused(_))
        ));
    }

    #[test]
    fn without_a_socket_path_the_link_opens_here() {
        assert!(matches!(
            start(Some(link()), None),
            Startup::Run {
                initial: Some(_),
                listener: None
            }
        ));
    }
}
