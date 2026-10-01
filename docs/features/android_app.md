# Feature: android_app

The Android app: a Jetpack Compose client for the same pull-request review
workflow as the desktop app, backed by the Rust core through UniFFI. It shows
a feed of open pull requests across the watched repositories, a pull
request's conversation, checks, branch and files, a single-file diff with
inline commenting and pending reviews, merging, and the paired desktop
(`rostrumd`) that runs local git operations on the phone's behalf.

## Scope

- The app's architecture: `RostrumApplication` and its `AppContainer` (manual
  DI), one activity, Navigation Compose with typed routes, ViewModels exposing
  `StateFlow`s of sealed UI states.
- `RostrumBackend`: the Kotlin interface every ViewModel depends on, shaped
  after `RostrumCore` in `rostrum-ffi`, with the domain types it speaks
  (`data/model/`), its typed error (`BackendError`) and result (`Outcome`).
- `FfiRostrumBackend`: the production backend, over the generated
  `uniffi.rostrum_ffi.RostrumCore` (one core per profile, from the core's
  profile registry), with record and error mappings in `data/ffi/`.
- Profiles (one per paired desktop, or per pasted token): see
  `docs/features/android_profiles.md`. Everything below that talks about "the
  backend" or "the session" means the active profile's.
- `FakeRostrumBackend`: an in-memory backend with the mockups' sample data,
  for unit tests and `@Preview`s only; nothing in the production graph uses
  it.
- Secrets at rest: an Android Keystore AES-GCM key sealing each secret into
  an app-private file, per profile; each profile's `SessionRepository`
  restores them into its backend at start-up and persists what sign-in and
  pairing return.
- Deep links (`rostrum://pair?…`, cold and warm start) and notification taps.
- Background notifications: a WorkManager periodic check (15 min, network
  connected) posting to two channels, and the runtime permission request.
- Every screen of the approved mockups, as amended (see *Decisions*).
- Copying the paired desktop's settings (repositories, pull requests per
  repository, feed preferences, stash default) onto the phone: offered right
  after pairing, and from Settings.

## Non-scope

- **GitHub Enterprise.** The core talks to github.com only; sign-in says so,
  and a desktop token for another host is refused.
- **Anything the core does not provide is not shown**: desktop-derived feed
  chips, code excerpts on conversation threads, pushed-commit events,
  worktree counts, conflicted-file lists, handoff descriptions, thread
  resolution, the lines between hunks, required-approval counts, and a
  per-file "viewed" state that survives the screen.
- In-app camera or QR scanning (pairing arrives by deep link, or by hand).
- GitHub's device-code sign-in (pairing hands over the desktop's token; a
  personal access token is the fallback).
- Avatars from the network: avatars are initials on a per-login colour.
- Light theme: the app is dark only.

## Decisions (these override the mockups)

