//! Issues as the phone shows them: a feed row, the detail screen, and what
//! the actions take.

use std::time::SystemTime;

use crate::{
    detail::TimelineEntry,
    types::{Chip, LabelView, UserRef},
};

/// Why an issue was closed, as GitHub records it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum IssueCloseReason {
    Completed,
    NotPlanned,
    /// Set by marking a duplicate on GitHub; never chosen here.
    Duplicate,
}

/// Open, or closed with the reason GitHub recorded (none for issues closed
/// before GitHub kept reasons).
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum IssueStatus {
    Open,
    Closed { reason: Option<IssueCloseReason> },
}

/// How the phone closes an issue — the two reasons a person can choose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum CloseIssueAs {
    Completed,
    NotPlanned,
}

/// Everything an issue row shows.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct IssueSummary {
    /// `owner/name`.
    pub repo: String,
    pub number: u32,
    pub title: String,
    pub url: String,
    pub status: IssueStatus,
    /// `open`, `closed`, `closed: not planned`, `closed: duplicate`.
    pub status_chip: Chip,
    pub author: Option<UserRef>,
    pub created_at: SystemTime,
    pub updated_at: SystemTime,
    pub labels: Vec<LabelView>,
    pub assignees: Vec<UserRef>,
    pub comment_count: u32,
    pub milestone: Option<String>,
    /// You opened it.
    pub is_yours: bool,
    /// It is assigned to you.
    pub assigned_to_you: bool,
}

/// The issue screen: the issue as of the detail query (so a just-closed issue
/// still shows, with Reopen), and its timeline — description first, then
/// comments and events, oldest first, markdown flattened.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct IssueDetail {
    pub issue: IssueSummary,
    pub timeline: Vec<TimelineEntry>,
}
