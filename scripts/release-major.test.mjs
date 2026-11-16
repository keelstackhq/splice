import test from "node:test";
import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { actionMajor, leaseValue, majorTagPlan, majorTagVerify, moveMajorTag } from "./release-major.mjs";

const NEW = "0123456789abcdef0123456789abcdef01234567";
const OLD = "fedcba9876543210fedcba9876543210fedcba98";
const OBJ = "1111111111111111111111111111111111111111";
const OBJ2 = "2222222222222222222222222222222222222222";

// `git ls-remote --tags` lines: [sha, ref].
const ls = (...rows) => rows.map(([sha, ref]) => `${sha}\trefs/tags/${ref}`).join("\n") + "\n";

test("the Action major is v1 up to splice 1.x, then v<major>", () => {
  assert.equal(actionMajor("v0.5.1"), "v1");
  assert.equal(actionMajor("v1.2.0"), "v1");
  assert.equal(actionMajor("v2.0.0"), "v2");
  assert.equal(actionMajor("v1"), "");
  assert.equal(actionMajor("0.5.1"), "");
});

test("a new stable release moves a stale v1 to its commit (annotated tag peeled)", () => {
  const remote = ls([OBJ, "v0.5.0"], [OLD, "v0.5.0^{}"], [OBJ2, "v0.5.1"], [NEW, "v0.5.1^{}"], [OLD, "v1"]);
  assert.deepEqual(majorTagPlan({ tag: "v0.5.1", remoteTags: remote }), {
    major: "v1",
    commit: NEW,
    move: true,
    reason: "v1 is at fedcba9: move it to v0.5.1 (0123456)",
  });
});

test("a missing major tag is created", () => {
  const remote = ls([NEW, "v0.5.1"]);
  const plan = majorTagPlan({ tag: "v0.5.1", remoteTags: remote });
  assert.equal(plan.move, true);
  assert.equal(plan.commit, NEW);
  assert.match(plan.reason, /v1 is missing/);
});

test("v1 already on the commit (a re-run) does not move", () => {
  const remote = ls([OBJ2, "v0.5.1"], [NEW, "v0.5.1^{}"], [NEW, "v1"]);
  const plan = majorTagPlan({ tag: "v0.5.1", remoteTags: remote });
  assert.equal(plan.move, false);
  assert.equal(plan.commit, NEW);
});

test("a prerelease never moves v1", () => {
  const remote = ls([NEW, "v0.6.0-rc.1"], [OLD, "v0.5.1"], [OLD, "v1"]);
  const plan = majorTagPlan({ tag: "v0.6.0-rc.1", remoteTags: remote });
  assert.equal(plan.move, false);
  assert.match(plan.reason, /prerelease/);
});

test("re-releasing an older version never moves v1 backwards", () => {
  const remote = ls([OLD, "v0.5.0"], [NEW, "v0.5.1"], [NEW, "v1"]);
  const plan = majorTagPlan({ tag: "v0.5.0", remoteTags: remote });
  assert.equal(plan.move, false);
  assert.match(plan.reason, /v0\.5\.1 is newer/);
  // A newer prerelease does not count: the stable release still moves v1.
  const pre = ls([OLD, "v0.5.0"], [NEW, "v0.6.0-rc.1"], [NEW, "v1"]);
  assert.equal(majorTagPlan({ tag: "v0.5.0", remoteTags: pre }).move, true);
  // A newer release of another Action major does not count either.
  const two = ls([OLD, "v0.5.0"], [NEW, "v2.0.0"], [NEW, "v1"]);
  assert.equal(majorTagPlan({ tag: "v0.5.0", remoteTags: two }).move, true);
});

test("a release tag missing on the remote is an error", () => {
  assert.throws(() => majorTagPlan({ tag: "v0.5.1", remoteTags: ls([OLD, "v1"]) }), /v0\.5\.1 is not on the remote/);
  assert.throws(() => majorTagPlan({ tag: "v1", remoteTags: "" }), /not a release tag/);
});

