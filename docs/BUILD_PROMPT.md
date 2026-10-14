# Build prompt

Paste one of the prompts below into Claude Code at the root of this repo. The superpowers
plugin must be installed (see `.claude/settings.json`). A bare `/loop` uses `.claude/loop.md`,
a short form of prompt A.

## A. Self-paced loop (recommended)

```
/loop Build splice from the plans in this repo, one task per iteration, until every plan is done.

You are a thin orchestrator. Keep your own context small:
- Read files by section (grep, offset/limit), never whole specs or plans.
- Delegate every search to the `explorer` subagent (haiku).
- Ask every subagent for a report of at most 15 lines.

Each iteration:
1. Read CLAUDE.md and docs/PROGRESS.md. Read only the spec sections
   (docs/superpowers/specs/2026-10-14-splice-design.md) and contract sections
   (docs/template-format.md) that the next task touches.
2. Pick the next unchecked task in plan order: 01-render-new, then 02-update-check, then
   03-release-action (docs/superpowers/plans/). Skip tasks listed under "Blocked" in
   docs/PROGRESS.md unless the blocker is resolved.
3. Dispatch the `implementer` subagent (sonnet) with the plan path and task number. It works
   test-first (superpowers:test-driven-development) and commits.
4. Dispatch the `reviewer` subagent (opus) with the plan path, task number and commit range.
   Send blocking findings back to `implementer`; repeat until the reviewer approves.
5. Verify yourself with superpowers:verification-before-completion: run
   cargo fmt --check, cargo clippy --all-targets --locked -- -D warnings, cargo nextest run,
   and for npm/ or action/ changes node --test npm/splice/test action/test scripts/.
   Look at the real output; never claim a pass you did not see.
6. Make sure the task's checkboxes are ticked, add one line to docs/PROGRESS.md
   (date · plan/task · commit · test summary), commit with the plan's message (amend nothing
   that is pushed), and push if a remote exists.
7. If a step needs something only a human can provide (GitHub org/repo, secrets, npm org,
   a release tag, running a workflow on GitHub), do every part you can locally, write the
   exact remaining action under "Blocked" in docs/PROGRESS.md, and move on to the next task
   that does not depend on it. Never fake a result, skip or weaken a test, or mark a blocked
   task done.
8. When all tasks of a milestone are done, run superpowers:requesting-code-review on the whole
   milestone, fix the findings, and note "Milestone N reviewed" in docs/PROGRESS.md.

Stop the loop when all three plans are fully checked (or only blocked tasks remain) and every
milestone review is recorded. Then write the "Status" section in docs/PROGRESS.md: what works,
how to try it (commands), and the list of blocked items for me.
```

## B. One long session (no /loop)

```
Build splice by executing the three plans in docs/superpowers/plans/ in order (01, 02, 03) with
superpowers:subagent-driven-development, using the `implementer` (sonnet) and `reviewer` (opus)
subagents from .claude/agents/ and `explorer` (haiku) for searches. Follow CLAUDE.md, the spec
and docs/template-format.md (never edit the contract). One commit per task, progress in
docs/PROGRESS.md, every claim backed by real command output
(superpowers:verification-before-completion). Anything that needs a human goes under
"Blocked" in docs/PROGRESS.md; continue with independent tasks. Request a code review at the
end of each milestone. Keep going until all tasks are done or only blocked ones remain, then
write the Status section.
```

## C. What you will need to provide (expected blockers)

- The GitHub org `keelstack` and the repo `keelstack/splice`; push the kit; make `ci-ok` a
  required check (optional: merge queue).
- The npm org `keelstack` and the repository secret `NPM_TOKEN` (granular token that can
  publish `@keelstack/*`). Later: switch to npm trusted publishing.
- The first release: push tag `v0.1.0`, then check the GitHub release (5 archives +
  `SHA256SUMS`) and the six npm packages; try `pnpm dlx @keelstack/splice --version`.
- The update bot: tag `v1` on the release commit, then add the workflow to a consumer repo
  (e.g. `keelstack/hull`) with `permissions: contents: write, pull-requests: write` and a token
  secret that can trigger CI on the bot's PR.
