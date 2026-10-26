# Progress

One line per finished task: `YYYY-MM-DD · plan/task · commit · test summary`.

## Done

- 2026-10-17 · 01/T0 · 2120a92 · docs only; stale org refs grep-verified
- 2026-10-17 · 01/T1 · c054c42 · CLI scaffold; unit 2, cli_basic 1 passing
- 2026-10-17 · 01/T2 · 10ade68 · source parsing and git wrapper; review clean
- 2026-10-17 · 01/T3 · 9abe113 · fetch/cache by commit; tests/fetch 5 passing (6 after lock-commit check)
- 2026-10-17 · 01/T4 · cf55827 · manifest validation; review clean
- 2026-10-17 · 01/T5 · b217a2d..95330ca · subset gate, renderer, conformance corpus; review round fixed 3 gate gaps, clean
- 2026-10-17 · 01/T6 · 4193485 · render_tree + classes; lib 25, render_tree 9 passing
- 2026-10-17 · 01/T7 · f95ffc1 · lock read/atomic write, fsio checks; lib 37, fetch 6 passing
- 2026-10-17 · 01/T8 · cd2a0e7..0aa315b · splice new; cli_new 15 passing; post output to stderr
- 2026-10-17 · 01/T9 · abd2847..f5cfcf5 · splice add, reduced planner; unit 48, cli_add 10 passing; conflicts write nothing
- 2026-10-17 · 01/T10 · 5f16bd9 · splice explain; cli_explain 5 passing
- 2026-10-17 · 01/final · c2c021b..427932f · final-review fixes (add --force keeps shared, absolute local source, files/-less piece, spec §8.3); cargo test 112 passing, fmt and clippy -D warnings clean
- 2026-10-17 · 02/T1 · ce6b4f0 · fixture v2/v3/v2-nolib, v1 extended, migrations loader; unit 54, manifest 5 passing
- 2026-10-17 · 02/T2 · 4e1d84f..ad185de · text 3-way merge (diffy diff3, contract labels, CRLF) and find_markers; unit 65 passing
- 2026-10-17 · 02/T3 · 31e3c33..cd2c4e7 · json/yaml/toml key-path merges and merge3 (fast paths, text fallback, markers on conflict); unit 101 passing
- 2026-10-17 · 02/T4 · 069a53c · pure update planner (Disk snapshot, Plan, render_removes, class changes in spec §10.12); planner 15 passing
- 2026-10-17 · 02/T5 · f8474b6..aa6b546 · apply (files, removals, steps, post, atomic lock last) and post::run_steps; apply 10 passing
- 2026-10-17 · 02/T6 · e087055..9d3ce8f · splice update (prepare + run: dirty/downgrade/min_splice/removed-piece refusals, drift refusal, moved tag, locked --data refused, no newer stable = up to date); cli_update 16 passing
- 2026-10-17 · 02/T7 · b0d87e8 · golden v1→v2 update with edits on every class (2 snapshots) + crash-recovery rerun; resolve-then-check test written but ignored until Task 8 (`splice check`); golden_update 2 passing, 1 ignored
- 2026-10-17 · 02/T8 · 7662b3a · splice check (owned drift, conflict markers, update available; --offline; fetch failure is a message); un-ignored resolve_conflict_then_check_passes, strengthened rerun_after_restore_succeeds; cli_check 7 passing, golden_update 3 passing
- 2026-10-17 · 02/T9 · 7e9d0d5 · splice diff (update plan as unified diff or --json changes, writes nothing, exit 0 on drift/conflict); add on the full planner (merges shared files, --allow-dirty, C3 writes nothing on drift/conflict, reduced planner deleted); check uses a local update-available flag, CRLF marker case; cli_diff 3, cli_add 16 passing
- 2026-10-17 · 02/final · 7288d7f · final-review fixes ([[remove]] paths see `template`, check preflights lock paths and reports the lock class, wrap_whole CRLF, TOML fallback guard, exit 3 after writes lists applied changes, prerelease-only template is up to date, explain human mode quiet on stderr, failed-post rerun converges); cargo test 243 passing, fmt and clippy -D warnings clean
- 2026-10-17 · 04/T1 · 8298140 · channel lock types (`[channel]`, `[sections]`), corruption checks, reserved `.splice/` paths; main lock byte-identical guard; review clean
- 2026-10-17 · 04/T2 · 351ba2c..44f1fa7 · channel path filters (`--only`/`--exclude`, stored meta); fix round 1: apply_tree and stored-meta tests (RED via reviewer mutations)
- 2026-10-17 · 04/T3 · c97c056 · source registry and one-owner claim check; lib tests green (RED 10 failed on stubs); review clean
- 2026-10-17 · 04/T4 · 839efab · channel fixtures (`channel`, `channel-b`) and helpers (`channel_repo*`, `chan_commit`); test-only
- 2026-10-17 · 04/T5 · 403f92f..f3e2450 · piece `[sections]`, fragment rendering, main refusal; fix round 1: symlinked fragments/`files/` refused; hull's Nunjucks corpus run must pick up the new conformance case `section-fragment` (C2)
- 2026-10-17 · 04/T6 · 4e4bcfe..f3f4915 · section markers and block edits (13 lib tests, RED on todo! stubs); fix round 1: body normalisation, marker injection refused, BOM, exact begin
- 2026-10-17 · 04/T7 · 35431b1 · pure section planner (insert, adopt, update, drift, gone, broken, --force)
- 2026-10-17 · 04/T8 · fd7b642 · apply with `LockOut`, section writes via fsio, deferred locks (`write_locks`); lib 216 passing, M2 suites unchanged
- 2026-10-17 · 04/T9 · d6309e7..05d3c42 · channel CLI grammar and flag conflicts; lib 217, all suites green; fix round 1: conflict tests assert both flag names
- 2026-10-17 · 04/T10 · 4f3a567 · `splice channel add`; cli_channel 17/17, 372 passing
- 2026-10-17 · 04/T11 · 04f85c6..0d49096 · `splice add --channel`; fix round 1: non-vacuous G10 seed-drop tests (RED by deleting each retain); 384 passing
- 2026-10-17 · 04/T12 · 81dc5e1 · `splice update --channel` (filters, `--data` refused); 401 passing
- 2026-10-17 · 04/T13 · 0d13901 · `--track` updates and subtree compare; 410 passing
- 2026-10-17 · 04/T14 · 98ce67d..4482506 · `splice update --all` (all files → dedup post → all locks last); fix round 1: spec rulings, main lock byte-identical in --all; 424 passing
- 2026-10-17 · 04/T15 · 2c393ad · `splice check` across sources and sections; cli_check 19 passing (RED 9 failed)
- 2026-10-17 · 04/T16 · 23bed57..b7857ce · `diff --channel`, `explain` across sources; fix round 1: drift reporting per command family, `HOST#NAME` paths
- 2026-10-17 · 04/T17 · fdee2f2..10954a7 · `splice channel remove`; fix round 1: overlap guard (exit 3) and more remove tests
- 2026-10-17 · 04/T18 · cedf673 · golden main + channel `update --all` (snapshots; fixed --all conflict attribution); review approved
- 2026-10-18 · 04/T19 · df7c542 · spec index (§10 item 15) and README Channels section; `readme_channel_flags_exist`; SKILL.md part landed in 03/T5
- 2026-10-20 · 04/T20 (03/T4b) · 8db06ce, 7c80eea · update-bot `scope` input (`main` | `all`, default by `.splice/channels/*.toml`); `to` with `all` refused; PR body grouped by source; fix 7c80eea reads bare/basic/literal lock keys; node tests 49 passed, actionlint and shellcheck clean
- 2026-10-18 · 04/T21 · splice 0.2.0 (Cargo.toml, Cargo.lock) · `cargo fmt --check` and `clippy --all-targets --locked -D warnings` clean; `cargo nextest run`: 461 tests run, 461 passed, 0 skipped; no `.snap.new` (snapshots redact the version); node tests: n/a (plan 03 not landed); npm package versions: after plan 03
- 2026-10-18 · 04/final · 0b68121 · final-review fix wave: section rerun after a failed step/post/lock write converges (no false drift), fragments refuse any channel's marker line, `--all` checks every source's targets before the first write, explain `source=` only with channel locks, `add --channel` refusal names `channel NAME`, Task 11/18 deferred minors; `cargo test` 465 passing, fmt and clippy -D warnings clean
- 2026-10-20 · 04/T21 npm part (03/T3) · af72999, b60f287 · npm launcher at 0.2.0 · node tests run: 26 passed (launcher, npm-stage, check-version); installed launcher + linux-x64 package prints `splice 0.2.0`; release.yml `npm` job added; fix b60f287 (pipefail, E404-only check); node tests 27 passed, actionlint clean
- 2026-10-20 · 03/T1 · 6e09191, fd5bce1, f6e73ff · toolchain pinned to 1.97; single-job CI with `ci-ok`; actionlint installed from its release with a pinned sha256; first CI run green (ci 77 s, ci-ok 4 s)
- 2026-10-20 · 03/T2 · cf0bd9a · release workflow for five targets with `SHA256SUMS`; Windows check fixed with cfg(unix); actionlint clean
- 2026-10-20 · 03/T4 · 76dea13, 80e999c, 8490dcc · update-bot composite action; fix round proves lease fetch (stale tracking ref) and clears checkout credentials; selftest fails if run 2 opens a new PR; node tests 39 passed, actionlint and shellcheck clean
- 2026-10-20 · 03/T5 · 5c96f2f · agent skill (`skills/splice/SKILL.md`, body 65 lines) and product README; `cargo nextest run`: 467 passed; review approved
- 2026-10-18 · 04/C1 · keelstackhq/decisions#2 (3bfa3ec, 75a0ab1; merged 32f618a) · `channel/` template and D-004; dry run 2026-10-20 with local splice 0.2.0 on a scratch hull 42fd8c3: stale snapshot → `channel add` exit 1 (owned conflict, nothing written); after syncing the snapshot → exit 0, `CLAUDE.md#decisions` created, `splice check --json --offline` exit 0
- 2026-10-18 · 04/C2 · keelstackhq/hull#5 (merged 42fd8c3) · `rigging/` channel template, `AGENTS.md` seed, `section-fragment` in hull's conformance run; hull owner sign-off recorded in hull PROGRESS; dogfood and the hull release stay Blocked
- 2026-10-21 · 04/C3 · hull#7, splice#9, hoard#3, berth#2, example-app#9, keelstack#5; decisions#4 (32fc80a) · all six repos get org decisions from `gh:keelstackhq/decisions//channel` (import-claude, or import-project in hull-generated repos), adopted with a local 0.2.0 build; sync hooks and `adopt/` removed; found and fixed the formatter bug (09b3a3a); update bot waits for npm
- 2026-10-22 · fix/action · update bot in a channel-only repo (no `.splice/lock.toml`): summary.mjs read the main lock when an up-to-date `update --all` printed no `source=` messages (ENOENT, keelstackhq/keelstack run 39750735299); now reads it only when it exists; 2 CLI tests; node tests 51 passed
- 2026-10-22 · 04/C3 (splice bot) · `.github/workflows/splice.yml` · update bot enabled in splice itself (`uses: ./action`, `scope: all`, weekly + workflow_dispatch, token `SPLICE_BOT_TOKEN` or `github.token`); `splice check --json` exit 0; first real run 39752767541 (42618b7): real `gh auth setup-git`, channel fetch, result `current`, no PR; `pr create`/`pr edit` still unproven (Blocked)
- 2026-10-21 · 04/C2 dogfood · local 0.2.0 build, `gh:keelstackhq/hull//rigging` at 09d2a8d · hull `base` render: `channel add --piece claude` refuses (3 of 4 agent files differ from base, exit 1, nothing written) until the four base agent files are removed, as the planned hull release does; then claude + `add skills --name agents|claude` ok, `check` and `update --all` clean, `pnpm format:check` clean. Plain repo: `--piece core --piece claude` writes the `AGENTS.md#rigging` section, `vp fmt` leaves it unchanged, `check` clean
- 2026-10-20 · 03/release dry run · https://github.com/keelstackhq/splice/actions/runs/39487980036 (d976df0) · `workflow_dispatch`: check ok; build + smoke ok on x86_64/aarch64 linux-musl (static check ok), aarch64-apple-darwin, x86_64-pc-windows-msvc (first Windows binary run); x86_64-apple-darwin built and Mach-O checked, not run; `release` and `npm` skipped as designed
- 2026-10-20 · 03/T4 selftest · https://github.com/keelstackhq/splice/actions/runs/39487984590 (d976df0) · `action-selftest.yml` green on GitHub: run 1 updated, run 2 rewrote the branch after a stale ref and edited the PR, run 3 drift failed without a push (`gh` stubbed)
- 2026-10-20 · port/raw · d20af8e · `{% raw %}` blocks in the subset gate and a stray `#}` rejected (spec §10 items 16, 17), adapted from PR #1 to this gate; 4 raw conformance cases; contract sign-off is Blocked
- 2026-10-20 · port/relative-sources · bcf753a · relative local sources in a lock resolve from the repo root (`parse_from`, main and channel locks), lenient canonicalisation (`lenient_canonical`, `strip_verbatim`) for the stored path and the cache key (spec §10 item 18); main already stored absolute paths, so `new`/`channel add` were not buggy; `tests/cli_local_source.rs`
- 2026-10-20 · port/posix-sh · 3ac583b · template commands run with POSIX `sh -c` on every OS; on Windows `sh` from PATH else Git for Windows' `sh.exe` via `git --exec-path` (`sh_from_exec_path`), exit 3 naming the missing sh (spec §10 item 19); main prints no shell-specific restore command, so `restore_cmd`/`Undo` were not needed
- 2026-10-20 · port/windows-ci · 13af910 · `windows-latest` leg in the CI `ci` job (matrix, `fail-fast: false`; shellcheck, node tests, dependency-tree check and actionlint stay Linux-only; `ci-ok` still aggregates `ci`); test portability audit from PR #1: symlink tests gated `#[cfg(unix)]`, `stage_all` + `assert_head_matches` (same-size edits, byte-exact fixture commits), `-c core.autocrlf=false` for test git, empty gitconfig file instead of `/dev/null`, lock `source = '...'` redaction; `cargo clippy --target x86_64-pc-windows-gnu` clean. First Windows run happens on GitHub
- 2026-10-20 · port/getting-started · c26784e · `docs/getting-started.md` ported from PR #1 and re-verified against this CLI (outputs recaptured from the 0.2.0 binary; adds a short Channels section, `add` never writes markers, `check` prints nothing when clean); linked from the README; covered by `tests/docs.rs`
- 2026-10-20 · port/review-fixes · acd5965 · test git env for every raw `git`/`sh` in tests (`sh_with_git_env`, `GIT_CONFIG_COUNT` autocrlf=false, porcelain via `git_stdout`); shellcheck installed on Linux only; `update`/`add`/`channel update` keep a relative lock source (`Lock::keep_relative_source`, 2 tests); Windows looks up only `sh.exe`; spec §10 items 17, 18 reworded (`#}` behaviour change, one-time Windows refetch)
- 2026-10-20 · port/docs-review · the port merge · getting-started samples re-captured (`--ref v1.0.0`, real `add --name docs` lines); `tests/docs.rs` trims `[]`, treats `pnpm dlx`/`npx @keelstackhq/splice` as `splice`, rejects unknown subcommands in code spans, with a negative test of the checker; SKILL.md exit-2 row and `add` merge wording fixed
- 2026-10-21 · contract/§9.21 · 2af3559 · portable path segments (`:`, control chars, trailing dot/space, Windows reserved names, `.git`) for rendered paths, piece ids and instance names (exit 2 user-chosen, 3 piece default), case-insensitive collisions incl. directory prefixes in `render_tree`; `cargo nextest run`: 492 passed
- 2026-10-21 · contract/§9.15-20,23,26 · 7382258 · `description`/`prompt` defaults, keyword var names reserved, comment `-`/`+` and non-ASCII tag whitespace rejected, symlinked manifests/piece dirs/migrations rejected, raw or `[sections]` need `min_splice` >= 0.2.0 (channel fixtures and `inline_template` moved to 0.2.0; spec §10 G15); `cargo nextest run`: 500 passed
- 2026-10-21 · contract/§9.22 · 5734ee9 · `[[remove]]` paths render with the from-version (test tells 1.0.0 from 2.0.0); 0-byte non-`.jinja` file test (§4, already passed); spec §10 maps items to contract §9, new item 20; `cargo nextest run`: 501 passed
- 2026-10-21 · contract/review · follow-up to 5734ee9 · merge base of `update` loads and renders leniently (`Template::load_merge_base`); instance-name exit code by source (`NameSource`), locked names re-checked (exit 3 naming the lock); duplicates and collisions count empty `.jinja` renders; `cargo nextest run`: 509 passed
- 2026-10-21 · fix/section-blank-lines · 09b3a3a · section blocks are written as `begin`, blank line, body, blank line, `end`; reading drops blank lines next to the markers before drift/adopt/hash (G7), so `vp fmt`/prettier-formatted hosts, the old layout with old locks and CRLF hosts check clean; in-body edits stay drift; golden snapshots gained the blank lines; `cargo nextest run`: 514 passed
- 2026-10-22 · release/0.2.0 · 1994e4b · tags `v0.2.0` and `v1` on 1994e4b (pushed by the owner); release run 39749263770 green: GitHub release with 5 archives + `SHA256SUMS`, six npm packages at 0.2.0 with provenance (trusted publishing); `npx -y @keelstackhq/splice@0.2.0 --version` and `pnpm dlx` print `splice 0.2.0` (linux-x64)
- 2026-10-25 · 05/T1 · eee0308 · `update --ref` and `diff --ref` with a git-ancestry check, `--to` refuses build metadata (F1)
- 2026-10-25 · 05/T2 · 55de5b3 · latest, `check` and migrations from an untagged lock use ancestry (`newest_descendant_tag`, `tag_commits`); golden F1 snapshot; 534 tests passing
- 2026-10-25 · 05/fix · 923e533 · review round: source fetched once per update, a lock commit missing from the source is a clear exit 3, tests pin migration ancestry (§10.21) and prerelease/self-tag cases
- 2026-10-26 · release · ci: a merged version bump tags and releases itself · merging a version bump to main tags and releases it (`release.yml` tag job, `scripts/release-tag.mjs`); node 58 passing

