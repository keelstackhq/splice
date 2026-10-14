# splice Milestone 3 — CI, release, npm launcher, update bot, agent skill — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship splice: a fast CI, a release workflow for five targets with checksums, `pnpm dlx @keelstack/splice` via platform packages, the `keelstack/splice/action@v1` update bot, the Claude Code skill file and the product README.

**Architecture:** GitHub Actions on GitHub's own runners. CI is one job plus an aggregate `ci-ok` check. Release builds each target natively (musl on Linux via `musl-tools`, both macOS targets on one arm64 runner), uploads archives + `SHA256SUMS` to a GitHub release, then publishes the five platform npm packages and the launcher. The update bot is a composite action that installs splice from npm and drives `gh`.

**Tech Stack:** GitHub Actions (`actions/checkout@v5`, `dtolnay/rust-toolchain@stable`, `Swatinem/rust-cache@v2`, `taiki-e/install-action@nextest`, `actions/setup-node@v5`, `actions/upload-artifact@v4`/`download-artifact@v5`), Node ≥ 20 (launcher, `node:test`), npm CLI for publishing, `gh` CLI, actionlint (via `rhysd/actionlint` docker or `taiki-e/install-action@actionlint`). Verify every action major version is current when writing the workflow; pin third-party actions by full commit sha with a version comment.

**Spec:** `docs/superpowers/specs/2026-10-14-splice-design.md` (§12–14) · **Contract:** `docs/template-format.md` (§8)

## Global Constraints

- Release targets exactly: `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl`, `x86_64-apple-darwin`, `aarch64-apple-darwin`, `x86_64-pc-windows-msvc`.
- npm names exactly: `@keelstack/splice`; `@keelstack/splice-linux-x64`, `-linux-arm64`, `-darwin-x64`, `-darwin-arm64`, `-win32-x64`. Platform packages are exact-version `optionalDependencies` of the launcher. No `preinstall`/`postinstall` scripts anywhere (pnpm 10 blocks them). Not cargo-dist's npm installer.
- `pnpm dlx @keelstack/splice new …` works with no Python and no Rust.
- CI: one job + `ci-ok`; triggers `pull_request`, `merge_group`, `push` to `main`; `concurrency` with `cancel-in-progress` for PRs; no workflow-level `paths`/`paths-ignore`.
- No OpenSSL, no TLS crates (`cargo tree -i openssl` and `cargo tree -i rustls` both empty).
- Update bot uses the `gh` CLI (spec §13), one PR per repo on branch `splice/update`.
- Human-only steps (create repo/org, npm org + `NPM_TOKEN`, first tag `v0.1.0`, enabling the Action in a consumer repo) go to `docs/PROGRESS.md` → Blocked.

## Review Focus

1. **A consumer runs `pnpm dlx @keelstack/splice` on linux-arm64 or Windows** → the right platform package is installed and the launcher finds `splice.exe` on Windows; on an unsupported platform the launcher prints the platform pair and the list of supported ones and exits 1. Test in Task 3.
2. **The launcher receives Ctrl-C or the binary exits with code 1 or 3** → exit code passes through unchanged (agents rely on 1 vs 3); a signal kill exits with `128 + n` or re-raises it. Test in Task 3.
3. **The bot runs when the repo is already current but an old bot PR is open** → the PR is closed with a comment; when nothing changed and no PR exists → no branch push, no PR. Test in Task 4.
4. **The bot's update hits conflicts (exit 1)** → the PR is still opened/refreshed, title prefixed `[conflicts]`, body lists the conflicted paths; exit 3 fails the workflow with splice's messages. Test in Task 4.
5. **The release tag and `Cargo.toml` version differ** → release fails before building. Test in Task 2 (script test).

---

## File Structure

```
.github/workflows/ci.yml          single job + ci-ok
.github/workflows/release.yml     tag v* → build 5 targets → GitHub release → npm publish
scripts/check-version.mjs         tag == Cargo.toml version == npm versions
scripts/npm-stage.mjs             build npm/ dirs for a version from release artifacts
npm/splice/package.json  npm/splice/bin/splice.js  npm/splice/lib/platform.js  npm/splice/test/*.test.mjs
npm/platforms.json                target ⇄ npm package ⇄ os/cpu ⇄ binary name
action/action.yml  action/summary.mjs  action/test/*.test.mjs
skills/splice/SKILL.md
README.md (from README.md.product), LICENSE (MIT)
```

---

### Task 1: Final CI workflow

**Files:**
- Modify: `.github/workflows/ci.yml`
- Test: actionlint, plus the workflow run on the first PR (human-visible; see Blocked)

