# Progress

One line per finished task: `YYYY-MM-DD · plan/task · commit · test summary`.

## Done

2026-10-14 · 01/1 · 91509c3, 5146899 · fmt+clippy clean, nextest 9/9 passed
2026-10-14 · 01/2 · 4bd8667, 04fa3a7 · fmt+clippy clean, nextest 13/13 passed
2026-10-14 · 01/3 · d887f00, 4584e35 · fmt+clippy clean, nextest 19/19 passed
2026-10-14 · 01/4 · 4ee049d, 7509265 · fmt+clippy clean, nextest 28/28 passed
2026-10-14 · 01/5 · cf8c979, 2b54b17 · fmt+clippy clean, nextest 37/37 passed (corpus 14 cases)
2026-10-14 · 01/6 · 6bfc236, b8fd6d5 · fmt+clippy clean, nextest 58/58 passed
2026-10-14 · 01/7 · e9b6260, 2f3ce9d · fmt+clippy clean, nextest 71/71 passed
2026-10-14 · 01/8 · c36008f, bc1e919 · fmt+clippy clean, nextest 93/93 passed
2026-10-14 · 01/9 · 133cdbc, 4966458 · fmt+clippy clean, nextest 113/113 passed
2026-10-14 · 01/10 · a0b087e, 8a73b02 · fmt+clippy clean, nextest 125/125 passed (a0b087e also adds a CLI test for the task 9 base-only fix)
2026-10-14 · Milestone 1 reviewed · d8591f0 · 8 findings fixed (.git segments, partial --json reports, shared ops prologue); nextest 128/128 passed
2026-10-14 · 02/1 · e6c843e, 4d90fa4 · fmt+clippy clean, nextest 138/138 passed
2026-10-14 · 02/2 · c9711ce · fmt+clippy clean, nextest 153/153 passed (plan test crlf_ours_is_kept corrected: adjacent edits conflict in diff3)
2026-10-14 · 02/3 · 95402f3, df9dd67, 694cff3 · fmt+clippy clean, nextest 185/185 passed
2026-10-14 · 02/4 · e3c3017, 4a6ef01 · fmt+clippy clean, nextest 201/201 passed
2026-10-14 · 02/5 · 4e71c56, da26f5f · fmt+clippy clean (also clippy --target x86_64-pc-windows-gnu), nextest 208/208 passed
2026-10-14 · 02/6 · 79c22bb, 3da40fa · fmt+clippy clean (linux + windows-gnu), nextest 228/228 passed
2026-10-14 · 02/7 · 7fe7d4f, ca1ed33 · fmt+clippy clean (linux + windows-gnu), nextest 234/234 passed (all 12 fixture rows verified)
2026-10-14 · 02/8 · 263f738, 5c7fa60 · fmt+clippy clean (linux + windows-gnu), nextest 249/249 passed
2026-10-14 · 02/9 · 336d613, d68311e · fmt+clippy clean (linux + windows-gnu), nextest 258/258 passed
2026-10-14 · Milestone 2 reviewed · fc0be00 · safe restore advice under --allow-dirty, refuse merging over markers, helpers; nextest 263/263 passed
2026-10-14 · 03/1 · c9b39df, 086963c · actionlint clean; fmt+clippy+nextest 263/263 locally, warm wall time 8.3s (first GitHub run: see Blocked)
2026-10-14 · 03/2 · fd61fd2 · actionlint clean, node 6/6, nextest 263/263; release build + musl build + smoke test run locally (GitHub dry run: see Blocked)
2026-10-14 · 03/3 · fffa3f0, 54cddfb · node 23/23, nextest 263/263, actionlint clean; local proof: staged linux-x64 + launcher installed via npm and via pnpm 10.28 → `splice --version` = `splice 0.1.0`, exit codes pass through
2026-10-14 · 03/4 · 2020d0e, 5017a25 · actionlint+shellcheck clean, node 47/47 (7 stub-gh/git publish tests), nextest 263/263; fixtures are real splice output (GitHub selftest: see Blocked)
2026-10-14 · 03/5 · b33b163, 6f439b5 · fmt+clippy clean (linux + windows-gnu), nextest 264/264 (incl. docs flag drift test), node 47/47
2026-10-14 · Milestone 3 reviewed · ad123cd, 5abc9be, 562408b · release tag filter, same-repo bot PRs, token fallback; relative local sources resolved from the repo root (found in a manual end-to-end run); nextest 273/273, node 48/48, actionlint+shellcheck clean

## Notes for later tasks

(none open)

## Blocked

<!-- Task, what is missing, exact action a human must take. -->

- GitHub org / repo name (decide first; blocks the items below): the spec, README, action README
  and npm defaults say `keelstack/splice` (Action `keelstack/splice/action@v1`), but this repo is
  `keelstackhq/splice`. Either move the repo to `keelstack/splice`, or change the docs and spec
  §12–§13 (and the fallback in `scripts/npm-stage.mjs` / `npm/splice/package.json`) to
  `keelstackhq`. npm provenance already follows `GITHUB_REPOSITORY`.
- Repo visibility: `npm publish --provenance` needs a public repo. Make it public before pushing
  the `v0.1.0` tag.
