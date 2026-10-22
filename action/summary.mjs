// Decide the update-bot result from splice's JSON report, its exit code and the tree state.
// A report with `source=` messages (`update --all`) gets one table per source: `main`, then each
// channel. Rows do not name their source; a `HOST#NAME` path is channel NAME's section, any other
// path is looked up in `owners` (Map path -> source name, built from the channel locks), else main.
// CLI: node summary.mjs <report.json> <exit-code>   (run inside the repo, after the update)
//   writes result, title and body-file to $GITHUB_OUTPUT; exits 1 with splice's messages on exit 2/3.
import { execFileSync } from "node:child_process";
import { appendFileSync, existsSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { SEMVER } from "./args.mjs";

export function summarize(report, exitCode, { version, dirty, owners = new Map() }) {
  const messages = report.messages ?? [];
  if (exitCode !== 0 && exitCode !== 1) {
    throw new Error(messages.join("\n") || `splice exited ${exitCode}`);
  }
  if (exitCode === 1 && !dirty) {
    return { title: "", body: messages.join("\n") + "\n", result: "drift" };
  }
  if (exitCode === 0 && !dirty) {
    return { title: "", body: messages.join("\n") + "\n", result: "current" };
  }
  const result = exitCode === 0 ? "updated" : "conflicts";
  const base = version ? `chore: splice update to ${version}` : "chore: splice update";
  const title = result === "conflicts" ? `[conflicts] ${base}` : base;
  const parts = [];
  const rows = report.changes ?? [];
  const sources = messages.filter((m) => m.startsWith("source=")).map((m) => m.slice(7));
  const rest = messages.filter((m) => !m.startsWith("source="));
  if (sources.length === 0) {
    if (rows.length > 0) parts.push(table(rows).join("\n"));
  } else {
    const nameOf = (c) => {
      const channel = /#([^#/]+)$/.exec(c.path)?.[1];
      return channel ?? owners.get(c.path) ?? "main";
    };
    const names = sources.map((m) => m.replace(/^channel:/, ""));
    for (const c of rows) if (!names.includes(nameOf(c))) names.push(nameOf(c));
    for (const name of names) {
      const own = rows.filter((c) => nameOf(c) === name);
      parts.push(`### ${name}\n\n${own.length > 0 ? table(own).join("\n") : "No file changes."}`);
    }
  }
  if (rest.length > 0) parts.push(rest.join("\n"));
  if (result === "conflicts") {
    parts.push("Resolve the conflict markers, then run `splice check --json`.");
  }
  return { title, body: parts.join("\n\n") + "\n", result };
}

function table(rows) {
  const order = [];
  for (const c of rows) if (!order.includes(c.action)) order.push(c.action);
  const lines = ["| action | path | class |", "| --- | --- | --- |"];
  for (const action of order) {
    for (const c of rows.filter((r) => r.action === action)) {
      lines.push(`| ${action} | \`${c.path}\` | ${c.class} |`);
    }
  }
  return lines;
}

// Paths of the `[files]` table of a lock. toml_edit writes bare, basic-quoted and literal-quoted keys.
const ESCAPES = { b: "\b", t: "\t", n: "\n", f: "\f", r: "\r", '"': '"', "\\": "\\" };
const unescape = (text) =>
  text.replace(/\\(?:([btnfr"\\])|u([0-9a-fA-F]{4})|U([0-9a-fA-F]{8}))/g, (_, c, u4, u8) =>
    c ? ESCAPES[c] : String.fromCodePoint(parseInt(u4 ?? u8, 16)),
  );

export function lockFiles(lock) {
  const section = /^\[files\]\s*$([\s\S]*?)(?=^\[|(?![\s\S]))/m.exec(lock)?.[1] ?? "";
  const key = /^(?:"((?:[^"\\]|\\.)*)"|'([^']*)'|([A-Za-z0-9_-]+))\s*=/gm;
  return [...section.matchAll(key)].map((m) => (m[1] !== undefined ? unescape(m[1]) : (m[2] ?? m[3])));
}

function channelOwners() {
  const owners = new Map();
  let files = [];
  try {
    files = readdirSync(".splice/channels").filter((f) => f.endsWith(".toml"));
  } catch {}
  for (const file of files) {
    const rel = `.splice/channels/${file}`;
    const name = file.slice(0, -5);
    const texts = [readFileSync(rel, "utf8")];
    try {
      texts.push(execFileSync("git", ["show", `HEAD:${rel}`], { encoding: "utf8", stdio: "pipe" }));
    } catch {}
    for (const text of texts) for (const path of lockFiles(text)) owners.set(path, name);
  }
  return owners;
}

export function parseReport(text, exitCode) {
  if (text.trim() === "") {
    throw new Error(`splice exited ${exitCode} without a JSON report; see its stderr above`);
  }
  return JSON.parse(text);
}

export function parseLockVersion(lock) {
  const section = lock.split(/^\[template\]\s*$/m)[1] ?? "";
  const version = /^version\s*=\s*"([^"]*)"/m.exec(section)?.[1];
  if (version === undefined || !SEMVER.test(version)) {
    throw new Error("`[template] version` in .splice/lock.toml is missing or not a semver");
  }
  return version;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const [reportPath, code] = process.argv.slice(2);
  try {
    const exitCode = Number(code);
    const report = parseReport(readFileSync(reportPath, "utf8"), exitCode);
    const dirty = execFileSync("git", ["status", "--porcelain"], { encoding: "utf8" }).trim() !== "";
    // A channel-only repo has no main lock; its title carries no version. Do not key this on
    // `source=` messages: an up-to-date `update --all` prints none.
    const version =
      exitCode !== 0 && exitCode !== 1
        ? "unknown"
        : existsSync(".splice/lock.toml")
          ? parseLockVersion(readFileSync(".splice/lock.toml", "utf8"))
          : null;
    const s = summarize(report, exitCode, { version, dirty, owners: channelOwners() });
    const bodyFile = join(process.env.RUNNER_TEMP || tmpdir(), "splice-pr-body.md");
    writeFileSync(bodyFile, s.body);
    appendFileSync(
      process.env.GITHUB_OUTPUT,
      `result=${s.result}\ntitle=${s.title}\nbody-file=${bodyFile}\n`,
    );
  } catch (err) {
    process.stderr.write(`splice update failed:\n${err.message}\n`);
    process.exit(1);
  }
}
