// Fixtures in ./fixtures are the stdout of `splice update --json --yes` (splice 0.2.0), run in a
// committed repo made with `splice new <tpl> <repo> --ref v1.0.0 --piece app:web --data project=demo
// --yes` from a git repo holding tests/fixtures/template/{v1,v2,v3} tagged v1.0.0, v2.0.0, v3.0.0:
//   conflict.json  edit notes.txt and .github/workflows/ci.yml (as golden_update), commit, `update --to v2.0.0`
//   drift.json     append a line to the owned .editorconfig, commit, `update --to v2.0.0`
//   updated.json   clean tree, `update --to v2.0.0`
//   current.json   commit that result, `update --to v2.0.0` again
//   error.json     clean tree, `update --to v3.0.0` (v3 has min_splice = "99.0.0")
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { summarize } from "../summary.mjs";

const load = (name) =>
  JSON.parse(readFileSync(new URL(`./fixtures/${name}`, import.meta.url), "utf8"));

test("conflict title and body", () => {
  const s = summarize(load("conflict.json"), 1, { version: "2.0.0", dirty: true });
  assert.equal(s.title, "[conflicts] chore: splice update to 2.0.0");
  assert.equal(s.result, "conflicts");
  assert.match(s.body, /\| conflict \| `[^`]+` \| shared \|/);
  assert.match(s.body, /Resolve the conflict markers, then run `splice check --json`\./);
});

test("drift is its own result", () => {
  const s = summarize(load("drift.json"), 1, { version: "1.0.0", dirty: false });
  assert.equal(s.result, "drift");
  assert.equal(s.title, "");
  assert.match(s.body, /owned by the template but was edited locally/);
});

test("clean exit 0 is current", () => {
  assert.equal(summarize(load("current.json"), 0, { dirty: false }).result, "current");
});

test("dirty exit 0 is updated, grouped by action", () => {
  const s = summarize(load("updated.json"), 0, { version: "2.0.0", dirty: true });
  assert.equal(s.result, "updated");
  assert.equal(s.title, "chore: splice update to 2.0.0");
  assert.match(s.body, /\| action \| path \| class \|/);
  assert.ok(s.body.indexOf("| create |") < s.body.indexOf("| delete |"));
});

test("exit 1 with a changed tree and no conflict rows still counts as conflicts", () => {
  const s = summarize({ ok: false, command: "update", changes: [], messages: ["x"] }, 1, {
    version: "2.0.0",
    dirty: true,
  });
  assert.equal(s.result, "conflicts");
});

test("exit 3 throws with messages", () => {
  assert.throws(() => summarize(load("error.json"), 3, {}), /template needs splice/);
});

test("exit 2 throws", () => {
  assert.throws(
    () => summarize({ ok: false, command: "update", changes: [], messages: ["dirty tree"] }, 2, {}),
    /dirty tree/,
  );
});
