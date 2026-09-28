//! A stand-in for api.github.com: enough of GraphQL and REST, over plain
//! HTTP, to drive the core's refresh, detail, diff, review and mutation paths
//! and to record what it sent.

use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use tokio::{io::AsyncWriteExt, net::TcpListener};

use super::{Log, Request};

/// One pull request as the fake serves it.
#[derive(Clone, Debug)]
pub struct Pr {
    pub number: u32,
    pub author: String,
    pub head_sha: String,
    pub is_draft: bool,
    /// `MERGEABLE`, `CONFLICTING` or `UNKNOWN`.
    pub mergeable: &'static str,
    pub merge_state: &'static str,
    pub review_requests: Vec<String>,
    /// `(ahead, behind)` from the compare query.
    pub divergence: (u32, u32),
}

impl Pr {
    pub fn new(number: u32, author: &str) -> Self {
        Self {
            number,
            author: author.into(),
            head_sha: format!("sha{number}"),
            is_draft: false,
            mergeable: "MERGEABLE",
            merge_state: "CLEAN",
            review_requests: Vec::new(),
            divergence: (1, 0),
        }
    }
}

/// What the fake answers with. Tests change it between calls.
#[derive(Default)]
pub struct World {
    pub viewer: String,
    /// `owner/name` → open pull requests; a repository absent here is
    /// answered as not found.
    pub repos: Vec<(String, Vec<Pr>)>,
    /// Answer every request with 401.
    pub reject_token: bool,
    /// Refuse merges with this reason (HTTP 405).
    pub refuse_merge: Option<String>,
}

pub struct FakeGitHub {
    pub world: Arc<Mutex<World>>,
    pub log: Log,
    pub graphql_url: String,
    pub rest_base: String,
}

impl FakeGitHub {
    pub async fn start(world: World) -> Self {
        let world = Arc::new(Mutex::new(world));
        let log = Log::default();
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let base = format!("http://{}", listener.local_addr().expect("addr"));
        let serving = world.clone();
        let recorded = log.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    return;
                };
                let world = serving.clone();
                let recorded = recorded.clone();
                tokio::spawn(async move {
                    let Some(request) = super::read_request(&mut stream).await else {
                        return;
                    };
                    let (status, body) = answer(&world, &request);
                    recorded.0.lock().expect("log").push(request);
                    let response = format!(
                        "HTTP/1.1 {status} Status\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = stream.write_all(response.as_bytes()).await;
                    let _ = stream.shutdown().await;
                });
            }
        });
        Self {
            world,
            log,
            graphql_url: format!("{base}/graphql"),
            rest_base: base,
        }
    }

    pub fn edit(&self, change: impl FnOnce(&mut World)) {
        change(&mut self.world.lock().expect("world"));
    }

    /// GraphQL requests whose document contains `needle`.
    pub fn graphql_calls(&self, needle: &str) -> Vec<Value> {
        self.log
            .requests()
            .into_iter()
            .filter(|request| request.path == "/graphql")
            .filter_map(|request| serde_json::from_str::<Value>(&request.body).ok())
            .filter(|body| body["query"].as_str().is_some_and(|query| query.contains(needle)))
            .collect()
    }

    /// REST requests to `path`.
    pub fn rest_calls(&self, method: &str, path: &str) -> Vec<Request> {
        self.log
            .requests()
            .into_iter()
            .filter(|request| request.method == method && request.path == path)
            .collect()
    }
}

fn answer(world: &Mutex<World>, request: &Request) -> (u16, String) {
    let world = world.lock().expect("world");
    if world.reject_token {
        return (401, json!({"message": "Bad credentials"}).to_string());
    }
    if request.path == "/graphql" {
        let body: Value = serde_json::from_str(&request.body).expect("graphql body");
        return (200, graphql(&world, &body).to_string());
    }
    rest(&world, request)
}

fn find<'a>(world: &'a World, variables: &Value) -> Option<&'a Vec<Pr>> {
    let name = format!(
        "{}/{}",
        variables["owner"].as_str().unwrap_or_default(),
        variables["name"].as_str().unwrap_or_default()
    );
    world
        .repos
        .iter()
        .find(|(repo, _)| repo == &name)
        .map(|(_, prs)| prs)
}

