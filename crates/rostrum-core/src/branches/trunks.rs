//! Which branches of a repository are trunks, and what GitHub says about them.

use std::collections::BTreeSet;

use super::name::TrunkName;

/// The branch names tried when a repository has no trunks configured, in the
/// order they are listed after the default branch.
pub const DEFAULT_TRUNK_CANDIDATES: [&str; 4] = ["main", "master", "staging", "develop"];

/// [`DEFAULT_TRUNK_CANDIDATES`] as names.
pub fn default_candidates() -> Vec<TrunkName> {
    DEFAULT_TRUNK_CANDIDATES
        .iter()
        .map(|raw| TrunkName::parse(raw).expect("the built-in candidates are valid names"))
        .collect()
}

/// How a repository's trunks are chosen.
///
/// `Configured(vec![])` is meaningful and distinct from `Detected`: it says
/// "the default branch alone", where `Detected` says "whichever of the usual
/// names exist". The config file expresses the difference as an empty array
/// versus no entry at all.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum TrunkChoice {
    #[default]
    Detected,
    Configured(Vec<TrunkName>),
}

impl TrunkChoice {
    /// Every branch name worth asking GitHub about: the configured ones in
    /// order, then the built-in candidates, without repeats.
    ///
    /// The candidates are asked for even when trunks are configured, so the
    /// editor can offer the ones that exist without a second request.
    pub fn names_to_probe(&self) -> Vec<TrunkName> {
        let mut names = Vec::new();
        let configured = match self {
            Self::Detected => &[][..],
            Self::Configured(names) => names.as_slice(),
        };
        for name in configured.iter().cloned().chain(default_candidates()) {
            if !names.contains(&name) {
                names.push(name);
            }
        }
        names
    }

    /// The explicit list this choice amounts to. `detected` is what
    /// detection found besides the default branch, which is what `Detected`
    /// means until the user edits it.
    pub fn explicit(&self, detected: &[TrunkName]) -> Vec<TrunkName> {
        match self {
            Self::Detected => detected.to_vec(),
            Self::Configured(names) => names.clone(),
        }
    }

    /// This choice with `name` appended, unless it is already there.
    ///
    /// Editing a detected choice turns it into a configured one starting
    /// from what was detected, so adding `qa` to an auto-detected
    /// `main, staging` gives `staging, qa` rather than `qa` alone.
    pub fn adding(&self, detected: &[TrunkName], name: TrunkName) -> Self {
        let mut names = self.explicit(detected);
        if !names.contains(&name) {
            names.push(name);
        }
        Self::Configured(names)
    }

    /// This choice without `name`. Like [`Self::adding`], a detected choice
    /// becomes the configured list it amounted to, less the name.
    pub fn removing(&self, detected: &[TrunkName], name: &TrunkName) -> Self {
        let mut names = self.explicit(detected);
        names.retain(|existing| existing != name);
        Self::Configured(names)
    }
}

/// What GitHub reports about a repository for the branch view: its page,
/// its stars, its default branch, and which of the probed names exist.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepoMeta {
    pub url: String,
    pub stars: u32,
    /// `None` for a repository with no commits yet, which has no default
    /// branch for anything to be compared against.
    pub default_branch: Option<TrunkName>,
    /// The subset of the probed names that exist as branches.
    pub existing: BTreeSet<TrunkName>,
}

/// A trunk other than the default branch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OtherTrunk {
    pub name: TrunkName,
    /// A configured trunk that GitHub does not have — renamed, deleted, or
    /// mistyped. Kept in the list so the mistake is visible, and never
    /// compared, since there is nothing to compare.
    pub exists: bool,
}

/// A repository's trunks, the default branch first.
///
/// The default branch is held apart from the rest so it can be neither
/// missing nor listed twice: every trunk's distance is measured from it, and
/// a tree whose reference point could be absent would have nothing to say.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Trunks {
    default: TrunkName,
    others: Vec<OtherTrunk>,
}

impl Trunks {
    /// Decide the trunks from the user's choice and what GitHub has.
    ///
    /// The default branch always comes first, whether or not it was
    /// configured. With nothing configured the others are whichever of
    /// [`DEFAULT_TRUNK_CANDIDATES`] exist, in that order; configured, they
    /// are the configured names in the configured order, existing or not.
    /// Repeats and the default branch itself are dropped from the others.
    pub fn resolve(
        default: TrunkName,
        choice: &TrunkChoice,
        existing: &BTreeSet<TrunkName>,
    ) -> Self {
        let wanted: Vec<OtherTrunk> = match choice {
            TrunkChoice::Detected => default_candidates()
                .into_iter()
                .filter(|name| existing.contains(name))
                .map(|name| OtherTrunk { name, exists: true })
                .collect(),
            TrunkChoice::Configured(names) => names
                .iter()
                .map(|name| OtherTrunk {
                    exists: existing.contains(name),
                    name: name.clone(),
                })
                .collect(),
        };

        let mut others: Vec<OtherTrunk> = Vec::with_capacity(wanted.len());
        for trunk in wanted {
            if trunk.name != default && !others.iter().any(|seen| seen.name == trunk.name) {
                others.push(trunk);
            }
        }
        Self { default, others }
    }

    pub fn default_branch(&self) -> &TrunkName {
        &self.default
    }

    pub fn others(&self) -> &[OtherTrunk] {
        &self.others
    }

    /// Every trunk name, the default branch first.
    pub fn names(&self) -> impl Iterator<Item = &TrunkName> {
        std::iter::once(&self.default).chain(self.others.iter().map(|other| &other.name))
    }

