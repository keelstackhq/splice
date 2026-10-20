# splice Milestone 3 — CI, release, npm launcher, update bot, agent skill — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship splice: a fast CI, a release workflow for five targets with checksums, `pnpm dlx @keelstackhq/splice` via platform packages published with npm trusted publishing, the `keelstackhq/splice/action@v1` update bot (with the channel `scope` from plan 04), the Claude Code skill file and the product README.

**Architecture:** GitHub Actions on GitHub's own runners. CI is one job `ci` plus an aggregate `ci-ok` check. Release builds each target natively in unprivileged jobs, uploads artifacts, then a `release` job (only `contents: write`) writes `SHA256SUMS` and the GitHub release, and an `npm` job (only `id-token: write`, environment `release`) publishes the five platform packages and the launcher with OIDC trusted publishing and provenance. Neither privileged job runs cargo. The update bot is a composite action that installs splice from npm and drives `gh`.

**Tech Stack:** GitHub Actions (`actions/checkout`, `Swatinem/rust-cache`, `taiki-e/install-action` for nextest and actionlint, `actions/setup-node`, `actions/upload-artifact`, `actions/download-artifact`), Node 24 (npm ≥ 11.5.1 for trusted publishing; launcher, `node:test`), npm CLI, `gh` CLI, actionlint + shellcheck. The toolchain comes from `rust-toolchain.toml` via `rustup toolchain install` (no `dtolnay/rust-toolchain`). **Every action is pinned by full commit sha with a `# vX.Y.Z` comment**; resolve the sha at write time with `git ls-remote https://github.com/<owner>/<repo> 'refs/tags/<tag>*'` and use the newest release tag of the current major (hull saw upload-artifact v7 and download-artifact v8 on 2026-10-17; check again).

**Spec:** `docs/superpowers/specs/2026-10-14-splice-design.md` (§12–14) · **Contract:** `docs/template-format.md` (§6, §8; unchanged) · **Pre-flight:** `.superpowers/sdd/2026-10-14-splice-03-release-action/preflight.md`

## Global Constraints

- Release targets exactly: `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl`, `x86_64-apple-darwin`, `aarch64-apple-darwin`, `x86_64-pc-windows-msvc`.
- npm names exactly: `@keelstackhq/splice`; `@keelstackhq/splice-linux-x64`, `-linux-arm64`, `-darwin-x64`, `-darwin-arm64`, `-win32-x64`. Platform packages are exact-version `optionalDependencies` of the launcher. No `scripts` key in any published `package.json` (pnpm blocks install scripts). Not cargo-dist's npm installer.
- First release is `v0.2.0` (Cargo is 0.2.0). Release tags match `v[0-9]+.[0-9]+.[0-9]+*`; the Action's major tag `v1` must not trigger a release.
- `pnpm dlx @keelstackhq/splice new …` works with no Python and no Rust.
- CI: one job `ci` + `ci-ok`; triggers `pull_request`, `merge_group`, `push` to `main`; `concurrency` with `cancel-in-progress` for PRs; no workflow-level `paths`/`paths-ignore`.
- Workflow hardening (org ruling, hull/hoist): top-level `permissions: {}`, granted per job; `persist-credentials: false` on every checkout; every `${{ }}` reaches `run:` only through `env:`; build jobs never hold write or `id-token` permissions; publish jobs only download artifacts.
- npm: trusted publishing (OIDC) with provenance, environment `release`, no `NPM_TOKEN`/`NODE_AUTH_TOKEN` secret; versions with `-` publish with `--tag next` and the GitHub release is `--prerelease`.
- No OpenSSL, no TLS crates: `! cargo tree -e normal --target all --locked | grep -E 'openssl|rustls|native-tls'`.
- Node tests always run through globs (Node 22+/24 fail on a directory or missing path argument): `node --test "npm/**/*.test.mjs" "action/**/*.test.mjs" "scripts/**/*.test.mjs"`.
- Update bot uses the `gh` CLI (spec §13), one PR per repo on branch `splice/update`.
- Public behaviour (launcher exit codes, Action inputs/outputs, bot results) changes only together with spec §12–§14 and the README.
- Human-only steps go to `docs/PROGRESS.md` → Blocked with the exact action.

## Review Focus

1. **A consumer runs `pnpm dlx @keelstackhq/splice` on linux-arm64 or Windows** → the right platform package is installed and the launcher finds `splice.exe` on Windows; on an unsupported platform the launcher prints the platform pair, the supported pairs and the source-install hint, and **exits 3**. Test in Task 3.
2. **The launcher receives a signal, or the binary exits with code 1 or 3** → the exit code passes through unchanged (agents rely on 1 vs 3); a signal death is re-raised on the launcher. Test in Task 3.
3. **The bot runs when the repo is already current but an old bot PR is open** → the PR is closed with a comment; nothing changed and no PR exists → no push, no PR. Test in Task 4.
4. **The bot's update hits conflicts (exit 1, files written)** → PR opened/refreshed, title prefixed `[conflicts]`, body lists the conflicted paths. **Owned or section drift (exit 1, nothing written)** → result `drift`, the job fails with splice's messages, nothing is pushed. Exit 2/3 fail the job with splice's messages. Test in Task 4.
5. **A second bot run when `splice/update` already exists on the remote** → the push succeeds (no `stale info`). Test in Task 4 (selftest runs the bot twice).
6. **The release tag, `Cargo.toml` and `npm/splice/package.json` versions differ, or the tag is not semver** → release fails before building. Test in Task 2.
7. **Pushing tag `v1`** → no release run. Test in Task 2 (actionlint + a test on the trigger pattern in `check-version.test.mjs`).

