//! Names that are safe to hand to a command line.
//!
//! Every type here is validated at construction. That is not defensiveness: a
//! branch can legally be named `--upload-pack=/bin/sh`, and `git rev-list` will
//! read it as an option no matter where it appears in the argument list. The
//! commands this crate issues also pass `--end-of-options` and `--`, but those
//! only help where git supports them, and a name that cannot be typed is better
//! rejected at the boundary than escaped at every use.
//!
//! The second reason names are types: git resolves a bare `main` through
//! `refs/tags/` *before* `refs/heads/`, so a tag named after a branch silently
//! changes what a command answers. [`Rev::as_arg`] always emits a fully
//! qualified ref, which has exactly one meaning.

use std::fmt;

use crate::error::GitError;

/// Why a name was rejected. Carried by
/// [`GitError::InvalidBranchName`](crate::GitError::InvalidBranchName) so a
/// caller can react to the specific problem rather than parse a message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum NameRejection {
    Empty,
    /// The reason this module exists.
    LeadingDash,
    /// Control characters cannot survive a `-z` or line-oriented parse.
    ControlCharacter,
    Space,
    /// `a..b` is range syntax.
    DoubleDot,
    /// `@{upstream}`, `@{1}` — reflog and upstream shorthand.
    ReflogSyntax,
    /// An empty path component: a leading, trailing, or doubled `/`.
    EmptyComponent,
    /// git reserves a leading `.` on any component.
    ComponentStartsWithDot,
    /// `.lock` is how git names its own lock files.
    LockSuffix,
    TrailingDot,
    /// A character git's own `check-ref-format` forbids.
    Forbidden(char),
    /// A lone `@` is git's shorthand for `HEAD`.
    JustAt,
    /// A remote name with a `/` would make `refs/remotes/<remote>/<branch>`
    /// ambiguous: `refs/remotes/a/b/c` could be remote `a` branch `b/c` or
    /// remote `a/b` branch `c`.
    SlashInRemote,
}

impl NameRejection {
    pub fn describe(self) -> String {
        match self {
            Self::Empty => "it is empty".to_string(),
            Self::LeadingDash => "it starts with `-`, which git reads as an option".to_string(),
            Self::ControlCharacter => "it contains a control character".to_string(),
            Self::Space => "it contains a space".to_string(),
            Self::DoubleDot => "it contains `..`, which is range syntax".to_string(),
            Self::ReflogSyntax => "it contains `@{`".to_string(),
            Self::EmptyComponent => "it has an empty `/`-separated component".to_string(),
            Self::ComponentStartsWithDot => "a `/`-separated component starts with `.`".to_string(),
            Self::LockSuffix => "a `/`-separated component ends with `.lock`".to_string(),
            Self::TrailingDot => "it ends with `.`".to_string(),
            Self::Forbidden(ch) => format!("it contains `{ch}`"),
            Self::JustAt => "`@` is git's shorthand for HEAD".to_string(),
            Self::SlashInRemote => "a remote name may not contain `/`".to_string(),
        }
    }
}

/// Characters git's `check-ref-format` forbids anywhere in a ref name.
const FORBIDDEN: [char; 7] = ['\\', '~', '^', ':', '?', '*', '['];

/// Apply the rules shared by branch and remote names.
fn reject(name: &str) -> Option<NameRejection> {
    if name.is_empty() {
        return Some(NameRejection::Empty);
    }
    if name.starts_with('-') {
        return Some(NameRejection::LeadingDash);
    }
    if name == "@" {
        return Some(NameRejection::JustAt);
    }
    if name.ends_with('.') {
        return Some(NameRejection::TrailingDot);
    }
    if name.contains("..") {
        return Some(NameRejection::DoubleDot);
    }
    if name.contains("@{") {
        return Some(NameRejection::ReflogSyntax);
    }
    for ch in name.chars() {
        if ch.is_control() {
            return Some(NameRejection::ControlCharacter);
        }
        if ch == ' ' {
            return Some(NameRejection::Space);
        }
        if FORBIDDEN.contains(&ch) {
            return Some(NameRejection::Forbidden(ch));
        }
    }
    // Catches a leading `/`, a trailing `/`, and `//` in one pass.
    for component in name.split('/') {
        if component.is_empty() {
            return Some(NameRejection::EmptyComponent);
        }
        if component.starts_with('.') {
            return Some(NameRejection::ComponentStartsWithDot);
        }
        if component.ends_with(".lock") {
            return Some(NameRejection::LockSuffix);
        }
    }
    None
}

/// A local branch name, validated on the way in.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BranchName(String);