test("the completeness check needs the major tag on the release commit", () => {
  assert.equal(majorTagVerify({ major: "v1", commit: NEW, remoteTags: ls([NEW, "v1"]) }), "");
  assert.match(majorTagVerify({ major: "v1", commit: NEW, remoteTags: "" }), /v1 is missing/);
  assert.match(majorTagVerify({ major: "v1", commit: NEW, remoteTags: ls([OLD, "v1"]) }), /v1 is at fedcba9, not 0123456/);
  // An annotated major tag counts by its peeled commit.
  assert.equal(majorTagVerify({ major: "v1", commit: NEW, remoteTags: ls([OBJ, "v1"], [NEW, "v1^{}"]) }), "");
});

test("release.yml moves the major tag only after npm, behind the release environment", () => {
  const yml = readFileSync(new URL("../.github/workflows/release.yml", import.meta.url), "utf8");
  const job = yml.split(/\n  (?=[a-z-]+:\n)/).find((j) => j.startsWith("major:"));
  assert.ok(job, "major job not found");
  assert.match(job, /needs: \[check, release, npm\]/);
  assert.match(job, /needs\.npm\.result == 'success'/);
  assert.match(job, /environment: release/);
  assert.match(job, /contents: write/);
  assert.match(job, /node scripts\/release-major\.mjs --tag "\$TAG"/);
});

test("the script moves v1 tag-only on a real remote, and fails when v1 is missing", () => {
  const dir = mkdtempSync(join(tmpdir(), "release-major-"));
  const env = {
    ...process.env,
    GIT_CONFIG_GLOBAL: join(dir, "gitconfig"),
    GIT_CONFIG_NOSYSTEM: "1",
    GIT_AUTHOR_NAME: "t",
    GIT_AUTHOR_EMAIL: "t@example.com",
    GIT_COMMITTER_NAME: "t",
    GIT_COMMITTER_EMAIL: "t@example.com",
  };
  writeFileSync(env.GIT_CONFIG_GLOBAL, "");
  const g = (cwd, ...args) => execFileSync("git", args, { cwd, env, encoding: "utf8" }).trim();
  const origin = join(dir, "origin.git");
  const work = join(dir, "work");
  g(dir, "init", "-q", "--bare", origin);
  g(dir, "init", "-q", "-b", "main", work);
  g(work, "remote", "add", "origin", origin);
  g(work, "commit", "-q", "--allow-empty", "-m", "one");
  const one = g(work, "rev-parse", "HEAD");
  g(work, "tag", "v1");
  g(work, "commit", "-q", "--allow-empty", "-m", "two");
  const two = g(work, "rev-parse", "HEAD");
  g(work, "tag", "-a", "v0.5.1", "-m", "splice 0.5.1");
  g(work, "push", "-q", "origin", "main", "refs/tags/v1", "refs/tags/v0.5.1");
  const script = fileURLToPath(new URL("./release-major.mjs", import.meta.url));
  const run = (tag) => spawnSync(process.execPath, [script, "--tag", tag], { cwd: work, env, encoding: "utf8" });

  const moved = run("v0.5.1");
  assert.equal(moved.status, 0, moved.stderr);
  assert.match(moved.stdout, new RegExp(`v1 is at ${one.slice(0, 7)}: move it to v0\\.5\\.1`));
  assert.equal(g(origin, "rev-parse", "refs/tags/v1"), two);
  // Only the tag moved: main and the release tag are untouched.
  assert.equal(g(origin, "rev-parse", "refs/heads/main"), two);
  assert.equal(g(origin, "rev-parse", "v0.5.1^{commit}"), two);

  const again = run("v0.5.1");
  assert.equal(again.status, 0, again.stderr);
  assert.match(again.stdout, /already on v0\.5\.1/);

  // A prerelease does not move v1, and the completeness check fails when v1 is gone.
  g(work, "tag", "v0.6.0-rc.1");
  g(work, "push", "-q", "origin", "refs/tags/v0.6.0-rc.1", ":refs/tags/v1");
  const missing = run("v0.6.0-rc.1");
  assert.equal(missing.status, 1);
  assert.match(missing.stderr, /release incomplete: v1 is missing on the remote/);
});

