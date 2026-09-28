# Feature: android_profiles

Several paired desktops on one phone. Each desktop is a **profile** with its
own repositories, filters, cache, drafts and GitHub account (the token its
desktop handed over, or one pasted for it); a profile can also be a pasted
GitHub token with no desktop. One profile is active at a time and the app
shows only that one; background notifications check every profile.

## Scope

- `ProfileManager`: the single owner of the core's profile registry, each
  profile's backend and session, and which profile is active.
- Per-profile secrets: the GitHub token, device token and desktop endpoint
  are sealed under the profile's id.
- The legacy wipe: on first launch of a profile-aware build, the
  single-profile state (`files/core` and the unkeyed secrets) is deleted
  without migration; the user pairs again.
- The UI following the active profile: the navigation graph and its
  ViewModels are rebuilt on a switch, starting at the feed.
- First run: pairing or a pasted token creates the first profile and makes it
  active (after the copy-settings question, for pairing).
- The profile switcher (feed pill, Desktop tab header): list, switch, "Pair
  another desktop", "Add a GitHub token profile".
- Pairing while set up: a new desktop gets a profile and "Switch to
  <machine>?"; a known desktop is re-paired in its own profile.
- Settings › Profiles: rename, remove (with confirmation), switch.
- Notifications across profiles: every signed-in profile with a toggle on is
  checked, titles carry the profile's label, and a tap switches to that
  profile before opening the pull request.

## Non-scope

- Migrating single-profile data. There is no backwards compatibility: the
  old state is wiped and the user re-pairs.
- Showing several profiles at once (a merged feed), or moving repositories or
  drafts between profiles.
- Two profiles for one desktop: the registry identifies a desktop by its
  certificate fingerprint and re-pairs into its profile.
- GitHub Enterprise, as everywhere in the app.

## Data and control flow

### Types

