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
      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1
      - uses: keelstackhq/splice/action@v1
        with:
          token: ${{ secrets.SPLICE_BOT_TOKEN || github.token }}   # optional, see below
```

Inputs: `version` (splice npm version, default `latest`), `to` (template version, default latest),
`scope` (`main` or `all`, see below), `token` (default `github.token`), `branch` (default `splice/update`), `labels` (default `splice`),
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

## Scope: main template and channels

`scope: main` runs `splice update`; `scope: all` runs `splice update --all --json --yes`, which
updates the main template and every channel together. The default is `all` when `.splice/channels/`
holds a lock, else `main`, so a repo that adopts a channel needs no workflow change. Either way
it is one branch and one PR. With `all` the PR body has one table per source (`### main`,
`### rig`, ...), main first; rows are attributed from the lock files, before and after the
update. `all` cannot be combined with `to` (splice rejects `--all --to`): set `scope: main` to
pin the main template. `scope: main` in a channel-only repo fails with splice's own message
(exit 2). The channel sources must be readable with the job's git credentials (private ones
need a token that can read them).

## Permissions and token

The job needs `contents: write` and `pull-requests: write`. Runners must have `git`, `gh` and
Node 20+ (GitHub-hosted Linux and macOS runners do). It targets github.com; on GitHub Enterprise
Server `gh` needs `GH_HOST` set in the job's `env`, which the action does not set for you.

The action pushes with `token`, not with the credential `actions/checkout` left behind: it passes
the token as a one-command `http.<server>/.extraheader` override, so it is never written to
`.git/config`. Two limits of the default `GITHUB_TOKEN`:

- it cannot push changes to `.github/workflows/*`, and templates often ship workflow files;
- PRs it opens do not trigger `pull_request` workflows, so the bot PR gets no CI.

So pass a GitHub App token or a fine-grained PAT with contents, pull-requests and workflows
write as `token`.

## Self-test

`.github/workflows/action-selftest.yml` is `workflow_dispatch` only. It builds splice from the
checkout and runs the action with `version: local` (testing only: skips the npm install and uses
the `splice` on PATH) against a throwaway project that is already current. The updated and
conflicts paths open real PRs, so try those by hand in a sandbox repository.
