# CLAUDE.md as a symlink to AGENTS.md — design

**Date:** 2026-07-25
**Supersedes** the mirror-copy behavior in
`docs/superpowers/specs/2026-07-10-agents-md-sync-design.md`.
**Module:** `src-tauri/src/agents_sync/`.

## Problem

Today `CLAUDE.md` is a byte-for-byte **copy** of `AGENTS.md`, kept in sync by a
hash-comparison state machine (`logic::decide`) with a per-machine baseline
persisted to `agents_sync.json` in the OS app-config dir.

That baseline does **not** travel with the vault. `CLAUDE.md` itself *is* git
tracked and syncs across machines. On any machine that did not create the
mirror, the baseline is empty, so `decide()` cannot tell which side changed:
whenever the two files are not byte-identical it falls through to
`PromptConflict` and shows the "CLAUDE.md has been modified…" dialog. This is
the reported bug — a fresh machine prompts on every startup.

Root cause: sync state that must be shared across machines lives in
machine-local app config, and `CLAUDE.md` is a tracked entity file that can
arrive stale relative to `AGENTS.md`.

## Decision

`CLAUDE.md` stops being an entity file. It becomes a **relative symlink** to
`AGENTS.md` in the same directory, `CLAUDE.md → AGENTS.md`, and is added to the
vault's `.gitignore`. Its entire lifecycle is owned by the sync service. A
symlink is transparent to readers, so Claude-family agents opening `CLAUDE.md`
still read `AGENTS.md`; on POSIX, writing through the symlink writes
`AGENTS.md`, so "edit either" holds naturally.

This makes the hash comparison, the persisted baseline, and the conflict dialog
obsolete — all are removed.

## Behavior

On each sync check (`run_check`), given the on-disk state of `AGENTS.md` and
`CLAUDE.md`:

| AGENTS.md | CLAUDE.md state | Action |
|---|---|---|
| exists | missing | create relative symlink `CLAUDE.md → AGENTS.md` |
| exists | already a symlink to same-dir `AGENTS.md` | none |
| exists | **regular file** | **silently** rename to `CLAUDE.YYYYMMDD.md`, then create symlink |
| exists | **symlink to something else** | recreate symlink to point at `AGENTS.md` |
| missing | our symlink (target `AGENTS.md`) | remove the dangling symlink |
| missing | regular file / absent | none |

Always, when a vault is configured: idempotently ensure `<vault>/.gitignore`
contains a `CLAUDE.md` line (create the file or append the line if absent).
Everything is silent — no dialogs.

### Implementation details

- **Relative symlink.** Target is the literal `AGENTS.md` (same directory), so
  the link survives the vault being moved or synced to a different absolute
  path.
- **Dated-backup collision.** Default backup name is `CLAUDE.YYYYMMDD.md` using
  the local date. If that name already exists, fall back to
  `CLAUDE.YYYYMMDD-2.md`, `-3.md`, … Never overwrite an existing backup. The
  backup is **not** gitignored (only `CLAUDE.md` is), so it stays visible and
  git-trackable for the user to review or delete.
- **Transactional symlink creation (protects Windows).** Creating a file
  symlink on Windows needs Developer Mode / admin and may fail; on failure we
  **skip** (log, no CLAUDE.md). To avoid leaving the user worse off, the two
  mutating branches roll back:
  - *Regular file:* rename to the dated backup → attempt symlink → on failure
    rename the backup back to `CLAUDE.md`.
  - *Wrong symlink:* remember the old target → remove → attempt new symlink →
    on failure recreate the old symlink.
- **Self-write loop.** The watcher fires on our own symlink create/remove. That
  is harmless: a subsequent `run_check` sees the correct symlink and does
  nothing (idempotent), so no baseline suppression is needed.

## Code changes

- **`logic.rs`** — replace the hash-pair `PairState` / `decide()` machine with a
  pure decision over an enum describing CLAUDE.md's kind
  (`Missing` / `CorrectSymlink` / `WrongSymlink` / `RegularFile`) plus whether
  AGENTS.md exists. Output an action enum
  (`None` / `CreateSymlink` / `BackupThenSymlink` / `RepointSymlink` /
  `RemoveDangling`). Keep it unit-tested with the same table above.
- **`mod.rs`** — `run_check` classifies on-disk state, calls the pure `decide`,
  performs the filesystem action (with rollback), and ensures `.gitignore`.
  Delete `prompt_conflict`, `backup_to_temp`, the `prompting` flag, and all
  baseline load/save. `edit_agents_md` / `open_agents_in_editor` still write the
  template on first use, then call `run_check`.
- **`baseline.rs`** — deleted (no persisted state). Remove `agents_sync.json`
  usage. Leftover `agents_sync.json` files from prior versions are simply
  ignored.
- **`watcher.rs`** — unchanged (still non-recursive on the root, filtering
  AGENTS.md / CLAUDE.md create/modify/remove).
- **`templates/AGENTS.md`** — line 4 reworded from "auto-generated copy" to
  "a symlink to this file".

## Existing vaults / migration path

The app migrates on its own via the table above: an existing regular `CLAUDE.md`
is renamed to `CLAUDE.YYYYMMDD.md` and replaced by the symlink the next time the
sync service runs. Note this happens in the working tree only — the app never
touches the git index. Because the old `CLAUDE.md` was tracked, git will show it
as deleted plus a new untracked `CLAUDE.YYYYMMDD.md`; the user commits that at
their leisure. To stop `CLAUDE.md` re-appearing as tracked on other machines,
the user removes it from the index once (`git rm --cached CLAUDE.md`); the
`.gitignore` line the app wrote then keeps it ignored, and each machine's sync
service recreates its own local symlink.

## Non-goals

- No text merge, no conflict resolution UI (deleted).
- No git index mutation by the app.
- No Windows fallback to file-copy: symlink-only, skip on failure.
- No reverse-generation: if only `CLAUDE.md` exists and `AGENTS.md` is missing,
  do nothing (dangling *symlink* cleanup aside).

## Testing

- `logic.rs` unit tests for every row of the decision table.
- Filesystem-level tests (tempdir) for: create-when-missing, no-op on correct
  symlink, backup-then-symlink for a regular file (incl. collision suffixing),
  repoint of a wrong symlink, dangling-symlink removal, rollback when symlink
  creation fails (simulated), and `.gitignore` idempotent append.