---

## File Structure

```
rust-toolchain.toml               channel = "1.97" (Task 1)
.github/workflows/ci.yml          job ci + ci-ok
.github/workflows/release.yml     tag v<semver> → check → build 5 targets → release → npm
.github/workflows/action-selftest.yml   workflow_dispatch: runs ./action twice on a temp repo
scripts/check-version.mjs (+ .test.mjs)  tag == Cargo.toml == launcher package.json
scripts/smoke.sh                  `splice new` smoke against tests/fixtures/template/v1
scripts/npm-stage.mjs (+ .test.mjs)      stage the six npm packages from release artifacts
npm/splice/package.json  npm/splice/platforms.json  npm/splice/bin/splice.js
npm/splice/lib/platform.js  npm/splice/test/*.test.mjs
action/action.yml  action/args.mjs  action/summary.mjs  action/test/*.test.mjs  action/test/fixtures/*.json
skills/splice/SKILL.md
LICENSE (MIT, Task 2)   README.md (extended in place, Tasks 4, 4b, 5)
tests/docs.rs                     README + SKILL flags exist in --help
```

---

### Task 1: Toolchain pin and final CI workflow

**Files:**
- Modify: `rust-toolchain.toml`, `.github/workflows/ci.yml`, `CLAUDE.md` (Commands: node test line)
- Test: actionlint + shellcheck, the local command list, and the workflow run on the first PR (human-visible)

**Interfaces:**
- Produces: job `ci`; required check `ci-ok` (`needs: [ci]`).

- [ ] **Step 1: Pin the toolchain.** `rust-toolchain.toml`: `channel = "1.97"`, `components = ["rustfmt", "clippy"]` (as hoist and berth). Run `cargo fmt --check && cargo clippy --all-targets --locked -- -D warnings && cargo nextest run --locked` — expect PASS. Commit alone: `chore: pin rust toolchain 1.97`.
- [ ] **Step 2: RED for the lint gate.** Install actionlint pinned (`taiki-e/install-action` with `tool: actionlint@<version>` in CI; locally the release binary of the same version, or `docker run --rm -v "$PWD:/repo" -w /repo rhysd/actionlint:<version>`). Copy `ci.yml` to a scratch dir with `run: echo "${{ github.event.pull_request.title }}"` added; actionlint (with shellcheck) must flag it. Paste the finding.
- [ ] **Step 3: Write the workflow**

```yaml
name: ci
on:
  pull_request:
  merge_group:
  push: { branches: [main] }
concurrency:
  group: ci-${{ github.workflow }}-${{ github.event.pull_request.number || github.ref }}
  cancel-in-progress: ${{ github.event_name == 'pull_request' }}
permissions: {}
jobs:
  ci:
    runs-on: ubuntu-latest
    timeout-minutes: 15
    permissions: { contents: read }
    env: { CARGO_TERM_COLOR: always }
    steps:
      # each `uses:` pinned: <owner>/<repo>@<40-hex sha> # vX.Y.Z
      # checkout (persist-credentials: false) → rustup toolchain install → rust-cache →
      # install-action (nextest, actionlint) → setup-node (node-version: 24) →
      # cargo fmt --check → cargo clippy --all-targets --locked -- -D warnings →
      # cargo nextest run --locked →
      # rustup target add x86_64-pc-windows-msvc && cargo check --locked --all-targets --target x86_64-pc-windows-msvc →
      # node --test "npm/**/*.test.mjs" "action/**/*.test.mjs" "scripts/**/*.test.mjs" →
      # ! cargo tree -e normal --target all --locked | grep -E 'openssl|rustls|native-tls' →
      # actionlint
  ci-ok:
    needs: [ci]
    if: always()
    runs-on: ubuntu-latest
    permissions: {}
    steps:
      - env: { RESULT: "${{ needs.ci.result }}" }
        run: '[ "$RESULT" = success ]'
```

`RUSTFLAGS` unset (keeps the cache valid). In `CLAUDE.md` Commands replace `node --test npm/splice/test action/test scripts/` with the glob line above.
- [ ] **Step 4: Run actionlint on the real file — no findings. Run the same commands locally — PASS** (the node globs match nothing yet and exit 0; the Windows `cargo check` needs only `rustup target add`). Note wall time in PROGRESS (target < 2 min warm).
- [ ] **Step 5: Commit** — `ci: single-job CI with ci-ok aggregate`

---

### Task 2: Release workflow, version check, smoke script, LICENSE

**Files:**
- Create: `.github/workflows/release.yml`, `scripts/check-version.mjs`, `scripts/smoke.sh`, `npm/splice/platforms.json`, `LICENSE` (MIT, holder as in `Cargo.toml` authors or "keelstackhq contributors")
- Modify: `Cargo.toml` (`[profile.release]` `lto = "thin"`, `codegen-units = 1`, `strip = true`; `publish = false` stays)
- Test: `scripts/check-version.test.mjs`

