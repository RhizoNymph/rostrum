package io.github.rhizonymph.rostrum.data.fake

/**
 * Every fallible [io.github.rhizonymph.rostrum.data.RostrumBackend] call, so a
 * test can make exactly one of them fail with
 * [FakeRostrumBackend.failNext].
 */
enum class FakeCall {
    SetGitHubToken, Viewer,
    Settings, AddRepo, RemoveRepo, SetRefreshInterval, SetPrsPerRepo, SetNotifications, SetAutostash,
    CachedFeed, RefreshFeed, RefreshRepo, SetQuery, SetFilter, ToggleAuthor, ClearFilter, ToggleCollapsed, AuthorRoster,
    PullDetail, CachedPullDetail, PullHeader, RepositoryLabels, AddLabel, RemoveLabel, AddComment, ReplyToThread,
    Merge, ClosePullRequest, ReopenPullRequest, SetDraft, UpdateBranch,
    FilesOverview, FileDiff,
    PendingReview, AddDraft, EditDraft, RemoveDraft, DiscardDrafts, SubmitReview,
    ParsePairingLink, PairWithLink, ProbeDesktop, PairManual, SetRemote, ClearRemote, RemoteStatus, MachineInfo,
    LocalStatus, RunLocalJob, AbortLocal, StartSyncAll, SyncAllStatus, Handoffs, RefreshGitHubTokenFromDesktop, Unpair,
    DesktopConfig, CopyDesktopConfig,
    CheckNotifications, MarkNotificationsSeen,
    SetFeedTab, SortSettings, SetRepoSort, SetItemSort,
    IssueDetail, CachedIssueDetail, CommentOnIssue, CloseIssue, ReopenIssue, AddIssueLabel, RemoveIssueLabel,
    AssignableUsers, AddIssueAssignee, RemoveIssueAssignee, CreateIssue, LoadEarlierIssue, EditIssue, LoadEarlierPull,
    PlanStackRewrite, CheckStackPlan, StackCandidates, MakeStack, ArrangeStack, ExtendStack, MergeStack, Unstack, StackJob,
    RepoOverview, BranchTree, Trunks, SetTrunks,
}
