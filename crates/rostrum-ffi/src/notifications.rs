//! Background notifications: what is new since the last check.

use crate::{engine::RostrumCore, error::RostrumError};

/// Something worth a system notification.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct NotificationEvent {
    pub kind: NotificationKind,
    /// `owner/name`.
    pub repo: String,
    pub number: u32,
    pub title: String,
    pub author: Option<String>,
    pub url: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum NotificationKind {
    /// A pull request appeared in a watched repository.
    NewPullRequest,
    /// Your review was requested on a pull request.
    ReviewRequested,
}

#[uniffi::export(async_runtime = "tokio")]
impl RostrumCore {
    /// For the WorkManager job: refresh the feed and report pull requests
    /// that appeared, and review requests that arrived, since the last check.
    /// The seen set is persisted, so the first check (and a newly watched
    /// repository's first) establishes a baseline and reports nothing. Events
    /// are filtered by the notification settings; your own pull requests are
    /// never reported, and a pull request is reported at most once per
    /// check, as a review request when it is both.
    pub async fn check_notifications(&self) -> Result<Vec<NotificationEvent>, RostrumError> {
        Err(RostrumError::unimplemented("check_notifications"))
    }

    /// Fold the feed as it stands into the seen set without reporting
    /// anything — call when the user has looked at the feed, so the next
    /// check does not announce what they already saw.
    pub async fn mark_notifications_seen(&self) -> Result<(), RostrumError> {
        Err(RostrumError::unimplemented("mark_notifications_seen"))
    }
}