**Interfaces:**
- Produces: release assets `splice-<target>.tar.gz` (Windows `.zip`), each holding `splice[.exe]`, `LICENSE`, `README.md`; `SHA256SUMS`.
- `npm/splice/platforms.json` (single source; read by `release.yml`'s matrix check, `npm-stage.mjs` and `platform.js`):

```json
[{"target":"x86_64-unknown-linux-musl","npm":"@keelstackhq/splice-linux-x64","os":"linux","cpu":"x64","bin":"splice"},
 {"target":"aarch64-unknown-linux-musl","npm":"@keelstackhq/splice-linux-arm64","os":"linux","cpu":"arm64","bin":"splice"},
 {"target":"x86_64-apple-darwin","npm":"@keelstackhq/splice-darwin-x64","os":"darwin","cpu":"x64","bin":"splice"},
 {"target":"aarch64-apple-darwin","npm":"@keelstackhq/splice-darwin-arm64","os":"darwin","cpu":"arm64","bin":"splice"},
 {"target":"x86_64-pc-windows-msvc","npm":"@keelstackhq/splice-win32-x64","os":"win32","cpu":"x64","bin":"splice.exe"}]
```

- `check-version.mjs`: `export function checkVersion({ tag, cargoToml, launcherPkg }) -> string` (the version). `tag` may be `null` (dispatch: no tag compare). Throws when the tag is not `v<semver>`, when the tag, `Cargo.toml` `[package] version`, `launcherPkg.version` or any `launcherPkg.optionalDependencies` value differ, or when an optional dependency is missing for a `platforms.json` entry. `launcherPkg` is optional until Task 3 creates the file (absent → only Cargo is checked). CLI: `node scripts/check-version.mjs [--tag vX.Y.Z]` prints the version.
- `smoke.sh <splice-binary>`: copy `tests/fixtures/template/v1` to a temp dir, `git -c core.autocrlf=false init -q`, commit with fixed author env vars, `git tag v1.0.0`, then `"$1" new "$tmp/tpl" "$tmp/out" --yes --data project=smoke --json`; assert exit 0 and `$tmp/out/.splice/lock.toml` exists. `"$1" --version` must print the expected version (`$EXPECT_VERSION`).

- [ ] **Step 1: Write failing tests** — `checkVersion({tag:"v0.3.0", cargoToml:'[package]\nversion = "0.2.0"'})` throws `tag v0.3.0 does not match Cargo.toml 0.2.0`; `tag:"v1"` throws `not a release tag`; matching inputs return `"0.2.0"`; `tag:null` returns the Cargo version; a `launcherPkg` with `version:"0.1.0"` throws naming `npm/splice/package.json`; an `optionalDependencies` pin `0.1.0` throws naming the package; a test asserts the `release.yml` trigger (read the file, find the `tags:` list) is exactly `v[0-9]+.[0-9]+.[0-9]+*` and that the string `v1` does not match it (simple translation of the GitHub filter to a RegExp in the test).
- [ ] **Step 2: Run `node --test "scripts/**/*.test.mjs"` — FAIL**
- [ ] **Step 3: Implement the script, `smoke.sh`, LICENSE, profile and the workflow.**
  - Triggers: `push: tags: ['v[0-9]+.[0-9]+.[0-9]+*']` and `workflow_dispatch` (dry run). `permissions: {}` at the top; `concurrency: release-${{ github.ref }}`, no cancel.
  - `check` (`contents: read`): on push `node scripts/check-version.mjs --tag "$TAG"` (`TAG` from `env:`), on dispatch without `--tag`; output `version`.
  - `build` (`contents: read`, matrix, `defaults.run.shell: bash`): `ubuntu-24.04` (x86_64 musl, `sudo apt-get install -y musl-tools`), `ubuntu-24.04-arm` (aarch64 musl), `macos-15` (both darwin targets, `rustup target add x86_64-apple-darwin`), `windows-2025` (msvc). Steps: checkout (`persist-credentials: false`) → `rustup toolchain install` → `rustup target add "$TARGET"` → `cargo build --release --locked --target "$TARGET"` → Linux: `out=$(file "$BIN")`; assert it matches `static-pie linked|statically linked` and does not contain `interpreter` → `scripts/smoke.sh` on the **native** target only (`x86_64-apple-darwin` on the arm64 runner gets `file` showing `Mach-O 64-bit executable x86_64` instead; record in PROGRESS) → archive with `LICENSE` and `README.md` → `upload-artifact` (`if-no-files-found: error`).
  - `release` (`needs: [check, build]`, `if: github.event_name == 'push'`, `contents: write`, environment `release`): download artifacts, `sha256sum` → `SHA256SUMS`, `gh release create "$TAG" --verify-tag --generate-notes` (plus `--prerelease` when `$VERSION` contains `-`; `gh release upload --clobber` when the release exists). No checkout, no cargo.
  - The `npm` job is added in Task 3.
- [ ] **Step 4: Run the tests — PASS; actionlint clean; `bash scripts/smoke.sh target/release/splice` with `EXPECT_VERSION=0.2.0` after a local `cargo build --release` — paste output. The `workflow_dispatch` dry run on GitHub is Blocked (human).**
- [ ] **Step 5: Commit** — `ci: release workflow for five targets with checksums`

---

### Task 3: npm launcher, platform packages, npm publish job

**Files:**
- Create: `npm/splice/package.json` (version `0.2.0`, `optionalDependencies` the five platform packages at `0.2.0`, `bin: {"splice": "bin/splice.js"}`, `type: "module"`, `license: "MIT"`, `files: ["bin", "lib", "platforms.json"]`, `engines.node: ">=20"`, no `scripts`), `npm/splice/bin/splice.js`, `npm/splice/lib/platform.js`, `npm/splice/test/launcher.test.mjs`, `scripts/npm-stage.mjs`, `scripts/npm-stage.test.mjs`
- Modify: `.github/workflows/release.yml` (`npm` job), `scripts/check-version.test.mjs` (now also against the real `npm/splice/package.json`)

**Interfaces:**
- `platform.js` (ESM): `export function packageFor(platform = process.platform, arch = process.arch) -> {npm, bin} | null` (reads `../platforms.json` relative to the module); `export function binaryPath({ resolve, platform, arch, env } = {}) -> string`. `resolve` defaults to `createRequire(import.meta.url).resolve`, `env` to `process.env`. If `env.SPLICE_BINARY` is set, return it. Otherwise resolve `<npm>/package.json` and join `bin/<bin>`. Unsupported pair or unresolvable package → throws `LauncherError` (exit code 3) with: `splice: no prebuilt binary for <platform>-<arch> (package <npm> is not installed); supported: linux-x64, linux-arm64, darwin-x64, darwin-arm64, win32-x64; download a release from https://github.com/keelstackhq/splice/releases or run \`cargo install --locked --git https://github.com/keelstackhq/splice keel-splice\``.
- `bin/splice.js`: `spawnSync(binaryPath(), process.argv.slice(2), { stdio: "inherit", windowsHide: true })` (no shell). `LauncherError` → message on stderr, exit 3. `result.error` → `splice: cannot run the binary: <msg>`, exit 3. `result.signal` → `process.kill(process.pid, result.signal)`, then `process.exit(1)` as fallback. Else `process.exit(result.status ?? 3)`.
- `npm-stage.mjs` (ESM): `export function stage({ version, artifactsDir, outDir, repository }) -> string[]` (staged dirs, platform packages first, launcher last); CLI `node scripts/npm-stage.mjs --version V --artifacts DIR --out DIR` with `repository` from `GITHUB_REPOSITORY` (validated `^[A-Za-z0-9._-]+/[A-Za-z0-9._-]+$`, else throw), printing one dir per line. Per platform: `out/splice-<os>-<cpu>/{package.json, bin/<bin>, LICENSE}` with `name`, `version`, `os: [os]`, `cpu: [cpu]`, `files: ["bin"]`, `license: "MIT"`, `repository: {type:"git", url:"git+https://github.com/<repository>.git"}`; unix binaries mode 755. Launcher: copy of `npm/splice/` (`bin`, `lib`, `platforms.json`, `LICENSE`, `README.md`) with `version` and every `optionalDependencies` pin set to `version`, plus `repository`. No `scripts`/`devDependencies` key in any output.

- [ ] **Step 1: Write failing tests**

```js
test("maps platforms", () => {
  assert.deepEqual(packageFor("win32", "x64"), { npm: "@keelstackhq/splice-win32-x64", bin: "splice.exe" });
  assert.equal(packageFor("linux", "ia32"), null);
});
test("unsupported platform message", () => {
  assert.throws(() => binaryPath({ resolve: () => { throw new Error("nf"); }, platform: "freebsd", arch: "x64", env: {} }),
    (e) => e.exitCode === 3 && /freebsd-x64.*linux-x64.*keel-splice/s.test(e.message));
});
test("SPLICE_BINARY override", () => { assert.equal(binaryPath({ env: { SPLICE_BINARY: "/x/splice" } }), "/x/splice"); });
for (const code of [0, 1, 3]) test(`exit ${code} passes through`, () => {
  // fake binary: a `#!/bin/sh\nexit <code>` script; launcher run with SPLICE_BINARY pointing at it
  const r = spawnSync(process.execPath, [launcher], { env: { ...process.env, SPLICE_BINARY: fake(code) } });
  assert.equal(r.status, code);
});
test("signal is re-raised", { skip: process.platform === "win32" }, () => {
  const r = spawnSync(process.execPath, [launcher], { env: { ...process.env, SPLICE_BINARY: fakeScript("kill -TERM $$") } });
  assert.equal(r.signal, "SIGTERM");
});
test("missing platform package exits 3", () => { /* launcher with a NODE_PATH that has no platform package and no override */ });
test("platform package resolves via node_modules", () => { /* temp node_modules/@keelstackhq/splice-<host>/{package.json,bin/splice}; NODE_PATH=tmp; exit 3 passes through */ });
// npm-stage.test.mjs
test("stage writes repository, license, no scripts, exact pins", () => { /* every staged package.json: repository.url === "git+https://github.com/keelstackhq/splice.git", license MIT, no scripts, launcher optionalDependencies all === version */ });
test("stage rejects a bad repository", () => { assert.throws(() => stage({ ..., repository: "a b" }), /owner\/repo/); });
```

- [ ] **Step 2: Run `node --test "npm/**/*.test.mjs" "scripts/**/*.test.mjs"` — FAIL**
- [ ] **Step 3: Implement.** Release `npm` job: `needs: [check, build, release]`, `if: github.event_name == 'push'`, environment `release`, `permissions: { contents: read, id-token: write }`, no checkout of credentials (`persist-credentials: false`; checkout is only for `scripts/` and `npm/`), no cargo. Steps: `setup-node` (`node-version: 24`, `registry-url: https://registry.npmjs.org`) → preflight: `private=$(gh api "repos/$GITHUB_REPOSITORY" --jq .private)`; if `true`, fail with `npm provenance needs a public repository: make keelstackhq/splice public, then re-run` (`GH_TOKEN: ${{ github.token }}` via env) → download artifacts to `$RUNNER_TEMP/artifacts` → `node scripts/npm-stage.mjs --version "$VERSION" --artifacts … --out "$RUNNER_TEMP/npm"` → for each staged dir in order: `npm view "$name@$ver" version` succeeds → `echo "skip $name@$ver: already published"`; else `npm publish "$dir" --provenance --access public --tag "$dist_tag"` (`dist_tag=next` when `$VERSION` contains `-`, else `latest`). No `NODE_AUTH_TOKEN` secret.
- [ ] **Step 4: Run tests — PASS.** Then locally: `cargo build --release`, put the host binary into `target/stage-artifacts/<host target>/splice`, `GITHUB_REPOSITORY=keelstackhq/splice node scripts/npm-stage.mjs --version 0.2.0 --artifacts target/stage-artifacts --out target/npm`, `npm pack` the host platform package and the launcher, `npm i --prefix "$scratch" <both tgz>`, run `"$scratch/node_modules/.bin/splice" --version` — paste output (`splice 0.2.0`). Run `node scripts/check-version.mjs` — prints `0.2.0`. actionlint clean.
- [ ] **Step 5: Commit** — `feat: npm launcher with per-platform packages`; add the PROGRESS line `04/T21 npm part` (launcher at 0.2.0, node tests run).

