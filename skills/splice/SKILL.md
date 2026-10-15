---
name: splice
description: Use when a repo has .splice/lock.toml or the user asks to create, update or check a project from a keelstack template (splice new, add, update, check, diff, explain). Covers JSON output, exit codes, file ownership and resolving update conflicts.
---

# splice

splice builds a repo from versioned template pieces and keeps it current with 3-way merges.
Run it as `splice`, or `pnpm dlx @keelstackhq/splice` if it is not installed.

## Rules

- Always pass `--json`. Stdout is then exactly one object:
  `{ok, command, changes: [{path, action, class}], messages}`. Post-command and git output go to stderr.
- Read `changes` and `messages`, not the human text. Pass `--yes` and `--data K=V` so nothing prompts.
- Never edit `.splice/lock.toml` by hand.
- Never pass `--allow-dirty` or `--force` unless the user asked for it.

## Exit codes

| Code | Meaning |
|---|---|
| 0 | ok; `check` found nothing |
| 1 | `check`: owned drift, conflict markers or an update is available. `update`/`add`: conflicts were written, or refused because of owned drift |
| 2 | usage: bad flags, missing `--data` value, dirty git tree, downgrade, no lock |
| 3 | template or environment error: bad manifest, `min_splice` too new, render error, git failure, failed migration or post command |

## Before editing a generated file

Run `splice explain <path> --json`. The class is in `changes[0].class`.

- `owned`: the template owns it. Change the template instead. A local edit makes `update` refuse
  (exit 1) until you revert it or the user accepts `--force`.
- `shared`: edit freely. `update` merges your change with the template's.
- `seed`: written once, yours. `update` never touches it.
- `unmanaged` in `messages`: not a splice file.

## Commands

- `splice check --json --offline`: drift and markers, no network. Without `--offline` it also
  looks for a newer template tag.
- `splice diff --json`: what `update` would change. Never writes, needs no clean tree.
- `splice update --json --yes [--to VERSION]`: needs a clean git tree (else exit 2).
- `splice add <piece> --json --yes [--name N]`: same tree rule and same merge rules as `update`.
- `splice new <source> [dest] --json --yes [--piece ID[:NAME]] [--data K=V]`

Dirty tree: ask the user to commit or stash, then run it again.

## Conflicts (`update` or `add` exit 1)

1. List files in `changes` with `action` `conflict`.
2. In each, find the blocks `<<<<<<< ours`, `||||||| base`, `=======`, `>>>>>>> template`.
   Keep the intent of both sides, then delete all marker lines.
3. Run `splice check --json --offline`. Done at exit 0. Any marker line left gives exit 1.
4. Post commands are skipped on conflict; the messages list them. Run them by hand.

`update`/`add` can also exit 1 with no changes when conflict markers are still on disk. `--force` does not override this.
Resolve them first, then run `splice check --json --offline`.

Owned drift refusal (exit 1, nothing written): the files are listed in `messages`. Revert them, or
move the change into the template, then run `update` again.

## After a failed update

Nothing is lost if the tree was clean before the run. Only then, and only if the user wants to
undo it: `cd <project dir> && git checkout -- . && git clean -fd`. If the tree was dirty
(`--allow-dirty`), git cannot tell the earlier changes apart, so do not run it.
