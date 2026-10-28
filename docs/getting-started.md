# Getting started: a new repository with splice and hull

This guide takes you from nothing to a repository that is generated from the `hull` template
and kept current by a weekly pull request. The output blocks were captured from the real
binary (splice 0.2.0) against a local copy of the test template (`tests/fixtures/template`,
versions 1.0.0 and 2.0.0). They are shortened, and hull's own file names will differ.

> Without Node, use a release binary, or build one with `cargo build --release` (the binary is
> `target/release/splice`), and write `splice` instead of `pnpm dlx @keelstackhq/splice` in every
> command below.

## 1. Prerequisites

- `git` on `PATH`. splice fetches templates only through the git CLI.
- One of: Node 20+ with pnpm (`pnpm dlx @keelstackhq/splice ...` runs it without installing), or
  a binary from the GitHub release (see the README, "Install"), or `cargo build --release`.

Check with `splice --version`.

## 2. Create the repository

```bash
pnpm dlx @keelstackhq/splice new gh:keelstackhq/hull//template my-app --piece <piece-id>
```

`gh:keelstackhq/hull//template` means: repository `keelstackhq/hull`, subdirectory `template`.
`splice new` always renders the template's base piece. `--piece ID` adds more, and
`--piece ID:NAME` names an instance of a piece that may be used several times (for example two
apps). Repeat the flag for several pieces. `--ref` picks a tag or branch instead of the latest
stable version, and `--pre` allows pre-releases.

**Finding the pieces and vars.** There is no command that lists them. Read the template: each
directory under `pieces/` is a piece, with its `piece.toml` (id, description, `requires`,
`multiple`, and its own `[vars]`), and the top-level `splice.toml` declares the template-wide
`[vars]` and the base piece. Browse them at `github.com/keelstackhq/hull/tree/main/template`, or
clone it. If you name a piece that does not exist, splice lists the ones that do:

```
$ splice new ./tpl a2 --piece nope --data project=x --yes
unknown piece `nope`; the template has: app, base, lib
exit 2
```

**Vars.** Values come from, in order, `--data KEY=VALUE` (repeatable), a prompt on a terminal,
or the var's default. With `--yes`, `--json`, or no terminal, splice never prompts. A var with
no default and no `--data` is then an error:

```
$ splice new ./tpl a1 </dev/null
missing value for var `project`; pass --data project=…
exit 2
```

A complete non-interactive run (local fixture, shortened):

```
$ splice new ./tpl my-app --data project=demo --yes --ref v1.0.0
create  .editorconfig  owned
create  .github/workflows/ci.yml  shared
create  README.md  seed
create  apps.txt  owned
create  config/settings.toml  shared
...
```

`dest` defaults to `.`. A directory that already has a `.splice/lock.toml` is refused (use
`update` or `add`). A non-empty directory needs `--force`, which overwrites owned and shared
files and keeps seeds.

## 3. What you get

- Every file, in one of three classes (the last column of the output above): `owned`,
  `shared`, `seed`. Section 4 says what they mean for you.
- `.splice/lock.toml`: the template source, the version and commit you were generated from, the
  var values, the pieces (and instance names), and a sha256 per generated file. Commit it. Never
  edit it by hand: it is how `check` and `update` know what you changed.
- The template's post commands run after `new` (and `update`), for example installing
  dependencies. Their output goes to stderr. `--no-post` skips them. If an update stops on
  conflicts, post commands are skipped: run them by hand once you have resolved the conflicts.
- Files that post commands write (in the fixture `.post-ran`; for hull, things like installed
  dependencies) are not tracked by splice. Commit the ones you want and put the rest in
  `.gitignore`.
- A git repository, if `dest` was not already inside one (`new` runs `git init`). splice does
  not commit. Make the first commit yourself, so later updates start from a clean tree:

```bash
cd my-app
git add -A
git commit -m "chore: initial commit from hull"
```

## 4. Daily use