- CI pin check: on the first CI run, confirm the `taiki-e/install-action` step installs both
  `nextest` and `actionlint` (it is pinned to the `nextest` tag's sha with `tool: nextest,actionlint`).
- Contract sign-off (does not block the build): hull must agree to spec §10 items 10–27 (post
  `when` required, manifest defaults, subset rules, symlinks, empty files, path-segment rules,
  piece names, unsafe-path writes), the §8.1 usage rules for `new`, and the §5 rule that template
  vars may not use `pieces`/`piece`/`template`. Then add them to `docs/template-format.md` §9 in
  both repos together.
- plan 03 Task 1 (CI on GitHub): the workflow is written and linted locally but has never run on
  GitHub. Action: open a PR from `claude/awesome-galileo-cunc0r` (with a personal token or the web
  UI, not `GITHUB_TOKEN`, which does not trigger CI), confirm both matrix legs (ubuntu, windows) and
  `ci-ok` pass, then make `ci-ok` a required check in branch protection for `main`.
- plan 03 Task 2 (release dry run): run `.github/workflows/release.yml` via "Run workflow"
  (workflow_dispatch) on the branch; confirm all five builds and smoke tests pass (macOS x64 runs
  under Rosetta). No release or npm publish happens on a dispatch.
- plan 03 Task 3 (npm publish): create the npm org `keelstack` (or adjust the six package names in
  `npm/platforms.json` / `npm/splice/package.json` if the org differs), add repo secret `NPM_TOKEN`
  (automation token with publish rights), then push tag `v0.1.0` (Cargo.toml is 0.1.0). The release
  workflow builds, creates the GitHub release and publishes with provenance; afterwards verify
  `pnpm dlx @keelstack/splice --version` on a clean machine. Note: the plan names the GitHub org
  `keelstack` while this repo is `keelstackhq/splice`; provenance now follows GITHUB_REPOSITORY.
- plan 03 Task 4 (update bot on GitHub): after the npm publish, run `.github/workflows/action-selftest.yml`
  (Run workflow) and confirm `result=current`. Then, in a sandbox consumer repo made with
  `splice new` from an older template tag, add a workflow using `keelstackhq/splice/action@<sha or v1>`
  with a GitHub App or fine-grained PAT (contents, pull-requests, workflows: write) as `token`,
  and check that it opens one PR on `splice/update`, refreshes it on rerun, prefixes `[conflicts]`
  on a conflicting edit, and closes it once the repo is current. Tag `v1` for the action after that.

## Status

Written 2026-10-14 when the build loop stopped. All three plans are done except plan 03 Task 2
Step 4 (the release dry run on GitHub), which needs the repo on GitHub; each milestone was
reviewed as a whole and its findings fixed.

**What works (verified locally):**
- `splice new | add | update | check | diff | explain`, all with `--json` (exactly one JSON object
  on stdout) and the contract exit codes (0 ok, 1 drift/conflict/update available, 2 usage,
  3 template/env error).
- Templates from a local git path or any git URL (`gh:`, https, ssh, file), `//subdir`, tags,
  `--ref`, `--pre`; a cache by URL and commit; `check --offline` makes no git call.
- The subset gate + MiniJinja renderer with a 14-case conformance corpus; path safety (no `..`,
  absolute, drive, `.git`, Windows reserved names, symlinks, lock path).
- Ownership classes: owned (drift refuses unless `--force`), shared (text diff3 and
  json/yaml/toml key-path merges, CRLF kept), seed (never modified).
- Migrations (`[[step]]`, `[[remove]]`), lock written last and atomically, post commands, restore
  advice only for a verified-clean tree, refusal to merge over leftover markers.
- Golden v1 → v2 update test covering every row of the plan's fixture table.
- CI and release workflows, npm launcher + platform packages (installed and run through pnpm
  10.28 locally), the update-bot composite Action (stub-tested), `skills/splice/SKILL.md`, README.
- Tests: 273 Rust (nextest), 48 Node; clippy clean on Linux and `x86_64-pc-windows-gnu`;
  actionlint + shellcheck clean; no openssl/rustls/native-tls in the dependency tree.

**Not verified:** anything that runs on GitHub or npm (CI runs, release builds on macOS/Windows/
arm64 runners, npm publish, the bot opening real PRs). The Windows code paths compile and lint
but have not run on Windows.

**Try it:**

```bash
cargo build --release
T=$(mktemp -d)
cp -r tests/fixtures/template/v1 "$T/tpl"
(cd "$T/tpl" && git init -q && git add -A && git commit -qm v1 && git tag v1.0.0)
cd "$T" && /path/to/splice/target/release/splice new ./tpl demo --piece app:web --data project=demo --yes
cd demo && git add -A && git commit -qm init
splice explain package.json --json   # class, piece, glob
splice check --json                  # exit 0
# make a v2: copy tests/fixtures/template/v2 over tpl, commit, tag v2.0.0, then:
splice diff                          # patches, writes nothing
splice update --json                 # exit 1: ci.yml conflict markers (the fixture's planned conflict)
```

The test suite: `cargo nextest run`; `node --test 'scripts/**/*.test.mjs'
'npm/splice/test/**/*.test.mjs' 'action/test/**/*.test.mjs'`.

**Blocked items for you:** see "Blocked" above, in this order: org/repo name → contract sign-off
with hull (independent) → make the repo public → open a PR to run CI and require `ci-ok` →
release dry run → npm org + `NPM_TOKEN` + tag `v0.1.0` → bot selftest and sandbox run → tag `v1`.
