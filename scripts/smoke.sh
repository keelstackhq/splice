#!/usr/bin/env bash
# Smoke test for a built splice binary: version, then `splice new` against the v1 fixture.
# Usage: EXPECT_VERSION=0.2.0 scripts/smoke.sh <splice-binary>
set -euo pipefail

bin=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

if [ -n "${EXPECT_VERSION:-}" ]; then
  got=$("$bin" --version)
  case "$got" in
    *" $EXPECT_VERSION") ;;
    *) echo "smoke: version is '$got', expected '$EXPECT_VERSION'" >&2; exit 1 ;;
  esac
fi

cp -R "$root/tests/fixtures/template/v1/." "$tmp/tpl"
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null
export GIT_AUTHOR_NAME=smoke GIT_AUTHOR_EMAIL=smoke@example.com
export GIT_COMMITTER_NAME=smoke GIT_COMMITTER_EMAIL=smoke@example.com
git -c core.autocrlf=false -C "$tmp/tpl" init -q
git -c core.autocrlf=false -C "$tmp/tpl" add -A
git -c core.autocrlf=false -C "$tmp/tpl" commit -q -m v1
git -C "$tmp/tpl" tag v1.0.0

"$bin" new "$tmp/tpl" "$tmp/out" --yes --data project=smoke --json > "$tmp/stdout.json"
test -f "$tmp/out/.splice/lock.toml" || { echo "smoke: lock.toml missing" >&2; exit 1; }
echo "smoke ok: $("$bin" --version)"
