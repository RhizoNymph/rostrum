# Feature: conflict_handoff

Handing a rebase or merge that stopped on conflicts to a user-configured
command running in a tmux session, with the context it needs already gathered.

## Scope

- The `conflict_handler` config setting and what turning it on changes.
- Gathering a context bundle from a stopped operation: conflicted paths and
  their kinds, the marker regions, the commit being applied, the commits on
  each side, the exact commands to continue or abort.
- Rendering that bundle as markdown, most actionable information first.
- Spawning the handler in a detached, named tmux session with the worktree as
  its working directory.
- Detecting, on a later look, whether that session is still running.

## Non-scope

- **Resolving conflicts.** Rostrum still has no conflict editor. It hands the
  problem to something that does and gets out of the way.
- **Watching the handler.** Rostrum does not poll the session, read its
  output, or know when it finishes. The next look at the worktree reads git's
  state, which is the only thing that matters.
- **Pushing.** The handler's instructions say not to; rostrum never does.
- Any handler-specific integration. `claude` is the example in the docs
  because it is installed here; the template is a shell command and anything
  that reads a file works.

## Turning it on

```json
"conflict_handler": {
  "command": "claude 'Resolve the conflicts described in {context}'"
}
```

Absent, a local conflict is aborted and the clone left as it was found — the
behaviour `local_git` documents. Present, three things change:

1. `Repo` is opened with `ConflictPolicy::Leave`, so `on_conflict` returns the
   conflict untouched and the worktree stays mid-operation.
2. On `Outcome::Conflicted`, `localops::run_local_job` gathers a
   `ConflictContext`, writes the bundle, and spawns the session.
3. If any of that fails before the session exists, the operation is aborted
   after all. The guarantee "either someone is finishing this, or the clone is
   as it was" holds in every path.

Two checks run *before* git does anything: the template must mention
`{context}` (a handler that never receives the bundle is a misconfiguration to
surface at click time), and no session may already exist for this pull request
(typing a second command into a running harness would interleave with it).

## The bundle

Rendered by `rostrum_handoff::render_bundle`, pure, from a `ConflictContext`
and the pull request's metadata. Section order is deliberate — the harness
reads top-down and the first screen should be enough to act on:

1. **What to do** — the instruction paragraph, the worktree, one sentence
   naming the operation ("a rebase of `feat-x` onto `refs/remotes/origin/main`,
   stopped at step 3 of 7 while applying `abc1234 subject`"), and the exact
   commands: mark resolved, mark removed, continue, and abort (labelled *do
   not run*).
2. **How to read the markers** — spelled out with the real ref names. In a
   rebase `<<<<<<< HEAD` is the *base* and `>>>>>>>` is the pull request's own
   commit being replayed; in a merge it is the reverse. This is the single most
   common cause of a wrong resolution, so it is never left to the reader. Plus:
   if git is holding an autostash it restores it after `--continue`; never
   `git stash pop` by hand.
3. **Conflicted paths** with kinds — `both modified`, `deleted by us`, and so
   on. The kind decides whether the file is marked with `git add` or `git rm`.
4. **Commit being applied** (rebase only) — full message.
5. **Conflict regions** — per file, the marker blocks with three lines of
   context, each line prefixed with its number so the harness can jump.
6. **Commits on each side** of the divergence, capped, with the true total so
   truncation is a fact rather than a guess.
7. **Pull request** — number, URL, base ← head, body capped at 4 KiB.
8. **git said** — the combined output from the stop.

Every cap surfaces as an explicit "(N more; run `git log …`)" or "(truncated;
open the file)" line. Nothing is dropped silently.

### Why the regions come from the file, not `git diff`

During a conflict `git diff` emits *combined* format for unmerged entries — two
columns, `++<<<<<<< HEAD` — and its shape depends on `merge.conflictStyle`.
The harness is going to open the file anyway, so the bundle hands it the file:
read from disk, a NUL in the first 8 KiB means binary, otherwise a pure parser
finds the `<<<<<<<` / `|||||||` / `=======` / `>>>>>>>` blocks. Delete-side
kinds have nothing on disk to show and say so.

### `GIT_EDITOR=true`, not `false`

Rostrum's own git runs with `GIT_EDITOR=false` so that any path that would open
an editor fails loudly. The continue command in the bundle uses `true`: the
harness must be able to accept the generated commit message non-interactively,
and `true` exits zero and accepts it. Two different callers, two different
correct answers.

## The tmux session

`rostrum_handoff::spawn` runs one tmux invocation:

```
tmux new-session -d -s <name> -c <worktree> \; send-keys -t =<name>: -l -- <command> \; send-keys -t =<name>: Enter
```

It starts an **interactive shell** and types the command into it, rather than
passing the command to `new-session` directly. Three reasons:

- A failing harness (`claude: command not found`, an expired login) leaves a
  visible pane with scrollback and a prompt, instead of tmux destroying the
  session and rostrum offering to spawn again with nothing to show.
