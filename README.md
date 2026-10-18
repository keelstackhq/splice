# splice

Start a repo from versioned template pieces and keep it up to date. A Copier alternative in one
static binary: no Python, no Rust toolchain. Part of [keelstack](https://github.com/keelstackhq):
tools for bootstrapping projects.

> Status: under construction. See `docs/superpowers/specs/2026-10-14-splice-design.md`.

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
| `splice update [--to V]` | Move to a newer template version; your edits are merged, never lost |
| `splice check` | Fail on edited owned files, leftover conflict markers, or a newer version |
| `splice diff [--to V]` | Show what `update` would change, as a unified diff; writes nothing |
| `splice explain <path>` | Which piece owns a file and how splice treats it |

Every command takes `--json` (one JSON object on stdout) and `--yes` (no prompts). Exit codes:
0 ok, 1 drift/conflict/update available, 2 usage error, 3 template error. `add` and `update`
refuse a dirty git tree (exit 2) unless `--allow-dirty`.

## How files are treated

| Class | On `update` |
|---|---|
| `owned` | Replaced. Local edits block the update (`--force` overrides). |
| `shared` | 3-way merged (JSON, YAML, TOML by key; text by line). Conflicts get diff3 markers. |
| `seed` | Written once, then yours. |

Template authors: see [`docs/template-format.md`](docs/template-format.md).

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
      - uses: actions/checkout@v5
      - uses: keelstackhq/splice/action@v1
        with:
          token: ${{ secrets.SPLICE_BOT_TOKEN }}   # GITHUB_TOKEN works, but its PRs do not trigger CI
```

It opens or refreshes one pull request (`splice/update`) with a summary of the changes.

License: MIT.