- **Sign in** offers "Pair with your desktop" first (pairing also hands over
  the desktop's GitHub token), then "Use a personal access token instead"
  with a token field only: github.com, and a line saying GitHub Enterprise
  isn't supported yet.
- **Pair** has no camera. A pairing link arrives by deep link, from "Open in
  Rostrum" on the desktop's page (`http://<desktop>:8484/`) or its QR code
  scanned by the phone's camera app. The screen shows the parsed preview
  (machine, addresses, fingerprint `4F2A · 91C0 · 7E3B`) and "Pair
  <machine>". By hand: host, port (default 8485) and code; the app probes the
  host and shows the fingerprint to compare before pairing.
- **Merge** offers merge commit, squash and rebase with commit title and
  message; the sheet's "Confirm merge" is the confirmation. Close and reopen
  confirm with a dialog; the draft toggle does not.
- The pull request screen has four tabs: Conversation, Files, Checks, Branch.
  Files opens on the overview; a file opens a single-file diff with
  previous/next.
- Bottom navigation: Feed, Desktop, Settings.
- **Comment now** (a single inline comment) is enabled only when no other
  drafts are pending, because the core posts inline comments only as a review
  and would send them too; otherwise it is disabled with a one-line reason.

## Architecture

```
MainActivity ──intent──▶ AppLinks.parse ──▶ AppLinkInbox (StateFlow<AppLink?>)
     │ setContent                                   │ drained by
     ▼                                              ▼
RostrumApp(container) ── profiles.state, active session ──▶ key(GraphKey) MainScaffold ── RostrumNavHost
     │                                         │  bottom bar (TopLevel), profile switcher
     │ LocalAppContainer, LocalSnackbarHostState, LocalProfileHandle, per-graph ViewModelStore
     ▼                                         ▼
feature …Route composables ── profileViewModel { c, p -> XViewModel(p.backend, …) }
                                   │ StateFlow<UiState / screen state>
                                   ▼
  ProfileHandle.backend = FfiRostrumBackend ── CoreHandle ── registry.core(id) ── RostrumCore (librostrum_ffi.so)
```

- `RostrumApplication.onCreate` builds the `AppContainer` and calls
  `start()`: create the notification channels, `profiles.start()` (wipe the
  legacy single-profile state, open the registry, restore every profile),
  then keep the WorkManager schedule in step with the profiles.
- `AppContainer` holds `clock`, `appScope`, `deviceName`, `profiles` (the
  `ProfileManager`), `links`, `appMessages`, `notificationPoster` and
  `notificationScheduler`. Nothing reaches for it globally except
  `NotificationWorker`; composables receive it through `LocalAppContainer`.
  `profileViewModel { container, profile -> … }` builds a ViewModel over the
  active profile's `ProfileHandle` (`LocalProfileHandle`);
  `rostrumViewModel { container -> … }` builds one that works across profiles
  (pairing, sign-in, the profile screens). Both are scoped to the navigation
  back-stack entry (with a `key` when one destination holds several).
- Features never import each other. Each exposes one `…Route` entry
  composable taking navigation callbacks; `RostrumNavHost` wires them, and it
  also injects the Files tab (`FilesOverviewTab`) and the review sheet
  (`SubmitReviewSheet`) into the pull request shell as composable slots.

### Session and navigation

`ProfileManager.state` is `Starting`, `Unavailable(error)` or
`Ready(profiles, active)`; the active profile's `SessionRepository.state` is
`Restoring` until its secrets are read, then `Ready(github: GitHubAuth,
desktop: DesktopLink)`. `RostrumApp` shows a splash while either is loading;
afterwards it keys the whole navigation graph on `GraphKey(profile id,
signed in)` (`GraphKey(null, false)` before any profile exists): the start
destination is `Feed` when signed in, `SignIn` otherwise. Switching
profiles, signing in or signing out rebuilds the graph, so no back stack
survives any of them, and `GraphViewModelStores` clears the previous graph's
ViewModels. Signing in, pairing, switching and signing out therefore need no
navigation code in the features.

Routes (`ui/navigation/Destinations.kt`, `@Serializable`): `SignIn`,
`Pair(link: String?)`, `AddTokenProfile`, `Feed`, `Desktop`, `Settings`,
`PullRequest(repo, number, tab: PrTab)`, `FileDiff(repo, number, fileIndex)`.
Bottom-bar switches use `navigateTopLevel` (pop to the start destination,
save and restore state, single top). The Desktop item carries a badge with
the number of handoff sessions waiting (`ShellViewModel`, polled every 60 s
while paired).

### Deep links

`MainActivity` (`singleTop`) reads its intent in `onCreate` (only when
`savedInstanceState == null`, so a rotation does not replay the launch
intent) and in `onNewIntent`. `AppLinks.parse` turns a VIEW of
`rostrum://pair…` into `AppLink.Pair(uri)` and a notification's extras
(`EXTRA_REPO`, `EXTRA_NUMBER`, `EXTRA_PROFILE`) into
`AppLink.OpenPullRequest(pr, profile)`. The link waits in `AppLinkInbox`.
The root routes it first (`routeOf`): another profile's pull request →
switch to that profile (the link stays pending); a removed profile's → drop
it with a snackbar. Then the `MainScaffold` of the right graph — which on a
cold start is after the profiles are restored — navigates (`Pair(link)`, or
the pull request when signed in) and consumes it. A newer link replaces an
unconsumed older one.

## Data flow

### Backend contract

- Every call is `suspend` and main-safe, and returns `Outcome<T>`:
  `Ok(value)` or `Err(BackendError)`. Nothing throws, so ViewModels never
  need a catch; `BackendError` mirrors the core's `RostrumError` variant for
  variant, and `describe()` gives the sentence the UI shows.
  `requiresSignIn` / `requiresPairing` classify errors for recovery actions.
- `feedUpdates: Flow<FeedSnapshot>` is the core's `FeedObserver`, bridged
  into a `MutableSharedFlow` (replay 1, drop-oldest): the observer is
  registered right after the core opens and emits each snapshot the core
  delivers on its own threads. Snapshots carry a `revision`; consumers keep
  the highest.
- `renderMarkdown(source, repo)` is the core's top-level function (the
  composer's Preview); `parsePairingLink` is suspend because it needs the
  core open.
- Pull requests are addressed by `PrRef(repo, number)` (number ≥ 1).
- Secrets are never persisted by the backend. `setGitHubToken` and
  `setRemote` hand them in at start-up; `PairingResult` and
  `refreshGitHubTokenFromDesktop` hand them back for the app to persist.

### Start-up

`ProfileManager.start()` wipes the legacy state, opens the registry, picks
the active profile and runs each profile's `session.restore()`, the active
one first (details in `android_profiles.md`). Per profile:

1. `session.restore()` reads `GitHubToken`; a present token goes to
   `backend.setGitHubToken(token)` → `SignedIn`. The first backend call opens
   the core (see *The core*). An
   unreadable token (Keystore key lost, corrupt file) is deleted and the user
   is signed out with a notice.
2. It reads `DesktopEndpoint` and `DeviceToken`; both present →
   `backend.setRemote` → `DesktopLink.Paired(status)`. Half a pairing is
   deleted.
3. The session becomes `Ready`; the UI leaves the splash.

### Sign-in and pairing

- Token: `signInWithToken(token)` trims it, hands it in, and verifies it with
  `viewer()`. Rejected → the backend's token is cleared and nothing is
  stored. Accepted → the token is sealed, and the session flips to `SignedIn`
  (the root rebuilds the graph at the feed).
- Token on first run: `ProfileManager.createTokenProfile(token)` makes a
  token-only profile, signs its session in as above (a rejected token
  removes the profile again), names it after the login, and the sign-in
  screen switches to it.
- Pairing (`ProfileManager.pairWithLink(uri)` or `pairManual(host, port,
  fingerprint, code)`): the registry pairs into a new profile, or into the
  profile already paired with that desktop, and that profile's core uses the
  desktop as its remote; the profile's session (`adoptPairing`) seals the
  endpoint and device token (if either write fails the remote is cleared and
  the error is `Storage`; a new profile is then removed). If the desktop handed over a GitHub
  token for github.com and the phone is signed out, that token is adopted and
  persisted, which signs the phone in. A token for another host is refused:
  the pairing stands, and the phone stays signed out with a notice.
- `unpair()` asks the desktop first; a desktop that already forgot this
  phone (`NotPaired`/`DeviceRevoked`) counts as success. Other failures keep
  the pairing; `forgetDesktop()` drops it locally regardless.

### Notifications

- `AppContainer.start()` watches the profile list and `signedInProfiles` and
  calls `NotificationScheduler.sync(wanted = profiles.wantsNotifications())`:
  the unique periodic work `rostrum.notifications.check` (15 min,
  `NetworkType.CONNECTED`, `KEEP`) exists exactly when some profile is
  signed in with at least one toggle on. Settings calls
  `onNotificationSettingsChanged()` after a toggle of any profile changes.
- `NotificationWorker` → `NotificationCheck.run()`: start the profiles
  (restoring each), then for every profile: signed out → skipped;
  both toggles off → skipped; otherwise `backend.checkNotifications()` (the
  core filters by the toggles and never reports your own pull requests; the
  first check is a baseline) and post each event. The result is per profile
  (`ProfileCheck`); the work retries when any profile hit a network or
  rate-limit error, and otherwise succeeds (sign-in problems and bugs give up
  on that profile until the next period).
- `NotificationContent.of(event, profile)` decides channel, stable id (per
  profile and pull request), title (prefixed with the profile's label:
  "nymph-desk · ada-lin asked for your review") and text. Channels:
  `new_pull_requests` "New pull requests" (low importance) and
  `review_requests` "Review requests". `NotificationPoster` checks the
  permission and the app-level switch, and the tap opens the app's launch
  intent with the extras `NotificationContent.tapExtras` gives (repository,
  number, profile id).
- The permission is requested once per install, the first time the signed-in
  feed is on screen (`RequestNotificationPermissionOnce`); Settings asks
  again when a toggle is turned on without it.

### The core

- Each profile's `FfiRostrumBackend` has a `CoreHandle` that gets the
  profile's `RostrumCore` from the registry (`registry.core(id)`, cached
  there) on the backend's first call, on `Dispatchers.IO` (loading JNA and
  `librostrum_ffi.so` happens there), under `files/rostrum/profiles/<id>` (the core's
  `config.json` and `cache.db`). The host tests open a standalone core over a
  directory instead (`CoreHandle.inDirectory`). It
  installs the log sink first (`installLogSink(FfiLogSink, INFO)`: core
  records go to logcat under `RostrumCore` as key=value lines), then runs
  `onOpened` (the feed observer). A failure to open, including a
  `LinkageError` from a missing library, is returned as an `Outcome.Err` and
  retried by the next call.