---

### Task 4: Update-bot composite Action

**Files:**
- Create: `action/action.yml`, `action/args.mjs`, `action/summary.mjs`, `action/test/args.test.mjs`, `action/test/summary.test.mjs`, `action/test/fixtures/{conflict,drift,current,updated,error}.json`, `.github/workflows/action-selftest.yml`
- Modify: spec §13 (inputs incl. `working-directory`, `version: none`, results incl. `drift`, push via `token`), `README.md` (Update bot section)
- Test: `node --test "action/**/*.test.mjs"`

**Interfaces:**
- Consumes: `splice update --json --yes` (contract §8 JSON; drift refusal = exit 1, `ok:false`, `changes: []`, paths in `messages` — `src/ops/update.rs:753`).
- Inputs: `version` (default `latest`; `latest`, `next`, a semver, or `none` = do not install, use `splice` on `PATH`), `to` (default empty = latest), `token` (default `${{ github.token }}`), `branch` (default `splice/update`), `labels` (default `splice`), `working-directory` (default `.`).
- Outputs: `result` (`updated|conflicts|current|drift`), `pr-url`.
- `args.mjs`: `export function validateInputs({ version, to, branch }) -> void` (throws on `version` not `latest|next|none|<semver>`, `to` not empty or semver; branch is checked in the shell with `git check-ref-format --branch`); `export function command({ to }) -> string[]` → `["update", "--json", "--yes"]` plus `["--to", to]` when set.
- `summary.mjs`: `export function summarize(report, exitCode, { version, dirty }) -> { title, body, result }`. `version` = `[template] version` read from `.splice/lock.toml` after the update; `dirty` = `git status --porcelain` is non-empty. Rules: exit 0 + clean → `current`; exit 0 + dirty → `updated`; exit 1 + dirty → `conflicts`; exit 1 + clean → `drift`; exit 2 or 3 → throws an Error whose message is `report.messages.join("\n")`. Title `chore: splice update to <version>`, prefixed `[conflicts] ` for `conflicts`. Body: table `| action | path | class |` of `changes` grouped by action, then messages, then `Resolve the conflict markers, then run \`splice check --json\`.` for conflicts. For `drift`: title empty, body = the messages.
- CLI entry (`node "$GITHUB_ACTION_PATH/summary.mjs" <report.json> <exit-code>`): writes `result`, `title` and a body file path to `$GITHUB_OUTPUT`.

