---
name: splice
description: Use when a repo has .splice/lock.toml or files in .splice/channels/, or the user asks to create, update or check a project from a keelstackhq template (splice new, add, update, check, diff, explain, channel add or remove), or to resolve splice merge conflicts.
---

# splice

splice renders versioned template pieces into a repo and keeps them current with a 3-way merge.
The state lives in `.splice/lock.toml` (main template) and `.splice/channels/NAME.toml`
(channels). Run it as `splice`, or `pnpm dlx @keelstackhq/splice` when it is not installed.

## Always

- Pass `--json`. Stdout is exactly one object `{ok, command, changes, messages}`. Each change
  is `{path, action, class}`. The last message of a failure says what went wrong and how to fix it.
- Read the exit code, not just the text:

| Code | Meaning |
|---|---|
| 0 | ok, nothing to do |
| 1 | drift, conflicts written, or an update is available |
| 2 | usage: bad flag, dirty tree, missing value, unmet `requires` |
| 3 | template or environment error (also a failed step after files were written) |

## Before editing a generated file

Run `splice explain <path> --json` and read the `class` and `source` messages.

- `owned`: the template replaces it on every update. Change the template, or accept that the
  next `splice update` refuses the drift (exit 1) until you pass `--force`.
- `shared`: merged by key or by line. Edit freely.
- `seed`: written once. It is yours.
- `unmanaged`: not from splice.

## Commands

- `splice new <source> [dest] --piece ID[:NAME] --data K=V --yes --json`
- `splice add <piece> --name NAME --json`
- `splice diff --json` shows what `update` would do and writes nothing. Run it first.
- `splice update --json`, optionally `--to VERSION`, or `--ref R` to move an untagged commit.
- `splice check --json --offline` is the cheap, network-free verification.

## Rules

- Never edit `.splice/lock.toml` or `.splice/channels/*.toml`. Never hand-fix a version in them.
- A dirty git tree is refused (exit 2). Commit or stash first. Do not pass `--allow-dirty`
  unless the user asked for it.
- After a failed `add` or `update` (exit 3), follow the restore command in the last message
  (a `git checkout` and `git clean`). Do not retry on top of a half-written tree.
- Do not pass `--force` unless the user accepts losing their edits to owned files.
- Do not pass `--no-post` to hide a failing post command; fix the command.

## Resolving conflicts

1. `splice update --json` exits 1 and lists `conflict` changes. Open each file.
2. Edit every `<<<<<<<` / `|||||||` / `=======` / `>>>>>>>` block. Keep the intent of both
   sides: the template's change and the local edit. Delete all markers.
3. Run `splice check --json --offline`. It exits 0 only when no markers or owned drift remain.
4. Run the repo's own checks, then commit.

## Channels

- Channels exist when `.splice/channels/` has files. A channel-only repo has no main lock.
- `splice explain <path> --json` names the owner: `source=main` or `source=channel:NAME`.
- Never edit inside a `<!-- splice:NAME:begin ... -->` block; change the channel source instead.
- `splice check --json` covers the main lock and every channel; `splice update --all` updates all.
- `splice update --channel NAME` and `splice diff --channel NAME` work on one channel.
- `splice channel add <source> --piece ID --track main` and `splice channel remove NAME` change
  the set of channels. Ask before running either.
