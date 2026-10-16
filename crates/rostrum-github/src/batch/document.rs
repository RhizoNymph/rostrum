//! Building a batched document and its variables.
//!
//! Owners and names travel as variables (`$o0`, `$n0`, …), never spliced into
//! the document text, exactly as the one-repository documents pass `$owner`
//! and `$name`; the document is built from the count alone.

use rostrum_core::RepoId;

/// The alias repository `index` is selected under.
pub fn repo_alias(index: usize) -> String {
    format!("r{index}")
}

/// The index an alias names, if it is one of the first `count`.
pub(crate) fn alias_index(alias: &str, count: usize) -> Option<usize> {
    let index: usize = alias.strip_prefix('r')?.parse().ok()?;
    // `r01` would parse as 1 but is not an alias this module wrote.
    (index < count && repo_alias(index) == alias).then_some(index)
}

/// A document selecting `selection` on `count` repositories, each under its
/// own alias, with `head` (e.g. `viewer { login avatarUrl }`) selected once
/// at the top beside `rateLimit`.
///
/// `selection` may use `$first`, which every batch declares. Only meaningful
/// for `count >= 1`: an empty selection set is a syntax error, so the client
/// never sends an empty batch.
pub fn build_repo_batch(selection: &str, count: usize, head: &str) -> String {
    let mut declarations = String::from("$first: Int!");
    let mut repositories = String::new();
    for index in 0..count {
        declarations.push_str(&format!(", $o{index}: String!, $n{index}: String!"));
        repositories.push_str(&format!(
            "  {}: repository(owner: $o{index}, name: $n{index}) {{{selection}  }}\n",
            repo_alias(index)
        ));
    }
    let head = if head.is_empty() {
        String::new()
    } else {
        format!("  {head}\n")
    };
    format!(
        "query({declarations}) {{\n  rateLimit {{ cost remaining resetAt }}\n{head}{repositories}}}\n"
    )
}

/// The variables a [`build_repo_batch`] document over `repos` expects.
pub fn repo_batch_variables(repos: &[RepoId], first: u32) -> serde_json::Value {
    let mut variables = serde_json::Map::new();
    variables.insert("first".into(), first.into());
    for (index, repo) in repos.iter().enumerate() {
        variables.insert(format!("o{index}"), repo.owner().into());
        variables.insert(format!("n{index}"), repo.name().into());
    }
    serde_json::Value::Object(variables)
}
