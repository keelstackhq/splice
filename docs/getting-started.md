# Getting started

> Human-only guide. Agents read `skills/splice/SKILL.md`. Flags and JSON: [reference](reference.md).

From nothing to a repo generated from a template and kept current by a weekly pull request.
Outputs below are shortened captures from the real binary. Run `splice` as
`pnpm dlx @keelstackhq/splice` if it is not installed (needs `git` on `PATH`; binaries and
`cargo build --release` are in [the reference](reference.md#install)).

## 1. Create

```bash
splice new gh:keelstackhq/hull//template my-app --piece <piece-id> --data project=demo --yes
cd my-app && git add -A && git commit -m "chore: initial commit from template"
```

`gh:keelstackhq/hull//template` = repo `keelstackhq/hull`, subdirectory `template`. The base piece
is always rendered; `--piece ID` adds more (`ID:NAME` names one of several instances). `--ref`
picks a tag or branch, `--pre` allows pre-releases. `splice new` runs `git init` if needed but
**never commits**: make the first commit yourself (`--commit` does it for you), so later
updates start from a clean tree.

There is no list command: read the template's `splice.toml` and `pieces/*/piece.toml`. A wrong
piece name lists the real ones. Var values come from `--data K=V`, a prompt on a terminal, or the
var's default; with `--yes`, `--json` or no terminal a missing one is an error (exit 2).

`.splice/lock.toml` records source, version, commit, vars, pieces and a sha256 per file. Commit
it, never edit it. Post commands (e.g. install) run after `new` and `update`; output goes to
stderr, `--no-post` skips them. Files they write are not tracked: commit or ignore them.

## 2. Who owns a file

```
$ splice explain package.json
package.json  shared  base/base  glob=package.json  merge=json
```

| Class | For you | On `splice update` |
|---|---|---|
| `owned` | Do not edit. | Replaced. If edited, update is refused (exit 1) until you revert or pass `--force`. |
| `shared` | Edit freely. | 3-way merged: JSON/YAML/TOML by key, text by line. Real conflicts: markers (text, TOML) or your value kept and named in messages (JSON, YAML). |
| `seed` | Yours. | Never touched. |
| unmanaged | Not splice's. | Ignored. |

To change an owned file, change the template. JSON/YAML lose comments on a real merge; TOML keeps them.

## 3. Add a piece

```bash
splice add <piece-id>                # renders at the locked version, never upgrades
splice add <piece-id> --name docs    # a second instance of a `multiple` piece
```

Needs a clean tree. Unlike `update`, `add` never writes markers: on a conflict or owned drift it
lists paths, writes nothing, exits 1. A piece that `requires` another goes after it.

## 4. Stay current

```
$ splice check
update  tool.config.mts  owned
update available: 1.0.0 → 2.0.0
exit 1
```

`check` exits 1 for an edited owned file, leftover markers or an available update; clean prints
nothing. `--offline` skips the version lookup. `splice diff [--to V]` shows what `update` would do
and writes nothing. `splice update [--to V | --ref R]` needs a clean tree, runs migrations, and
writes the lock last (a failed step keeps the old lock).

**Conflicts.** Open each file `update` names. Text and TOML get diff3 markers:

```
<<<<<<< ours
MINE Template fixture.
||||||| base
Template fixture.
=======
Template fixture, second edition.
>>>>>>> template
```

Keep what you want, delete the marker lines, run `splice check` (exit 0 when none are left; it
cannot see JSON/YAML key conflicts, so read the messages). Run the skipped post commands, commit.
To abort: `git checkout -- . && git checkout HEAD -- .splice/lock.toml`.

**Owned drift.** `update` refuses and writes nothing:

```
`apps/web/tsconfig.json` is owned by the template but was edited locally
nothing was written; revert the files above or pass --force to replace them
```

Revert (`git checkout <commit-before-edit> -- <file>` and commit) or `splice update --force`
(your edit stays only in git history). git is the undo button: `git checkout -- . && git clean -fd`.

## 5. Channels (optional)

A channel is a second template (shared agent rules, org decisions) with its own lock in
`.splice/channels/NAME.toml`.

```bash
splice channel add gh:keelstackhq/decisions//channel --track main --piece import-project
splice update --all     # main template and every channel
```

Never edit inside a `<!-- splice:NAME:begin ... -->` block. Flags: reference "Channels".

## 6. Automate updates

The `keelstackhq/splice/action` Action runs `splice update`, opens or refreshes one PR from
`splice/update` (`[conflicts]` prefix when conflicts were written), and closes it when you are
current. Owned drift and template errors fail the job. Inputs: [reference](reference.md#update-bot).

Plain `GITHUB_TOKEN` cannot push changes under `.github/workflows/`, and its PRs trigger no CI.
Use a GitHub App token (below), or **session mode**: a person or agent runs
`git switch -C splice/update-manual && splice update --all --yes && splice check`, commits,
pushes and opens the PR.

**App token.**
1. Create a GitHub App in your org, no webhook. Permissions: Contents, Pull requests, Workflows,
   all read and write. Install it here, and on any private template or channel repo (then add
   `owner:` and `repositories:` to the token step: it also fetches templates).
2. Generate a private key; note the client ID.
3. Create environment `splice-bot` limited to branch `main`, no reviewers or wait timer. Add
   secret `SPLICE_APP_PRIVATE_KEY` and variable `SPLICE_APP_CLIENT_ID`.
4. Save as `.github/workflows/splice-update.yml`, then run it once by hand:

```yaml
name: splice-update
on:
  schedule: [{ cron: "17 6 * * 1" }]   # weekly, Monday 06:17 UTC
  workflow_dispatch:
permissions:
  contents: read   # the Action pushes and opens the PR with the app token only
jobs:
  update:
    runs-on: ubuntu-latest
    environment: splice-bot
    steps:
      - uses: actions/create-github-app-token@64ab729a6294000f5aa4a4bc0b73d1f32bc45510 # v3.3.0
        id: app-token
        with:
          client-id: ${{ vars.SPLICE_APP_CLIENT_ID }}
          private-key: ${{ secrets.SPLICE_APP_PRIVATE_KEY }}
          permission-contents: write
          permission-pull-requests: write
          permission-workflows: write
      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1
        with:
          persist-credentials: false
      - uses: keelstackhq/splice/action@5d64f250fd1e0fcf27690b425a2f9f8de5fd4ac0 # v0.3.0
        with:
          token: ${{ steps.app-token.outputs.token }}
```

Actions are pinned by commit SHA with the tag as a comment, so Renovate or Dependabot can bump
both. A fine-grained personal access token (Contents, Pull requests, Workflows; this repo only)
stored as `SPLICE_BOT_TOKEN` works if you cannot create an app, but it is tied to a person.

## 7. Exit codes

0 ok · 1 drift, conflicts or update available · 2 usage (dirty tree, missing `--data`, unknown
piece, no lock: run in the project or `-C <dir>`) · 3 template or environment error (bad
manifest, git failure, failed step or post; the lock still matches the old state, fix and rerun).
`--json` prints one object `{ok, command, changes, messages}` on stdout:

```
$ splice check --json --offline
{"ok":false,"command":"check","changes":[{"path":"tool.config.mts","action":"update","class":"owned"}],"messages":[]}
```
