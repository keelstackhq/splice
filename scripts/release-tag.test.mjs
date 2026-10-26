import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { releaseTag } from "./release-tag.mjs";

const SHA = "0123456789abcdef0123456789abcdef01234567";

test("an untagged version gets its tag", () => {
  assert.equal(releaseTag({ version: "0.2.1", remoteTags: "" }), "v0.2.1");
  assert.equal(releaseTag({ version: "0.3.0-rc.1", remoteTags: "" }), "v0.3.0-rc.1");
});

test("an existing tag (plain or peeled) means no release", () => {
  assert.equal(releaseTag({ version: "0.2.1", remoteTags: `${SHA}\trefs/tags/v0.2.1\n` }), "");
  assert.equal(
    releaseTag({ version: "0.2.1", remoteTags: `${SHA}\trefs/tags/v0.2.1\n${SHA}\trefs/tags/v0.2.1^{}\n` }),
    "",
  );
});

test("other tags do not count, also not as prefixes", () => {
  const remoteTags = [`${SHA}\trefs/tags/v0.2.10`, `${SHA}\trefs/tags/v0.2.1-rc.1`, `${SHA}\trefs/tags/v1`].join("\n");
  assert.equal(releaseTag({ version: "0.2.1", remoteTags }), "v0.2.1");
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
