# Progress

Status and human-only blockers. Finished tasks go to `docs/history.md`, one line each.

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

splice 0.4.0 is released (a merged version bump tags and releases itself); main carries the
contract rules 9.15-28, channels, `adopt`, the update-bot Action and the npm launcher.
Everything that needs a human is under Blocked; the review-fix round is in its pull request.
