import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, writeFileSync, readFileSync, chmodSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

const script = join(dirname(fileURLToPath(import.meta.url)), "..", "publish.sh");

// Runs publish.sh with stub `gh` and `git` on PATH that log their argv, one line per call.
function run(env, { openPr = "", staged = true, crossPr = "" } = {}) {
  const dir = mkdtempSync(join(tmpdir(), "pub-"));
  const log = join(dir, "log");
  const out = join(dir, "out");
  writeFileSync(log, "");
  writeFileSync(out, "");
  const stub = (name, body) => {
    const p = join(dir, name);
    writeFileSync(p, `#!/bin/bash\necho "${name} $*" >> "$STUB_LOG"\n${body}\n`);
    chmodSync(p, 0o755);
  };
  stub(
    "git",
    `case "$1 $2" in
  "diff --cached") [ "$STUB_STAGED" = 1 ] && exit 1 || exit 0;;
  "ls-remote origin") printf 'abc123\\trefs/heads/x\\n';;
esac
case " $* " in *" ls-remote "*) printf 'abc123\\trefs/heads/x\\n';; esac`,
  );
  stub(
    "gh",
    `case "$1 $2" in
  "pr list") case " $* " in *isCrossRepository*) echo "$STUB_PR";; *) echo "$STUB_ALLPR";; esac;;
  "pr create") echo https://github.com/o/r/pull/9;;
  "pr view") echo https://github.com/o/r/pull/5;;
  "repo view") echo trunk;;
esac`,
  );
  const r = spawnSync("bash", [script], {
    env: {
      PATH: `${dir}:${process.env.PATH}`,
      STUB_LOG: log,
      STUB_PR: openPr,
      STUB_ALLPR: crossPr,
      STUB_STAGED: staged ? "1" : "0",
      GITHUB_OUTPUT: out,
      GH_TOKEN: "tok",
      GH_REPO: "o/r",
      BRANCH: "splice/update",
      LABELS: "splice, bot",
      RESULT: "updated",
      TITLE: "chore: splice update to 2.0.0",
      BODY: "body text",
      ...env,
    },
    encoding: "utf8",
  });
  return { ...r, calls: readFileSync(log, "utf8").trim().split("\n").filter(Boolean), outputs: readFileSync(out, "utf8") };
}

const has = (calls, re) => calls.some((c) => re.test(c));

test("current + open PR: close with comment, delete branch, no push", () => {
  const r = run({ RESULT: "current", TITLE: "", BODY: "" }, { openPr: "5" });
  assert.equal(r.status, 0, r.stderr);
  assert.ok(has(r.calls, /^gh pr close 5 --comment Already up to date\. --delete-branch$/), r.calls.join("\n"));
  assert.ok(!has(r.calls, /push/));
  assert.ok(!has(r.calls, /pr (create|edit)/));
  assert.match(r.outputs, /^result=current$/m);
  assert.match(r.outputs, /^pr-url=$/m);
});

test("current + no PR: no push, no PR calls", () => {
  const r = run({ RESULT: "current", TITLE: "", BODY: "" });
  assert.equal(r.status, 0, r.stderr);
  assert.ok(!has(r.calls, /push|commit|switch/), r.calls.join("\n"));
  assert.ok(!has(r.calls, /^gh pr (close|create|edit)/));
  assert.match(r.outputs, /^result=current$/m);
});

test("updated + no PR: push then gh pr create with base and labels", () => {
  const r = run({});
  assert.equal(r.status, 0, r.stderr);
  const push = r.calls.findIndex((c) => /^git .* push /.test(c));
  const create = r.calls.findIndex((c) => /^gh pr create /.test(c));
  assert.ok(push >= 0 && create > push, r.calls.join("\n"));
  assert.match(r.calls[push], /--force-with-lease=splice\/update:abc123 origin HEAD:refs\/heads\/splice\/update/);
  assert.match(r.calls[create], /--base trunk --head splice\/update --title chore: splice update to 2\.0\.0 --body body text --label splice --label bot/);
  assert.match(r.outputs, /^result=updated$/m);
  assert.match(r.outputs, /^pr-url=https:\/\/github\.com\/o\/r\/pull\/9$/m);
});

test("updated + open PR: push and gh pr edit, no create", () => {
  const r = run({}, { openPr: "5" });
  assert.ok(has(r.calls, /^gh pr edit 5 --title chore: splice update to 2\.0\.0 --body body text$/), r.calls.join("\n"));
  assert.ok(!has(r.calls, /^gh pr create/));
  assert.ok(has(r.calls, / push /));
  assert.match(r.outputs, /^pr-url=https:\/\/github\.com\/o\/r\/pull\/5$/m);
});

test("conflicts: title keeps the [conflicts] prefix", () => {
  const r = run({ RESULT: "conflicts", TITLE: "[conflicts] chore: splice update to 2.0.0" });
  assert.ok(has(r.calls, /^gh pr create .*--title \[conflicts\] chore: splice update to 2\.0\.0 /), r.calls.join("\n"));
  assert.ok(has(r.calls, /^git commit -m \[conflicts\] chore/));
  assert.match(r.outputs, /^result=conflicts$/m);
});

test("push authenticates with the input token, not the checkout credential", () => {
  const r = run({ GH_TOKEN: "s3cret" });
  const b64 = Buffer.from("x-access-token:s3cret").toString("base64");
  const push = r.calls.find((c) => / push /.test(c));
  assert.ok(push.includes(`http.https://github.com/.extraheader=AUTHORIZATION: basic ${b64}`), push);
  assert.ok(push.includes("-c http.https://github.com/.extraheader= "), "existing header must be reset first: " + push);
  assert.ok(!push.includes("s3cret"));
  assert.ok(r.calls.findIndex((c) => / ls-remote /.test(c)) >= 0);
  assert.ok(!has(r.calls, /config .*extraheader/), "token must not be written to .git/config");
});

test("nothing staged after a reported change is treated as current", () => {
  const r = run({}, { staged: false, openPr: "5" });
  assert.ok(has(r.calls, /^gh pr close 5 /), r.calls.join("\n"));
  assert.ok(!has(r.calls, / push /));
  assert.match(r.outputs, /^result=current$/m);
});

test("only a cross-repo (fork) PR on the branch: never edited or closed", () => {
  // The stub honours the --jq filter by returning only same-repo numbers; the script must ask for it.
  const upd = run({}, { crossPr: "7" });
  assert.ok(has(upd.calls, /^gh pr list .*isCrossRepository/), upd.calls.join("\n"));
  assert.ok(!has(upd.calls, /pr (edit|close) 7/));
  assert.ok(has(upd.calls, /^gh pr create /));
  const cur = run({ RESULT: "current", TITLE: "", BODY: "" }, { crossPr: "7" });
  assert.ok(!has(cur.calls, /pr (edit|close)/));
  assert.match(cur.outputs, /^result=current$/m);
});
