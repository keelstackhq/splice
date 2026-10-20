import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync, writeFileSync, mkdtempSync, mkdirSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";
import { summarize, scopeDefault, command, lockKey } from "../summary.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const fx = (n) => join(here, "fixtures", n);
// Fixtures are real `splice update --json --yes` output (see fixtures/README.md).
const load = (n) => JSON.parse(readFileSync(fx(n), "utf8"));

test("conflict title and body", () => {
  const s = summarize(load("conflict.json"), 1, "2.0.0");
  assert.equal(s.title, "[conflicts] chore: splice update to 2.0.0");
  assert.equal(s.result, "conflicts");
  assert.match(s.body, /\| conflict \| `\.github\/workflows\/ci\.yml` \| shared \|/);
  assert.match(s.body, /splice check/);
  assert.match(s.body, /Conflicts/);
});

test("clean update", () => {
  const s = summarize(load("updated.json"), 0, "2.0.0");
  assert.equal(s.title, "chore: splice update to 2.0.0");
  assert.equal(s.result, "updated");
  assert.match(s.body, /\| create \| `CHANGELOG\.md` \| seed \|/);
  assert.match(s.body, /\| delete \| `old\.txt` \| owned \|/);
  assert.doesNotMatch(s.body, /Conflicts/);
});

test("changes are grouped by action in a stable order", () => {
  const { body } = summarize(load("conflict.json"), 1, "2.0.0");
  const order = ["conflict", "create", "update", "delete"].map((a) => body.indexOf(`| ${a} |`));
  assert.ok(order.every((i) => i >= 0), String(order));
  assert.deepEqual(order, [...order].sort((a, b) => a - b));
});

test("real current report is current", () => {
  const s = summarize(load("current.json"), 0, "2.0.0");
  assert.equal(s.result, "current");
});

test("no changes is current", () => {
  assert.equal(
    summarize({ ok: true, command: "update", changes: [], messages: ["up to date"] }, 0, "1.0.0").result,
    "current",
  );
});

test("exit 3 throws with messages", () => {
  assert.throws(() => summarize(load("error.json"), 3, "1.0.0"), /template needs splice/);
});

test("owned drift (exit 1, no changes) is an error, not a conflicts PR", () => {
  assert.throws(() => summarize(load("drift.json"), 1, "1.0.0"), /`apps\/web\/tsconfig\.json` is owned by the template but was edited locally/);
  assert.throws(() => summarize(load("drift.json"), 1, "1.0.0"), /owned files were edited locally, or conflict markers are left on disk/);
});

test("exit 2 throws", () => {
  assert.throws(() => summarize({ ok: false, command: "update", changes: [], messages: ["bad flag"] }, 2, "1"), /bad flag/);
});

test("pipes and backticks in paths cannot break the table", () => {
  const r = {
    ok: true,
    command: "update",
    changes: [{ path: "a|b`c.txt", action: "create", class: "seed" }],
    messages: [],
  };
  const { body } = summarize(r, 0, "1.0.0");
  assert.ok(body.includes("| create | ``a\\|b`c.txt`` | seed |"), body);
  const row = body.split("\n").find((l) => l.includes("a\\|b"));
  assert.equal(row.replace(/\\\|/g, "").split("|").length, 5); // leading, 3 cells, trailing
});

test("a path that starts with a backtick gets padded fences", () => {
  const r = { ok: true, command: "update", changes: [{ path: "`x", action: "create", class: "seed" }], messages: [] };
  assert.ok(summarize(r, 0, "1").body.includes("| `` `x `` |"), summarize(r, 0, "1").body);
});

test("@ in messages is neutralised so nobody is mentioned", () => {
  const r = { ok: true, command: "update", changes: [{ path: "x", action: "update", class: "owned" }], messages: ["cc @octocat and @org/team"] };
  const { body } = summarize(r, 0, "1.0.0");
  assert.ok(!/@(?!\u200b)/.test(body), body);
  assert.ok(body.includes("@\u200boctocat"));
});

test("messages are listed in the body", () => {
  const r = { ok: true, command: "update", changes: [{ path: "x", action: "update", class: "owned" }], messages: ["note one"] };
  assert.match(summarize(r, 0, "1.0.0").body, /- note one/);
});