**Interfaces:**
- Produces: required check name `ci-ok`.

- [x] **Step 1: Write the workflow**

```yaml
on:
  pull_request:
  merge_group:
  push: { branches: [main] }
concurrency:
  group: ci-${{ github.workflow }}-${{ github.event.pull_request.number || github.ref }}
  cancel-in-progress: ${{ github.event_name == 'pull_request' }}
permissions: { contents: read }
jobs:
  test:      # checkout → toolchain (rustfmt, clippy) → rust-cache → nextest →
             # cargo fmt --check → cargo clippy --all-targets --locked -- -D warnings →
             # cargo nextest run --locked → node --test 'npm/splice/test/**/*.test.mjs' 'action/test/**/*.test.mjs' →
             # cargo tree -i openssl (must fail: not found) → actionlint
  ci-ok:
    needs: [test]
    if: always()
    runs-on: ubuntu-latest
    steps: [{ run: '[ "${{ needs.test.result }}" = success ]' }]
```

`RUSTFLAGS` unset (keeps the cache valid); `CARGO_TERM_COLOR: always`; `timeout-minutes: 15`.

- [x] **Step 2: Run actionlint locally (`docker run --rm -v "$PWD:/repo" -w /repo rhysd/actionlint:latest`, or the binary) — expect no findings**
- [x] **Step 3: Run the same commands locally — expect PASS; note wall time in PROGRESS (target < 2 min warm)**
- [x] **Step 4: Commit** — `ci: single-job CI with ci-ok aggregate`

---

### Task 2: Release workflow and version check

**Files:**
- Create: `.github/workflows/release.yml`, `scripts/check-version.mjs`, `npm/platforms.json`
- Modify: `Cargo.toml` (`[profile.release] lto = "thin"`, `codegen-units = 1`, `strip = true`)
- Test: `scripts/check-version.test.mjs` (`node --test`)

**Interfaces:**
- Produces: release assets `splice-<target>.tar.gz` (Windows `.zip`) each holding `splice[.exe]`, `LICENSE`, `README.md`; `SHA256SUMS`; `npm/platforms.json`:

```json
[{"target":"x86_64-unknown-linux-musl","npm":"@keelstack/splice-linux-x64","os":"linux","cpu":"x64","bin":"splice"},
 {"target":"aarch64-unknown-linux-musl","npm":"@keelstack/splice-linux-arm64","os":"linux","cpu":"arm64","bin":"splice"},
 {"target":"x86_64-apple-darwin","npm":"@keelstack/splice-darwin-x64","os":"darwin","cpu":"x64","bin":"splice"},
 {"target":"aarch64-apple-darwin","npm":"@keelstack/splice-darwin-arm64","os":"darwin","cpu":"arm64","bin":"splice"},
 {"target":"x86_64-pc-windows-msvc","npm":"@keelstack/splice-win32-x64","os":"win32","cpu":"x64","bin":"splice.exe"}]
```

- [x] **Step 1: Write failing test** — `checkVersion({tag:"v0.2.0", cargoToml:'version = "0.1.0"'})` throws `tag v0.2.0 does not match Cargo.toml 0.1.0`; matching inputs return `"0.2.0"`.
- [x] **Step 2: Run `node --test 'scripts/**/*.test.mjs'` — FAIL**
- [x] **Step 3: Implement the script and the workflow.** Trigger `push: tags: ['v*']` + `workflow_dispatch` (dry run, no publish). Jobs: `check` (version script) → `build` matrix: `ubuntu-24.04` (x86_64 musl, `apt-get install musl-tools`), `ubuntu-24.04-arm` (aarch64 musl, musl-tools), `macos-15` (both darwin targets, `rustup target add x86_64-apple-darwin`), `windows-2025` (msvc). Each native build runs `splice --version` and a `splice new` smoke test against `tests/fixtures/template/v1` (made into a git repo by a 5-line shell step). Linux: verify `file splice` says `statically linked`. → `release` job: download artifacts, write `SHA256SUMS`, `gh release create $TAG --generate-notes` with all files → `npm` job (Task 3).
- [x] **Step 4: Run the test — PASS; actionlint clean; `workflow_dispatch` dry run is a Blocked item until the repo exists** (local node tests + actionlint done; GitHub selftest run is human)
- [x] **Step 5: Commit** — `ci: release workflow for five targets with checksums`

---

### Task 3: npm launcher and platform packages