test("the lease uses the tag's own object, not its peeled commit", () => {
  assert.equal(leaseValue({ major: "v1", remoteTags: ls([OLD, "v1"]) }), OLD);
  assert.equal(leaseValue({ major: "v1", remoteTags: ls([OBJ, "v1"], [OLD, "v1^{}"]) }), OBJ);
  assert.equal(leaseValue({ major: "v1", remoteTags: "" }), "");
});

// A bare origin and a clone with v1 on commit one and annotated v0.5.1 on commit two.
function remote({ annotatedMajor = false } = {}) {
  const dir = mkdtempSync(join(tmpdir(), "release-major-"));
  const env = {
    ...process.env,
    GIT_CONFIG_GLOBAL: join(dir, "gitconfig"),
    GIT_CONFIG_NOSYSTEM: "1",
    GIT_AUTHOR_NAME: "t",
    GIT_AUTHOR_EMAIL: "t@example.com",
    GIT_COMMITTER_NAME: "t",
    GIT_COMMITTER_EMAIL: "t@example.com",
  };
  writeFileSync(env.GIT_CONFIG_GLOBAL, "");
  const g = (cwd, ...args) => execFileSync("git", args, { cwd, env, encoding: "utf8" }).trim();
  const origin = join(dir, "origin.git");
  const work = join(dir, "work");
  g(dir, "init", "-q", "--bare", origin);
  g(dir, "init", "-q", "-b", "main", work);
  g(work, "remote", "add", "origin", origin);
  g(work, "commit", "-q", "--allow-empty", "-m", "one");
  const one = g(work, "rev-parse", "HEAD");
  if (annotatedMajor) g(work, "tag", "-a", "v1", "-m", "v1");
  else g(work, "tag", "v1");
  g(work, "commit", "-q", "--allow-empty", "-m", "two");
  const two = g(work, "rev-parse", "HEAD");
  g(work, "tag", "-a", "v0.5.1", "-m", "splice 0.5.1");
  g(work, "push", "-q", "origin", "main", "refs/tags/v1", "refs/tags/v0.5.1");
  const git = (...args) => execFileSync("git", args, { cwd: work, env, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] });
  return { dir, env, g, origin, work, one, two, git };
}

test("an annotated v1 is replaced by the lightweight tag on the release commit", () => {
  const r = remote({ annotatedMajor: true });
  const lines = [];
  moveMajorTag({ tag: "v0.5.1", git: r.git, log: (l) => lines.push(l) });
  assert.match(lines[0], /move it to v0\.5\.1/);
  assert.equal(r.g(r.origin, "rev-parse", "refs/tags/v1"), r.two);
  assert.equal(r.g(r.origin, "cat-file", "-t", "refs/tags/v1"), "commit");
});

test("the lease refuses when v1 moves between ls-remote and push", () => {
  const r = remote();
  // Someone else moves v1 to a third commit right after the script read the remote.
  r.g(r.work, "commit", "-q", "--allow-empty", "-m", "three");
  const three = r.g(r.work, "rev-parse", "HEAD");
  let raced = false;
  const git = (...args) => {
    const out = r.git(...args);
    if (args[0] === "ls-remote" && !raced) {
      raced = true;
      r.g(r.work, "push", "-q", "--force", "origin", `${three}:refs/tags/v1`);
    }
    return out;
  };
  assert.throws(() => moveMajorTag({ tag: "v0.5.1", git, log: () => {} }), /stale info|rejected/);
  assert.equal(r.g(r.origin, "rev-parse", "refs/tags/v1"), three);
});
