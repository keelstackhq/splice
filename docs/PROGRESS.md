# Progress

One line per finished task: `YYYY-MM-DD · plan/task · commit · test summary`.

## Done

2026-10-14 · 01/1 · 91509c3, 5146899 · fmt+clippy clean, nextest 9/9 passed
2026-10-14 · 01/2 · 4bd8667, 04fa3a7 · fmt+clippy clean, nextest 13/13 passed
2026-10-14 · 01/3 · d887f00, 4584e35 · fmt+clippy clean, nextest 19/19 passed
2026-10-14 · 01/4 · 4ee049d, 7509265 · fmt+clippy clean, nextest 28/28 passed
2026-10-14 · 01/5 · cf8c979, 2b54b17 · fmt+clippy clean, nextest 37/37 passed (corpus 14 cases)

## Notes for later tasks

- `tests/fixtures/template/v2` is a placeholder (v1 + one README line) used only for tags in
  `tests/git.rs`; plan 02 Task 1 replaces it wholesale.
- Task 7: the lock loader must build its source through `TemplateSource::parse` (rejects `-…`
  and `::` specs).
- Task 6: a template `//subdir` that is a symlink can point `root` outside the cache; reject it
  with the path-safety work.
- Task 8: `Git::is_clean` treats a fresh dir inside a parent repo as a work tree; compare with
  `--show-toplevel`.

## Blocked

<!-- Task, what is missing, exact action a human must take. -->

- Contract sign-off (does not block the build): spec §10 items 10–14 (post `when` required,
  manifest defaults, subset rules) and the spec §5 rule that template vars may not use
  `pieces`/`piece`/`template` need hull's agreement; then add them to `docs/template-format.md` §9
  in both repos together.

## Status

<!-- Written when the build loop stops. -->