- [ ] **Step 1: Generate the fixtures** from the built binary on `tests/fixtures` (record each command in a header comment of `summary.test.mjs`): `conflict.json` (new at v1, edit a shared file as in `golden_update`, update to v2), `drift.json` (edit an owned file, update), `updated.json` (clean update v1→v2), `current.json` (update at v2), `error.json` (update to v3, `min_splice = "99.0.0"`: "template needs splice >= …").
- [ ] **Step 2: Write failing tests**

```js
test("conflict title and body", () => {
  const s = summarize(load("conflict.json"), 1, { version: "2.0.0", dirty: true });
  assert.equal(s.title, "[conflicts] chore: splice update to 2.0.0"); assert.equal(s.result, "conflicts");
  assert.match(s.body, /\| conflict \| `[^`]+` \| shared \|/);
});
test("drift is its own result", () => {
  const s = summarize(load("drift.json"), 1, { version: "1.0.0", dirty: false });
  assert.equal(s.result, "drift"); assert.match(s.body, /owned by the template but was edited locally/);
});
test("clean exit 0 is current", () => { assert.equal(summarize(load("current.json"), 0, { dirty: false }).result, "current"); });
test("dirty exit 0 is updated", () => { assert.equal(summarize(load("updated.json"), 0, { version: "2.0.0", dirty: true }).result, "updated"); });
test("exit 3 throws with messages", () => { assert.throws(() => summarize(load("error.json"), 3, {}), /template needs splice/); });
test("exit 2 throws", () => { assert.throws(() => summarize({ ok: false, command: "update", changes: [], messages: ["dirty tree"] }, 2, {}), /dirty tree/); });
test("command and inputs", () => {
  assert.deepEqual(command({ to: "" }), ["update", "--json", "--yes"]);
  assert.deepEqual(command({ to: "2.0.0" }), ["update", "--json", "--yes", "--to", "2.0.0"]);
  assert.throws(() => validateInputs({ version: "1; rm -rf /", to: "", branch: "x" }), /version/);
});
```

- [ ] **Step 3: Run — FAIL**
- [ ] **Step 4: Implement `action.yml`** (`runs.using: composite`, every step `shell: bash`, **every input reaches the scripts through `env:`**, never `${{ inputs.* }}` inside `run:`):
  1. Validate: `node "$GITHUB_ACTION_PATH/args.mjs" validate`; `git check-ref-format --branch "$BRANCH"`.
  2. Install: unless `VERSION=none`, `npm i -g "@keelstackhq/splice@$VERSION"`.
  3. Git identity `github-actions[bot]` / `41898282+github-actions[bot]@users.noreply.github.com`; `gh auth setup-git` with `GH_TOKEN=$TOKEN` so pushes use `token`, not checkout's credentials.
  4. In `working-directory`: `set +e`; `splice $(node args.mjs command)` > report, capture exit; `set -e`; `dirty` from `git status --porcelain`; `node summary.mjs` → outputs.
  5. By `result`:
     - `current`: `gh pr list --head "$BRANCH" --state open --json number --jq '.[0].number'` → if found, `gh pr close <n> --comment "Already up to date." --delete-branch`.
     - `updated`/`conflicts`: `git switch -C "$BRANCH"`, `git add -A`, commit with the title; `git fetch --no-tags --depth=1 origin "+refs/heads/$BRANCH:refs/remotes/origin/$BRANCH" || true`; `git push --force-with-lease="$BRANCH" origin "HEAD:refs/heads/$BRANCH"`; then `gh pr edit` the open PR (title, `--body-file`) or `gh pr create --base "$(gh repo view --json defaultBranchRef --jq .defaultBranchRef.name)" --head "$BRANCH" --title … --body-file …`; then `gh pr edit --add-label "$LABELS"` and on failure only `echo "::warning::could not add labels"`.
     - `drift`: print the body (splice's messages) and `exit 1`; no push.
     - summary threw (exit 2/3): print the messages, `exit 1`.
  Spec §13 + README: inputs and results as above; `permissions: contents: write, pull-requests: write`; the example uses `actions/checkout` pinned by sha with `persist-credentials: false`; PRs and pushes made with `GITHUB_TOKEN` do not trigger `pull_request` workflows, so pass an App or fine-grained token as `token`.
- [ ] **Step 5: Run tests — PASS; actionlint on `action-selftest.yml`** (`workflow_dispatch` only, `contents: read`): builds splice with cargo, puts it on `PATH`, creates a temp consumer repo from `tests/fixtures/template/v1` and a bare "origin" remote, runs `./action` with `version: none` twice (the second run must push without `stale info`), and asserts `result`. A local dry run of the same script with `gh` stubbed by a shell function is enough here; the GitHub run is Blocked (human).
- [ ] **Step 6: Commit** — `feat: update-bot composite action`

---

### Task 4b: Update-bot `scope` input for channels (plan 04 Task 20)

**Files:**
- Modify: `action/action.yml`, `action/args.mjs`, `action/summary.mjs`, `action/test/*.test.mjs`, spec §13, `README.md` (Update bot section), `docs/superpowers/plans/2026-10-17-splice-04-channels.md` (tick Task 20), `docs/PROGRESS.md` (replace the `04/T20 · Blocked` line)
- Test: `node --test "action/**/*.test.mjs"`

