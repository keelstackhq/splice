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

## Notes for later tasks

(none open)

## Blocked

<!-- Task, what is missing, exact action a human must take. -->

- Contract sign-off (does not block the build): hull must agree to spec §10 items 10–26 (post
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

## Status

<!-- Written when the build loop stops. -->