Before you edit a file you did not write, ask splice who owns it:

```
$ splice explain package.json
package.json  shared  base/base  glob=package.json  merge=json
$ splice explain tool.config.mts
tool.config.mts  owned  base/base  glob=tool.config.mts  merge=none
$ splice explain src/x
src/x  unmanaged
```

| Class | For you | On `splice update` |
|---|---|---|
| `owned` | Do not edit. The template controls it. | Replaced. If you edited it, the update is refused (exit 1, nothing written) until you revert the edit or pass `--force`. |
| `shared` | Edit freely; add your own keys and lines. | 3-way merged: JSON, YAML and TOML by key, text by line. Real conflicts get markers. |
| `seed` | Yours from day one. | Never touched. |
| unmanaged | Not splice's. | Ignored. |

If you need to change an owned file, change the template instead, or accept that `update`
needs `--force` for it. JSON and YAML lose comments when a real merge rewrites them; TOML keeps
them.

## 5. Add a piece later

```bash
git status                      # must be clean: commit first
pnpm dlx @keelstackhq/splice add <piece-id>
pnpm dlx @keelstackhq/splice add <piece-id> --name docs      # a second instance of a `multiple` piece
```

`add` renders the piece at the template version in your lock (it does not upgrade you), merges
into shared files, and updates the lock. `--data KEY=VALUE` sets the piece's vars. Unlike
`update`, `add` never writes conflict markers: on a conflict or on owned drift it lists the
paths, writes nothing and exits 1. Example, for a project that was created without the `app`
piece:

```
$ splice add app --yes
update  apps.txt  owned
create  apps/web/package.json  shared
create  apps/web/src/index.html  seed
create  apps/web/src/main.ts  seed
create  apps/web/tsconfig.json  owned
create  only-with-app.txt  owned
update  package.json  shared
$ splice add app --name docs --yes
create  apps/docs/package.json  shared
create  apps/docs/src/index.html  seed
...
$ splice add app --name docs --yes
piece `app` already has an instance named `docs`
exit 2
```

A dirty tree is refused with exit 2 (`--allow-dirty` overrides it, but then git cannot tell your
earlier edits from splice's). A piece that `requires` another must be added after it.

## 6. Stay current

Two commands that never write, then one that does:

```
$ splice check
update  tool.config.mts  owned
update available: 1.0.0 → 2.0.0
exit 1
```

`check` exits 1 for an available update, an edited owned file (listed above as `update ...
owned`, here because `tool.config.mts` was edited), or leftover conflict markers. Without any
of these it prints nothing and exits 0. `splice check --offline` skips the version lookup (and
makes no git call), so it only reports drift and markers.

`splice diff` prints the unified diff of what `update` would change, and `splice diff --to
2.0.0` targets a version. It writes nothing, needs no clean tree and exits 0 even when the
update would conflict.

`splice update` needs a clean tree, then moves to the latest stable version (`--to VERSION` for
another; it never downgrades). `--ref BRANCH|SHA` works from any lock; the target must descend
from the locked commit (`--force` overrides). A plain update from an untagged lock takes only a
stable tag that descends from its commit:

```
$ splice update
migration 2.0.0: Record that the migration ran
update  .editorconfig  owned
update  .github/workflows/ci.yml  shared
create  CHANGELOG.md  seed
conflict  notes.txt  shared
delete  old.txt  owned
...
`notes.txt` has conflicts (line 1); resolve the diff3 markers
conflicts in notes.txt: resolve the diff3 markers
exit 1
```

Version migrations (renames, removals, steps) listed by the template run as part of the update.
The lock is written last: if a migration step fails, the old lock stays.

**Resolving conflicts.** Open each file named in the output. The conflicting block looks like:

```
<<<<<<< ours
MINE Template fixture.
||||||| base
Template fixture.
=======
Template fixture, second edition.
>>>>>>> template
```

`ours` is your version, `base` the old template, below `=======` the new template. Keep what you
want, delete all four marker lines, then run `splice check`. It exits 0 when no markers or
drift remain. Run the post commands that were skipped by hand, then commit.

**Owned drift.** If you edited an owned file and committed the edit, `update` refuses and
writes nothing (an uncommitted edit stops earlier, at the dirty-tree check, exit 2):

```
$ splice update
`apps/web/tsconfig.json` is owned by the template but was edited locally
nothing was written; revert the files above or pass --force to replace them
exit 1
```

Either put the file back to what splice wrote and commit that, or replace it with the
template's version. To revert, check the file out from the commit before your edit and commit:

```bash
git checkout <commit-before-the-edit> -- apps/web/tsconfig.json
git commit -m "revert tsconfig edit"     # or: git revert <the edit commit>
splice update
```

Or run `splice update --force`, which replaces owned files with the template's version (your edit
is gone from the working tree, though still in git history). Shared files still merge and seeds
are never touched.