**Interfaces:**
- Input `scope` (`main` | `all`; default empty = `all` when `.splice/channels/` holds a `*.toml`, else `main`).
- `args.mjs`: `export function scopeDefault({ channels }) -> "main" | "all"`; `command({ scope, to })` → `all`: `["update", "--all", "--json", "--yes"]`; `main`: Task 4's command. `scope: "all"` with a non-empty `to` throws `` `to` needs `scope: main` `` (spec §3: `--all` excludes `--to`) before splice runs. `scope: main` in a channel-only repo is left to splice (exit 2 → job fails with its message).
- `summary.mjs`: one PR either way; the body groups rows by the report's `source=` messages (`main`, then each `channel:NAME` under a `NAME` heading). Title for `all`: `chore: splice update` plus ` to <version>` only when the main lock exists.

- [ ] **Step 1: Write failing tests** — `scopeDefault({channels: ["rig.toml"]}) === "all"`, `scopeDefault({channels: []}) === "main"`, `command({scope:"all", to:""})` equals `["update","--all","--json","--yes"]`, `command({scope:"all", to:"2.0.0"})` throws `/to.*scope: main/`, `summarize()` on a fixture generated from `update --all` on the channel fixture (`tests/fixtures/channel`) puts `source=channel:<name>` rows under a `<name>` heading.
- [ ] **Step 2: Run — FAIL.**  **Step 3: Implement** (input via `env:`; the channel scan is `ls .splice/channels/*.toml` in `working-directory`). **Step 4: Run — PASS; actionlint clean.**
- [ ] **Step 5: Commit** — `feat: update bot scope input for channels`