function cli(reportName, code, extra = {}) {
  const dir = mkdtempSync(join(tmpdir(), "sum-"));
  const out = join(dir, "out");
  writeFileSync(out, "");
  const report = reportName === null ? join(dir, "missing.json") : fx(reportName);
  const r = spawnSync(process.execPath, [join(here, "..", "summary.mjs"), report, String(code), "2.0.0"], {
    env: { ...process.env, GITHUB_OUTPUT: out, ...extra },
    encoding: "utf8",
  });
  return { ...r, out: readFileSync(out, "utf8") };
}

test("cli writes result, title and a heredoc body to GITHUB_OUTPUT", () => {
  const r = cli("conflict.json", 1);
  assert.equal(r.status, 0, r.stderr);
  assert.match(r.out, /^result=conflicts$/m);
  assert.match(r.out, /^title=\[conflicts\] chore: splice update to 2\.0\.0$/m);
  const m = r.out.match(/^body<<(\S+)\n([\s\S]*)\n\1\n/m);
  assert.ok(m, r.out);
  assert.match(m[2], /\| conflict \|/);
  assert.ok(m[1].length >= 16);
});

test("cli exits 1 with splice's messages on exit 3", () => {
  const r = cli("error.json", 3);
  assert.equal(r.status, 1);
  assert.match(r.stderr, /template needs splice/);
});

test("cli exits 1 on owned drift", () => {
  const r = cli("drift.json", 1);
  assert.equal(r.status, 1);
  assert.match(r.stderr, /owned by the template but was edited locally/);
});

test("cli exits 1 when splice printed no JSON", () => {
  const r = cli(null, 3);
  assert.equal(r.status, 1);
  assert.match(r.stderr, /no JSON/);
});

test("cli current", () => {
  const r = cli("current.json", 0);
  assert.equal(r.status, 0, r.stderr);
  assert.match(r.out, /^result=current$/m);
});

test("scope defaults to all when a channel lock exists, else main", () => {
  assert.equal(scopeDefault({ channels: ["rig.toml"] }), "all");
  assert.equal(scopeDefault({ channels: [] }), "main");
  assert.equal(scopeDefault({ channels: [".rig.toml.tmp123", "notes.txt"] }), "main");
});

test("command per scope", () => {
  assert.deepEqual(command("all"), ["update", "--all", "--json", "--yes"]);
  assert.deepEqual(command("main"), ["update", "--json", "--yes"]);
  assert.deepEqual(command("main", "2.0.0"), ["update", "--json", "--yes", "--to", "2.0.0"]);
  assert.throws(() => command("all", "2.0.0"), /scope: main/);
  assert.throws(() => command("everything"), /main or all/);
});

