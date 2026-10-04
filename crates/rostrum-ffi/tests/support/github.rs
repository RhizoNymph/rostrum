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
    /// The branch it targets; `topic-N` stacks it on #N.
    pub base: String,
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
            base: "main".into(),
        }
    }
}

/// One open issue as the fake serves it, in `octo/repo`.
#[derive(Clone, Debug)]
pub struct Iss {
    pub number: u32,
    pub author: String,
    pub labels: Vec<String>,
    pub assignees: Vec<String>,
    pub comments: u32,
    pub title: String,
    pub body: String,
    /// GitHub's `updatedAt`; an edit through the fake moves it to
    /// [`EDITED_AT`].
    pub updated_at: String,
}

/// When an issue edited through the fake was last updated.
pub const EDITED_AT: &str = "2026-03-01T00:00:00Z";

impl Iss {
    pub fn new(number: u32, author: &str) -> Self {
        Self {
            number,
            author: author.into(),
            labels: Vec::new(),
            assignees: Vec::new(),
            comments: 0,
            title: format!("Issue {number}"),
            body: "It **breaks**.".into(),
            updated_at: format!("2026-01-02T00:00:{:02}Z", number % 60),
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
    /// `octo/repo`'s open issues; other repositories have none. Closing one
    /// through the REST call drops it, reopening is a no-op.
    pub issues: Vec<Iss>,
    /// `octo/repo`'s Stacks API answer (the JSON array); `None` answers 404,
    /// as for a repository without stacks.
    pub stacks: Option<Value>,
    /// The default branch, and the other branches that exist, for the
    /// branch-tree query.
    pub default_branch: Option<String>,
    pub branches: Vec<String>,
    pub stars: u32,
    /// Who issues can be assigned to.
    pub assignees: Vec<String>,
    /// Serve conversations (issue and pull request comments) in two pages:
    /// the newest holds [`newest_comment`], the earlier one two older
    /// comments.
    pub paged: bool,
    /// `octo/repo`'s head-commit check contexts per pull request, as the CI
    /// query's `contexts.nodes`.
    pub ci: Vec<(u32, Vec<Value>)>,
    /// The answer to every re-run request; `None` is 201.
    pub rerun: Option<(u16, String)>,
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
            .filter(|body| {
                body["query"]
                    .as_str()
                    .is_some_and(|query| query.contains(needle))
            })
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
    let mut world = world.lock().expect("world");
    if world.reject_token {
        return (401, json!({"message": "Bad credentials"}).to_string());
    }
    if request.path == "/graphql" {
        let body: Value = serde_json::from_str(&request.body).expect("graphql body");
        return (200, graphql(&world, &body).to_string());
    }
    rest(&mut world, request)
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

    // The CI query: open pull requests with their check contexts. Matched
    // before the feed query, which also lists open pull requests.
    if query.contains("contexts(first") && query.contains("pullRequests(states: OPEN") {
        if find(world, variables).is_none() {
            return json!({"data": {"repository": null}});
        }
        let nodes: Vec<Value> = world
            .ci
            .iter()
            .map(|(number, contexts)| {
                json!({
                    "number": number,
                    "headRefOid": format!("sha{number}aaaaaaa"),
                    "commits": {"nodes": [{"commit": {
                        "oid": format!("sha{number}aaaaaaa"),
                        "statusCheckRollup": {"state": "PENDING", "contexts": {
                            "totalCount": contexts.len(),
                            "nodes": contexts
                        }}
                    }}]}
                })
            })
            .collect();
        return json!({"data": {
            "rateLimit": {"cost": 1, "remaining": 4999, "resetAt": "2030-01-01T00:00:00Z"},
            "repository": {"pullRequests": {"nodes": nodes}}
        }});
    }
    if query.contains("issues(states: OPEN") {
        if find(world, variables).is_none() {
            return json!({
                "data": {"repository": null},
                "errors": [{"type": "NOT_FOUND", "message": "Could not resolve to a Repository", "path": ["repository"]}]
            });
        }
        let nodes: Vec<Value> = if variables["name"] == "repo" {
            world.issues.iter().map(issue_node).collect()
        } else {
            Vec::new()
        };
        return json!({
            "data": {
                "rateLimit": {"cost": 1, "remaining": 4999, "resetAt": "2030-01-01T00:00:00Z"},
                "repository": {"issues": {"nodes": nodes}}
            }
        });
    }
    if query.contains("issue(number:") {
        let number = variables["number"].as_u64().unwrap_or_default();
        let found = world
            .issues
            .iter()
            .find(|issue| u64::from(issue.number) == number);
        let Some(issue) = found else {
            return json!({"data": {"repository": {"issue": null}}});
        };
        return json!({"data": {"repository": {"issue": issue_detail_node(issue, world.paged, variables)}}});
    }
    if query.contains("defaultBranchRef") {
        let mut repository = serde_json::Map::new();
        repository.insert("url".into(), json!("https://github.com/octo/repo"));
        repository.insert("stargazerCount".into(), json!(world.stars));
        repository.insert(
            "defaultBranchRef".into(),
            world
                .default_branch
                .as_ref()
                .map_or(Value::Null, |name| json!({"name": name})),
        );
        let mut index = 0;
        while let Some(qualified) = variables[format!("r{index}")].as_str() {
            let name = qualified.trim_start_matches("refs/heads/");
            let exists = world.default_branch.as_deref() == Some(name)
                || world.branches.iter().any(|branch| branch == name);
            repository.insert(
                format!("r{index}"),
                if exists {
                    json!({"name": name})
                } else {
                    Value::Null
                },
            );
            index += 1;
        }
        return json!({"data": {"repository": repository}});
    }

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
        return json!({"data": {"repository": {"pullRequest": conversation(world.paged, variables)}}});
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
        "baseRefName": pr.base,
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

fn issue_node(issue: &Iss) -> Value {
    json!({
        "id": format!("I_{}", issue.number),
        "number": issue.number,
        "title": issue.title,
        "url": format!("https://github.com/octo/repo/issues/{}", issue.number),
        "state": "OPEN",
        "stateReason": null,
        "createdAt": format!("2026-01-01T00:00:{:02}Z", issue.number % 60),
        "updatedAt": issue.updated_at,
        "author": {"login": issue.author, "avatarUrl": null},
        "assignees": {"nodes": issue.assignees.iter().map(|login| json!({"login": login, "avatarUrl": null})).collect::<Vec<_>>()},
        "labels": {"nodes": issue.labels.iter().map(|name| json!({"name": name, "color": "d73a4a"})).collect::<Vec<_>>()},
        "comments": {"totalCount": issue.comments},
        "milestone": {"title": "v1"}
    })
}

/// Whether a paged document asked for `connection` (absent: yes).
fn wants(variables: &Value, switch: &str) -> bool {
    variables[switch].as_bool().unwrap_or(true)
}

fn comment(id: &str, body: &str, at: &str, login: &str) -> Value {
    json!({"id": id, "body": body, "createdAt": at, "author": {"login": login, "avatarUrl": null}})
}

/// A conversation's comments connection: one page, or with `paged` the newest
/// page (one comment, two more before cursor `older`) or the earlier one.
fn comments_page(paged: bool, variables: &Value, single: Value) -> Value {
    if !paged {
        return json!({"totalCount": 1, "nodes": [single]});
    }
    match variables["commentsBefore"].as_str() {
        None => json!({
            "totalCount": 3,
            "pageInfo": {"startCursor": "older", "hasPreviousPage": true},
            "nodes": [single]
        }),
        Some(_) => json!({
            "totalCount": 3,
            "pageInfo": {"startCursor": "oldest", "hasPreviousPage": false},
            "nodes": [
                comment("IC_OLD1", "First!", "2026-01-01T12:00:00Z", "dave"),
                comment("IC_OLD2", "Second", "2026-01-01T13:00:00Z", "erin")
            ]
        }),
    }
}

/// [`issue_node`] with a body, one comment, and a closed (not planned) then
/// reopened history with a cross-reference and an unassignment.
fn issue_detail_node(issue: &Iss, paged: bool, variables: &Value) -> Value {
    let mut node = issue_node(issue);
    let actor = json!({"login": "bob", "avatarUrl": null});
    node["body"] = json!(issue.body);
    node.as_object_mut().expect("object").remove("comments");
    if wants(variables, "withComments") {
        node["comments"] = comments_page(
            paged,
            variables,
            comment("IC_9", "Same here", "2026-01-03T00:00:00Z", "carol"),
        );
    }
    if wants(variables, "withEvents") {
        node["timelineItems"] = json!({"totalCount": 4, "nodes": [
            {"__typename": "ClosedEvent", "createdAt": "2026-01-04T00:00:00Z", "actor": actor, "stateReason": "NOT_PLANNED"},
            {"__typename": "ReopenedEvent", "createdAt": "2026-01-05T00:00:00Z", "actor": actor},
            {"__typename": "UnassignedEvent", "createdAt": "2026-01-06T00:00:00Z", "actor": actor, "assignee": {"login": "dave"}},
            {"__typename": "CrossReferencedEvent", "createdAt": "2026-01-07T00:00:00Z", "actor": actor,
             "source": {"__typename": "PullRequest", "number": 1, "title": "Pull request 1", "repository": {"nameWithOwner": "octo/repo"}}}
        ]});
    }
    node
}

fn conversation(paged: bool, variables: &Value) -> Value {
    let mut node = json!({
        "state": "OPEN",
        "body": "Fixes the thing.",
        "createdAt": "2026-01-01T00:00:00Z",
        "author": {"login": "alice", "avatarUrl": null},
        "commits": {"nodes": [{"commit": {"statusCheckRollup": {"contexts": {"nodes": [
            {"__typename": "CheckRun", "name": "ci", "conclusion": "SUCCESS", "status": "COMPLETED", "detailsUrl": null}
        ]}}}}]}
    });
    if wants(variables, "withComments") {
        node["comments"] = comments_page(
            paged,
            variables,
            comment("IC_1", "Looks **good**", "2026-01-02T00:00:00Z", "bob"),
        );
    }
    if wants(variables, "withReviews") {
        node["reviews"] = json!({"totalCount": 0, "nodes": []});
    }
    if wants(variables, "withThreads") {
        node["reviewThreads"] = json!({"totalCount": 1, "nodes": [{
            "id": "RT_1", "path": "src/lib.rs", "line": 11, "originalLine": 11, "diffSide": "RIGHT",
            "isResolved": false, "isOutdated": false,
            "comments": {"nodes": [{
                "id": "RC_1", "databaseId": 555, "body": "why two?", "createdAt": "2026-01-03T00:00:00Z",
                "author": {"login": "bob", "avatarUrl": null}, "pullRequestReview": null
            }]}
        }]});
    }
    if wants(variables, "withEvents") {
        node["timelineItems"] = json!({"totalCount": 0, "nodes": []});
    }
    node
}

fn rest(world: &mut World, request: &Request) -> (u16, String) {
    let path = request.path.split('?').next().unwrap_or_default();
    if let Some(answer) = issue_rest(world, request.method.as_str(), path, &request.body) {
        return answer;
    }
    match (request.method.as_str(), path) {
        ("GET", "/repos/octo/repo/pulls/1/files") => {
            (200, serde_json::to_string(&super::files()).expect("files"))
        }
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

/// The Stacks API, assignees, and every issue REST call, for `octo/repo`.
/// The CI fixtures `rostrum-github` decodes in its own tests.
const JOB_LOG: &str =
    include_str!("../../../rostrum-github/fixtures/ci/job_log_failed_excerpt.txt");
const CHECK_RUN: &str =
    include_str!("../../../rostrum-github/fixtures/ci/check_run_third_party.json");
const ANNOTATIONS: &str =
    include_str!("../../../rostrum-github/fixtures/ci/check_run_annotations.json");

/// Job logs, check-run output and re-runs, for `octo/repo`.
fn ci_rest(world: &World, method: &str, path: &str) -> Option<(u16, String)> {
    let rest = path.strip_prefix("/repos/octo/repo/")?;
    match method {
        "GET" if rest.starts_with("actions/jobs/") && rest.ends_with("/logs") => {
            Some((200, JOB_LOG.to_string()))
        }
        "GET" if rest.starts_with("check-runs/") && rest.ends_with("/annotations") => {
            Some((200, ANNOTATIONS.to_string()))
        }
        "GET" if rest.starts_with("check-runs/") => Some((200, CHECK_RUN.to_string())),
        "POST"
            if rest.ends_with("/rerun")
                || rest.ends_with("/rerun-failed-jobs")
                || rest.ends_with("/rerequest") =>
        {
            Some(world.rerun.clone().unwrap_or((201, String::new())))
        }
        _ => None,
    }
}

fn issue_rest(world: &mut World, method: &str, path: &str, body: &str) -> Option<(u16, String)> {
    if let Some(answer) = ci_rest(world, method, path) {
        return Some(answer);
    }
    let ok = |value: Value| Some((200, value.to_string()));
    match (method, path) {
        ("GET", "/repos/octo/repo/stacks") => {
            return match &world.stacks {
                Some(stacks) => ok(stacks.clone()),
                None => Some((404, json!({"message": "Not Found"}).to_string())),
            };
        }
        ("GET", "/repos/octo/repo/assignees") => {
            return ok(Value::Array(
                world
                    .assignees
                    .iter()
                    .map(|login| json!({"login": login, "avatar_url": null}))
                    .collect(),
            ));
        }
        ("POST", "/repos/octo/repo/issues") => {
            return Some((
                201,
                json!({"number": 99, "html_url": "https://github.com/octo/repo/issues/99"})
                    .to_string(),
            ));
        }
        _ => {}
    }
    let rest = path.strip_prefix("/repos/octo/repo/issues/")?;
    let (number, tail) = rest.split_once('/').unwrap_or((rest, ""));
    let number: u32 = number.parse().ok()?;
    // Pull request #1's comment and label routes stay with the caller.
    let index = world
        .issues
        .iter()
        .position(|issue| issue.number == number)?;
    let body: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    match (method, tail) {
        ("POST", "comments") => {
            world.issues[index].comments += 1;
            ok(json!({"id": 1}))
        }
        ("PATCH", "") => {
            if let Some(title) = body["title"].as_str() {
                let issue = &mut world.issues[index];
                issue.title = title.to_string();
                issue.body = body["body"].as_str().unwrap_or_default().to_string();
                issue.updated_at = EDITED_AT.into();
            }
            if body["state"] == "closed" {
                world.issues.remove(index);
            }
            ok(json!({}))
        }
        ("POST", "labels") => ok(json!([])),
        ("DELETE", tail) if tail.starts_with("labels/") => ok(json!([])),
        ("POST", "assignees") | ("DELETE", "assignees") => ok(json!({})),
        _ => None,
    }
}
