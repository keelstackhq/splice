# Getting started: a new repository with splice and hull

This guide takes you from nothing to a repository that is generated from the `hull` template
and kept current by a weekly pull request. Output blocks were captured from the real binary run
against a local copy of the test template (`tests/fixtures/template`, versions 1.0.0 and 2.0.0).
They are abridged (`...` marks cut lines, and `explain` detail lines and some `N changes`
summaries are left out), and hull's own file names will differ.

> Until the first release, neither the npm package `@keelstackhq/splice` nor the update Action
> `keelstackhq/splice/action@v1` is published. Use a release binary, or build one with
> `cargo build --release` (the binary is `target/release/splice`), and write `splice` instead of
> `pnpm dlx @keelstackhq/splice` in every command below.

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
clone it. If you give an unknown piece, splice stops with `unknown piece `nope`` (exit 2).

**Vars.** Run once without `--yes` to see the prompts and their defaults before you script it.
Every var has a prompt and may have a default. Values come from, in order, `--data
KEY=VALUE` (repeatable), a prompt on a terminal, or the default. With `--yes`, or without a
terminal, or with `--json`, splice never prompts. A var that has no default and no `--data`
is then an error:

```
$ splice new ./tpl//template a1 </dev/null
missing value for var `project`; pass --data project=…
exit 2
```

A complete non-interactive run (local fixture, abridged):

```
$ splice new ./tpl//template my-app --piece app:web --piece lib --data project=demo --yes
create   .editorconfig  owned
create   .github/workflows/ci.yml  shared
create   README.md  seed
create   apps/web/package.json  shared
create   apps/web/src/main.ts  seed
create   apps/web/tsconfig.json  owned
...
16 changes
```

`dest` defaults to `.`. A directory that already has a `.splice/lock.toml` is refused (use
`update` or `add`); a non-empty directory needs `--force`, which overwrites owned and shared
files and keeps seeds.

## 3. What you get

- Every file, in one of three classes (the first column of the output above): `owned`,
  `shared`, `seed`. Section 4 says what they mean for you.
- `.splice/lock.toml`: the template source, the version and commit you were generated from, the
  var values, the pieces (and instance names), and a sha256 per generated file. Commit it. Never
  edit it by hand; it is how `check` and `update` know what you changed.
- The template's post commands run after `new` (and `update`), for example installing
  dependencies. Their output goes to stderr. `--no-post` skips them. If an update stops on
  conflicts, post commands are skipped and splice prints the command to run by hand.
- Files that post commands write (in the fixture `.post-ran`; for hull, things like installed
  dependencies) are not tracked by splice. Commit the ones you want and put the rest in
  `.gitignore`.