impl BranchName {
    pub fn new(raw: impl Into<String>) -> Result<Self, GitError> {
        let name = raw.into();
        match reject(&name) {
            Some(reason) => Err(GitError::InvalidBranchName { name, reason }),
            None => Ok(Self(name)),
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The fully qualified form: `refs/heads/<name>`.
    pub fn qualified(&self) -> String {
        format!("refs/heads/{}", self.0)
    }
}

impl fmt::Display for BranchName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::str::FromStr for BranchName {
    type Err = GitError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

/// A remote name, validated with the same rules plus a ban on `/`.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Remote(String);

impl Remote {
    pub fn new(raw: impl Into<String>) -> Result<Self, GitError> {
        let name = raw.into();
        if name.contains('/') {
            return Err(GitError::InvalidBranchName {
                name,
                reason: NameRejection::SlashInRemote,
            });
        }
        match reject(&name) {
            Some(reason) => Err(GitError::InvalidBranchName { name, reason }),
            None => Ok(Self(name)),
        }
    }

    /// `origin`, the name every rostrum-managed clone uses.
    pub fn origin() -> Self {
        Self("origin".to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Remote {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A branch on a remote: the pair, never a pre-joined string.
///
/// Keeping the two halves apart is what lets [`RemoteRef::fetch_refspec`] name
/// the source and destination separately, which is the difference between
/// updating `refs/remotes/origin/<branch>` and only writing `FETCH_HEAD`.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RemoteRef {
    pub remote: Remote,
    pub branch: BranchName,
}

impl RemoteRef {
    pub fn new(remote: Remote, branch: BranchName) -> Self {
        Self { remote, branch }
    }

    /// `origin/main` as a [`RemoteRef`], for the common case.
    pub fn origin(branch: BranchName) -> Self {
        Self::new(Remote::origin(), branch)
    }

    /// The local ref a fetch updates: `refs/remotes/<remote>/<branch>`.
    pub fn tracking_ref(&self) -> String {
        format!("refs/remotes/{}/{}", self.remote, self.branch)
    }

    /// The refspec to fetch with.
    ///
    /// The leading `+` is not optional. `git fetch origin main` writes
    /// `FETCH_HEAD` and leaves `refs/remotes/origin/main` untouched, so every
    /// divergence computed afterwards would be stale; and without `+` a
    /// force-pushed branch — routine for pull requests — is rejected as a
    /// non-fast-forward.
    pub fn fetch_refspec(&self) -> String {
        format!("+refs/heads/{}:{}", self.branch, self.tracking_ref())
    }
}

impl fmt::Display for RemoteRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.remote, self.branch)
    }
}

/// Something a command can be pointed at.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Rev {
    Head,
    Local(BranchName),
    Remote(RemoteRef),
}

impl Rev {
    /// The argument to pass to git — always fully qualified.
    ///
    /// A bare `main` is resolved against `refs/tags/` before `refs/heads/`, so
    /// a tag sharing a branch's name would quietly change the answer to every
    /// `rev-list` in this crate.
    pub fn as_arg(&self) -> String {
        match self {
            Self::Head => "HEAD".to_string(),
            Self::Local(branch) => branch.qualified(),
            Self::Remote(remote) => remote.tracking_ref(),
        }
    }
}

impl fmt::Display for Rev {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.as_arg())
    }
}

/// A git object id: 40 hex digits under SHA-1, 64 under SHA-256.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Oid(String);

