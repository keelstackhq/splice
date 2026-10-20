// Fixtures in ./fixtures are the stdout of `splice update --json --yes` (splice 0.2.0), run in a
// committed repo made with `splice new <tpl> <repo> --ref v1.0.0 --piece app:web --data project=demo
// --yes` from a git repo holding tests/fixtures/template/{v1,v2,v3} tagged v1.0.0, v2.0.0, v3.0.0:
//   conflict.json  edit notes.txt and .github/workflows/ci.yml (as golden_update), commit, `update --to v2.0.0`
//   drift.json     append a line to the owned .editorconfig, commit, `update --to v2.0.0`
//   updated.json   clean tree, `update --to v2.0.0`
//   current.json   commit that result, `update --to v2.0.0` again
//   all.json       two sources: main template (app:web) and channel rig; `update --all` from v1.0.0 to v2.0.0
//   error.json     clean tree, `update --to v3.0.0` (v3 has min_splice = "99.0.0")
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { lockFiles, parseLockVersion, parseReport, summarize } from "../summary.mjs";

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

test("no JSON on stdout names the exit code", () => {
  assert.throws(
    () => parseReport("  \n", 3),
    /splice exited 3 without a JSON report; see its stderr above/,
  );
  assert.deepEqual(parseReport('{"ok":true}', 0), { ok: true });
});

test("lock version must be semver, so no newline reaches GITHUB_OUTPUT", () => {
  assert.equal(parseLockVersion('[template]\nversion = "2.0.0"\n'), "2.0.0");
  assert.throws(() => parseLockVersion('[template]\nversion = "2.0.0\\nresult=evil"\n'), /lock/);
  assert.throws(() => parseLockVersion("[vars]\n"), /lock/);
});

test("update --all groups rows by source", () => {
  const lock = '[files]\n".claude/agents/reviewer.md" = { class = "owned", sha256 = "a" }\n';
  const owners = new Map(lockFiles(lock).map((path) => [path, "rig"]));
  const s = summarize(load("all.json"), 0, { version: "2.0.0", dirty: true, owners });
  assert.equal(s.title, "chore: splice update to 2.0.0");
  const main = s.body.indexOf("### main");
  const rig = s.body.indexOf("### rig");
  assert.ok(main >= 0 && rig > main, s.body);
  const at = (needle) => s.body.indexOf(needle);
  assert.ok(at("`.claude/agents/reviewer.md`") > rig);
  assert.ok(at("`AGENTS.md#rig`") > rig);
  assert.ok(at("`package.json`") > main && at("`package.json`") < rig);
  assert.ok(at("`CHANGELOG.md`") < rig);
});

test("update --all title without a main lock", () => {
  const report = { ok: true, command: "update", changes: [], messages: ["source=channel:rig"] };
  const s = summarize(report, 0, { version: null, dirty: true });
  assert.equal(s.title, "chore: splice update");
  assert.match(s.body, /### rig/);
  assert.doesNotMatch(s.body, /### main/);
});

test("lockFiles reads the [files] table only", () => {
  const text = '[files]\n"a.txt" = { class = "owned" }\n"b#x" = { class = "shared" }\n\n[sections]\n"H.md" = { created = true }\n';
  assert.deepEqual(lockFiles(text), ["a.txt", "b#x"]);
});

// rig-keys.toml is the channel lock written by splice for a channel whose files are named
// `Makefile` (bare key), `a b.txt` (basic string), `we"ird.txt` (literal string) and `constructor`;
// all-keys.json is `update --all` from v1.0.0 to v2.0.0 for it.
test("channel rows are found under every lock key form", () => {
  const lock = readFileSync(new URL("./fixtures/rig-keys.toml", import.meta.url), "utf8");
  const files = lockFiles(lock);
  for (const p of ["Makefile", "a b.txt", 'we"ird.txt', "constructor"]) assert.ok(files.includes(p), p);
  const owners = new Map(files.map((path) => [path, "rig"]));
  const s = summarize(load("all-keys.json"), 0, { version: "2.0.0", dirty: true, owners });
  const rig = s.body.indexOf("### rig");
  assert.ok(rig > s.body.indexOf("### main"));
  for (const p of ["Makefile", "a b.txt", 'we"ird.txt', "constructor"]) {
    assert.ok(s.body.indexOf(`\`${p}\``) > rig, p);
  }
  assert.ok(s.body.indexOf("`package.json`") < rig);
});

test("lockFiles unescapes basic strings and reads literal strings", () => {
  const text = [
    "[files]",
    String.raw`"q\"b\\s\u00e9" = { class = "owned" }`,
    String.raw`'l\"x' = { class = "owned" }`,
    "bare-1_x = { class = \"owned\" }",
  ].join("\n");
  assert.deepEqual(lockFiles(text), ['q"b\\sé', String.raw`l\"x`, "bare-1_x"]);
});

test("a path named like an Object property is main without an owner", () => {
  const report = {
    ok: true,
    command: "update",
    changes: [{ path: "constructor", action: "update", class: "owned" }],
    messages: ["source=main", "source=channel:rig"],
  };
  const s = summarize(report, 0, { version: "2.0.0", dirty: true, owners: new Map() });
  assert.ok(s.body.indexOf("`constructor`") < s.body.indexOf("### rig"), s.body);
});

test("only a channel drifting is drift", () => {
  const s = summarize(load("drift-channel.json"), 1, { version: "1.0.0", dirty: false });
  assert.equal(s.result, "drift");
  assert.match(s.body, /owned by channel rig but was edited locally/);
});
