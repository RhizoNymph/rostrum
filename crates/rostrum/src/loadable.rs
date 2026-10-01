//! An asynchronously fetched value with an explicit failure state.

/// Async resource with an explicit failure state, so the UI can tell "still
/// loading" from "loaded and empty" from "failed".
pub enum Loadable<T> {
    Idle,
    Loading,
    Loaded(T),
    Failed(String),
}

impl<T> Loadable<T> {
    pub fn loaded(&self) -> Option<&T> {
        match self {
            Self::Loaded(value) => Some(value),
            _ => None,
        }
    }

    pub fn is_idle(&self) -> bool {
        matches!(self, Self::Idle)
    }
}
