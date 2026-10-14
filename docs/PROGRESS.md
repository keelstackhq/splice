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

## Notes for later tasks

- `tests/fixtures/template/v2` is a placeholder (v1 + one README line) used only for tags in
  `tests/git.rs`; plan 02 Task 1 replaces it wholesale.
- Task 8: validate `--piece ID:NAME` / `--data name=` with `paths::validate_segment` up front as a
  usage error (exit 2) before `render_tree` (which exits 3). Surface `Template::warnings` and
  `Lock::read_with_warnings` warnings as Report messages (stderr in human mode). Give a clear error
  when `.splice` is a regular file.
- Task 8: `Git::is_clean` treats a fresh dir inside a parent repo as a work tree; compare with
  `--show-toplevel`.

## Blocked

<!-- Task, what is missing, exact action a human must take. -->

- Contract sign-off (does not block the build): spec §10 items 10–18 (post `when` required,
  manifest defaults, subset rules, symlinks, empty files, path-segment rules, piece names) and the spec §5 rule that template vars may not use
  `pieces`/`piece`/`template` need hull's agreement; then add them to `docs/template-format.md` §9
  in both repos together.

## Status

<!-- Written when the build loop stops. -->
