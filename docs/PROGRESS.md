# Progress

One line per finished task: `YYYY-MM-DD · plan/task · commit · test summary`.

## Done

2026-10-14 · 01/1 · 91509c3, 5146899 · fmt+clippy clean, nextest 9/9 passed
2026-10-14 · 01/2 · 4bd8667, 04fa3a7 · fmt+clippy clean, nextest 13/13 passed

## Notes for later tasks

- `tests/fixtures/template/v2` is a placeholder (v1 + one README line) used only for tags in
  `tests/git.rs`; plan 02 Task 1 replaces it wholesale.
- Task 7: the lock loader must build its source through `TemplateSource::parse` (rejects `-…`
  and `::` specs).
- Task 8: `Git::is_clean` treats a fresh dir inside a parent repo as a work tree; compare with
  `--show-toplevel`.

## Blocked

<!-- Task, what is missing, exact action a human must take. -->

## Status

<!-- Written when the build loop stops. -->