**Restoring.** git is the undo button. If an update did something you do not want and the tree
was clean before, run this in the project directory:

```bash
git checkout -- . && git clean -fd
```

splice prints this advice itself when a step or post command fails after files were written.

## 7. Channels (optional)

A channel is a second template, for example shared agent rules or org decisions, that you add
next to the main one. It has its own lock in `.splice/channels/NAME.toml` and its own update
schedule, and it never changes `.splice/lock.toml`.

```bash
pnpm dlx @keelstackhq/splice channel add gh:keelstackhq/decisions//channel --track main --piece import-project
pnpm dlx @keelstackhq/splice update --all     # the main template and every channel
pnpm dlx @keelstackhq/splice check            # covers every channel too
```

`explain` then names the owner of a path (`source=main` or `source=channel:NAME`). Never edit
inside a `<!-- splice:NAME:begin ... -->` block that a channel put into a file of yours: change
the channel source instead. The README section "Channels" lists the flags (`--only`,
`--exclude`, `--track`, `--ref`, `channel remove`), and the design is in
[`docs/superpowers/specs/2026-10-17-splice-channels-design.md`](superpowers/specs/2026-10-17-splice-channels-design.md).

## 8. Automate updates with the bot

The bot is the `keelstackhq/splice/action` Action: it runs `splice update` and opens or refreshes
one pull request from the branch `splice/update` (title prefixed `[conflicts]` when markers were
written), and closes it when you are current. Owned drift and template errors fail the job. If
the repo has channels, the bot updates them too (the `scope` input, default `all` when
`.splice/channels/` holds a `*.toml`). Inputs and outputs are in
[the README](../README.md#update-bot).

**What plain `GITHUB_TOKEN` cannot do.** Templates ship workflow files, and the default
`GITHUB_TOKEN`:

- cannot push changes under `.github/workflows/` (the push is refused), so any template update
  that touches a workflow fails;
- opens a pull request that triggers no `pull_request` workflows, so the bot PR gets no CI checks
  and a branch ruleset that requires them blocks the merge.

It works only while no update touches a workflow and you accept PRs without CI. Pick one of the
paths below instead; none needs a personal access token.

### 8.1 A GitHub App token (recommended)

1. Create a GitHub App owned by your org (Settings, Developer settings, GitHub Apps, New). No
   webhook. Repository permissions: **Contents: Read and write**, **Pull requests: Read and
   write**, and **Workflows: Read and write** (updates that change files under
   `.github/workflows/` need it, and hull's do; the workflow below requests it, so token creation
   fails without it). Install it on this repository. If the template or a channel repo is
   private, install it there too and add `owner:` and `repositories:` (this repo and the
   template repos) to the token step, because the token also fetches the templates.
2. Generate a private key for the app. Note its client ID.
3. In the repository, create an environment `splice-bot` (Settings, Environments). Under
   "Deployment branches and tags" choose "Selected branches and tags" and add `main` only. Add
   the private key as the environment secret `SPLICE_APP_PRIVATE_KEY` and the client ID as the
   environment variable `SPLICE_APP_CLIENT_ID`. Only jobs that run on `main` and name this
   environment can read the key. The default branch must be `main`, since scheduled runs start
   there, and the environment needs no required reviewers or wait timer, or the weekly run waits.
4. Save as `.github/workflows/splice-update.yml`:

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

The token is short-lived (one hour) and is revoked when the job ends. `create-github-app-token`
v3.3.0 is commit `64ab729…` (2026-10-12), and `keelstackhq/splice/action` v0.3.0 is commit
`5d64f25…`. Actions are pinned by full commit SHA, with the tag as a comment; Renovate's
`github-actions` manager (or Dependabot) reads the comment and bumps SHA and comment together.
Run the workflow once by hand (Actions, splice-update, Run workflow) to see it work. Merge the PR
like any other.

### 8.2 Session mode (no bot workflow)

No workflow and no stored token: a person, or an agent session with push access, runs the update
on a branch and opens the PR. CI runs as for any PR you push.

```bash
git switch main && git pull
git switch -C splice/update-manual   # not the bot's branch
pnpm dlx @keelstackhq/splice update --all --yes   # main template and every channel
pnpm dlx @keelstackhq/splice check                # exit 0, or fix what it lists
git add -A && git commit -m "chore: splice update"
git push -u origin splice/update-manual
gh pr create   # or open the PR in the GitHub UI
```

`update` exits 0 when it applied cleanly or there was nothing to do. Exit 1 means owned drift
(nothing written: revert the files or pass `--force`) or conflicts (diff3 markers written:
resolve them, section 6, before you commit). `splice check` then confirms no markers are left.
Run `splice check` from CI or a scheduled job if you want to be told when an update is
available (exit 1).

### 8.3 A fine-grained personal access token (fallback)

Only if you cannot create an app: resource owner is you or your org, repository access is only
this repository, permissions Contents, Pull requests and Workflows, each "Read and write". Store
it in the `splice-bot` environment (as in 8.1) as `SPLICE_BOT_TOKEN` and pass
`token: ${{ secrets.SPLICE_BOT_TOKEN }}` instead of the app token step. It is tied to a person
and lives until it expires, which is why the app is preferred.

## 9. Troubleshooting by exit code

| Code | Meaning | Common messages and what to do |
|---|---|---|
| 0 | ok; `check` found nothing | |
| 1 | drift, conflicts, or an update available | `update available: A → B`: run `splice diff`, then `splice update`. `... is owned by the template but was edited locally`: revert it or use `--force`. `... has conflicts` or `unresolved conflict markers` in `check`: resolve them (section 6). |
| 2 | usage error | `the working tree has uncommitted changes; commit or stash them first (or pass --allow-dirty)`. `missing value for var `X`; pass --data X=…`. On `update` of a piece var, with `--channel` or `--all`, and in `splice diff`, the message says to give the var a default in the template or to rerun the command at a terminal, because `--data` is not accepted there. `unknown piece `P``. `piece `app` already has an instance named `docs``. `<dir> has no .splice/lock.toml; run `splice new` first` (run in the project, or pass `-C <dir>`). `<dir> already has a .splice/lock.toml; use `splice update` or `splice add``. `target X is older than the locked Y`. |
| 3 | template or environment error | `template source X is not a git repository`: wrong path or URL, or no access. `template needs splice >= X (running Y); run `pnpm dlx @keelstackhq/splice@latest ...``: upgrade splice. Bad manifest, render error, a git failure, or a failed migration step or post command: read the message on stderr. After a failed update the lock still matches the old state, so fix the cause and rerun. |

Add `--json` to any command for one machine-readable object on stdout (`ok`, `command`,
`changes`, `messages`); everything else goes to stderr:

```
$ splice check --json --offline
{"ok":false,"command":"check","changes":[{"path":"tool.config.mts","action":"update","class":"owned"}],"messages":[]}
```
