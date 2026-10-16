//! What each part of the app has spent, for the periodic log line.

/// The parts of the app that spend budget, as the tally names them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Feature {
    /// Open pull requests, on the poll.
    Feed,
    /// Open issues.
    Issues,
    /// CI checks, on the poll and the CI view's watch.
    Ci,
    /// The Closed tab's lists.
    Closed,
    /// Distance from base, after each refresh.
    Divergence,
    /// Re-queries chasing merge states GitHub is still computing.
    Probes,
    /// GitHub's stacks (REST).
    Stacks,
    /// Everything on demand: the detail panes, the repository view, logs,
    /// mutations.
    Detail,
}

impl Feature {
    pub const ALL: [Feature; 8] = [
        Self::Feed,
        Self::Issues,
        Self::Ci,
        Self::Closed,
        Self::Divergence,
        Self::Probes,
        Self::Stacks,
        Self::Detail,
    ];

    fn index(self) -> usize {
        self as usize
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Feed => "feed",
            Self::Issues => "issues",
            Self::Ci => "ci",
            Self::Closed => "closed",
            Self::Divergence => "divergence",
            Self::Probes => "probes",
            Self::Stacks => "stacks",
            Self::Detail => "detail",
        }
    }
}

/// GraphQL points and requests, and REST requests, per [`Feature`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SpendTally {
    points: [u64; Feature::ALL.len()],
    graphql_requests: [u64; Feature::ALL.len()],
    rest_requests: [u64; Feature::ALL.len()],
}

impl SpendTally {
    /// One GraphQL request that cost `points`.
    pub fn record_graphql(&mut self, feature: Feature, points: u32) {
        self.points[feature.index()] += u64::from(points);
        self.graphql_requests[feature.index()] += 1;
    }

    /// One REST request.
    pub fn record_rest(&mut self, feature: Feature) {
        self.rest_requests[feature.index()] += 1;
    }

    pub fn points(&self, feature: Feature) -> u64 {
        self.points[feature.index()]
    }

    pub fn graphql_requests(&self, feature: Feature) -> u64 {
        self.graphql_requests[feature.index()]
    }

    pub fn rest_requests(&self, feature: Feature) -> u64 {
        self.rest_requests[feature.index()]
    }

    pub fn total_points(&self) -> u64 {
        self.points.iter().sum()
    }

    pub fn total_graphql_requests(&self) -> u64 {
        self.graphql_requests.iter().sum()
    }

    pub fn total_rest_requests(&self) -> u64 {
        self.rest_requests.iter().sum()
    }

    pub fn is_empty(&self) -> bool {
        self.total_graphql_requests() == 0 && self.total_rest_requests() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn features_are_tallied_apart() {
        let mut tally = SpendTally::default();
        tally.record_graphql(Feature::Feed, 5);
        tally.record_graphql(Feature::Feed, 1);
        tally.record_graphql(Feature::Ci, 3);
        tally.record_rest(Feature::Stacks);
        assert_eq!(tally.points(Feature::Feed), 6);
        assert_eq!(tally.graphql_requests(Feature::Feed), 2);
        assert_eq!(tally.points(Feature::Ci), 3);
        assert_eq!(tally.points(Feature::Stacks), 0);
        assert_eq!(tally.rest_requests(Feature::Stacks), 1);
        assert_eq!(tally.total_points(), 9);
        assert_eq!(tally.total_graphql_requests(), 3);
        assert!(!tally.is_empty());
    }

    #[test]
    fn every_feature_has_its_own_slot_and_label() {
        let labels: std::collections::BTreeSet<_> =
            Feature::ALL.iter().map(|feature| feature.label()).collect();
        assert_eq!(labels.len(), Feature::ALL.len());
        for (index, feature) in Feature::ALL.iter().enumerate() {
            assert_eq!(feature.index(), index);
        }
    }
}
