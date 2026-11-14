---
name: splice
description: Use when a repo has .splice/lock.toml or files in .splice/channels/, or the user asks to create, update or check a project from a keelstackhq template (splice new, add, update, check, diff, explain, channel add or remove), or to resolve splice merge conflicts.
---

# splice
splice renders versioned template pieces into a repo and keeps them current with a 3-way merge.
State: `.splice/lock.toml` (main template), `.splice/channels/NAME.toml` (channels). Run `splice`,
or `pnpm dlx @keelstackhq/splice` when it is not installed.

## Always

- Pass `--json`. Stdout is one object `{ok, command, changes, messages}`; a change is
  `{path, action, class}`. The last message of a failure says what to do next.
- Exit: 0 ok · 1 drift, conflicts written, or update available · 2 usage (bad flag, dirty tree,
  unmet `requires`) · 3 template or environment error (also a failed step).

## Before editing a generated file

Run `splice explain <path> --json`; read `class` and `source`.

- `owned`: replaced on every update. Change the template, or `update` refuses (exit 1) until `--force`.
- `shared`: merged by key or line. Edit freely. `seed`: written once, yours. `unmanaged`: not splice's.

## Commands

- `splice new <source> [dest] --piece ID[:NAME] --data K=V --yes --commit --json`: no commit
  without `--commit`, and `add`/`update` refuse a dirty tree. A var with no default needs
  `--data` (else exit 2). A local source needs a `vX.Y.Z` tag or `--ref`.
- `splice add <piece> --name NAME --json`: leaves changes to commit. `-C <dir>` replaces `cd`.
- `splice diff --json`: what `update` would do; writes nothing. Run it first.
- `splice update --json` (`--to VERSION`, or `--ref R`); `splice check --json --offline`: no network.

## Rules

- Never edit `.splice/lock.toml` or `.splice/channels/*.toml`. No `--no-post` to hide a failing post.
- A dirty tree is refused (exit 2): commit or stash. No `--allow-dirty` unless asked.
- After a failed `add` or `update` (exit 3), run the restore command in the last message first.
- No `--force` unless the user accepts losing edits to owned files (`update --force` on a current
  repo restores drifted owned files only).

## Resolving conflicts

1. `update` exits 1 with `conflict` changes. The last message says the lock is already at the
   target and how to abort (`git checkout -- . && git checkout HEAD -- .splice/lock.toml`).
2. Text and TOML files hold `<<<<<<<` / `|||||||` / `=======` / `>>>>>>>` blocks: merge both
   sides, delete the markers.
3. JSON and YAML files hold your value on each conflicting key (valid file, no markers). A
   message per key shows both values; edit the key if the template's value is wanted.
4. `splice check --json --offline` exits 0 when no markers or owned drift remain (it does not
   see key conflicts: use the messages). Run the repo's checks, then commit.

## Channels

- Channels exist when `.splice/channels/` has files; a channel-only repo has no main lock.
- `explain` names the owner: `source=main` or `source=channel:NAME`.
- Never edit inside a `<!-- splice:NAME:begin ... -->` block; change the channel source.
- `check` and `update --all` cover every lock; `--channel NAME` limits `update` and `diff`.
- `channel add <source> --piece ID --track main` and `channel remove NAME` change the set of
  channels: ask first.