- Every method is one core call inside `ffiCall`, which maps
  `RostrumException` (sealed; the `when` in `FfiErrors.kt` is exhaustive) to
  `BackendError`, and UniFFI's `InternalException` (a Rust panic) to
  `BackendError.Internal`. Nothing else is caught; cancellation propagates.
- Numbers narrow from UniFFI's unsigned types; negative Kotlin inputs
  (`fileIndex`, ports outside 1..65535) are refused as `InvalidInput` before
  reaching the core.
- The notification worker, in a cold process, gets the same container and
  cores; `NotificationCheck` restores every profile (which calls
  `setGitHubToken`) before `checkNotifications`, as the core requires.

### Copying the desktop's settings

- `RostrumBackend.desktopConfig()` returns a `DesktopConfigPreview`: the
  desktop's repositories in its order, `added` (on the desktop, not here),
  `removed` (here, dropped by copying), its pull requests per repository,
  feed preferences and stash default, and `changesAnything`.
  `copyDesktopConfig()` re-reads the desktop's settings (never applying a
  stale preview), replaces the phone's, persists them and returns the new
  `Settings`. Both fail with `NotPaired` when unpaired.
- `DesktopConfigCopier` (in `ui/desktopconfig/`) is shared by both entry
  points: `preview()` returns a `DesktopConfigOffer` (the preview plus what
  it would change, in words), and `copy(machine)` = `copyDesktopConfig()`
  then `refreshFeed()` (a failed refresh does not undo the copy), returning
  "Copied 7 repositories from framework".
- The core counts a reorder alone as a change (it reorders the feed), so
  `changesAnything` can be true with nothing added or removed.
  `DesktopConfigText.changeLines(preview, phone)` compares with this profile's
  `Settings` and says "Adds 6 repositories", "Removes 2 repositories",
  "Reorders your repositories to match framework", or "Changes pull requests
  per repository (10 → 25), feed filters and the stash default"; without the
  phone's settings it admits "Reorders your repositories or changes feed
  settings to match framework". The core clamps `prsPerRepo` to 1..=100,
  drops the desktop's duplicate repositories and blank authors, and keeps
  pending drafts on repositories that copying drops.