---

### Task 5: Agent skill file and product README

**Files:**
- Create: `skills/splice/SKILL.md`, `tests/docs.rs`
- Modify: `README.md` (extend in place; there is no `README.md.product`), `tests/common/mod.rs` and `tests/cli_basic.rs` (move `unknown_flags` from `tests/cli_basic.rs` to `tests/common`), spec §14 (skill trigger names channel-only repos), `docs/superpowers/plans/2026-10-17-splice-04-channels.md` (tick the Task 19 SKILL note), `docs/PROGRESS.md` (remove the Blocked line "SKILL.md channel lines")
- Test: `tests/docs.rs::docs_mention_only_real_flags`; `readme_channel_flags_exist` stays green

**Interfaces:**
- Consumes: final CLI (plans 01, 02, 04), Action inputs (Tasks 4, 4b).
- `tests/common::unknown_flags(text, help) -> Vec<&str>` (moved, unchanged).

- [ ] **Step 1: Write failing test** — `docs_mention_only_real_flags`: extract `--[a-z-]+` tokens from `README.md` and `skills/splice/SKILL.md`; collect `--help` of `new`, `add`, `update`, `check`, `diff`, `explain`, `channel add`, `channel remove`; every token must appear in that help, except an allowlist for other tools (`--force-with-lease`, `--frozen-lockfile`, …, kept short, each with a comment).
- [ ] **Step 2: Run — FAIL (SKILL.md missing)**
- [ ] **Step 3: Write the docs.** `SKILL.md` frontmatter `name: splice`, `description:` "Use when a repo has .splice/lock.toml or files in .splice/channels/, or the user asks to create, update or check a project from a keelstackhq template…". Body ≤ 80 lines: always `--json`; read `changes` and the exit code table (0/1/2/3); before editing a file run `splice explain <path> --json` (owned → change the template or accept drift; shared → edit freely; seed → yours); resolving conflicts (edit marker blocks, keep the intent of both, then `splice check --json --offline`); never edit `.splice/lock.toml` or `.splice/channels/*.toml`; dirty tree → commit or stash first, never `--allow-dirty` without being asked; after a failed update follow the restore command in the message.
- [ ] **Step 3b: Channel lines in `SKILL.md` (≤ 10, plan 04 Task 19):** channels exist when `.splice/channels/` has files; `splice explain <path> --json` names the source (`source=main` or `source=channel:NAME`); never edit inside `<!-- splice:NAME:begin …` blocks, change the channel source instead; `splice check --json` covers every channel.
- [ ] **Step 3c: README, in place.** Keep `## Channels` and the `\n## ` boundary after it (the channels test relies on both). Add: install (`pnpm dlx @keelstackhq/splice`, release archives + `SHA256SUMS`, the `keel-splice` source install); `channel add|remove` rows in the commands table; the bot section with `scope` and the four results; the Renovate alternative (spec §13 caveat); writing templates (link to the contract); one paragraph comparing with Copier; license (`LICENSE`). Replace "Status: under construction" with the current state (0.2.0, unreleased until the tag).
- [ ] **Step 4: Run `cargo nextest run --test docs --test cli_basic` — PASS**
- [ ] **Step 5: Commit** — `docs: agent skill and product README`

---

## Milestone wrap-up (after Task 5)

- [ ] Rewrite `docs/PROGRESS.md` Blocked: drop "Plan 03: not executed" and "SKILL.md channel lines"; replace "Release 0.2.0 (npm)" (NPM_TOKEN) with the Blocked items below. C2's dogfood step and C3 stay Blocked until 0.2.0 is on npm and `v1` exists. Status line updated.
- [ ] Spec §15: milestone 3 done; milestone 4's "follow plan 03" sentence points at the Blocked release.
- [ ] Full checks: `cargo fmt --check && cargo clippy --all-targets --locked -- -D warnings && cargo nextest run --locked`, the node glob line, actionlint — paste summaries. Commit `docs: milestone 3 progress`.

## Blocked (expected, human-only — copy into docs/PROGRESS.md when reached)

