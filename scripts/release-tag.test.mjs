import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { releaseTag } from "./release-tag.mjs";

const SHA = "0123456789abcdef0123456789abcdef01234567";

const OTHER = "fedcba9876543210fedcba9876543210fedcba98";
const TAGOBJ = "1111111111111111111111111111111111111111";

test("an untagged version is tagged and released", () => {
  assert.deepEqual(releaseTag({ version: "0.2.1", remoteTags: "", sha: SHA }), { tag: "v0.2.1", create: true });
  assert.deepEqual(releaseTag({ version: "0.3.0-rc.1", remoteTags: "", sha: SHA }), {
    tag: "v0.3.0-rc.1",
    create: true,
  });
});

test("a tag on another commit releases nothing", () => {
  const plain = `${OTHER}\trefs/tags/v0.2.1\n`;
  assert.deepEqual(releaseTag({ version: "0.2.1", remoteTags: plain, sha: SHA }), { tag: "", create: false });
  // Annotated: the peeled commit counts, not the tag object.
  const annotated = `${TAGOBJ}\trefs/tags/v0.2.1\n${OTHER}\trefs/tags/v0.2.1^{}\n`;
  assert.deepEqual(releaseTag({ version: "0.2.1", remoteTags: annotated, sha: SHA }), { tag: "", create: false });
});

test("a tag on the run's own commit is released again without creating it (re-run)", () => {
  const annotated = `${TAGOBJ}\trefs/tags/v0.2.1\n${SHA}\trefs/tags/v0.2.1^{}\n`;
  assert.deepEqual(releaseTag({ version: "0.2.1", remoteTags: annotated, sha: SHA }), {
    tag: "v0.2.1",
    create: false,
  });
  // The annotated tag object's own sha never matches the commit.
  const objOnly = `${SHA}\trefs/tags/v0.2.1\n${OTHER}\trefs/tags/v0.2.1^{}\n`;
  assert.deepEqual(releaseTag({ version: "0.2.1", remoteTags: objOnly, sha: SHA }), { tag: "", create: false });
});

test("other tags do not count, also not as prefixes", () => {
  const remoteTags = [`${SHA}\trefs/tags/v0.2.10`, `${SHA}\trefs/tags/v0.2.1-rc.1`, `${SHA}\trefs/tags/v1`].join("\n");
  assert.deepEqual(releaseTag({ version: "0.2.1", remoteTags, sha: SHA }), { tag: "v0.2.1", create: true });
});

const yml = readFileSync(new URL("../.github/workflows/release.yml", import.meta.url), "utf8");
const job = (name) => {
  const m = yml.match(new RegExp(`^  ${name}:\\n([\\s\\S]*?)(?=^  [a-z-]+:\\n|(?![\\s\\S]))`, "m"));
  assert.ok(m, `job ${name} not found`);
  return m[1];
};

test("release.yml runs on pushes to main and tags only on keelstackhq/splice main pushes", () => {
  assert.match(yml, /^on:\n  push:\n    branches:\n      - main\n    tags:\n/m);
  const tag = job("tag");
  assert.match(tag, /github\.repository == 'keelstackhq\/splice'/);
  assert.match(tag, /github\.ref == 'refs\/heads\/main'/);
  assert.match(tag, /contents: write/);
  assert.match(tag, /node scripts\/release-tag\.mjs/);
  assert.match(tag, /if: steps\.decide\.outputs\.create == 'true'/);
  // A lost push race must not fall back to the planned tag.
  assert.match(tag, /tag: \$\{\{ steps\.decide\.outputs\.create == 'true' && steps\.tag\.outputs\.tag \|\|/);
});

test("release.yml publishes only with a computed tag, behind the release environment", () => {
  for (const name of ["release", "npm"]) {
    const j = job(name);
    assert.match(j, /environment: release/, name);
    assert.match(j, /needs\.check\.outputs\.publish == 'true'/, name);
  }
  // Every job but `tag` and `check` takes the tag from check's output, never from the event.
  for (const name of ["build", "release", "npm"]) {
    assert.doesNotMatch(job(name), /github\.ref_name|github\.event_name/, name);
  }
});

test("release.yml builds the commit of the run and never cancels a release", () => {
  for (const name of ["check", "build", "npm"]) {
    assert.match(job(name), /ref: \$\{\{ github\.sha \}\}/, name);
  }
  assert.doesNotMatch(yml, /cancel-in-progress: true/);
  for (const name of ["release", "npm"]) {
    assert.match(job(name), /group: release-tag-\$\{\{ needs\.check\.outputs\.tag \}\}/, name);
  }
});
