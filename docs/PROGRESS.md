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

## Notes for later tasks

- plan 02 Task 1: `Lock::read_with_warnings` warnings are not yet surfaced by any command; surface
  them as Report messages in `update`/`check`.
- plan 02 (update/check): `Git::is_clean` treats a fresh dir inside a parent repo as a work
  tree; compare with `--show-toplevel`.

- plan 02 Task 9 (`add` on the full planner): spec §10 item 20 promises a dirty-tree check and
  `--allow-dirty` for `add`; update the §3 CLI table and README with it.

- plan 02 (apply): keep the partial Report on a failure inside the file-write loop (before the
  lock), as `SpliceError::Partial` does after it; extend the case-insensitive collision check to
  directory prefixes (`Apps/x` vs `apps/y`).

- plan 02 Task 6 (`update`): snapshot base ∪ theirs ∪ rendered removes (the planner skips a
  remove path missing from `Disk`); map `Disk::snapshot`'s wrapped "unsafe path" io error back
  to exit 3 with its message.

## Blocked

<!-- Task, what is missing, exact action a human must take. -->

- Contract sign-off (does not block the build): hull must agree to spec §10 items 10–22 (post
  `when` required, manifest defaults, subset rules, symlinks, empty files, path-segment rules,
  piece names, unsafe-path writes), the §8.1 usage rules for `new`, and the §5 rule that template
  vars may not use `pieces`/`piece`/`template`. Then add them to `docs/template-format.md` §9 in
  both repos together.

## Status

<!-- Written when the build loop stops. -->
