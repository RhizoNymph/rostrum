//! What a stack *is*, independent of where it was learned.
//!
//! A stack is an ordered chain of pull requests in one repository: the bottom
//! one targets a trunk branch, and each one above it targets the head branch
//! of the one below. GitHub (through `gh stack` and its Stacks API) is the
//! source of truth for stacks that exist; rostrum also *detects* chains that
//! could be made into one. Both are a [`Stack`]; only the first has a
//! [`StackNumber`].

use std::{fmt, num::NonZeroU32};

use serde::{Deserialize, Serialize};

use crate::model::{PrNumber, RepoId};

/// GitHub's repository-scoped stack number, the one its UI shows and its API
/// paths take. Never zero: gh-stack itself uses zero for "no number yet", and
/// carrying that sentinel here would let "unknown" pass for a stack.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct StackNumber(NonZeroU32);

impl StackNumber {
    pub fn new(raw: u32) -> Option<Self> {
        NonZeroU32::new(raw).map(Self)
    }

    pub fn get(self) -> u32 {
        self.0.get()
    }
}

impl fmt::Display for StackNumber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Why a stack value could not be built.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum StackError {
    #[error("a stack needs at least one pull request")]
    NoMembers,
    #[error("{0} appears twice in one stack")]
    DuplicateMember(PrNumber),
    #[error("`{name}` is not a usable branch name: {reason}")]
    InvalidRef { name: String, reason: &'static str },
}

/// A branch name as GitHub reports it: non-empty, no whitespace or control
/// characters, and not starting with `-` (which every command line rostrum
/// builds would read as a flag).
///
/// Deliberately looser than `rostrum_git::BranchName`, which applies git's
/// full `check-ref-format` rules: this type travels through the feed and the
/// cache, and the strict check happens where a name reaches `git`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct RefName(String);

impl RefName {
    pub fn new(raw: impl Into<String>) -> Result<Self, StackError> {
        let name = raw.into();
        let reason = if name.is_empty() {
            Some("it is empty")
        } else if name.starts_with('-') {
            Some("it starts with `-`")
        } else if name.chars().any(|c| c.is_whitespace() || c.is_control()) {
            Some("it contains whitespace or a control character")
        } else {
            None
        };
        match reason {
            Some(reason) => Err(StackError::InvalidRef { name, reason }),
            None => Ok(Self(name)),
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RefName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<String> for RefName {
    type Error = StackError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<RefName> for String {
    fn from(value: RefName) -> Self {
        value.0
    }
}

/// The pull requests of a stack, bottom (nearest the trunk) first.
///
/// Non-empty and duplicate-free by construction, so `bottom()` and `top()`
/// need no `Option` and a stack can never list one pull request at two
/// heights.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "Vec<PrNumber>", into = "Vec<PrNumber>")]
pub struct StackMembers(Vec<PrNumber>);

impl StackMembers {
    pub fn new(members: Vec<PrNumber>) -> Result<Self, StackError> {
        if members.is_empty() {
            return Err(StackError::NoMembers);
        }
        let mut seen = std::collections::BTreeSet::new();
        for member in &members {
            if !seen.insert(*member) {
                return Err(StackError::DuplicateMember(*member));
            }
        }
        Ok(Self(members))
    }

    pub fn bottom(&self) -> PrNumber {
        self.0[0]
    }

    pub fn top(&self) -> PrNumber {
        self.0[self.0.len() - 1]
    }

    pub fn as_slice(&self) -> &[PrNumber] {
        &self.0
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Always false; present because clippy expects it beside `len`.
    pub fn is_empty(&self) -> bool {
        false
    }

    pub fn contains(&self, number: PrNumber) -> bool {
        self.0.contains(&number)
    }

    /// Height of `number` in the stack, zero at the bottom.
    pub fn position(&self, number: PrNumber) -> Option<usize> {
        self.0.iter().position(|member| *member == number)
    }
}

impl TryFrom<Vec<PrNumber>> for StackMembers {
    type Error = StackError;

    fn try_from(value: Vec<PrNumber>) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<StackMembers> for Vec<PrNumber> {
    fn from(value: StackMembers) -> Self {
        value.0
    }
}

/// One stack of pull requests in one repository.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Stack {
    pub repo: RepoId,
    /// `Some` for a stack GitHub knows about; `None` for a chain rostrum
    /// detected from base and head branches, which "Make stack" can turn into
    /// one.
    pub number: Option<StackNumber>,
    /// The branch the bottom pull request targets.
    pub trunk: RefName,
    pub members: StackMembers,
}

impl Stack {
    pub fn is_on_github(&self) -> bool {
        self.number.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(numbers: &[u32]) -> Vec<PrNumber> {
        numbers.iter().copied().map(PrNumber).collect()
    }

    #[test]
    fn a_stack_number_is_never_zero() {
        assert_eq!(StackNumber::new(0), None);
        assert_eq!(StackNumber::new(7).map(StackNumber::get), Some(7));
    }

    #[test]
    fn members_must_be_non_empty_and_unique() {
        assert_eq!(StackMembers::new(vec![]), Err(StackError::NoMembers));
        assert_eq!(
            StackMembers::new(n(&[1, 2, 1])),
            Err(StackError::DuplicateMember(PrNumber(1)))
        );
        let members = StackMembers::new(n(&[4, 9, 2])).expect("valid");
        assert_eq!(members.bottom(), PrNumber(4));
        assert_eq!(members.top(), PrNumber(2));
        assert_eq!(members.position(PrNumber(9)), Some(1));
        assert_eq!(members.position(PrNumber(5)), None);
        assert_eq!(members.len(), 3);
    }

    #[test]
    fn a_single_member_is_both_bottom_and_top() {
        let members = StackMembers::new(n(&[3])).expect("valid");
        assert_eq!(members.bottom(), members.top());
    }

    #[test]
    fn ref_names_reject_what_would_break_a_command_line() {
        assert!(RefName::new("feature/a").is_ok());
        assert!(RefName::new("").is_err());
        assert!(RefName::new("-rf").is_err());
        assert!(RefName::new("has space").is_err());
        assert!(RefName::new("tab\there").is_err());
        assert!(RefName::new("nl\n").is_err());
    }

    #[test]
    fn invariants_survive_a_serde_round_trip_and_bad_payloads_are_rejected() {
        let stack = Stack {
            repo: RepoId::new("o", "r"),
            number: StackNumber::new(12),
            trunk: RefName::new("main").expect("valid"),
            members: StackMembers::new(n(&[1, 2])).expect("valid"),
        };
        let text = serde_json::to_string(&stack).expect("encodes");
        let back: Stack = serde_json::from_str(&text).expect("decodes");
        assert_eq!(back, stack);

        let empty = text.replace("[1,2]", "[]");
        assert!(serde_json::from_str::<Stack>(&empty).is_err());
        let dup = text.replace("[1,2]", "[1,1]");
        assert!(serde_json::from_str::<Stack>(&dup).is_err());
        let zero = text.replace("\"number\":12", "\"number\":0");
        assert!(serde_json::from_str::<Stack>(&zero).is_err());
        let blank_trunk = text.replace("\"trunk\":\"main\"", "\"trunk\":\"\"");
        assert!(serde_json::from_str::<Stack>(&blank_trunk).is_err());
    }
}