    /// Whether `branch` is one of the trunks.
    pub fn contains(&self, branch: &str) -> bool {
        self.names().any(|name| name.as_str() == branch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(raw: &str) -> TrunkName {
        TrunkName::parse(raw).expect("valid test name")
    }

    fn set(raws: &[&str]) -> BTreeSet<TrunkName> {
        raws.iter().map(|raw| name(raw)).collect()
    }

    fn other_names(trunks: &Trunks) -> Vec<(&str, bool)> {
        trunks
            .others()
            .iter()
            .map(|other| (other.name.as_str(), other.exists))
            .collect()
    }

    #[test]
    fn detection_keeps_the_candidates_that_exist_in_candidate_order() {
        let trunks = Trunks::resolve(
            name("main"),
            &TrunkChoice::Detected,
            &set(&["develop", "staging", "main", "feature"]),
        );
        assert_eq!(trunks.default_branch().as_str(), "main");
        assert_eq!(
            other_names(&trunks),
            vec![("staging", true), ("develop", true)]
        );
    }

    /// The default branch leads even when it is not one of the usual names.
    #[test]
    fn an_unusual_default_branch_still_comes_first() {
        let trunks = Trunks::resolve(
            name("trunk"),
            &TrunkChoice::Detected,
            &set(&["main", "master"]),
        );
        let names: Vec<&str> = trunks.names().map(TrunkName::as_str).collect();
        assert_eq!(names, ["trunk", "main", "master"]);
    }

    #[test]
    fn detection_with_no_candidates_leaves_the_default_alone() {
        let trunks = Trunks::resolve(name("main"), &TrunkChoice::Detected, &set(&["main"]));
        assert!(trunks.others().is_empty());
        assert_eq!(trunks.names().count(), 1);
    }

    #[test]
    fn configured_trunks_keep_their_order_and_report_missing_ones() {
        let trunks = Trunks::resolve(
            name("main"),
            &TrunkChoice::Configured(vec![name("staging"), name("qa"), name("develop")]),
            &set(&["main", "develop", "staging"]),
        );
        assert_eq!(
            other_names(&trunks),
            vec![("staging", true), ("qa", false), ("develop", true)]
        );
    }

    /// Listing the default branch, or a name twice, must not produce a
    /// second row for it.
    #[test]
    fn configured_repeats_and_the_default_itself_are_dropped() {
        let trunks = Trunks::resolve(
            name("main"),
            &TrunkChoice::Configured(vec![
                name("staging"),
                name("main"),
                name("staging"),
                name("develop"),
            ]),
            &set(&["main", "staging", "develop"]),
        );
        let names: Vec<&str> = trunks.names().map(TrunkName::as_str).collect();
        assert_eq!(names, ["main", "staging", "develop"]);
    }

    /// An empty configured list is "the default branch only", not "detect".
    #[test]
    fn an_empty_configuration_means_the_default_branch_only() {
        let trunks = Trunks::resolve(
            name("main"),
            &TrunkChoice::Configured(Vec::new()),
            &set(&["main", "master", "staging", "develop"]),
        );
        assert!(trunks.others().is_empty());
    }

    #[test]
    fn contains_matches_the_default_and_the_others() {
        let trunks = Trunks::resolve(
            name("main"),
            &TrunkChoice::Configured(vec![name("staging")]),
            &set(&["staging"]),
        );
        assert!(trunks.contains("main"));
        assert!(trunks.contains("staging"));
        assert!(!trunks.contains("develop"));
    }

    #[test]
    fn adding_to_a_detected_choice_starts_from_what_was_detected() {
        let detected = [name("staging")];
        assert_eq!(
            TrunkChoice::Detected.adding(&detected, name("qa")),
            TrunkChoice::Configured(vec![name("staging"), name("qa")])
        );
    }

    #[test]
    fn adding_a_name_already_listed_changes_nothing_but_pins_the_list() {
        let choice = TrunkChoice::Configured(vec![name("qa")]);
        assert_eq!(choice.adding(&[], name("qa")), choice);
        assert_eq!(
            TrunkChoice::Detected.adding(&[name("qa")], name("qa")),
            TrunkChoice::Configured(vec![name("qa")])
        );
    }

    #[test]
    fn removing_keeps_the_rest_in_order() {
        let choice = TrunkChoice::Configured(vec![name("a"), name("b"), name("c")]);
        assert_eq!(
            choice.removing(&[], &name("b")),
            TrunkChoice::Configured(vec![name("a"), name("c")])
        );
    }

    /// Removing the last detected trunk leaves "default branch only", not
    /// detection — which would bring it straight back.
    #[test]
    fn removing_the_last_detected_trunk_pins_an_empty_list() {
        assert_eq!(
            TrunkChoice::Detected.removing(&[name("staging")], &name("staging")),
            TrunkChoice::Configured(Vec::new())
        );
    }

    #[test]
    fn probing_asks_for_configured_names_then_the_candidates_once_each() {
        let probe = TrunkChoice::Configured(vec![name("qa"), name("main")]).names_to_probe();
        let names: Vec<&str> = probe.iter().map(TrunkName::as_str).collect();
        assert_eq!(names, ["qa", "main", "master", "staging", "develop"]);

        let detected = TrunkChoice::Detected.names_to_probe();
        let names: Vec<&str> = detected.iter().map(TrunkName::as_str).collect();
        assert_eq!(names, DEFAULT_TRUNK_CANDIDATES);
    }
}