- **After pairing into a new profile** (`PairViewModel`, link or manual, on
  first run or after "Switch to <machine>"): the preview of that profile is
  read. `changesAnything` → the Pair screen becomes `CopySettingsStep` ("Copy
  settings from <machine>?", the desktop's repositories with added ones
  marked "+", a "Removed from this profile" group, and a one-line summary of
  the feed preferences), with "Copy settings" and "Not now"; Back means not
  now. Re-pairing a known desktop never asks. Nothing would change → straight on. The preview fails →
  straight on, with a snackbar saying why. The "Copied …" and failure
  messages go through `AppContainer.appMessages`, collected at the root, so
  they survive the switch to the feed.
- **The question comes before the switch**: the registry never activates a
  paired profile, so the graph on screen (first run's, or the current
  profile's) stays while the question is open; `PairViewModel` switches to
  the new profile once it is answered, which rebuilds the graph at its feed.
- **From Settings**: under a connected desktop, "Copy settings from
  <machine>" opens `CopySettingsSheet`, driven by
  `DesktopConfigSheetViewModel` (`Closed` → `Loading` → `Ready(preview,
  copy)` or `Failed(error)`). Ready and changing something: the replace
  warning ("This replaces this profile's repositories, …"), "N repositories will
  be removed from this profile." when any are, the preview, and "Replace
  settings" / "Cancel". Nothing would change: "This profile already has
  <machine>'s settings." A failed copy stays open with the error. After a
  copy the sheet closes, shows "Copied …", and Settings reloads its list.
  Settings also reloads whenever it resumes, e.g. back from pairing.

### Secrets at rest

`EncryptedFileSecretStore` keeps one file per profile and `SecretKey` under
`noBackupFilesDir/secrets/profiles/<profile id>/<name>.sealed`:
`[version=1][iv length][iv][ciphertext+tag]`. `SecretStore` is keyed by
profile; a session sees only its profile's slice (`forProfile(id)`, a
`ProfileSecrets`), and removing a profile deletes its directory
(`deleteProfile`). `AndroidKeystoreCipher` seals with AES-256-GCM under a
non-exportable Keystore key (`rostrum.secrets.v1`, no user authentication,
so the background check can read the token while the screen is locked).
Writes go to a temp file renamed into place. Reads distinguish `Present`,
`Absent` and `Failed(KeystoreUnavailable | Corrupt | Io)`; `Present` never
prints its value.

## UI conventions

- Screens are stateless composables taking a state object and callbacks
  (previewable with `PreviewData`), wrapped by a `…Route` that builds the
  ViewModel and collects its flows with `collectAsStateWithLifecycle`.
- Content is `UiState<T>`: `Loading`, `Loaded(data)`, `Error(BackendError)`.
  In-flight user actions are `ActionState`. One-shot messages go through
  `Messages` and appear in the app's snackbar (`CollectMessages`).
- Shared components (`ui/components/`): `RostrumCard` (surface, 1dp border,
  radius 16), `StatusChip` (22dp, radius 6, role colours), `LabelChip`,
  `FilterPill`, `RefTag`, `ShaTag`, pill buttons (primary, tonal, merge,
  outlined, danger, text) that keep 48dp touch targets, `RostrumIconButton`
  (48dp, required content description), `RostrumSwitchVisual`/`SwitchRow`,
  `CheckboxVisual`, `RadioVisual`, `SegmentedToggle`, `Avatar`, `CiGlyph`,
  `RostrumTextField`, `CopyCommandRow`, `RostrumBottomSheet` (#1c2029, radius
  28, 32×4 handle, always expanded), `ConfirmDialog`, `LoadingView`,
  `ErrorView`, `EmptyView`, `FieldError`, `ScreenHeader`, `BackTopBar`, and
  `MarkdownBlocks` for the core's flat markdown.
- `RostrumIcons` are the mockups' SVG line icons rebuilt as `ImageVector`s.
- `ColorRole.colors()` maps a role to text, tint (15–18% fill) and solid
  colours; `labelColors` makes a GitHub label colour legible on the dark
  palette.
- Pure formatting lives in `ui/format/Formatters.kt` (relative ages, initials,
  repo split, `+N`/`−N` with U+2212, line ranges, sides, paths, durations).

## Screens

Each package has one `…Route` (builds the ViewModel, collects state and
messages, owns sheets and dialogs), a stateless screen, smaller section
composables, a pure mapping file with its own tests, and previews.

### Feed (`ui/feed/`)

- `FeedViewModel(backend, session, clock)`: paints `cachedFeed()`, then
  `refreshFeed()` and `markNotificationsSeen()`; follows `feedUpdates`,
  keeping the highest revision; debounces the search text 250 ms into
  `setQuery`; chips and sheet switches go through `setFilter`, author toggles
  and Clear through the backend; the roster is `authorRoster(5)` until "Show
  all" asks for `authorRoster(null)`; the desktop pill asks `machineInfo()`
  and again when pairing changes; auto-refresh repeats on the settings
  interval while the screen is started. A sign-in error shows a "Sign in
  again" banner (sign out); other errors go to the snackbar, and become the
  error state only when nothing is on screen.
