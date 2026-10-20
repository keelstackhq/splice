#!/usr/bin/env bash
# Open, refresh or close the update PR. Driven entirely by env (see action.yml):
#   GH_TOKEN GH_REPO BRANCH LABELS RESULT TITLE BODY GITHUB_OUTPUT [GITHUB_SERVER_URL]
set -euo pipefail

server="${GITHUB_SERVER_URL:-https://github.com}"

# git with the input token. actions/checkout stores its own credential as an extraheader in
# .git/config; an empty -c value resets that list for this command only, then ours is added.
# Nothing is written to .git/config.
auth=$(printf 'x-access-token:%s' "$GH_TOKEN" | base64 | tr -d '\n')
echo "::add-mask::$auth"
git_auth() {
  git -c "http.${server}/.extraheader=" -c "http.${server}/.extraheader=AUTHORIZATION: basic ${auth}" "$@"
}

pr=$(gh pr list --head "$BRANCH" --state open --json number,isCrossRepository --jq '[.[]|select(.isCrossRepository|not)][0].number // empty')

finish_current() {
  if [ -n "$pr" ]; then
    gh pr close "$pr" --comment "Already up to date." --delete-branch
  fi
  echo "result=current" >> "$GITHUB_OUTPUT"
  echo "pr-url=" >> "$GITHUB_OUTPUT"
  exit 0
}

# Already current: no push, no PR; an open bot PR is closed.
if [ "$RESULT" = current ]; then finish_current; fi

git switch -C "$BRANCH"
git add -A -- .
# The report listed changes but the tree is unchanged (e.g. only skips): treat as current.
if git diff --cached --quiet; then finish_current; fi
git commit -m "$TITLE"
remote_sha=$(git_auth ls-remote origin "refs/heads/$BRANCH" | cut -f1)
git_auth push --force-with-lease="$BRANCH:$remote_sha" origin "HEAD:refs/heads/$BRANCH"

if [ -n "$pr" ]; then
  gh pr edit "$pr" --title "$TITLE" --body "$BODY"
  url=$(gh pr view "$pr" --json url --jq .url)
else
  label_args=()
  IFS=',' read -ra wanted <<< "$LABELS"
  for l in "${wanted[@]}"; do
    l="${l#"${l%%[![:space:]]*}"}"; l="${l%"${l##*[![:space:]]}"}"
    [ -n "$l" ] || continue
    gh label create "$l" >/dev/null 2>&1 || true   # fails when it exists; fine
    label_args+=(--label "$l")
  done
  base=$(gh repo view --json defaultBranchRef --jq .defaultBranchRef.name)
  url=$(gh pr create --base "$base" --head "$BRANCH" --title "$TITLE" --body "$BODY" ${label_args[@]+"${label_args[@]}"})
fi
echo "result=$RESULT" >> "$GITHUB_OUTPUT"
echo "pr-url=$url" >> "$GITHUB_OUTPUT"