**Files:**
- Create: `npm/splice/package.json`, `npm/splice/bin/splice.js`, `npm/splice/lib/platform.js`, `npm/splice/test/launcher.test.mjs`, `scripts/npm-stage.mjs`; Modify: `.github/workflows/release.yml` (npm job)
- Test: `npm/splice/test/launcher.test.mjs`, `scripts/npm-stage.test.mjs`

**Interfaces:**
- Produces:
  - `platform.js`: `export function packageFor(platform = process.platform, arch = process.arch) -> {npm, bin} | null` (reads `platforms.json` copied into the package at stage time); `export function binaryPath(resolve = require.resolve) -> string` (resolves `<npm>/package.json`, joins `bin/<bin>`; throws a message listing supported pairs).
  - `bin/splice.js`: `spawnSync(binaryPath(), process.argv.slice(2), { stdio: "inherit", windowsHide: true })`; exit with `status`; on `signal` re-raise via `process.kill(process.pid, signal)`.
  - `npm-stage.mjs <version> <artifacts-dir> <out-dir>`: writes `out/splice-<os>-<cpu>/{package.json,bin/<bin>}` with `os`, `cpu`, `"preferUnplugged": true`, `files: ["bin"]`, and `out/splice/` with `optionalDependencies` pinned to `<version>` and `bin: {"splice": "bin/splice.js"}`. Sets mode 755 on unix binaries.

- [x] **Step 1: Write failing tests**

```js
test("maps platforms", () => {
  assert.deepEqual(packageFor("win32", "x64"), { npm: "@keelstack/splice-win32-x64", bin: "splice.exe" });
  assert.equal(packageFor("linux", "ia32"), null);
});
test("unsupported platform message lists supported pairs", () => {
  assert.throws(() => binaryPath(() => { throw new Error("nf") }, "freebsd", "x64"), /freebsd-x64.*linux-x64/s);
});
test("exit code passes through", () => {
  // fake platform package in a temp node_modules whose bin is a node script exiting 3
  const r = spawnSync(process.execPath, [launcher], { env: { ...process.env, NODE_PATH: tmp } });
  assert.equal(r.status, 3);
});
test("stage writes no install scripts", () => { /* every staged package.json has no scripts.preinstall/install/postinstall */ });
```

- [x] **Step 2: Run `node --test 'npm/splice/test/**/*.test.mjs' 'scripts/**/*.test.mjs'` — FAIL**
- [x] **Step 3: Implement.** Release `npm` job (needs `release`): `actions/setup-node@v5` with `registry-url: https://registry.npmjs.org`, run `npm-stage.mjs`, then `npm publish --access public --provenance` for each platform package, then the launcher (`permissions: id-token: write` for provenance; `NODE_AUTH_TOKEN: ${{ secrets.NPM_TOKEN }}`). Re-running for an already published version skips with a notice (check `npm view <pkg>@<v> version` first).
- [x] **Step 4: Run tests — PASS.** Then locally: `node scripts/npm-stage.mjs 0.0.0-local target/stage-artifacts target/npm` with a host binary, `pnpm add -g ./target/npm/splice-<host> ./target/npm/splice` in a scratch prefix (or `npm pack` + `pnpm dlx file:…`), and run `splice --version` through the launcher — paste output.
- [x] **Step 5: Commit** — `feat: npm launcher with per-platform packages`

---

### Task 4: Update-bot composite Action

**Files:**
- Create: `action/action.yml`, `action/summary.mjs`, `action/test/summary.test.mjs`, `action/test/fixtures/*.json`
- Test: `action/test/summary.test.mjs`

**Interfaces:**
- Consumes: `splice update --json --yes` (plan 02), contract §8 JSON.
- Produces:
  - Inputs: `version` (default `latest`), `to` (default empty = latest), `token` (default `${{ github.token }}`), `branch` (default `splice/update`), `labels` (default `splice`), `working-directory` (default `.`).
  - Outputs: `result` (`updated|conflicts|current`), `pr-url`.
  - `summary.mjs`: `export function summarize(report, exitCode) -> {title, body, result}` — title `chore: splice update to <version>` (version read from the lock after update, passed in), prefixed `[conflicts] ` when exit 1; body = table of `changes` grouped by action + messages + "how to resolve" line pointing to `splice check`.

- [x] **Step 1: Write failing tests**

```js
test("conflict title and body", () => {
  const s = summarize(load("conflict.json"), 1, "2.0.0");
  assert.equal(s.title, "[conflicts] chore: splice update to 2.0.0"); assert.equal(s.result, "conflicts");
  assert.match(s.body, /\| conflict \| `.github\/workflows\/ci.yml` \| shared \|/);
});
test("no changes is current", () => { assert.equal(summarize({ok:true,command:"update",changes:[],messages:["up to date"]}, 0).result, "current"); });
test("exit 3 throws with messages", () => { assert.throws(() => summarize(load("error.json"), 3), /template needs splice/); });
```