## Blocked

<!-- Task, what is missing, exact action a human must take. -->

- release/auto-tag: before merging the auto-release PR, the owner allows branch `main` in the
  `release` environment (Settings → Environments → release → Deployment branches and tags: add
  branch `main` next to tag `v*`); otherwise `release` and `npm` are refused on the main-push run.
  If a tag ruleset restricts creating `v*` tags, allow GitHub Actions (or the run's GITHUB_TOKEN)
  to create them.

- Task 03/T4 · Real `gh auth setup-git`, `pr create` and `pr edit` with a token: `action-selftest.yml` passed on GitHub but stubs `gh`, so the first consumer run (plan 04 C3) is the first real check.
- **Plan 04 C3 update bot:** enable the bot (`scope: all`) in the six repos; its prerequisites (`@keelstackhq/splice` 0.2.0 on npm, Action tag `v1`) are done.
- The x86_64-apple-darwin binary is cross-built on arm64 and only checked with `file`; it is never run.
- **Bot tokens:** each adopting repo needs the bot workflow enabled and a token that can read `keelstackhq/decisions` and `keelstackhq/hull` if they are private (repo secret, e.g. a GitHub App token passed as `token`).
- **hull release** removing the four agent files (C2 last step): tag a hull version.

## Status

Milestones 2 and 3 done (release workflow, npm launcher, update-bot Action, skill file, product README). Milestone 4 (channels) code complete at 0.2.0 (final review fixes in 0b68121); all 0.2.0 code, with the §9.15–26 contract change, is merged (keelstackhq/splice#10, d777ddf; `windows-latest` CI green); 0.2.0 is released (tags `v0.2.0` and `v1`, npm, GitHub release). Cross-repo C1 (decisions channel, D-004), C2 (hull `rigging/`, dogfooded with a local build) and C3 (six repos on the decisions channel) are done; the update bot (C3) and the hull release are next (Blocked above). Ported on top of main from PR #1: `{% raw %}` blocks and a stray `#}` rejected (spec §10 items 16, 17), relative local lock sources (item 18), POSIX `sh` for template commands (item 19), a Windows CI leg, and `docs/getting-started.md`.
