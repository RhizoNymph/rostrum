//! `GitHubClient::stacks` against a one-shot local HTTP responder: the path
//! it asks for, a page of stacks, and the 404 that means "not enabled".

use std::{
    io::{BufRead, BufReader, Write},
    net::TcpListener,
    sync::mpsc,
    thread,
};

use rostrum_core::{PrNumber, RepoId};
use rostrum_github::{GitHubClient, RepoStacks, Token};

/// Serve exactly one request with `status` and `body`, reporting the request
/// line it received.
fn serve_once(status: &'static str, body: &'static str) -> (String, mpsc::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let base = format!("http://{}", listener.local_addr().expect("addr"));
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let (stream, _) = listener.accept().expect("accept");
        let mut reader = BufReader::new(stream.try_clone().expect("clone"));
        let mut request_line = String::new();
        reader.read_line(&mut request_line).expect("read");
        // Drain the headers.
        let mut line = String::new();
        while reader.read_line(&mut line).map(|n| n > 2).unwrap_or(false) {
            line.clear();
        }
        tx.send(request_line.trim().to_string()).expect("report");
        let mut stream = stream;
        write!(
            stream,
            "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        )
        .expect("respond");
    });
    (base, rx)
}

fn client(base: &str) -> GitHubClient {
    GitHubClient::with_endpoints(Token::new("t"), &format!("{base}/graphql"), base).expect("client")
}

#[tokio::test]
async fn lists_the_repository_s_stacks() {
    let (base, request) = serve_once(
        "200 OK",
        r#"[{"number": 7, "base": {"ref": "main"}, "open": true, "pull_requests": [{"number": 1}, {"number": 2}]}]"#,
    );
    let answer = client(&base)
        .stacks(&RepoId::new("octo", "repo"))
        .await
        .expect("answers");
    assert_eq!(
        request.recv().expect("request seen"),
        "GET /repos/octo/repo/stacks?per_page=100 HTTP/1.1"
    );
    let RepoStacks::Available(stacks) = answer else {
        panic!("expected stacks, got {answer:?}");
    };
    assert_eq!(stacks.len(), 1);
    assert_eq!(stacks[0].members.as_slice(), &[PrNumber(1), PrNumber(2)]);
}

#[tokio::test]
async fn a_404_means_stacks_are_not_enabled() {
    let (base, _request) = serve_once("404 Not Found", r#"{"message": "Not Found"}"#);
    let answer = client(&base)
        .stacks(&RepoId::new("octo", "repo"))
        .await
        .expect("answers");
    assert_eq!(answer, RepoStacks::Unavailable);
}

#[tokio::test]
async fn a_server_error_is_an_error() {
    let (base, _request) = serve_once("500 Internal Server Error", "{}");
    assert!(
        client(&base)
            .stacks(&RepoId::new("octo", "repo"))
            .await
            .is_err()
    );
}
