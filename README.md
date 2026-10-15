# splice

Start a repo from versioned template pieces and keep it up to date. A Copier alternative in one
static binary: no Python, no Rust toolchain. Part of [keelstack](https://github.com/keelstackhq):
tools for bootstrapping projects.

## Install

With pnpm (Node 20+) you can run splice without installing it:

```bash
pnpm dlx @keelstackhq/splice --version
```

`@keelstackhq/splice` is a small launcher. The binary comes from a per-platform package
(`@keelstackhq/splice-linux-x64`, `-linux-arm64`, `-darwin-x64`, `-darwin-arm64`, `-win32-x64`).
There are no install scripts.

Or download a binary from the GitHub release for your target (`splice-<target>.tar.gz`, `.zip` on
Windows) and check it against `SHA256SUMS`. Targets: `x86_64-unknown-linux-musl`,
`aarch64-unknown-linux-musl`, `x86_64-apple-darwin`, `aarch64-apple-darwin`,
`x86_64-pc-windows-msvc`. `git` must be on `PATH`: all fetching goes through it.

## Quick start

New to splice? Follow [`docs/getting-started.md`](docs/getting-started.md), a step-by-step guide
for a new repository.

```bash
splice new gh:keelstackhq/hull//template my-app
cd my-app
splice add preact-app --name web
splice update        # 3-way merge to the latest template version
splice check         # exit 1 on drift, conflicts or an available update
```

Prefix each command with `pnpm dlx @keelstackhq/` if splice is not installed. `update` and `add`
need a clean git tree: commit first.

## Commands

| Command | What it does |
|---|---|
| `splice new <source> [dest]` | Render the template's base piece and any `--piece ID[:NAME]` into `dest`, write `.splice/lock.toml`. `--data K=V` sets a var, `--ref` picks a ref, `--pre` allows pre-releases. |
| `splice add <piece>` | Add a piece (or another instance of a `multiple` piece, with `--name`) at the lock's template version. |
| `splice update` | Move to the latest stable version, or `--to VERSION`. Your edits are merged, never lost. |
| `splice check` | Exit 1 on edited owned files, leftover conflict markers, or a newer version. `--offline` skips the version lookup and makes no git call. It never writes. |
| `splice diff` | Show what `update` would change (`--to VERSION`). Writes nothing and needs no clean tree. |
| `splice explain <path>` | Which piece owns a file, its class and the matching glob. |

Flags on every command: `--json` (stdout is exactly one JSON object, other output goes to
stderr), `--yes` (no prompts, use defaults) and `-C <dir>` (project root). Without a TTY or with
`--json` splice never prompts; a missing value is a usage error naming the `--data` flag.

`new`, `add` and `update` also take `--no-post` (skip the template's post commands) and
`--force`. `update` and `add` take `--allow-dirty`: they refuse a dirty git tree (exit 2) without
it, and with it git cannot separate your earlier changes from splice's.

`--force` on `update` and `add` only replaces owned files that you edited or deleted. Shared
files still merge, and seeds are never touched. On `new` it allows a non-empty `dest`:
owned and shared files are overwritten, seeds kept.

### Exit codes

| Code | Meaning |
|---|---|
| 0 | ok; `check` found nothing |
| 1 | `check`: drift, markers or update available. `update`/`add`: conflicts written, or refused because of owned drift |
| 2 | usage error: bad flags, missing var value, dirty tree, downgrade, no lock |
| 3 | template or environment error: bad manifest, `min_splice` too new, render error, git failure, failed migration or post command |

If an update goes wrong after files were written and the tree was clean before it, undo it with
`git checkout -- . && git clean -fd` in the project directory. The lock is written last, so a failed
migration step leaves the old lock in place.

## How files are treated

The classes and globs are defined in [`docs/template-format.md`](docs/template-format.md).

| Class | On `update` |
|---|---|
| `owned` | Replaced. Local edits block the update (exit 1, nothing written) until reverted or `--force`. |
| `shared` | 3-way merged (JSON, YAML, TOML by key; text by line). Conflicts get diff3 markers and exit 1. |
| `seed` | Written once, then yours. Never modified. |

After a conflict, edit the files between `<<<<<<< ours` and `>>>>>>> template` (the `|||||||
base` part shows the common ancestor), remove the marker lines, then run `splice check`.
JSON and YAML lose comments on a real merge; TOML keeps them.

## Update bot

```yaml
# .github/workflows/splice.yml
name: splice-update
on:
  schedule: [{ cron: "17 6 * * 1" }]   # weekly, Monday
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

It opens or refreshes one pull request from branch `splice/update` with a summary of the
changes, prefixes the title with `[conflicts]` when there are conflict markers, and closes the
PR when the repo is already current. Owned drift and template errors fail the job.

Pass a GitHub App token or a fine-grained token as `SPLICE_BOT_TOKEN`. Without that secret the
example falls back to `GITHUB_TOKEN`, which cannot push changes to `.github/workflows/*`, and
PRs it opens do not trigger CI. Inputs, outputs and
details: [`action/README.md`](action/README.md).

### Renovate instead

A Renovate `customManagers` regex entry on `.splice/lock.toml` (`version = "..."`, datasource
`github-tags`) plus a `postUpgradeTasks` entry running `splice update --to {{newVersion}} --yes`
works. `postUpgradeTasks` only run on self-hosted Renovate with `allowedCommands` set. Without
them Renovate would only change the version string, which breaks the lock. So the Action is
the default.

## Writing templates

The template format (layout, manifests, the rendering subset, ownership globs, the lock, version
migrations, JSON shape) is in [`docs/template-format.md`](docs/template-format.md). Template
rendering is a small frozen subset, so old template versions keep rendering the same.

## Agents

`skills/splice/SKILL.md` is a Claude Code skill: always `--json`, `explain` before editing a
generated file, resolve conflicts by editing marker blocks, never edit the lock.

## Compared with Copier

Copier is mature but needs Python, and it replays old template versions under the new Copier,
which breaks when the template language moves. splice is one static binary, renders a frozen
subset, and a template pins its minimum splice with `min_splice`. It does the same 3-way update,
and adds structured merges for JSON, YAML and TOML. It has no hooks beyond the template's post
commands and migration steps.

## License

MIT, see [LICENSE](LICENSE).
