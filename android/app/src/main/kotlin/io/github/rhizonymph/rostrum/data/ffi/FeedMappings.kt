package io.github.rhizonymph.rostrum.data.ffi

import io.github.rhizonymph.rostrum.data.model.AuthorChip
import io.github.rhizonymph.rostrum.data.model.AuthorRoster
import io.github.rhizonymph.rostrum.data.model.BaseDivergence
import io.github.rhizonymph.rostrum.data.model.FeedPreferences
import io.github.rhizonymph.rostrum.data.model.FeedSnapshot
import io.github.rhizonymph.rostrum.data.model.GitHubStatus
import io.github.rhizonymph.rostrum.data.model.NotificationEvent
import io.github.rhizonymph.rostrum.data.model.NotificationKind
import io.github.rhizonymph.rostrum.data.model.PrSummary
import io.github.rhizonymph.rostrum.data.model.RepoBody
import io.github.rhizonymph.rostrum.data.model.RepoLoad
import io.github.rhizonymph.rostrum.data.model.RepoSection
import io.github.rhizonymph.rostrum.data.model.Settings
import uniffi.rostrum_ffi.AuthorChip as FfiAuthorChip
import uniffi.rostrum_ffi.AuthorRoster as FfiAuthorRoster
import uniffi.rostrum_ffi.BaseDivergence as FfiBaseDivergence
import uniffi.rostrum_ffi.FeedPreferences as FfiFeedPreferences
import uniffi.rostrum_ffi.FeedSnapshot as FfiFeedSnapshot
import uniffi.rostrum_ffi.GitHubStatus as FfiGitHubStatus
import uniffi.rostrum_ffi.NotificationEvent as FfiNotificationEvent
import uniffi.rostrum_ffi.NotificationKind as FfiNotificationKind
import uniffi.rostrum_ffi.PrSummary as FfiPrSummary
import uniffi.rostrum_ffi.RepoBody as FfiRepoBody
import uniffi.rostrum_ffi.RepoLoad as FfiRepoLoad
import uniffi.rostrum_ffi.RepoSection as FfiRepoSection
import uniffi.rostrum_ffi.Settings as FfiSettings

/* Session, settings, feed and notifications: generated records → model. */

internal fun FfiGitHubStatus.toModel(): GitHubStatus = when (this) {
    is FfiGitHubStatus.NoToken -> GitHubStatus.NoToken
    is FfiGitHubStatus.Unverified -> GitHubStatus.Unverified
    is FfiGitHubStatus.Verified -> GitHubStatus.Verified(viewer.toModel())
    is FfiGitHubStatus.Invalid -> GitHubStatus.Invalid(reason)
}

internal fun FfiFeedPreferences.toModel() = FeedPreferences(
    hideDrafts = hideDrafts,
    hideEmptyRepos = hideEmptyRepos,
    authors = authors,
    includeInvolved = includeInvolved,
)

internal fun FeedPreferences.toFfi() = FfiFeedPreferences(
    hideDrafts = hideDrafts,
    hideEmptyRepos = hideEmptyRepos,
    authors = authors,
    includeInvolved = includeInvolved,
)

internal fun FfiSettings.toModel() = Settings(
    repos = repos,
    refreshIntervalSecs = refreshIntervalSecs.toLong(),
    prsPerRepo = prsPerRepo.toInt(),
    notifyNewPullRequests = notifyNewPullRequests,
    notifyReviewRequests = notifyReviewRequests,
    autostash = autostash,
    feed = feed.toModel(),
)

internal fun FfiRepoLoad.toModel(): RepoLoad = when (this) {
    is FfiRepoLoad.Idle -> RepoLoad.Idle
    is FfiRepoLoad.Loading -> RepoLoad.Loading
    is FfiRepoLoad.Loaded -> RepoLoad.Loaded(at)
    is FfiRepoLoad.Failed -> RepoLoad.Failed(reason, at)
}

internal fun FfiRepoBody.toModel(): RepoBody = when (this) {
    is FfiRepoBody.Collapsed -> RepoBody.Collapsed
    is FfiRepoBody.Loading -> RepoBody.Loading
    is FfiRepoBody.Failed -> RepoBody.Failed(reason)
    is FfiRepoBody.Empty -> RepoBody.Empty
    is FfiRepoBody.Pulls -> RepoBody.Pulls(pulls.map { it.toModel() })
}

internal fun FfiBaseDivergence.toModel() = BaseDivergence(
    behind = behind.toInt(),
    ahead = ahead.toInt(),
    baseRef = baseRef,
    fastForwards = fastForwards,
    summary = summary,
)

internal fun FfiPrSummary.toModel() = PrSummary(
    repo = repo,
    number = number.toInt(),
    title = title,
    url = url,
    author = author?.toModel(),
    createdAt = createdAt,
    updatedAt = updatedAt,
    isDraft = isDraft,
    checks = checks?.toModel(),
    checksRole = checksRole.toModel(),
    reviewDecision = reviewDecision?.toModel(),
    reviewChip = reviewChip?.toModel(),
    mergeStatus = mergeStatus.toModel(),
    mergeChip = mergeChip?.toModel(),
    baseDivergence = baseDivergence?.toModel(),
    behindChip = behindChip?.toModel(),
    labels = labels.map { it.toModel() },
    additions = additions.toInt(),
    deletions = deletions.toInt(),
    changedFiles = changedFiles.toInt(),
    commentCount = commentCount.toInt(),
    reviewRequested = reviewRequested,
    isYours = isYours,
    headRef = headRef,
    baseRef = baseRef,
)

internal fun FfiRepoSection.toModel() = RepoSection(
    repo = repo,
    load = load.toModel(),
    openCount = openCount.toInt(),
    visibleCount = visibleCount.toInt(),
    collapsed = collapsed,
    body = body.toModel(),
)

internal fun FfiFeedSnapshot.toModel() = FeedSnapshot(
    revision = revision.toLong(),
    repos = repos.map { it.toModel() },
    hiddenEmptyRepos = hiddenEmptyRepos.toInt(),
    totalOpen = totalOpen.toInt(),
    visibleOpen = visibleOpen.toInt(),
    query = query,
    preferences = preferences.toModel(),
    filterActive = filterActive,
    mergeStatesSettling = mergeStatesSettling,
    viewer = viewer?.toModel(),
)

internal fun FfiAuthorChip.toModel() = AuthorChip(login, avatarUrl, openPrs.toInt(), isViewer, selected)

internal fun FfiAuthorRoster.toModel() = AuthorRoster(authors.map { it.toModel() }, hidden.toInt())

internal fun FfiNotificationKind.toModel(): NotificationKind = when (this) {
    FfiNotificationKind.NEW_PULL_REQUEST -> NotificationKind.NewPullRequest
    FfiNotificationKind.REVIEW_REQUESTED -> NotificationKind.ReviewRequested
}

internal fun FfiNotificationEvent.toModel() = NotificationEvent(
    kind = kind.toModel(),
    repo = repo,
    number = number.toInt(),
    title = title,
    author = author,
    url = url,
)
