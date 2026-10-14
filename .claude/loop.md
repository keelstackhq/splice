Build splice: one plan task per iteration (full rules: docs/BUILD_PROMPT.md, section A).

1. Read CLAUDE.md and docs/PROGRESS.md. Find the next unchecked task in plan order
   (01-render-new → 02-update-check → 03-release-action), skipping Blocked ones.
2. Dispatch `implementer` (test-first, that task only), then `reviewer` on its diff. Fix
   findings via `implementer`. Delegate searches to `explorer`.
3. Verify with real output: cargo fmt --check, cargo clippy --all-targets --locked -- -D warnings,
   cargo nextest run (plus node --test for npm/ and action/ changes).
4. Tick boxes, add one PROGRESS line, commit with the plan's message, push.
5. Human-only step → Blocked with the exact action; move on. Milestone done →
   superpowers:requesting-code-review.

Stop when every task is checked or only Blocked remain; then write Status in docs/PROGRESS.md.
