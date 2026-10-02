//! The issues half of the repository view's sidebar: what its header counts
//! and what it says when it has no rows.
//!
//! Reads `RepoState::issues` and `RepoState::issues_load`, which the store's
//! poll cycle fills for every watched repository. The rows themselves are the
//! feed's (`feed::issue_row_content`).

use rostrum_core::LoadState;

/// What to say in place of the list when it has no rows.
pub fn empty_message(load: &LoadState) -> String {
    match load {
        LoadState::Idle => "Issues are not loaded yet".to_string(),
        LoadState::Loading => "Loading…".to_string(),
        LoadState::Failed { message, .. } => message.clone(),
        LoadState::Loaded { .. } => "No open issues".to_string(),
    }
}

/// The header count: known once the list has loaded at least once.
pub fn header_count(load: &LoadState, len: usize) -> Option<usize> {
    match load {
        LoadState::Idle | LoadState::Loading if len == 0 => None,
        _ => Some(len),
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;

    #[test]
    fn the_count_is_unknown_until_the_list_has_loaded() {
        assert_eq!(header_count(&LoadState::Idle, 0), None);
        assert_eq!(header_count(&LoadState::Loading, 0), None);
        assert_eq!(
            header_count(&LoadState::Loaded { at: Utc::now() }, 0),
            Some(0)
        );
        // A background refresh of a loaded list keeps showing its count.
        assert_eq!(header_count(&LoadState::Loading, 4), Some(4));
    }

    #[test]
    fn an_empty_list_explains_itself_by_load_state() {
        assert_eq!(empty_message(&LoadState::Idle), "Issues are not loaded yet");
        assert_eq!(
            empty_message(&LoadState::Loaded { at: Utc::now() }),
            "No open issues"
        );
        assert_eq!(
            empty_message(&LoadState::Failed {
                message: "boom".into(),
                at: Utc::now()
            }),
            "boom"
        );
    }
}