- The shell's rc files give the user's real `PATH`. A desktop-launched rostrum
  often has a minimal one, and `claude` lives wherever the rc files put it.
- The template is interpreted by the user's own shell with the user's own
  rules. Rostrum quotes only the two paths it inserts.

Verified against tmux 3.6 during development, because the obvious spelling is
wrong: `send-keys -t =<name>` fails with "can't find pane" — `send-keys` takes
a pane target and reads a bare `=name` as a pane. `=<name>:` (exact session,
its current window) works. `has-session -t =<name>` is correct as-is, and the
`=` matters there too: without it `x-1` prefix-matches a session `x-12`.

### Session names are sanitised on our side

`rostrum-<owner>-<repo>-<n>`, with `.` and `:` and anything outside
`[A-Za-z0-9_-]` replaced by `_`. tmux ≥ 3.0 silently rewrites `.` and `:` to
`_` itself — so if rostrum asked for `rostrum-a-b.c-7` and tmux stored
`rostrum-a-b_c-7`, `has-session -t =rostrum-a-b.c-7` would say "no" forever and
every click would spawn a duplicate. Sanitising first makes the name we check
the name tmux keeps. Confirmed live: the raw dotted name is parsed as
`window.pane` and fails.

### The environment is inherited — the opposite of `rostrum-git`

`rostrum-git` builds its child environment from an allowlist because it *parses
git's output* and `GIT_DIR`, `LANGUAGE`, or an askpass helper can change what a
spawned git says or does. The tmux client parses nothing — it relays argv to a
server — and what eventually runs is the user's interactive tool, which needs
exactly what `rostrum-git` drops: `DISPLAY` for browser auth, a real `TERM`,
the full `PATH`, whatever API keys the harness reads.

Two tmux facts make this load-bearing rather than merely convenient. If
rostrum's call is what starts the tmux server, the server's global environment
is a *copy* of rostrum's — a stripped one would poison every session the user
opens afterwards. And when a server already exists, only `update-environment`
variables refresh from the client, so the interactive shell's rc files are what
make the environment right. `TMUX` alone is removed, so a rostrum launched from
inside tmux does not trip the nesting check.

## Detecting an existing handoff

The detail pane's local row, on load, reads the worktree's status. When
something is in progress and a handler is configured, it asks
`session_exists(session_name(repo, number))` and renders one of:

- **Running** — "handed off to tmux session `X` — `tmux attach -t =X`", with
  an Abort button.
- **Gone** — the operation is still in progress but no session by that name
  exists: the harness finished without continuing, or was killed. Same
  message, same Abort button, and the user can re-run the operation to hand
  it off again.

Abort takes its target from the worktree's own state via
`InProgress::abort_target()`, so it cannot run `merge --abort` on a rebase.

Three independent barriers prevent a double spawn: the detail pane's `busy`
guard, the pre-spawn `session_exists` check, and tmux's own refusal to create
a session whose name exists.

## Where the bundle lives

`dirs::cache_dir()/rostrum/handoff/<session>.md` — the session name and the
file name are the same identity. Written to a `.tmp` sibling and renamed over
the target so the harness never reads a half-written file. Always overwritten,
never auto-deleted: the harness may still have it open, it is a few kilobytes,
and the next handoff for the same pull request replaces it.

## Invariants

- Nothing is spawned unless the bundle is complete on disk.
- A handoff that could not be started leaves the clone as it was found.
- The bundle is a pure function of `ConflictContext` and `PrMeta`; no I/O in
  rendering.
- The session name is a pure function of `(RepoId, PrNumber)`, and is what
  both the spawn and the later `has-session` use.
- The tmux client is bounded by a five-second timeout; killing the client never
  kills the harness.

## Files

| File | Role |
|---|---|
| `crates/rostrum-handoff/src/lib.rs` | Crate doc, the env argument, `hand_off` composition |
| `crates/rostrum-handoff/src/bundle.rs` | `PrMeta`, `render_bundle`, `DEFAULT_INSTRUCTIONS` |
| `crates/rostrum-handoff/src/template.rs` | `substitute`, `shell_quote` |
| `crates/rostrum-handoff/src/session.rs` | `session_name`, `spawn_argv`, `session_exists`, `spawn` |
| `crates/rostrum-handoff/src/store.rs` | `context_path`, `write_context` |
| `crates/rostrum-handoff/src/error.rs` | `HandoffError` |
| `crates/rostrum-git/src/context.rs` | `ConflictContext` and its pure parsers |
| `crates/rostrum-git/src/context/regions.rs` | `extract_conflict_regions`, the marker parser |
| `crates/rostrum-git/src/repo/describe.rs` | `Repo::conflict_context` — the I/O |
| `crates/rostrum/src/localops.rs` | `run_local_job` — where the handoff is invoked |
| `crates/rostrum/src/config.rs` | `ConflictHandler` |
