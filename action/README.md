# splice update bot

Composite action that runs `splice update --json --yes` and keeps one pull request, from branch
`splice/update`, in step with the template.

```yaml
name: splice-update
on:
  schedule: [{ cron: "0 6 * * 1" }]
  workflow_dispatch:
permissions:
  contents: write
  pull-requests: write
jobs:
  update:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v5
      - uses: keelstack/splice/action@v1
        with:
          token: ${{ secrets.SPLICE_BOT_TOKEN }}   # optional, see below
```

Inputs: `version` (splice npm version, default `latest`), `to` (template version, default latest),
`token` (default `github.token`), `branch` (default `splice/update`), `labels` (default `splice`),
`working-directory` (default `.`). Outputs: `result` (`updated`, `conflicts` or `current`) and
`pr-url`.

## What it does

- Changes, clean: commits to the branch, force-pushes with a lease, opens or edits the PR.
- Conflicts (splice exit 1 with conflicted files): same, with the title prefixed `[conflicts]` and
  the conflicted paths listed in the body.
- Already current: no push and no PR. An open bot PR is closed with a comment and its branch is
  deleted.
- Owned drift (exit 1, nothing written, no conflicts) and template errors (exit 3): the job fails
  and splice's messages are printed. Nothing is hidden in a PR; fix the local edit or use
  `splice update --force` yourself.

## Permissions and token

The job needs `contents: write` and `pull-requests: write`. Runners must have `git`, `gh` and
Node 20+ (GitHub-hosted Linux and macOS runners do).

PRs opened with `GITHUB_TOKEN` do not trigger `pull_request` workflows. To get CI on the bot PR,
pass a GitHub App token or a fine-grained PAT (contents and pull requests: write) as `token`.

## Self-test

`.github/workflows/action-selftest.yml` is `workflow_dispatch` only. It builds splice from the
checkout and runs the action with `version: local` (testing only: skips the npm install and uses
the `splice` on PATH) against a throwaway project that is already current. The updated and
conflicts paths open real PRs, so try those by hand in a sandbox repository.
