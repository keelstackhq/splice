// Summarise `splice update --json` output for the update-bot PR. Node built-ins only.
//   node summary.mjs <report.json> <splice-exit-code> <template-version>
//   node summary.mjs args <main|all|""> <to>   one `splice update` argument per line
// Writes `result`, `title` and `body` to $GITHUB_OUTPUT; exits 1 (messages on stderr)
// when the update failed outright (template error, usage error, owned drift).
import { readFileSync, appendFileSync, readdirSync, existsSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { randomBytes } from "node:crypto";
import { fileURLToPath } from "node:url";

const ORDER = ["conflict", "create", "update", "merge", "delete", "skip"];

// Inline code span safe inside a table cell: `|` escaped, fence longer than any backtick run.
const cell = (p) => {
  const t = String(p).replaceAll("|", "\\|");
  const longest = Math.max(0, ...(t.match(/`+/g) ?? []).map((r) => r.length));
  const fence = "`".repeat(longest + 1);
  const pad = t.startsWith("`") || t.endsWith("`") ? " " : "";
  return `${fence}${pad}${t}${pad}${fence}`;
};
// No pings from messages (paths in code spans cannot ping).
const defang = (m) => String(m).replaceAll("@", "@\u200b");

// Channels spec §3 (bot): `scope` defaults to `all` when `.splice/channels/` holds a lock.
export function scopeDefault({ channels }) {
  return channels.some((n) => n.endsWith(".toml") && !n.startsWith(".")) ? "all" : "main";
}

// The `splice update` arguments for a scope. `--all` conflicts with `--to` (splice exits 2), so
// that pairing is refused here with the way out.
export function command(scope, to = "") {
  if (/[\r\n]/.test(to) || to.startsWith("-")) throw new Error("input `to` must be a version, not a flag or multi-line text");
  if (scope === "main") return ["update", "--json", "--yes", ...(to ? ["--to", to] : [])];
  if (scope === "all") {
    if (to) throw new Error("input `to` needs `scope: main`: `splice update --all` cannot take --to");
    return ["update", "--all", "--json", "--yes"];
  }
  throw new Error(`input \`scope\` must be main or all, not '${scope}'`);
}

// Key of a `[files]`/`[sections]` line in a lock: toml_edit writes bare keys when it can
// (`Makefile = …`), else basic `"…"` (escapes) or literal `'…'` ones. null: not a key line.
export function lockKey(line) {
  const m = line.match(/^(?:([A-Za-z0-9_-]+)|"((?:[^"\\]|\\.)*)"|'([^']*)')\s*=/);
  if (!m) return null;
  if (m[1] !== undefined) return m[1];
  if (m[3] !== undefined) return m[3];
  try {
    return JSON.parse(`"${m[2]}"`);
  } catch {
    return m[2];
  }
}

// Source of a changed path: section rows are `HOST#CHANNEL`; files come from the lock tables
// (`owners`: path -> source name), anything unknown belongs to the main template.
const sourceOf = (path, owners) => {
  const i = path.lastIndexOf("#");
  if (i > 0 && owners.sections?.has(path.slice(i + 1))) return path.slice(i + 1);
  return owners.files?.get(path) ?? "main";
};

const TAG = /^source=(main|channel:.+)$/;

export function summarize(report, exitCode, version, owners = {}) {
  const changes = report.changes ?? [];
  const messages = report.messages ?? [];
  const conflicted = changes.some((c) => c.action === "conflict");
  if (exitCode !== 0 && !(exitCode === 1 && conflicted)) {
    // Exit 1 without a conflict is owned drift: nothing was written, so there is nothing to
    // put in a PR. Failing the run makes the drift visible instead of hiding it in a PR.
    const why = exitCode === 1 ? "owned files were edited locally, or conflict markers are left on disk" : `splice exited ${exitCode}`;
    throw new Error(`${why}\n${messages.join("\n")}`);
  }
  if (changes.length === 0) return { title: "", body: "", result: "current" };

  const result = conflicted ? "conflicts" : "updated";
  const title = `${conflicted ? "[conflicts] " : ""}chore: splice update to ${version}`;
  const lines = [`Automated \`splice update\` to template version ${version}.`, ""];
  if (conflicted) {
    lines.push("## Conflicts", "");
    for (const c of changes.filter((c) => c.action === "conflict")) lines.push(`- ${cell(c.path)}`);
    lines.push("");
  }
  const rank = (a) => (ORDER.includes(a) ? ORDER.indexOf(a) : ORDER.length);
  const table = (rows) => {
    const sorted = rows
      .map((c, i) => [c, i])
      .sort((a, b) => rank(a[0].action) - rank(b[0].action) || a[1] - b[1]);
    return [
      "| action | path | class |",
      "| --- | --- | --- |",
      ...sorted.map(([c]) => `| ${c.action} | ${cell(c.path)} | ${c.class} |`),
    ];
  };
  // `update --all` marks each source in `messages` (`source=main`, `source=channel:NAME`);
  // one table per source then, main first. Without channel tags: one flat table (scope main).
  const tags = messages.filter((m) => TAG.test(m));
  if (tags.some((t) => t.startsWith("source=channel:"))) {
    const names = new Set(["main"]);
    for (const t of tags) names.add(t.replace(/^source=(channel:)?/, ""));
    const by = new Map();
    for (const c of changes) {
      const n = sourceOf(c.path, owners);
      names.add(n);
      by.set(n, [...(by.get(n) ?? []), c]);
    }
    lines.push("## Changes", "");
    const order = ["main", ...[...names].filter((n) => n !== "main").sort()];
    for (const n of order) if (by.has(n)) lines.push(`### ${n}`, "", ...table(by.get(n)), "");
    lines.pop();
  } else {
    lines.push("## Changes", "", ...table(changes));
  }
  const notes = messages.filter((m) => !TAG.test(m));
  if (notes.length) lines.push("", "## Messages", "", ...notes.map((m) => `- ${defang(m)}`));
  lines.push(
    "",
    conflicted
      ? "## How to resolve\n\nEdit the files above to remove the conflict markers, then run `splice check` until it exits 0."
      : "## How to resolve\n\nNothing to do if CI is green; run `splice check` locally to confirm the repo matches the template.",
  );
  return { title, body: lines.join("\n"), result };
}

// Lock tables of the working directory (`ref` null) or of HEAD: which source locks which path.
function lockTables(ref) {
  const files = new Map();
  const sections = new Set();
  const read = (rel) => {
    try {
      return ref
        ? execFileSync("git", ["show", `${ref}:./${rel}`], { encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] })
        : readFileSync(rel, "utf8");
    } catch {
      return null;
    }
  };
  const keys = (text, source, version) => {
    let table = "";
    for (const line of text.split("\n")) {
      const h = line.match(/^\[([a-z]+)\]/);
      if (h) table = h[1];
      const k = lockKey(line);
      if (k !== null && table === "files") files.set(k, source);
    }
    if (source !== "main") sections.add(source);
    return version;
  };
  const versionOf = (text) => text.match(/^\[template\][^[]*?^version\s*=\s*"([^"]*)"/ms)?.[1];
  let main = null;
  const lock = read(".splice/lock.toml");
  if (lock) main = keys(lock, "main", versionOf(lock));
  let listing = [];
  if (ref) {
    try {
      listing = execFileSync("git", ["ls-tree", "--name-only", ref, ".splice/channels/"], { encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] })
        .split("\n").filter(Boolean).map((p) => p.split("/").pop());
    } catch {}
  } else if (existsSync(".splice/channels")) listing = readdirSync(".splice/channels");
  const channels = {};
  for (const f of listing.filter((n) => n.endsWith(".toml") && !n.startsWith(".")).sort()) {
    const text = read(`.splice/channels/${f}`);
    if (!text) continue;
    const name = f.slice(0, -5);
    channels[name] = keys(text, name, versionOf(text));
  }
  return { files, sections, main, channels };
}

// Owners of every path the update touched: new locks win, old locks (HEAD) cover deletions.
function ownersFromLocks() {
  const now = lockTables(null);
  const old = lockTables("HEAD");
  const files = new Map([...old.files, ...now.files]);
  return { files, sections: new Set([...old.sections, ...now.sections]), main: now.main, channels: now.channels };
}

function main([reportPath, code, version]) {
  if (reportPath === "args") {
    try {
      const scope = code || scopeDefault({ channels: existsSync(".splice/channels") ? readdirSync(".splice/channels") : [] });
      process.stdout.write(`${command(scope, version ?? "").join("\n")}\n`);
    } catch (e) {
      process.stderr.write(`${e.message}\n`);
      process.exit(1);
    }
    return;
  }
  const exitCode = Number(code);
  let report;
  try {
    report = JSON.parse(readFileSync(reportPath, "utf8"));
  } catch {
    process.stderr.write(`splice exited ${exitCode} and printed no JSON report\n`);
    process.exit(1);
  }
  let s;
  try {
    const owners = ownersFromLocks();
    // A channel-only repo has no main lock: name the channel versions instead.
    // With a main lock the title also names each channel `update --all` covered: `2.0.0, rig 1.1.0`.
    const hasMain = version && version !== "unknown";
    const touched = new Set((report.messages ?? []).map((m) => m.match(/^source=channel:(.+)$/)?.[1]).filter(Boolean));
    const chans = Object.entries(owners.channels)
      .filter(([n]) => !hasMain || touched.has(n))
      .map(([n, v]) => `${n} ${v}`);
    const named = [...(hasMain ? [version] : []), ...chans].join(", ") || "unknown";
    s = summarize(report, exitCode, named, owners);
  } catch (e) {
    process.stderr.write(`${e.message}\n`);
    process.exit(1);
  }
  const delim = `SPLICE_EOF_${randomBytes(16).toString("hex")}`;
  appendFileSync(
    process.env.GITHUB_OUTPUT,
    `result=${s.result}\ntitle=${s.title}\nbody<<${delim}\n${s.body}\n${delim}\n`,
  );
}

if (process.argv[1] === fileURLToPath(import.meta.url)) main(process.argv.slice(2));
