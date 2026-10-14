# splice

Bootstrap a repo from versioned template pieces (`splice new`, `splice add`) and keep it current
(`splice update`, `splice check`, `splice diff`). A Copier alternative in one static Rust binary;
no Python. First consumer: `keelstack/hull` (`gh:keelstack/hull//template`).

- Contract: `docs/template-format.md` — binding, shared with hull. **Never edit it here alone.**
- Spec: `docs/superpowers/specs/2026-10-14-splice-design.md` — source of truth. Read only the
  section you touch; update it in the same commit when behaviour changes.
- Plans: `docs/superpowers/plans/` (01 render-new, 02 update-check, 03 release-action).
- Progress: `docs/PROGRESS.md` — one line per finished task; human-only items under Blocked.

## Layout

```
src/main.rs, cli.rs      clap CLI → lib::run → Report → exit code
src/error.rs, output.rs  SpliceError (exit codes), Report/Change (--json shape)
src/source.rs git.rs fetch.rs   source specs, git CLI wrapper, Fetcher (cache by URL+commit)
src/manifest.rs vars.rs  splice.toml / piece.toml, validation, var prompts
src/subset.rs render.rs  subset gate + MiniJinja; render_tree → RenderedTree
src/classify.rs paths.rs ownership globs, safe rendered paths
src/lock.rs              .splice/lock.toml (atomic write)
src/merge/               text (diffy), json, yaml, toml, keypath
src/planner.rs apply.rs migrate.rs post.rs markers.rs
src/ops/                 new add update check diff explain
tests/                   cli_*.rs, golden_update.rs, conformance.rs, common/, fixtures/
npm/  action/  skills/splice/SKILL.md  .github/workflows/
```

## Commands

```bash
cargo fmt --check && cargo clippy --all-targets --locked -- -D warnings
cargo nextest run                       # all tests; needs `git` on PATH
cargo nextest run -E 'test(golden)'     # one group
cargo insta review                      # inspect snapshot changes; never blind-accept
node --test 'scripts/**/*.test.mjs' 'npm/splice/test/**/*.test.mjs' 'action/test/**/*.test.mjs'
```

Run fmt, clippy and the tests you touched before every commit; the full suite before pushing.

## Rules that must never break

- **The contract wins.** Template format, lock shape, classes, subset, markers, JSON shape and
  exit codes come from `docs/template-format.md`. A gap → decide in spec §10, never in code only.
- **Never lose user edits silently.** Owned drift refuses `update` (exit 1) unless `--force`;
  shared files merge or get diff3 markers; seeds are never modified.
- **Lock last, atomically** (temp file + rename). A crash or failed migration step must leave a
  lock that matches the old state.
- **Dirty tree → refuse** (exit 2) unless `--allow-dirty`; git is the undo button.
- **Subset only.** The gate rejects any syntax outside spec §6 before MiniJinja runs. Old
  templates must keep rendering identically on every future splice (conformance corpus).
- **`--json` stdout is exactly one object.** Post/step/git output goes to stderr.
- **Exit codes:** 0 ok · 1 drift/conflict/update available · 2 usage · 3 template/env error.
- **Paths are safe:** rendered paths are relative, no `..`, never `.splice/lock.toml`.
- No network or TLS crates; all fetching via the `git` CLI. No `unsafe`.

## How to work here

- Model routing: use the subagents in `.claude/agents/` — `explorer` (haiku) for searching
  code/docs, `implementer` (sonnet) for one plan task, `reviewer` (opus) for the task diff. The
  main session orchestrates and stays thin.
- Test first (superpowers:test-driven-development): failing test → watch it fail → implement →
  pass → commit. One commit per task, message as in the plan.
- Execute plans with superpowers:subagent-driven-development, one task at a time, in order.
- Read big files by section (offset/limit or grep), not whole.
- Ask before adding a crate or npm package not in a plan's Tech Stack.
- Evidence before claims (superpowers:verification-before-completion): paste real output.
- Public behaviour (flags, JSON, exit codes, lock, Action inputs) changes only with spec + README.

## Gotchas

- MiniJinja drops the final newline unless `keep_trailing_newline(true)`; Nunjucks keeps it.
- `Environment::new()` auto-escapes `.html`; use `Environment::empty()` + only `lower`/`upper`.
- diffy writes `||||||| original` / `>>>>>>> theirs`; relabel inside conflict hunks only.
- `serde_yaml` is archived; we use `yaml_serde` (YAML org fork). JSON/YAML lose comments on a
  real merge; TOML keeps them (`toml_edit`).
- `std::env::set_var` is `unsafe` in edition 2024: pass `Fetcher::new(cache)` in tests.
- Tests need `GIT_CONFIG_GLOBAL` (empty file) and author env vars; `tests/common` sets them.
- Git on Windows: always `-c core.autocrlf=false`, or hashes differ.
- PRs opened with `GITHUB_TOKEN` do not trigger CI; the bot docs say to pass another token.
- npm: no install scripts (pnpm 10 blocks them); the launcher resolves the platform package.