fn graphql(world: &World, body: &Value) -> Value {
    let query = body["query"].as_str().unwrap_or_default();
    let variables = &body["variables"];
    let viewer = json!({"login": world.viewer, "avatarUrl": null});

    if query.contains("pullRequests(states: OPEN") {
        let Some(prs) = find(world, variables) else {
            return json!({
                "data": {"viewer": viewer, "repository": null},
                "errors": [{"type": "NOT_FOUND", "message": "Could not resolve to a Repository", "path": ["repository"]}]
            });
        };
        let nodes: Vec<Value> = prs.iter().map(pr_node).collect();
        return json!({
            "data": {
                "rateLimit": {"cost": 1, "remaining": 4999, "resetAt": "2030-01-01T00:00:00Z"},
                "viewer": viewer,
                "repository": {"pullRequests": {"nodes": nodes}}
            }
        });
    }
    if query.contains("compare(headRef") {
        let prs = find(world, variables).cloned().unwrap_or_default();
        let mut repository = serde_json::Map::new();
        let mut index = 0;
        while let Some(head) = variables[format!("h{index}")].as_str() {
            let answer = prs
                .iter()
                .find(|pr| format!("topic-{}", pr.number) == head)
                .map(|pr| {
                    json!({"compare": {"aheadBy": pr.divergence.0, "behindBy": pr.divergence.1, "status": "DIVERGED"}})
                });
            repository.insert(format!("p{index}"), answer.unwrap_or(Value::Null));
            index += 1;
        }
        return json!({"data": {"repository": repository}});
    }
    if query.contains("reviewThreads") {
        return json!({"data": {"repository": {"pullRequest": conversation()}}});
    }
    if query.contains("convertPullRequestToDraft") {
        return json!({"data": {"payload": {"pullRequest": {"id": variables["id"], "isDraft": true}}}});
    }
    if query.contains("markPullRequestReadyForReview") {
        return json!({"data": {"payload": {"pullRequest": {"id": variables["id"], "isDraft": false}}}});
    }
    if query.contains("updatePullRequestBranch") {
        return json!({"data": {"payload": {"pullRequest": {"headRefOid": "updated"}}}});
    }
    if query.contains("viewer { login avatarUrl }") {
        return json!({"data": {"viewer": viewer}});
    }
    json!({"errors": [{"message": format!("the fake does not know {query}")}]})
}

fn pr_node(pr: &Pr) -> Value {
    json!({
        "id": format!("PR_{}", pr.number),
        "number": pr.number,
        "title": format!("Pull request {}", pr.number),
        "url": format!("https://github.com/octo/repo/pull/{}", pr.number),
        "isDraft": pr.is_draft,
        "createdAt": "2026-01-01T00:00:00Z",
        "updatedAt": format!("2026-01-01T00:00:{:02}Z", pr.number % 60),
        "author": {"login": pr.author, "avatarUrl": null},
        "headRefName": format!("topic-{}", pr.number),
        "headRefOid": pr.head_sha,
        "baseRefName": "main",
        "additions": 2,
        "deletions": 1,
        "changedFiles": 1,
        "mergeable": pr.mergeable,
        "mergeStateStatus": pr.merge_state,
        "reviewDecision": null,
        "assignees": {"nodes": []},
        "reviewRequests": {"nodes": pr.review_requests.iter().map(|login| json!({"requestedReviewer": {"login": login, "avatarUrl": null}})).collect::<Vec<_>>()},
        "labels": {"nodes": [{"name": "bug", "color": "d73a4a"}]},
        "comments": {"totalCount": 0},
        "commits": {"nodes": [{"commit": {"statusCheckRollup": {"state": "SUCCESS"}}}]}
    })
}

fn conversation() -> Value {
    json!({
        "state": "OPEN",
        "body": "Fixes the thing.",
        "createdAt": "2026-01-01T00:00:00Z",
        "author": {"login": "alice", "avatarUrl": null},
        "comments": {"nodes": [
            {"id": "IC_1", "body": "Looks **good**", "createdAt": "2026-01-02T00:00:00Z", "author": {"login": "bob", "avatarUrl": null}}
        ]},
        "reviews": {"nodes": []},
        "reviewThreads": {"nodes": [{
            "id": "RT_1", "path": "src/lib.rs", "line": 11, "originalLine": 11, "diffSide": "RIGHT",
            "isResolved": false, "isOutdated": false,
            "comments": {"nodes": [{
                "id": "RC_1", "databaseId": 555, "body": "why two?", "createdAt": "2026-01-03T00:00:00Z",
                "author": {"login": "bob", "avatarUrl": null}, "pullRequestReview": null
            }]}
        }]},
        "timelineItems": {"nodes": []},
        "commits": {"nodes": [{"commit": {"statusCheckRollup": {"contexts": {"nodes": [
            {"__typename": "CheckRun", "name": "ci", "conclusion": "SUCCESS", "status": "COMPLETED", "detailsUrl": null}
        ]}}}}]}
    })
}

fn rest(world: &World, request: &Request) -> (u16, String) {
    let path = request.path.split('?').next().unwrap_or_default();
    match (request.method.as_str(), path) {
        ("GET", "/repos/octo/repo/pulls/1/files") => (
            200,
            serde_json::to_string(&super::files()).expect("files"),
        ),
        ("GET", "/repos/octo/repo/labels") => (
            200,
            json!([{"name": "bug", "color": "d73a4a"}, {"name": "help wanted", "color": "008672"}])
                .to_string(),
        ),
        ("PUT", "/repos/octo/repo/pulls/1/merge") => match &world.refuse_merge {
            Some(reason) => (405, json!({"message": reason}).to_string()),
            None => (200, json!({"merged": true}).to_string()),
        },
        ("POST", "/repos/octo/repo/pulls/1/reviews")
        | ("POST", "/repos/octo/repo/issues/1/comments")
        | ("POST", "/repos/octo/repo/pulls/1/comments/555/replies")
        | ("POST", "/repos/octo/repo/issues/1/labels")
        | ("PATCH", "/repos/octo/repo/pulls/1") => (200, "{}".into()),
        ("DELETE", path) if path.starts_with("/repos/octo/repo/issues/1/labels/") => {
            (200, "[]".into())
        }
        _ => (404, json!({"message": "Not Found"}).to_string()),
    }
}