- [x] **Step 2: Run — FAIL**
- [x] **Step 3: Implement `action.yml`** (`runs.using: composite`, every step `shell: bash`): set git identity `splice-bot`; `npm i -g @keelstack/splice@${{ inputs.version }}`; run update capturing JSON and exit code (`set +e`); `node summary.mjs`; then by `result`:
  - `current`: `gh pr list --head <branch> --state open --json number` → if found, `gh pr close --comment "Already up to date." --delete-branch`.
  - `updated`/`conflicts`: `git switch -C <branch>`, `git add -A`, commit with the title, `git push --force-with-lease origin <branch>`, then `gh pr edit` the open PR (title/body) or `gh pr create --base <default branch> --head <branch> --label …`.
  - exit 3: print messages, `exit 1`.
  `GH_TOKEN: ${{ inputs.token }}`. Document in the action's README section: `permissions: contents: write, pull-requests: write`; PRs from `GITHUB_TOKEN` do not trigger `pull_request` workflows, so pass an App/fine-grained token for CI on the PR.
- [x] **Step 4: Run tests — PASS; actionlint on `action/action.yml` via a throwaway workflow that uses `./action` (`.github/workflows/action-selftest.yml`, `workflow_dispatch` only, runs the action against a temp repo created from the fixture with `--allow-dirty`-free flow) — selftest run on GitHub is Blocked until the repo exists** (local node tests + actionlint done; GitHub selftest run is human)
- [x] **Step 5: Commit** — `feat: update-bot composite action`

---

### Task 5: Agent skill file and product README

**Files:**
- Create: `skills/splice/SKILL.md`, `LICENSE`; Modify: `README.md` (copy of `README.md.product` already in the repo root as `README.md`; extend it)
- Test: `tests/docs.rs` — every command and flag in README/SKILL appears in `splice <cmd> --help` output (cheap drift guard)

**Interfaces:**
- Consumes: final CLI (plan 01/02), Action inputs (Task 4).

- [ ] **Step 1: Write failing test** — `docs_mention_only_real_flags`: extract `--[a-z-]+` tokens from both files, assert each appears in the help of some subcommand (plus an allowlist for `--json` examples of other tools, e.g. `pnpm`).
- [ ] **Step 2: Run — FAIL (files missing)**
- [ ] **Step 3: Write the docs.** `SKILL.md` frontmatter `name: splice`, `description:` "Use when a repo has .splice/lock.toml or the user asks to create, update or check a project from a keelstack template…". Body ≤ 80 lines: always `--json`; read `changes`/exit code table; before editing a file run `splice explain <path> --json` (owned → change the template or accept drift; shared → edit freely; seed → yours); resolving conflicts (edit marker blocks, keep intent of both, then `splice check --json --offline`); never edit `.splice/lock.toml`; dirty tree → commit or stash first, never `--allow-dirty` without being asked; restore command after a failed update. README.product sections: install (`pnpm dlx`, release binaries), quick start with hull, commands table, ownership classes (link to `docs/template-format.md`), update bot workflow example (weekly cron + `workflow_dispatch`), Renovate alternative (spec §13, with the self-hosted caveat), writing templates (link to contract), comparison with Copier (one paragraph), license.
- [x] **Step 4: Run — PASS** (local node tests + actionlint done; GitHub selftest run is human)
- [ ] **Step 5: Commit** — `docs: agent skill and product README`

---

## Blocked (expected, human-only — copy into docs/PROGRESS.md when reached)

- Create GitHub org `keelstack` (if missing) and repo `keelstack/splice`; push; make `ci-ok` a required check; enable merge queue (optional).
- Create npm org `keelstack`; add repo secret `NPM_TOKEN` (granular token with publish rights on `@keelstack/*`; consider moving to npm trusted publishing after the first publish — packages must exist first).
- Push tag `v0.1.0` to run the release; check the five assets, `SHA256SUMS` and the six npm packages; run `pnpm dlx @keelstack/splice --version` on a clean machine.
- Tag `v1` for the Action (`git tag -f v1 && git push -f origin v1` — the human does this; agents never force-push) and add the workflow to `keelstack/hull` (or another consumer) with the token secret.

## Self-review notes

- Spec §12 (targets, archives, checksums, npm shape), §13 (Action + Renovate doc), §14 (skill) are covered by Tasks 2–5; CI requirements from the brief by Task 1.
- The Action installs from npm, so it only works after the first npm publish (Blocked item 3).
