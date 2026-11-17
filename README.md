# splice

Start a repo from versioned template pieces and keep it current with a 3-way merge. A Copier
alternative in one static binary: no Python, no Rust toolchain. Part of
[keelstackhq](https://github.com/keelstackhq).

```bash
pnpm dlx @keelstackhq/splice new gh:keelstackhq/hull//template my-app --piece app:web
cd my-app
pnpm dlx @keelstackhq/splice update   # 3-way merge to the latest template version
pnpm dlx @keelstackhq/splice check    # exit 1 on drift, conflicts or an available update
```

## Why

- **Pieces:** a template is a set of pieces you pick (`new`, `add`), not one monolith.
- **New questions surface:** a var a template update adds is listed in `update` output and the
  bot PR body, not discovered later.
- **Updates, not copies:** `update` merges the new template into your edits. Owned files refuse
  drift (exit 1, `--force` replaces), shared files merge by key or line, seeds are never touched.
- **One static binary:** git is the only dependency; npm ships the same binary.
- **Machine-readable:** every command takes `--json` and uses fixed exit codes.

## Docs

- [Getting started](docs/getting-started.md): a walkthrough for people.
- [Reference](docs/reference.md): install, commands, file classes, channels, the update bot,
  releasing.
- [Agent skill](skills/splice/SKILL.md): the contract for coding agents.
- [Template format](docs/template-format.md): the contract shared with hull.
- [Design spec](docs/superpowers/specs/2026-10-14-splice-design.md).

MIT, see [`LICENSE`](LICENSE).