impl Oid {
    pub fn parse(raw: impl Into<String>) -> Result<Self, GitError> {
        let value = raw.into();
        let plausible_length = matches!(value.len(), 40 | 64);
        let hex = value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        if plausible_length && hex {
            Ok(Self(value))
        } else {
            Err(GitError::InvalidOid { value })
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The abbreviation git itself prints in messages.
    pub fn short(&self) -> &str {
        &self.0[..7]
    }

    /// Whether this is the all-zeros id git uses for "no such object", which is
    /// what a fetch prints as the old value of a newly created ref.
    pub fn is_null(&self) -> bool {
        self.0.bytes().all(|b| b == b'0')
    }
}

impl fmt::Display for Oid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rejection(name: &str) -> NameRejection {
        match BranchName::new(name) {
            Err(GitError::InvalidBranchName { reason, .. }) => reason,
            other => panic!("expected `{name}` to be rejected, got {other:?}"),
        }
    }

    /// The whole reason this type exists: git reads a leading `-` as an option
    /// wherever it appears, so a branch named after one is an argument
    /// injection waiting for a command that lacks `--end-of-options`.
    #[test]
    fn a_branch_named_like_an_option_is_rejected() {
        assert_eq!(rejection("--upload-pack=evil"), NameRejection::LeadingDash);
        assert_eq!(rejection("-f"), NameRejection::LeadingDash);
        assert_eq!(rejection("--"), NameRejection::LeadingDash);
    }

    #[test]
    fn revision_syntax_is_not_a_branch_name() {
        assert_eq!(rejection("a..b"), NameRejection::DoubleDot);
        assert_eq!(rejection("main@{upstream}"), NameRejection::ReflogSyntax);
        assert_eq!(rejection("main^"), NameRejection::Forbidden('^'));
        assert_eq!(rejection("main~1"), NameRejection::Forbidden('~'));
        assert_eq!(rejection("main:other"), NameRejection::Forbidden(':'));
        assert_eq!(rejection("@"), NameRejection::JustAt);
    }

    #[test]
    fn glob_and_path_characters_are_rejected() {
        assert_eq!(rejection("feat/*"), NameRejection::Forbidden('*'));
        assert_eq!(rejection("feat/[a]"), NameRejection::Forbidden('['));
        assert_eq!(rejection("feat/?"), NameRejection::Forbidden('?'));
        assert_eq!(rejection("feat\\x"), NameRejection::Forbidden('\\'));
    }

    /// A name with a newline or a NUL would split a `-z` record or a line of
    /// porcelain output into two, which is worse than being rejected.
    #[test]
    fn whitespace_and_control_characters_are_rejected() {
        assert_eq!(rejection("my branch"), NameRejection::Space);
        assert_eq!(rejection("my\tbranch"), NameRejection::ControlCharacter);
        assert_eq!(rejection("my\nbranch"), NameRejection::ControlCharacter);
        assert_eq!(rejection("my\0branch"), NameRejection::ControlCharacter);
        assert_eq!(rejection("my\u{7f}branch"), NameRejection::ControlCharacter);
    }

    #[test]
    fn structurally_broken_paths_are_rejected() {
        assert_eq!(rejection(""), NameRejection::Empty);
        assert_eq!(rejection("/main"), NameRejection::EmptyComponent);
        assert_eq!(rejection("main/"), NameRejection::EmptyComponent);
        assert_eq!(rejection("feat//x"), NameRejection::EmptyComponent);
        assert_eq!(rejection("main."), NameRejection::TrailingDot);
        assert_eq!(
            rejection("feat/.hidden"),
            NameRejection::ComponentStartsWithDot
        );
        assert_eq!(rejection("feat/x.lock"), NameRejection::LockSuffix);
        assert_eq!(rejection("main.lock"), NameRejection::LockSuffix);
    }

    /// Ordinary names, including the ones real repositories use, must survive.
    #[test]
    fn realistic_branch_names_are_accepted() {
        for name in [
            "main",
            "feat/branch-divergence",
            "user/123-fix_thing",
            "release/v1.2.3",
            "dependabot/cargo/serde-1.0.228",
            "ünïcode",
            "a",
        ] {
            assert!(BranchName::new(name).is_ok(), "rejected `{name}`");
        }
    }

    #[test]
    fn a_remote_may_not_contain_a_slash() {
        let Err(GitError::InvalidBranchName { reason, .. }) = Remote::new("up/stream") else {
            panic!("expected a slash in a remote name to be rejected");
        };
        assert_eq!(reason, NameRejection::SlashInRemote);
        assert!(Remote::new("origin").is_ok());
        assert!(Remote::new("-origin").is_err());
    }

    /// The refspec is the single most consequential string in the crate: the
    /// destination is what makes the tracking ref move, and the `+` is what
    /// lets it move backwards after a force-push.
    #[test]
    fn a_remote_ref_names_its_tracking_ref_and_refspec() {
        let remote_ref = RemoteRef::origin(BranchName::new("feat/x").expect("valid"));
        assert_eq!(remote_ref.tracking_ref(), "refs/remotes/origin/feat/x");
        assert_eq!(
            remote_ref.fetch_refspec(),
            "+refs/heads/feat/x:refs/remotes/origin/feat/x"
        );
        assert_eq!(remote_ref.to_string(), "origin/feat/x");
    }

    /// A bare name would resolve `refs/tags/` first, so every rev this crate
    /// passes is qualified.
    #[test]
    fn every_rev_renders_a_fully_qualified_ref() {
        let branch = BranchName::new("main").expect("valid");
        assert_eq!(Rev::Head.as_arg(), "HEAD");
        assert_eq!(Rev::Local(branch.clone()).as_arg(), "refs/heads/main");
        assert_eq!(
            Rev::Remote(RemoteRef::origin(branch)).as_arg(),
            "refs/remotes/origin/main"
        );
    }

    #[test]
    fn object_ids_must_be_lowercase_hex_of_a_known_width() {
        let sha1 = "b87d11011148b979094156442b1e1d8d9dbed5ff";
        let sha256 = "a".repeat(64);
        assert_eq!(Oid::parse(sha1).expect("valid").as_str(), sha1);
        assert!(Oid::parse(&sha256).is_ok());

        for bad in [
            "",
            "b87d110",
            "B87D11011148B979094156442B1E1D8D9DBED5FF",
            "g87d11011148b979094156442b1e1d8d9dbed5ff",
            "(initial)",
            &"a".repeat(41),
        ] {
            assert!(Oid::parse(bad).is_err(), "accepted `{bad}`");
        }
    }

    /// A fetch prints the null id as the old value of a ref it just created.
    #[test]
    fn the_null_id_is_recognised_at_both_widths() {
        assert!(Oid::parse("0".repeat(40)).expect("valid").is_null());
        assert!(Oid::parse("0".repeat(64)).expect("valid").is_null());
        assert!(
            !Oid::parse("b87d11011148b979094156442b1e1d8d9dbed5ff")
                .expect("valid")
                .is_null()
        );
    }

    #[test]
    fn short_ids_match_gits_own_abbreviation() {
        let oid = Oid::parse("b87d11011148b979094156442b1e1d8d9dbed5ff").expect("valid");
        assert_eq!(oid.short(), "b87d110");
    }
}
