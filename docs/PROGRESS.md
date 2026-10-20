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
- 2026-10-18 · 04/T19 · df7c542 · spec index (§10 item 15) and README Channels section; `readme_channel_flags_exist`; SKILL.md part after plan 03 (Blocked)
- 2026-10-20 · 04/T20 · 7d02b68 · update-bot `scope` input (`main`|`all`, default from `.splice/channels/`); `all` runs `splice update --all --json --yes`; PR body grouped per source (rows attributed from the old and new lock files, since `changes` carry no source); `all` with `to` refused; node tests 31 action tests passing, actionlint clean
- 2026-10-18 · 04/T21 · splice 0.2.0 (Cargo.toml, Cargo.lock) · `cargo fmt --check` and `clippy --all-targets --locked -D warnings` clean; `cargo nextest run`: 461 tests run, 461 passed, 0 skipped; no `.snap.new` (snapshots redact the version); node tests: n/a (plan 03 not landed); npm package versions: after plan 03
- 2026-10-18 · 04/final · 0b68121 · final-review fix wave: section rerun after a failed step/post/lock write converges (no false drift), fragments refuse any channel's marker line, `--all` checks every source's targets before the first write, explain `source=` only with channel locks, `add --channel` refusal names `channel NAME`, Task 11/18 deferred minors; `cargo test` 465 passing, fmt and clippy -D warnings clean
- 2026-10-20 · 03/T1 · 4e3874e · single-job CI with `ci-ok` (ubuntu only for now), pinned actions, node tests, no-TLS-crate check, actionlint 1.7.7 sha256-pinned; actionlint clean locally (shellcheck not installed locally); fmt/clippy clean, nextest 465 passed (~64 s cold local; CI wall time unmeasured until the first PR run)
- 2026-10-20 · 03/T2 · ba30bc4 · release workflow (5 targets, static check, smoke test, `SHA256SUMS`), `scripts/check-version.mjs`, `npm/platforms.json`, `[profile.release]`; tag filter is `v[0-9]+.[0-9]+.[0-9]+*` instead of the plan's `v*` (consistent with spec §12); new `.gitattributes` (LF checkouts on Windows, fixtures `-text`); node tests 6 passing, actionlint clean, smoke steps run locally with the debug binary; the `workflow_dispatch` dry run is Blocked; the npm job uses `scripts/npm-stage.mjs` (T3); packaging needs LICENSE (T5)
- 2026-10-20 · 03/T3 · 02dbdd9 · npm launcher (`@keelstackhq/splice`, platform lookup from `platforms.json`, exit/signal passthrough), `scripts/npm-stage.mjs`, package versions 0.2.0 (04/T21 npm part); node tests 23 passing; staged locally and ran the launcher: `splice 0.2.0`; release npm job already matches the script CLI; LICENSE copied only once Task 5 adds it
- 2026-10-20 · 03/T4 · 83bf01c · update-bot composite action (`action.yml`, `publish.sh` pushes with the input token via a one-command extraheader, `summary.mjs`), fixtures regenerated from the 0.2.0 binary, `action-selftest.yml` (manual), CI runs `shellcheck action/publish.sh`; node tests 25 action tests passing; actionlint clean (shellcheck not installed locally); spec §13 updated; selftest run on GitHub Blocked

## Blocked

<!-- Task, what is missing, exact action a human must take. -->

- Contract wording: `docs/template-format.md` still says `keelstack` (org and scope). Change it
  jointly with hull in the same week (D-003: byte-identical in both repos); never edit it here alone.
- Joint contract PR for hull's portable-path rules, not yet in contract §9: Windows-reserved names,
  `.git` segments, trailing dot or space, `:`, control characters, and case-insensitive path
  collisions. Until it lands, splice does not reject these paths. Open it against both repos.
- SKILL.md channel lines: plan 03 not executed. The lines are in plan 03 Task 5 (Step 3b); add them when `skills/splice/SKILL.md` is created.
- **Plan 03:** executed through Task 4; Tasks 5+ pending. Task 19's SKILL.md part, C2's dogfood step and C3 wait for it.
- **Release 0.2.0 (npm):** the release workflow exists (03/T2) but needs `LICENSE`, which 03/T5 adds. After that: run its `workflow_dispatch` dry run once on keelstackhq/splice (plan 03 Task 2 Step 4) and check all five targets build and smoke-test. Then: the `NPM_TOKEN` repo secret, an owner of the npm scope `keelstackhq`, then push tag `v0.2.0` on keelstackhq/splice after this work is on `main`.
- **Decisions PR merge:** the owner merges C1 (`channel/` template and D-004) in keelstackhq/decisions (D-002).
- **hull sign-off:** the hull owner approves the hull spec change (AGENTS.md to seed, `rigging/`) and answers whether `[sections]` in a hull-published template needs a contract note (D-003).
- **Action selftest:** run `.github/workflows/action-selftest.yml` (`workflow_dispatch`) once on keelstackhq/splice, and try the updated/conflicts PR paths by hand in a sandbox repo (plan 03 Task 4 Step 4).
- **Bot tokens:** each adopting repo needs the bot workflow enabled and a token that can read `keelstackhq/decisions` and `keelstackhq/hull` if they are private (repo secret, e.g. a GitHub App token passed as `token`).
- **hull release** removing the four agent files (C2 last step): tag a hull version.

## Status

Milestone 2 done. Milestone 4 (channels) code complete at 0.2.0 (final review fixes in 0b68121); the npm release, bot scope, skill file and cross-repo tasks (C1–C3) are Blocked above.
