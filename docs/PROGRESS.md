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

## Blocked

<!-- Task, what is missing, exact action a human must take. -->

- Contract wording: `docs/template-format.md` still says `keelstack` (org and scope). Change it
  jointly with hull in the same week (D-003: byte-identical in both repos); never edit it here alone.
- Joint contract PR for hull's portable-path rules, not yet in contract §9: Windows-reserved names,
  `.git` segments, trailing dot or space, `:`, control characters, and case-insensitive path
  collisions. Until it lands, splice does not reject these paths. Open it against both repos.

## Status

Milestone 2 done.