- Repo settings on `keelstackhq/splice` (the repo exists): make `ci-ok` the only required check; merge queue optional.
- **Visibility:** make `keelstackhq/splice` public before the first tag (npm provenance needs a public repo; the `npm` job fails fast otherwise).
- **Environment:** create the GitHub environment `release` (Settings → Environments) with a tag rule `v*` and required reviewers; the reviewers are the only gate before publishing.
- **npm:** an owner of the npm scope `keelstackhq` publishes each of the six names once by hand (e.g. `0.0.0-placeholder` with `--tag next`, or 0.2.0 from a local stage), then adds a trusted publisher on each package (repo `keelstackhq/splice`, workflow `release.yml`, environment `release`); tag within 2 days.
- Run the `release` workflow by `workflow_dispatch` once (dry run: check, build, smoke on all five targets) and the `action-selftest` workflow.
- Push tag `v0.2.0` after plan 03 is on `main`; check the five archives, `SHA256SUMS` and the six npm packages; run `pnpm dlx @keelstackhq/splice@0.2.0 --version` on a clean machine.
- Tag `v1` for the Action (`git tag -f v1 && git push -f origin v1`; the human does this, agents never force-push; it does not trigger a release) and enable the bot in a consumer (plan 04 C3) with a token secret.

## Pre-flight rulings applied

| ID | Change |
|---|---|
| A1 | Node tests run via globs everywhere (Global Constraints, T1, T3–T4b); T1 fixes `CLAUDE.md`. |
| A2 | `npm/splice/platforms.json` is the one committed source (T2), used by stage and `platform.js`. |
| A3 | `binaryPath({ resolve, platform, arch, env })` (T3 interface and test). |
| A4 | Unsupported/missing platform exits 3 with the `keel-splice` hint; `SPLICE_BINARY` override; spec §12. |
| A5 | `LICENSE` created in T2. |
| A6 | `checkVersion` also checks the launcher version and pins; tests per field. |
| A7 | `summarize(report, exitCode, { version, dirty })`. |
| A8 | `to` with `scope: all` throws before splice runs (T4b). |
| A9 | T1 defines the sha pin set; T2 reuses it; actionlint covers both. |
| B1, B9 | Spec §13 (inputs, results, `scope`) in T4/T4b; spec §14 skill trigger in T5. |
| B2, E2 | Trusted publishing, Node 24, no npm token; first publish + trusted publisher are Blocked. |
| B3, E4 | Blocked "repo public"; fail-fast preflight in the `npm` job. |
| B4 | Stage writes `repository` from `GITHUB_REPOSITORY`, `license`, no `scripts`; tested. |
| B5 | `--tag next` and `--prerelease` for versions with `-`. |
| B6 | Tag trigger `v[0-9]+.[0-9]+.[0-9]+*`; `check-version` rejects non-semver; test. |
| B7 | Global hardening constraints; `ci-ok` reads the result via `env:`. |
| B8 | Composite inputs only via `env:`; `validateInputs`; `git check-ref-format`. |
| B10 | First release `v0.2.0`. |
| C1 | `drift` result from exit code + `git status`; exit 2/3 fail; fixtures. |
| C2 | Fetch the bot branch ref before `--force-with-lease`; selftest runs twice. |
| C3 | `gh auth setup-git` with `token`; README example with `persist-credentials: false`. |
| C4 | Labels added after the PR, failure is a warning. |
| C5 | `unknown_flags` moved to `tests/common`; docs test covers `channel add|remove`. |
| C6 | README extended in place, Channels boundary kept. |
| C7 | Linux check accepts `static-pie linked` or `statically linked`, no `interpreter`. |
| C8 | `scripts/smoke.sh` (tag `v1.0.0`, `--data project=smoke`, `core.autocrlf=false`). |
| C9 | Smoke on native targets only; x86_64 darwin gets a `file` check. |
| C10 | Job name `ci`. |
| C11 | `rust-toolchain.toml` pinned to 1.97 (T1 Step 1, own commit); `rustup toolchain install`, no dtolnay. |
| C12 | `[profile.release]` added; `publish = false` stays. |
| D1 | Pinned actionlint + shellcheck; RED on a scratch copy. |
| D2 | `workflow_dispatch` dry run: no tag compare, `release`/`npm` only on push. |
| D3 | Signal and exit 0/1/3 tests. |
| D4 | `createRequire(import.meta.url).resolve` default resolver. |
| D5 | Fixtures generated from the binary; commands recorded. |
| D6 | `version: none` for the selftest (spec §13). |
| D7 | `cargo check --target x86_64-pc-windows-msvc` in CI. |
| D8 | TLS guard via `cargo tree … | grep`. |
| E1, E3, E5, E6 | Blocked list rewritten (repo exists; environment; `v0.2.0`; `v1` is human-only and does not release). |
| E7 | hull launcher parity noted; no hull change. |
| F1 | T5 Step 3b + plan 04 T19 tick + PROGRESS line removed. |
| F2 | Task 4b. |
| F3 | T3 creates the launcher at 0.2.0; PROGRESS `04/T21 npm part`. |
| F4, F5 | Milestone wrap-up. |

## Self-review notes

- Spec §12 (targets, archives, checksums, npm shape, launcher exit codes, trusted publishing), §13 (Action, `scope`, results, Renovate doc), §14 (skill) are covered by Tasks 2–5; CI by Task 1.
- The Action installs from npm, so outside the selftest (`version: none`) it works only after the first npm publish (Blocked).
- Revised 2026-10-20 after the pre-flight scan; task numbers unchanged, Task 4b inserted.
