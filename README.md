# splice

Start a repo from versioned template pieces and keep it up to date. A Copier alternative in one
static binary: no Python, no Rust toolchain. Part of [keelstackhq](https://github.com/keelstackhq):
tools for bootstrapping projects.

> Status: 0.2.0 is released (GitHub release `v0.2.0`, npm `@keelstackhq/splice`, Action
> `keelstackhq/splice/action@v1`). Design:
> `docs/superpowers/specs/2026-10-14-splice-design.md`. New here? Start with
> [`docs/getting-started.md`](docs/getting-started.md).

## Install

- **npm:** `pnpm dlx @keelstackhq/splice <command>` (or `npx`). The launcher runs the prebuilt
  binary of your platform; there are no install scripts. `SPLICE_BINARY` runs a specific binary.
- **Release archives:** download `splice-<target>.tar.gz` (`.zip` on Windows) and `SHA256SUMS` from
  the GitHub release, check the sum, put `splice` on your `PATH`. Targets: Linux x86_64 and
  aarch64 (static musl), macOS x86_64 and aarch64, Windows x86_64.
- **From source:** `cargo install --locked --git https://github.com/keelstackhq/splice keel-splice`
  (the binary is `splice`).

```bash
pnpm dlx @keelstackhq/splice new gh:keelstackhq/hull//template my-app
cd my-app
pnpm dlx @keelstackhq/splice add preact-app --name web
pnpm dlx @keelstackhq/splice update        # 3-way merge to the latest template version
pnpm dlx @keelstackhq/splice check         # exit 1 on drift, conflicts or an available update
```

| Command | What it does |
|---|---|
| `splice new <source> [dest]` | Render the template's base piece and any `--piece` into `dest` |
| `splice add <piece>` | Add a piece (or another instance of a `multiple` piece); other files merge as on `update`, and a conflict writes nothing |
| `splice update [--to V \| --ref R]` | Move to a newer template version, or to a branch or sha with `--ref`; your edits are merged, never lost |
| `splice check` | Fail on edited owned files, leftover conflict markers, or a newer version |
| `splice diff [--to V \| --ref R]` | Show what `update` would change, as a unified diff; writes nothing |
| `splice explain <path>` | Which piece owns a file and how splice treats it |
| `splice channel add <source>` | Apply a second template next to the main one, with its own lock (see Channels) |
| `splice channel remove <name>` | Remove a channel: delete its unedited files and blocks, then its lock |

Every command takes `--json` (one JSON object on stdout) and `--yes` (no prompts). Exit codes:
0 ok, 1 drift/conflict/update available, 2 usage error, 3 template error. `add` and `update`
refuse a dirty git tree (exit 2) unless `--allow-dirty`.

**Untagged commits.** A repo rendered with `--ref <branch|sha>` records version `0.0.0+g<sha7>`.
`splice update --ref <branch|sha>` moves it to a later commit; the target must be a git
descendant of the locked commit (exit 2 otherwise, `--force` overrides). Without `--ref`, such a
repo moves only to a stable tag that descends from its commit, and `check` reports only those.

## How files are treated

| Class | On `update` |
|---|---|
| `owned` | Replaced. Local edits block the update (`--force` overrides). |
| `shared` | 3-way merged (JSON, YAML, TOML by key; text by line). Conflicts get diff3 markers. |
| `seed` | Written once, then yours. |

## Writing templates

A template is a directory with `splice.toml` and pieces. The format (manifests, the Jinja subset,
ownership classes, merges, migrations) is specified in
[`docs/template-format.md`](docs/template-format.md). It is shared with
[hull](https://github.com/keelstackhq/hull), the first template, and is binding for both repos.

## Compared with Copier

Copier is the closest tool: it also renders a template and updates it later. splice is one static
binary with no Python, so `pnpm dlx` is enough, and it classifies every file (`owned`, `shared`,
`seed`) so that `update` knows what may be replaced, merged or left alone. Templates use a small
Jinja subset that any implementation can follow, not Python-specific features. splice does not
run arbitrary template code; it runs only the `post` commands and migration steps a template
declares. It does not read Copier templates.

## Channels

A channel is a second format-1 template applied next to the main one (or alone), with its own
lock in `.splice/channels/<name>.toml`. Use it for shared agent rules, skills or org decisions
that should update on their own schedule. The main `.splice/lock.toml` is never changed by a
channel. Details: `docs/superpowers/specs/2026-10-17-splice-channels-design.md`.

```bash
splice channel add gh:keelstackhq/decisions//channel --track main --piece import-project
splice channel add <source> --as rig --piece claude --only 'docs/**' --exclude '.claude/hooks/**'
splice add skills --channel rig --name agents --data dir=agents   # another piece instance
splice update --channel rig            # one channel; --only/--exclude replace the stored filters
splice update --all                    # main and every channel; files first, post once, locks last
splice check                           # covers the main lock and every channel
splice diff --channel rig              # what update --channel would change
splice channel remove rig              # delete unedited files, strip its blocks, delete its lock
```

- **Filters.** `--only` and `--exclude` (globs) are stored in the channel lock. Dropped paths are
  never written or locked.
- **Tracking.** `--track BRANCH` follows a branch head instead of version tags; `update` then
  reports "up to date" when the channel's subdirectory is unchanged. `--ref` pins a tag, branch
  or sha. `--to` does not apply to a tracked channel.
- **One owner per path.** A path owned by two sources, or a section host that is another
  source's file, is refused (exit 2) with the path and the owner; fix it with `--exclude PATH`.
- **Sections.** A channel can own a block inside a Markdown file nobody owns (for example
  `AGENTS.md`), between `<!-- splice:NAME:begin ... -->` and `<!-- splice:NAME:end -->`. Edit
  the channel source, not the block: `update` refuses an edited block unless `--force`, and
  broken markers are fixed by hand. A report names a block as `HOST#NAME`.
- **Drift reporting.** The `add` family (`add`, `add --channel`, `channel add`) lists drifted
  paths as `conflict`/`owned` changes and writes nothing. The `update` family (`update`,
  `update --channel`, `update --all`) lists drift in `messages` only. `check` reports
  `update`/`owned`. `diff` prints messages and exits 0.
- **Remove.** `channel remove` keeps edited files and blocks (`--force` keeps them and removes
  the rest instead of refusing), never deletes seeds, and deletes the channel lock last.
  `--allow-dirty` works as for `update`.
- **Lock last.** Every command writes locks after all files, steps and post commands. A failure
  leaves the old locks; the message says how to restore with git.

## Update bot

```yaml
# .github/workflows/splice.yml
on:
  schedule: [{ cron: "17 6 * * 1" }]
  workflow_dispatch:
permissions: { contents: write, pull-requests: write }
jobs:
  update:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1
        with:
          persist-credentials: false
      - uses: keelstackhq/splice/action@v1
        with:
          token: ${{ secrets.SPLICE_BOT_TOKEN }}   # GITHUB_TOKEN works, but its PRs do not trigger CI
```

It opens or refreshes one pull request (`splice/update`) with a summary of the changes.

Inputs: `version` (`latest`, `next`, a semver, or `none` to use the `splice` on `PATH`), `to`
(template version, default latest), `scope` (`main` or `all`; default `all` when
`.splice/channels/` holds a `*.toml`, else `main`; `all` runs `splice update --all` and groups the PR
body by source; `to` needs `scope: main`), `token` (default `github.token`), `branch` (default
`splice/update`), `labels` (default `splice`), `working-directory` (default `.`). Outputs:
`result` and `pr-url`. `result` is one of:

| `result` | Meaning | Bot does |
|---|---|---|
| `updated` | clean update | commits to the branch, opens or edits the PR |
| `conflicts` | merge conflicts | same, PR title starts with `[conflicts]` |
| `current` | nothing to do | closes an open bot PR |
| `drift` | owned files edited locally | fails with splice's messages, pushes nothing |

Pushes and PRs made with `GITHUB_TOKEN` do not trigger `pull_request` workflows; pass a GitHub
App or fine-grained token as `token` to get CI on the bot PR. The push always uses `token`, even if checkout kept its credentials, and it also authenticates the template fetches of `splice update` (private templates). Run the bot right after checkout: untracked files left by earlier steps count as an update and get committed. The job needs
`contents: write` and `pull-requests: write`.

### Renovate instead of the Action

Renovate can do the same job with a `customManagers` regex entry on `.splice/lock.toml`
(`version = "..."`, datasource `github-tags`) plus a `postUpgradeTasks` entry that runs
`splice update --to {{newVersion}} --yes`. `postUpgradeTasks` only run on self-hosted Renovate
with `allowedCommands` set. Without them Renovate only bumps the version string, which breaks the
lock. That is why the Action is the default. This setup is documented, not shipped.

## License

MIT, see [`LICENSE`](LICENSE).