- `FeedUiState.kt` (sealed search, filter-sheet and desktop-pill states; the
  roster exists only while the sheet is open), `FeedMapping.kt` (row chips,
  meta line, active-filter count, "Authors: me, ada-lin", pill status).
- UI: `FeedScreen`, `FeedHeader` (wordmark, "N open", the profile pill —
  the active profile's label with a dot for its desktop's state; tapping it
  opens the profile switcher — search,
  filter button with the active dot), `FeedList` + `CardSegment` (each repo
  card is several lazy items so long repos stay lazy), `PrRow`,
  `FeedFilterSheet`, `FeedActions`, `FeedPreviews`.

### Pull request (`ui/pr/`)

- `PrDetailViewModel(pr, backend, clock)`: cached detail first, then the
  network (a failed refresh keeps what is shown); comment, thread reply,
  labels, draft toggle, close/reopen, branch update guarded by `headSha`, and
  the merge form (a refused merge keeps the sheet open with the error).
  `BranchViewModel(pr, backend)`: settings (stash default), remote status,
  `localStatus`, the four local jobs, abort; loads the first time the Branch
  tab shows.
- `PullRequestRoute` owns the selected tab (saveable, starting at
  `initialTab`), the merge and label sheets, close/reopen confirmations and
  the `reviewSheet` slot (refresh on submit); it reloads the detail when the
  screen resumes, so draft counts follow a trip into the diff.
- `PrScreen` (header, tab row, body or the `filesTab` slot, composer on
  Conversation and Checks), `PrChrome` (`PrTopBar`, `PrTabRow`),
  `conversation/` (`ConversationTab`, `PrSummaryBlock`, `TimelineItems`,
  `ThreadCard`), `checks/ChecksTab`, `branch/` (`BranchTab`, `LocalCard`),
  `merge/` (`MergeFormState`: method, default title/message per method;
  `MergeSheet`), `labels/LabelPickerSheet`, `common/` (`PrMapping`,
  `ComposerBar`, `PrUi`).

### Files, diff and review (`ui/pr/files/`, `ui/review/`)