- A git repository, if `dest` was not already inside one (`git init`). splice does not commit.
  Make the first commit yourself, so later updates start from a clean tree:

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
tool.config.mts  owned  base/base  glob=tool.config.mts  merge=text
$ splice explain apps/web/src/main.ts
apps/web/src/main.ts  seed  app/web  glob=src/**  merge=text
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
into shared files, and updates the lock. `--data KEY=VALUE` sets the piece's vars. Example, for a
project that was created without the `app` piece:

```
$ splice add app --yes
update   apps.txt  owned
create   apps/web/package.json  shared
create   apps/web/src/main.ts  seed
create   apps/web/tsconfig.json  owned
create   only-with-app.txt  owned
update   package.json  shared
6 changes
$ splice add app --name docs --yes
create   apps/docs/package.json  shared
...
$ splice add app --name docs --yes
duplicate instance `app:docs`; give it another name
exit 2
```

A dirty tree is refused with exit 2 (`--allow-dirty` overrides it, but then git cannot tell your
earlier edits from splice's). A piece that `requires` another must be added after it.

## 6. Stay current

Three read-only or preview commands, then one that writes:

```
$ splice check
0 changes
update available: 1.0.0 → 2.0.0
exit 1
```

`check` exits 1 for an available update, an edited owned file, or leftover conflict markers.
`splice check --offline` skips the version lookup (and makes no git call), so it only reports
drift and markers.

`splice diff` prints the unified diff of what `update` would change, and `splice diff --to 2.0.0`
targets a version. It writes nothing and needs no clean tree.

`splice update` needs a clean tree, then moves to the latest stable version (`--to VERSION` for
another; it never downgrades):

```
$ splice update
update   .editorconfig  owned
update   .github/workflows/ci.yml  shared
create   CHANGELOG.md  seed
conflict notes.txt  shared
delete   old.txt  owned
...
10 changes
notes.txt: conflicts: line 2
post commands skipped because of conflicts; run them by hand after resolving: `echo post> .post-ran`
exit 1
```

Version migrations (renames, removals, steps) listed by the template run as part of the update.
The lock is written last: if a migration step fails, the old lock stays.

**Resolving conflicts.** Open each file named in the output. The conflicting block looks like:

```
<<<<<<< ours
line two MINE
||||||| base
line two
=======
line 2 from template v2
>>>>>>> template
```

`ours` is your version, `base` the old template, below `=======` the new template. Keep what you
want, delete all four marker lines, then run `splice check`. It exits 0 when no markers or
drift remain. Run the post commands that were skipped by hand, then commit.

**Owned drift.** If you edited an owned file and committed the edit, `update` refuses and
writes nothing (an uncommitted edit stops earlier, at the dirty-tree check, exit 2):

```
$ splice update
apps/web/tsconfig.json: owned file was edited or deleted locally; update refused (--force replaces it)
nothing was written
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
are never touched. Both paths were run against the real binary.

**Restoring.** git is the undo button. If an update did something you do not want and the tree
was clean before, run this in the project directory:

```bash
git checkout -- . && git clean -fd
```

splice prints this advice itself when it applies (only when it verified the tree was clean).

## 7. Automate updates with the bot

Save as `.github/workflows/splice-update.yml`:

```yaml
name: splice-update
on:
  schedule: [{ cron: "17 6 * * 1" }]   # weekly, Monday 06:17 UTC
  workflow_dispatch:
permissions:
  contents: write
  pull-requests: write
jobs:
  update:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1
      - uses: keelstackhq/splice/action@v1
        with:
          token: ${{ secrets.SPLICE_BOT_TOKEN || github.token }}
```

It opens or refreshes one pull request from the branch `splice/update` (title prefixed
`[conflicts]` when markers were written), and closes it when you are current. Owned drift and
template errors fail the job. Inputs and outputs are in [`action/README.md`](../action/README.md).

**Why a token.** The default `GITHUB_TOKEN` cannot push changes under `.github/workflows/`, and
templates ship workflow files. Pull requests it opens also do not trigger your CI workflows, so
the bot PR would get no checks. Create one of:

1. A fine-grained personal access token: resource owner is you or your org, repository access is
   only this repository, permissions Contents, Pull requests and Workflows, each "Read and
   write".
2. A GitHub App installed on the repository with the same three permissions; generate an installation
   token in the workflow (for example with `actions/create-github-app-token`) and pass that as
   `token`.

Store it under Settings, Secrets and variables, Actions, as `SPLICE_BOT_TOKEN`. Without it the
`||` falls back to `github.token` and the limits above apply. Run the workflow once by hand
(Actions, splice-update, Run workflow) to see it work. Merge the PR like any other.

## 8. Troubleshooting by exit code

| Code | Meaning | Common messages and what to do |
|---|---|---|
| 0 | ok; `check` found nothing | |
| 1 | drift, conflicts, or an update available | `update available: A → B`: run `splice diff`, then `splice update`. `owned file was edited or deleted locally; update refused`: revert it or use `--force`. `conflicts: line N` or markers in `check`: resolve them (section 6). |
| 2 | usage error | `the working tree has uncommitted changes; commit or stash them, or pass --allow-dirty`. `missing value for var `X`; pass --data X=…`. `unknown piece `P``. `duplicate instance `app:docs`; give it another name`. `<dir> has no .splice/lock.toml; use `splice new` first` (run in the project, or pass `-C <dir>`). `./dest already has .splice/lock.toml; use `splice update` or `splice add``. `target version X is older than the locked version Y; splice does not downgrade`. |
| 3 | template or environment error | `template source X not found`: wrong path, or no access to the repository. `template needs splice >= X (running Y); run `pnpm dlx @keelstackhq/splice@latest ...``: upgrade splice. Bad manifest, render error, a git failure, or a failed migration step or post command: read the message on stderr; after a failed update the lock still matches the old state, so fix the cause and rerun. |

Add `--json` to any command for one machine-readable object on stdout (`ok`, `command`,
`changes`, `messages`); everything else goes to stderr.