- `ProfileId` (`data/model/Profile.kt`): a value class whose constructor is
  private; `ProfileId.of(raw)` accepts only `[A-Za-z0-9_-]{1,64}` (the
  registry's ids are 16 hex characters), since ids name directories.
- `Profile(id, label, kind, githubLogin, createdAt, lastUsed)`, with
  `ProfileKind.Desktop(machine, fingerprintShort)` or
  `ProfileKind.TokenOnly`; `ProfilePairing(profile, created, pairing)`.
  They mirror the generated `ProfileInfo`, `ProfileKind` and
  `ProfilePairing`.
- `ProfileRegistryApi` (`data/profiles/`): the registry as the app uses it,
  returning `Outcome`s — `profiles()` (most recently used first),
  `activeProfile()`, `setActiveProfile(id)`, `backend(id)` (one
  `RostrumBackend` over `registry.core(id)`), `createTokenProfile(label)`,
  `pairDesktopWithLink(uri, deviceName)`, `pairDesktopManual(…)` (both
  re-pair a known desktop and never change the active profile),
  `renameProfile`, `setProfileLogin`, `removeProfile` (unpairs best-effort,
  closes the core, deletes its data), and `parsePairingLink` /
  `probeDesktop`, which the Pair screen needs before any profile exists.
- `ProfilesState`: `Starting`, `Unavailable(error)`, or `Ready(profiles,
  active)` whose constructor requires `active` to be one of `profiles`.
- `ProfileHandle(id, backend, session)`: what a profile's screens use.
- `PairedProfile(profile, created, machine)`, and `ProfileRemoval`:
  `Inactive(removed)`, `ActiveReplaced(removed, next)`,
  `LastRemoved(removed)`.
- `BackendError.ProfileNotFound(id)` mirrors the core's new error variant.

### Start-up (`ProfileManager.start`)

1. `LegacyStateWipe` deletes `files/core` and the unkeyed
   `no_backup/secrets/{github_token,device_token,desktop_endpoint}.{sealed,tmp}`
   (only those names, so the per-profile directory beside them survives).
   It runs before the registry opens, on every start; after the first it
   finds nothing.
2. The registry lists the profiles and the active one. With profiles but
   none (validly) active, the most recently used is made active.
   A failing registry makes the state `Unavailable`; the root shows the error
   with Retry, and `start()` may be called again.
3. The state becomes `Ready`, so the UI can show the active profile (its
   session is still `Restoring`, behind the splash).
4. Every profile's `session.restore()` runs, the active one first: its
   secrets go into its own core (`setGitHubToken`, `setRemote`).
5. `start()` returns once all are restored (the notification worker relies
   on that); later calls return at once.

Each `ProfileHandle` is made on first use (`handle(id)`), with a watcher in
the manager's scope: when the session becomes signed in, `viewer()` names
the login and `setProfileLogin` records it (signed out clears it); it also
keeps `signedInProfiles` current.

### Switching

`switchTo(id)` restores the profile's session if needed, calls
`setActiveProfile`, and re-reads the registry into `state`. The root
composes the active profile's graph under `key(GraphKey(id, signedIn))`,
with `LocalProfileHandle` set to its handle and `LocalViewModelStoreOwner`
set to a store of its own from `GraphViewModelStores` (held in the
activity's store). A different key clears the previous store, so the old
profile's ViewModels (and the NavController's entries) stop; the same key
after rotation gets the same store back. The new graph starts at the feed
(or at sign-in when that profile has no working token).

### First run

With no profile, the root shows the sign-in graph (`GraphKey(null, false)`).

- **Token**: `SignInViewModel(profiles, profile = null)` calls
  `createTokenProfile(token, label = null)`: the registry makes a token-only
  profile named "GitHub"; its session signs in (checked with `viewer()`); a
  rejected token removes the profile and its secrets again; the profile is
  renamed after the login and its login recorded. Then `switchTo`.
- **Pairing**: `PairViewModel` calls `pairWithLink` / `pairManual`, which
  pair through the registry into a new profile; the profile's session
  `adoptPairing`s the result (endpoint and device token sealed; the
  handed-over github.com token adopted when the profile has none). If the
  secrets can't be saved, a new profile is removed again. With no active
  profile the copy-settings question is asked for the new profile (its
  backend, through `DesktopConfigCopier`), and only when it is answered
  (or skipped because nothing would change, or the preview failed) does the
  VM `switchTo` the new profile, which rebuilds the graph at its feed.

### Pairing while set up

`PairViewModel.afterPairing`:

- `created` → `SwitchOffer(profile, machine, current)`; the Pair screen shows
  `SwitchProfileStep` ("Switch to <machine>?", "Switch to <machine>" /
  "Stay on <current>"; Back means stay). Switching asks the copy question for
  the new profile, then switches. Staying says "Paired with <machine>.
  Switch to it from the profile menu." and leaves.
- not `created` (a desktop some profile is already paired with) → that
  profile's pairing is renewed, the snackbar says "Re-paired <machine>", and
  the active profile does not change.

Messages go through `AppContainer.appMessages`, collected at the root, so
they survive the graph being rebuilt.

### The switcher and Settings

- `ProfileSwitcherSheet` (in `MainScaffold`, opened from the feed's profile
  pill, the Desktop tab's header pill, and "Switch profile" on a signed-out
  profile's sign-in screen) lists `ProfileRow`s: label, "machine · @login"
  (or "GitHub token"), a check on the active one.
  `ProfileSwitcherViewModel.switchTo` does nothing for the active profile
  (the sheet closes); otherwise it switches and says "Switched to <label>";
  a failure stays on the sheet. "Pair another desktop" explains the pairing
  page (`http://<desktop>:8484/`, Open in Rostrum or the QR code) and opens
  the Pair screen, which also has the manual entry; "Add a GitHub token
  profile" opens `AddTokenProfileRoute` (name, optional; token), which
  creates the profile and switches to it.
- Settings › Profiles (`ProfilesSettingsSection`, injected by the nav host
  as a slot so Settings doesn't import the profiles feature; shown even when
  the profile's settings fail to load): the rows (tap to switch), each with
  Rename (a dialog; blank refused) and Remove. Remove confirms with "Remove
  <label>?" and "This unpairs <machine> and deletes this phone's data for
  it: its repositories, filters, cache, drafts and GitHub token." (token
  profiles: without the unpairing), plus "Rostrum switches to <next>." or
  "You'll be back at sign-in." for the active one.
- `ProfileManager.remove(id)`: `registry.removeProfile` (a failure keeps
  everything), then the profile's secrets are deleted, its handle dropped,
  and — when it was active — the most recently used remaining profile made
  active, or none. The root follows `state`: the next profile's feed, or the
  first-run sign-in.

### Notifications

- `AppContainer` resyncs the WorkManager schedule whenever the set of
  profiles or of signed-in profiles changes, and after any notification
  toggle: `wantsNotifications()` is true when some signed-in profile has a
  toggle on.
- `NotificationCheck.run()` starts the manager (restoring every profile),
  then per profile: `SignedOut`, `NotificationsOff`, `Posted(n)`,
  `Retry(error)` (network, rate limit) or `GaveUp(error)`. The worker
  retries if any profile asked to.
- Each notification's title starts with the profile's label, its id is
  stable per profile and pull request, and its tap intent carries
  `EXTRA_PROFILE`. `AppLinks.parse` reads it back into
  `AppLink.OpenPullRequest(pr, profile)`; `routeOf(link, state)` makes the
  root switch to that profile first (`SwitchFirst`), drop a link whose
  profile was removed (`ProfileGone`), or leave it to the graph on screen
  (`Show`), whose `MainScaffold` opens the pull request only when the link's
  profile is its own.

### The adapter (`data/ffi/FfiProfileRegistry.kt`)

The only file that touches the generated `uniffi.rostrum_ffi.ProfileRegistry`
(`ProfileRegistry.open(rootDir)` over `files/profiles`). Each profile's
backend is `FfiRostrumBackend(id, openCore = { registry.core(id) })`, so the
`CoreHandle` gets the profile's cached core from the registry. Until the core
side lands in this branch, every call answers
`Internal("profiles need a newer core")`.

## Files

| File | Role / key exports |
|---|---|
| `data/model/Profile.kt` | `ProfileId`, `ProfileKind`, `Profile`, `ProfilePairing` |
| `data/profiles/ProfileRegistryApi.kt` | `ProfileRegistryApi` |
| `data/profiles/ProfileManager.kt` | `ProfileManager`: `state`, `signedInProfiles`, `start`, `handle`, `switchTo`, `createTokenProfile`, `pairWithLink`, `pairManual`, `rename`, `remove`, `wantsNotifications`, `parsePairingLink`, `probeDesktop` |
| `data/profiles/ProfilesState.kt` | `ProfilesState`, `ProfileHandle`, `PairedProfile`, `ProfileRemoval` |
| `data/profiles/LegacyState.kt` | `LegacyCleanup`, `LegacyStateWipe`, `LegacyWipeReport` |
| `data/ffi/FfiProfileRegistry.kt` | The registry adapter |
| `data/ffi/CoreHandle.kt` | `CoreOpener`; a handle over any opener (`inDirectory` for standalone cores) |
| `data/secrets/SecretStore.kt`, `EncryptedFileSecretStore.kt` | `SecretStore` keyed by profile, `ProfileSecrets`, `forProfile`, `deleteProfile` |
| `data/session/SessionRepository.kt` | One profile's session; `adoptPairing` |
| `di/AppContainer.kt` | Builds the manager (registry under `files/profiles`, secrets under `no_backup/secrets/profiles`, the legacy wipe); schedule resync |
| `ui/app/RostrumApp.kt` | The graph keyed by `GraphKey`, `FollowNotificationProfile`, the switcher sheet in `MainScaffold` |
| `ui/app/GraphViewModelStores.kt` | `GraphKey`, `GraphViewModelStores` |
| `ui/common/ViewModels.kt` | `LocalProfileHandle`, `profileViewModel` |
| `ui/navigation/AppLinks.kt` | `EXTRA_PROFILE`, `LinkRoute`, `routeOf` |
| `ui/onboarding/PairViewModel.kt` | `SwitchOffer`, `CopyOffer(profile, …)`, the three branches |
| `ui/onboarding/SwitchProfileStep.kt` | "Switch to <machine>?" |
| `ui/onboarding/SignInViewModel.kt` | New profile vs. the signed-out active profile |
| `ui/profiles/ProfileRows.kt` | `ProfileRow`, `profileRows`, `ProfileText` (removal copy, messages) |
| `ui/profiles/ProfileSwitcherViewModel.kt`, `ProfileSwitcherSheet.kt` | The switcher; `ProfileRowView` |
| `ui/profiles/ProfilesSettingsViewModel.kt`, `ProfilesSection.kt` | Settings › Profiles: rename and remove dialogs |
| `ui/profiles/AddTokenProfileViewModel.kt`, `AddTokenProfileRoute.kt` | Add a GitHub token profile |
| `ui/components/Chips.kt` (`HeaderPill`), `PairingPage.kt` | The header pill; the pairing page sentence |
| `notifications/NotificationContent.kt`, `NotificationWorker.kt`, `NotificationScheduler.kt`, `NotificationPoster.kt` | Label-prefixed specs, `tapExtras`, the per-profile check, `sync(wanted)` |

Paths are under `android/app/src/main/kotlin/io/github/rhizonymph/rostrum/`.
Tests: `data/profiles/ProfileManagerTest`, `LegacyStateWipeTest`,
`data/secrets/SecretStorageTest` (per-profile store),
`ui/onboarding/PairProfileBranchesTest`, `PairCopyStepTest`,
`PairViewModelTest`, `SignInViewModelTest`, `ui/profiles/*Test`,
`ui/app/GraphViewModelStoresTest`, `ui/navigation/AppLinksTest` (routing),
`notifications/NotificationsTest`; `testing/FakeProfileRegistry.kt` is the
in-memory registry (a desktop known by its short fingerprint, ids `p1`,
`p2`, …) and `testProfileManager()` runs the manager's watchers on the
test's scheduler outside `backgroundScope` (whose work `advanceUntilIdle`
does not wait for).

## Invariants

- **A profile's secrets are only ever read or written under its id**, and
  only by its own session; removing a profile deletes them.
- **`ProfilesState.Ready.active` is one of its profiles** (checked on
  construction), and `state` is always re-read from the registry after a
  change, never patched locally.
- **Only `ProfileManager` changes the set of profiles or the active one**;
  the registry never activates a profile by pairing.
- **Nothing of one profile runs in another's graph**: the graph is keyed by
  the profile, its ViewModels live in a store cleared on a switch, and a
  notification for another profile is opened only after switching.
- **A profile that could not be set up is not kept**: a rejected token, or a
  pairing whose secrets could not be saved, removes the profile it created.
- **The legacy wipe touches only the single-profile files** it knows by
  name, and runs before the registry opens.