- `FilesOverviewViewModel(pr, backend)` and `FilesOverviewTab`: summary
  strip, `ChangeMap` (columns and tiles by share with visible minimums; a
  tile shows name and counts stacked, inline, vertical or nothing by room;
  colour blends success→danger by `removedRatio` at the tile's heat) laid out
  by `OverviewLayout`, and `RankedFiles`. "Diff" opens the largest file.
- `FileDiffViewModel(pr, fileIndex, backend)` and `FileDiffRoute`: files are
  ordered by the overview's ranking ("1/7", previous/next inside the screen);
  soft wrap, local "Viewed"; `DiffSelection` holds a single-line or range
  selection, kept on one side and within one hunk; drafts add/edit/remove;
  thread replies. `DiffBody`/`DiffLineRows` draw hunks and 20dp lines (one
  horizontal scroll offset shared by all unwrapped lines); `DiffInlineCards`
  draws threads and drafts; `DiffScreen` adds the header, sub-bar, hint strip
  and pending-review bar.
- `LineCommentSheet` + `CommentComposer`: anchor chip, Write/Preview (Preview
  uses the core's `renderMarkdown(source, repo)`; a failure shows inline),
  "Add to review" (count badge) and "Comment now" (`addDraft` then
  `submitReview(Comment, "", includeDrafts = true)`), enabled only when no
  other drafts are pending, with a one-line reason otherwise. Edit mode saves
  or deletes.
- `SubmitReviewViewModel(pr, backend)` and `SubmitReviewSheet`: pending list
  (editing in the same sheet), summary, verdict radios; `ReviewRules` turns
  Approve and Request changes off while drafts are stale or the pull request
  is yours (a blocked choice falls back to Comment); drafts are included when
  present and current; Discard drafts confirms. `ReviewLabels` formats
  locations and anchors.
- Line numbers are tapped in the 52dp gutter-plus-marker column at the
  mockup's 20dp line height; each commentable line also carries a "Comment on
  line N" accessibility action.

### Settings, desktop and onboarding (`ui/settings/`, `ui/desktop/`, `ui/onboarding/`)

- `SettingsViewModel(backend, session, onNotificationSettingsChanged)`, over
  the active profile, under the "Profiles" section (`ui/profiles/`, a slot
  wired by the nav host):
  account (viewer, "github.com", sign out with confirmation, "Use <machine>'s GitHub
  token" when paired), repositories (add with inline `InvalidRepo`/
  `DuplicateRepo` errors then `refreshRepo`; remove; sublines from the
  desktop's clones and the feed's hidden repos), desktop row, refresh
  interval choice, notification toggles (and the permission prompt when one
  turns on). Files: `SettingsModels`, `SettingsSections`, `SettingsScreen`.
- `DesktopViewModel(backend, session, clock)`: machine card ("Connected ·
  N clones"), handoff sessions (session name, worktree, pull request and head
  ref when known; copy attach command, open the pull request's Branch tab,
  abort with confirmation), sync all (polls `syncAllStatus` every second while running;
  buttons and the stash switch are disabled meanwhile), last run (problem
  entries listed, updated ones behind an expandable row), options menu
  (refresh, get token, unpair with a "Forget on this phone" fallback). Files:
  `DesktopModels`, `HandoffSection`, `SyncAllSection`, `LastRunSection`,
  `DesktopScreen`.
- `SignInViewModel(profiles, profile)`: pairing first; the token form (a
  github.com token; Enterprise isn't supported) behind an expandable row. On
  first run (`profile` null) the token makes a new profile and switches to
  it; for a profile that lost its token it signs that profile in again,
  names it ("Sign in to use Work again."), shows the involuntary sign-out
  notice, and offers "Switch profile".
- `PairViewModel(profiles, link, appMessages)`: a link is read by the core
  (`Reading`, then) and previewed (machine,
  addresses, fingerprint) and paired with one tap; an invalid link falls back
  to the manual form: host, port (8485), code → `probeDesktop` → compare the
  fingerprint → `pairManual`. `PairingInput` validates and normalises the
  manual fields. After pairing: first run → the copy question, then the new
  profile becomes active; a new desktop while set up → `SwitchProfileStep`
  ("Switch to <machine>?", "Stay on <current>"), and switching asks the copy
  question for the new profile before switching; a known desktop →
  "Re-paired <machine>" and nothing switches. Files: `PairSections`,
  `PairScreen`, `SwitchProfileStep`, `CopySettingsStep`, `OnboardingParts`,
  `SignInScreen`.

### Where the screens depart from the mockups

- No "Resolve" on threads, no "Expand N lines" between hunks, and "Viewed" is
  local only: the core has no API for any of them.
- Not shown because the core does not provide them: the feed's desktop chips
  ("handed off", "↑2 unpushed"), code excerpts on conversation threads,
  commit SHAs on push events, the worktree count, the Branch tab's conflicted
  files, and handoff descriptions ("Rebase onto main stopped · 3 conflicted
  files"; the abort button just reads "Abort").
- The Branch tab's Reviews tile reads "Review required" and the merge sheet's
  "Approved" (not "0 of 1 approvals" / "Approved by you"): the header has no
  approval counts or approver.
- The filter sheet's author line cannot say "asked you to review": the roster
  has no such field.
- The Branch tab's desktop card points to Settings › Desktop when unpaired
  instead of opening pairing.
- A review in the timeline shows as an event row above its thread cards.
- The account line reads "signed in with a token" (no device-code flow), and
  Pair has no "Skip" (Back leaves).
- The diff hint reads "hold and drag for a range", since a range starts with a
  long press.

## Mapping to rostrum-ffi

`data/ffi/*Mappings.kt` map every generated record and enum to the model one
to one (`u32` → `Int`, `u64` → `Long`, `u16`/`u8` → `Int`, `SystemTime` →
`Instant`, ARGB `u32` → `Int` keeping its bits), and the inputs back
(`FeedPreferences`, `CommentAnchor`, `Side`, `MergeMethod`,
`BranchUpdateMethod`, `ReviewEvent`, `LocalOp`, `SyncAllOp`). Mappings are
extension functions (`toModel()`, `toFfi()`), unit-tested on the JVM from
constructed generated records (constructing them does not load the library).

What the core does not provide, and what the app does instead:

| Missing from the core | In the app |
|---|---|
| GitHub Enterprise hosts | github.com only; sign-in says so |
| Posting one inline comment outside a review | "Comment now" = `addDraft` + `submitReview(Comment)`, offered only with no other drafts pending |
| Desktop-derived feed chips, thread code excerpts, pushed-commit events | not shown |
| Worktree counts, conflicted-file lists, handoff descriptions | not shown; a handoff shows its session, pull request, worktree and attach command |
| Resolving threads, lines between hunks, persistent "viewed" | not offered ("Viewed" is local to the screen) |
| Required/current approval counts, the approver | generic wording ("Review required", "Approved") |

## Files

| File | Role |
|---|---|
| `RostrumApplication.kt` | Builds and starts the `AppContainer` |
| `MainActivity.kt` | Single activity; hands intents to `AppLinkInbox` |
| `di/AppContainer.kt` | Object graph (`profiles`); notification schedule sync |
| `data/RostrumBackend.kt` | The backend interface (every signature) |
| `data/BackendError.kt` | `BackendError`, `RemoteErrorCode`, `describe()`, `requiresSignIn`, `requiresPairing` |
| `data/Outcome.kt` | `Outcome` (`Ok`/`Err`) and `map`, `andThen`, `onOk`, `onErr`, `valueOrNull`, `errorOrNull` |
| `data/RostrumLog.kt` | `RostrumLog` key=value logging, `Outcome.logErr` |
| `data/model/*.kt` | Domain records: `Common`, `Session`, `Feed`, `Markdown`, `Detail`, `Diff`, `Review`, `Remote`, `Profile` |
| `data/ffi/FfiRostrumBackend.kt` | The production backend, one per profile: one core call per method, observer bridged to `feedUpdates` |
| `data/ffi/CoreHandle.kt` | Opens one `RostrumCore` through a `CoreOpener` (off the main thread), log sink, `onOpened`; `inDirectory` for standalone cores |
| `data/ffi/FfiProfileRegistry.kt` | The core's `ProfileRegistry` behind `ProfileRegistryApi` (see `android_profiles.md`) |
| `data/ffi/FfiErrors.kt` | `RostrumException.toBackendError()`, `RemoteErrorCode` mapping, `ffiCall` |
| `data/ffi/FfiLogSink.kt` | Core `tracing` records → logcat key=value lines |
| `data/ffi/FfiDesktopConfig.kt` | `DesktopConfigPreview` mapping for `desktopConfig` / `copyDesktopConfig` |
| `data/ffi/CommonMappings.kt`, `FeedMappings.kt`, `DetailMappings.kt`, `DiffMappings.kt`, `RemoteMappings.kt` | Generated records ↔ model |
| `data/fake/FakeRostrumBackend.kt` | In-memory backend for tests and previews; `failNext(FakeCall, error)` |
| `data/fake/FakeDesktop.kt` | The fake's desktop: pairing (the core's link format), remote, local jobs, sync all, handoffs |
| `data/fake/FakeCall.kt` | One entry per fallible backend call |
| `data/fake/FakeFeedAssembler.kt` | Feed filtering, sections, roster (as the core does it) |
| `data/fake/FakeDiffs.kt` | Hunks → rows with anchors, threads, drafts; overview layout |
| `data/fake/FakePull.kt` | Sample pull request → `PrSummary`, `PullHeader`, verdict |
| `data/fake/Sample*.kt`, `FakeHighlighter.kt`, `FakeMarkdown.kt` | The mockups' data, syntax colours, markdown |
| `data/secrets/SecretStore.kt` | `SecretKey` (GitHub token, device token, desktop endpoint), `SecretStore` (keyed by profile), `ProfileSecrets`, `forProfile`, `SecretRead`, `SecretWrite`, `SecretStoreError` |
| `data/secrets/SecretCipher.kt` | `SecretCipher`, `SealedBox`, `CipherOutcome`, `SecretEnvelope` format |
| `data/secrets/AndroidKeystoreCipher.kt` | AES-256-GCM under a Keystore key |
| `data/secrets/EncryptedFileSecretStore.kt` | One sealed file per profile and secret, atomic replace, `deleteProfile` |
| `data/session/SessionRepository.kt` | One profile's `SessionState`, `GitHubAuth`, `DesktopLink`; restore, sign-in, `adoptPairing`, sign-out, unpair |
| `data/profiles/*.kt` | `ProfileManager`, `ProfilesState`, `ProfileHandle`, `ProfileRegistryApi`, legacy wipe (see `android_profiles.md`) |
| `notifications/NotificationContent.kt` | `RostrumChannel`, `NotificationSpec`, `NotificationContent.of`, `tapExtras`, `NotificationTap` |
| `notifications/NotificationScheduler.kt` | `BackgroundWork`, `WorkManagerBackgroundWork`, `NotificationScheduler` |
| `notifications/NotificationPoster.kt` | Channels and posting |
| `notifications/NotificationWorker.kt` | `NotificationCheck` (every profile), `ProfileCheck`, `CheckResult`, `NotificationWorker` |
| `notifications/NotificationPermission.kt` | `rememberNotificationPermission`, `RequestNotificationPermissionOnce` |
| `ui/app/RostrumApp.kt` | Root: splash, the graph keyed by profile and sign-in, scaffold, bottom bar, snackbar, profile switcher, link routing and draining |
| `ui/app/GraphViewModelStores.kt` | `GraphKey`; one ViewModel store per graph, cleared when the graph changes |
| `ui/app/ShellViewModel.kt` | Desktop badge |
| `ui/navigation/Destinations.kt` | `Destination`, `PrTab`, `TopLevel` |
| `ui/navigation/RostrumNavHost.kt` | The graph; `openPullRequest`, `navigateTopLevel` |
| `ui/navigation/AppLinks.kt` | `AppLink`, `AppLinks.parse`, `AppLinkInbox`, `LinkRoute`, `routeOf` |
| `ui/desktopconfig/DesktopConfigCopier.kt` | Preview, and copy + feed refresh, shared by pairing and Settings |
| `ui/desktopconfig/DesktopConfigText.kt` | Titles, replace/removal copy, the preferences summary, repository rows |
| `ui/desktopconfig/DesktopConfigPreviewView.kt` | The desktop's repositories ("+" for added), the removed group, the summary line |
| `ui/onboarding/CopySettingsStep.kt` | The post-pairing question |
| `ui/onboarding/SwitchProfileStep.kt` | "Switch to <machine>?" after pairing a new desktop while set up |
| `ui/profiles/*.kt` | Profile switcher sheet, Settings' Profiles section, Add a GitHub token profile (see `android_profiles.md`) |
| `ui/settings/DesktopConfigSheetViewModel.kt`, `CopySettingsSheet.kt` | Settings › "Copy settings from <machine>" |
| `ui/navigation/BottomNavBar.kt` | Feed · Desktop · Settings with the badge |
| `ui/common/UiState.kt` | `UiState`, `ActionState` |
| `ui/common/ViewModels.kt` | `LocalAppContainer`, `LocalProfileHandle`, `rostrumViewModel`, `profileViewModel` |
| `ui/common/Messages.kt` | `Messages`, `LocalSnackbarHostState`, `CollectMessages` |
| `ui/components/*` | Shared components (see *UI conventions*) |
| `ui/format/Formatters.kt` | Pure formatting |
| `ui/preview/PreviewData.kt` | Sample data for previews |
| `ui/feed/`, `ui/pr/`, `ui/pr/files/`, `ui/review/`, `ui/settings/`, `ui/desktop/`, `ui/onboarding/` | Screens (see *Screens*) |
| `res/drawable/ic_notification.xml` | Status-bar icon |

All paths are under `android/app/src/main/kotlin/io/github/rhizonymph/rostrum/`
unless they start with `res/`. Tests mirror them under `src/test/`;
`testing/TestSupport.kt` holds `MainDispatcherExtension`, `TEST_CLOCK`,
`testBackend()`, `orFail()`, `InMemoryProfileSecrets`, `InMemorySecretVault`,
`pid()` and `RecordingBackgroundWork`; `testing/FakeProfileRegistry.kt`
holds the in-memory registry and `testProfileManager()`.

## Invariants

- **Secrets never leave the secret store in the clear.** Only a profile's
  `SessionRepository` reads or writes them (its own profile's only), and
  `ProfileManager` deletes a removed profile's; nothing logs them
  (`SecretRead.Present` and `DesktopGitHubToken` redact themselves); the
  backend keeps them in memory only.
- **No exceptions cross the backend boundary.** Failures are `Outcome.Err`
  with a `BackendError`; the only catches are the Keystore cipher's and the
  file store's explicit exception types, and in `data/ffi/` the generated
  `RostrumException`, UniFFI's `InternalException` and `LinkageError`.
- **One core per profile**, from the registry, shared by the UI and the
  notification worker; secrets reach it only through `setGitHubToken` and
  `setRemote`.
- **Features do not import each other**; only `RostrumNavHost` knows them
  all.
- **The graph is keyed on the profile and its sign-in**, so a signed-out
  user can never reach a signed-in screen through the back stack, and no
  screen of one profile survives a switch to another.
- **Copying never applies a stale preview**: `copyDesktopConfig` re-reads
  the desktop; the preview only decides whether and what to ask.
- **The graph on screen waits for the copy question**: a paired profile
  becomes active only when the question is answered.
- **A link is acted on once**: rotation does not replay it, and the inbox
  forgets it once the navigation host takes it.
- **Merge and branch updates pass the rendered head** (`headSha`) so a push
  made after the screen was drawn is refused rather than acted on blind.
- **Irreversible actions confirm**: merge (the sheet's Confirm), close,
  reopen, unpair, abort, discard drafts. The draft toggle does not.
- **Touch targets are at least 48dp** and every icon-only button has a
  content description.

## Testing

JVM unit tests (JUnit 5, `kotlinx-coroutines-test`) cover the data layer
(secret envelope and the per-profile file store with a fake cipher, the
legacy wipe, session restore/sign-in/pairing/unpair, `ProfileManager` over
`FakeProfileRegistry`, the fake backend's behaviour, markdown, feed
assembly),
formatting, links, notification content/scheduling/check, and every
ViewModel. ViewModels run on `FakeRostrumBackend` with `failNext` for error
paths and a fixed clock. `FfiMappingsTest` covers every error variant and
the records of each area. To observe `Messages.flow` in a test, collect it with
`backgroundScope.launch(UnconfinedTestDispatcher(testScheduler))`; a collector
on the default test dispatcher misses messages sent by the last task. `android.util.Log` returns defaults in unit tests
(`isReturnDefaultValues`). The Keystore cipher itself needs a device and is
not unit-tested.

### Host smoke test

`./gradlew :app:hostSmokeTest` builds `librostrum_ffi.so` for the build
machine (`cargoHostBuild`: `cargo build --locked --lib -p rostrum-ffi`), puts
`target/debug` on `jna.library.path`, and runs the JUnit tests tagged
`host-smoke` from the unit test classpath (plus JNA's desktop jar, which
carries the host's `libjnidispatch`). `HostSmokeTest` opens one real core on
a temp directory and drives it through `FfiRostrumBackend`: status and
warnings, settings, `addRepo` validation errors, the cached feed, the
observer reaching `feedUpdates`, `parsePairingLink` on a sample link,
`renderMarkdown`, error mapping (`NotSignedIn`, `NotPaired`, including
`desktopConfig` and `copyDesktopConfig` unpaired), that a
token never reaches the data directory, and the profile registry through
`FfiProfileRegistry` (see `android_profiles.md`). No network. The normal unit run
excludes the tag, and the tests skip themselves when not started by this
task.

`./gradlew :app:liveDesktopCheck` (tag `live-desktop`, run by hand, never in
CI) pairs with the `rostrumd` on this machine using a link from
`POST http://127.0.0.1:8484/pairing-codes` saved in the file named by
`ROSTRUM_LIVE_PAIR_URI_FILE`, calls `machineInfo`, `handoffs` and
`refreshGitHubTokenFromDesktop` (checking only that a token came back), and
unpairs; `ROSTRUM_LIVE_RESULT_FILE` receives a secret-free summary.