test("update --all rows are grouped under a heading per source", () => {
  const owners = {
    files: new Map([[".claude/agents/explorer.md", "rig"], [".claude/agents/reviewer.md", "rig"], [".claude/skills/verify/SKILL.md", "rig"]]),
    sections: new Set(["rig"]),
  };
  const { body, result } = summarize(load("all.json"), 0, "2.0.0", owners);
  assert.equal(result, "updated");
  const main = body.indexOf("### main");
  const rig = body.indexOf("### rig");
  assert.ok(main >= 0 && rig > main, body);
  const [mainPart, rigPart] = [body.slice(main, rig), body.slice(rig)];
  assert.match(mainPart, /`\.editorconfig`/);
  assert.match(mainPart, /`old\.txt`/);
  assert.doesNotMatch(mainPart, /AGENTS\.md#rig/);
  assert.match(rigPart, /\| update \| `AGENTS\.md#rig` \| owned \|/);
  assert.match(rigPart, /\| delete \| `\.claude\/agents\/explorer\.md` \| owned \|/);
  assert.match(rigPart, /`\.claude\/skills\/verify\/SKILL\.md`/);
  assert.doesNotMatch(rigPart, /editorconfig/);
  assert.doesNotMatch(body, /source=/);
});

test("scope main output keeps the flat table", () => {
  const { body } = summarize(load("updated.json"), 0, "2.0.0");
  assert.doesNotMatch(body, /### /);
});

test("cli attributes rows from the new locks and HEAD's (deletions)", () => {
  const dir = mkdtempSync(join(tmpdir(), "own-"));
  const sh = (...a) => spawnSync("git", ["-C", dir, "-c", "user.name=t", "-c", "user.email=t@e", ...a], { encoding: "utf8" });
  const lock = (files) => `format = 1\n[template]\nsource = "x"\nversion = "2.0.0"\ncommit = "c"\n\n[files]\n${files.map((f) => `"${f}" = { class = "owned", sha256 = "s" }\n`).join("")}`;
  mkdirSync(join(dir, ".splice", "channels"), { recursive: true });
  const write = (rel, text) => writeFileSync(join(dir, rel), text);
  write(".splice/lock.toml", lock([".editorconfig", "old.txt"]));
  write(".splice/channels/rig.toml", lock([".claude/agents/explorer.md", ".claude/agents/reviewer.md"]));
  sh("init", "-q", "-b", "main"); sh("add", "-A"); sh("commit", "-q", "-m", "old");
  write(".splice/lock.toml", lock([".editorconfig"]));
  write(".splice/channels/rig.toml", lock([".claude/agents/reviewer.md"]));
  const out = join(dir, "out");
  writeFileSync(out, "");
  const r = spawnSync(process.execPath, [join(here, "..", "summary.mjs"), fx("all.json"), "0", "2.0.0"], {
    cwd: dir,
    env: { ...process.env, GITHUB_OUTPUT: out },
    encoding: "utf8",
  });
  assert.equal(r.status, 0, r.stderr);
  const body = readFileSync(out, "utf8");
  const rig = body.slice(body.indexOf("### rig"));
  assert.match(rig, /explorer\.md/);
  assert.match(rig, /reviewer\.md/);
  assert.doesNotMatch(body.slice(body.indexOf("### main"), body.indexOf("### rig")), /explorer|reviewer/);
});

test("cli args prints the update command, one argument per line", () => {
  const run = (...a) => spawnSync(process.execPath, [join(here, "..", "summary.mjs"), "args", ...a], { encoding: "utf8", cwd: mkdtempSync(join(tmpdir(), "args-")) });
  assert.equal(run("all", "").stdout, "update\n--all\n--json\n--yes\n");
  assert.equal(run("", "").stdout, "update\n--json\n--yes\n"); // no channel dir: default main
  assert.equal(run("main", "1.2.3").stdout, "update\n--json\n--yes\n--to\n1.2.3\n");
  const bad = run("all", "1.2.3");
  assert.equal(bad.status, 1);
  assert.match(bad.stderr, /scope: main/);
});

test("lockKey reads bare, basic-quoted (escapes) and literal-quoted keys", () => {
  assert.equal(lockKey('Makefile = { class = "owned", sha256 = "s" }'), "Makefile");
  assert.equal(lockKey('LICENSE-MIT = { class = "seed" }'), "LICENSE-MIT");
  assert.equal(lockKey('"docs/a b.md" = { class = "owned" }'), "docs/a b.md");
  assert.equal(lockKey('"we\\"ird\\\\x" = 1'), 'we"ird\\x');
  assert.equal(lockKey("'dir/it\"s.md' = { class = \"owned\" }"), 'dir/it"s.md');
  assert.equal(lockKey('# comment = 1'), null);
  assert.equal(lockKey("[files]"), null);
});

test("cli attributes bare-key channel lock rows and names channel versions in the title", () => {
  const dir = mkdtempSync(join(tmpdir(), "bare-"));
  mkdirSync(join(dir, ".splice", "channels"), { recursive: true });
  const lock = (v, files) => `format = 1\n[template]\nsource = "x"\nversion = "${v}"\ncommit = "c"\n\n[files]\n${files}`;
  writeFileSync(join(dir, ".splice/lock.toml"), lock("2.0.0", 'README.md = { class = "owned", sha256 = "s" }\n'));
  writeFileSync(join(dir, ".splice/channels/rig.toml"), lock("1.1.0", 'Makefile = { class = "owned", sha256 = "s" }\n\'dir/x.md\' = { class = "owned", sha256 = "s" }\n'));
  spawnSync("git", ["-C", dir, "init", "-q"]);
  const out = join(dir, "out");
  writeFileSync(out, "");
  const r = spawnSync(process.execPath, [join(here, "..", "summary.mjs"), fx("all.json"), "0", "2.0.0"], {
    cwd: dir, env: { ...process.env, GITHUB_OUTPUT: out }, encoding: "utf8",
  });
  assert.equal(r.status, 0, r.stderr);
  const body = readFileSync(out, "utf8");
  assert.match(body, /^title=chore: splice update to 2\.0\.0, rig 1\.1\.0$/m);
  assert.ok(body.includes("### rig"), body);
});

test("command rejects a `to` with a newline or a leading dash", () => {
  assert.throws(() => command("main", "1.0\n--force"), /to/);
  assert.throws(() => command("main", "--force"), /to/);
  assert.deepEqual(command("main", "v1.2.3"), ["update", "--json", "--yes", "--to", "v1.2.3"]);
});
