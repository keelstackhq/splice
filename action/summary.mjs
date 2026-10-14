// Summarise `splice update --json` output for the update-bot PR. Node built-ins only.
//   node summary.mjs <report.json> <splice-exit-code> <template-version>
// Writes `result`, `title` and `body` to $GITHUB_OUTPUT; exits 1 (messages on stderr)
// when the update failed outright (template error, usage error, owned drift).
import { readFileSync, appendFileSync } from "node:fs";
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

export function summarize(report, exitCode, version) {
  const changes = report.changes ?? [];
  const messages = report.messages ?? [];
  const conflicted = changes.some((c) => c.action === "conflict");
  if (exitCode !== 0 && !(exitCode === 1 && conflicted)) {
    // Exit 1 without a conflict is owned drift: nothing was written, so there is nothing to
    // put in a PR. Failing the run makes the drift visible instead of hiding it in a PR.
    const why = exitCode === 1 ? "owned files were edited locally; nothing was written" : `splice exited ${exitCode}`;
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
  lines.push("## Changes", "", "| action | path | class |", "| --- | --- | --- |");
  const rank = (a) => (ORDER.includes(a) ? ORDER.indexOf(a) : ORDER.length);
  const sorted = changes
    .map((c, i) => [c, i])
    .sort((a, b) => rank(a[0].action) - rank(b[0].action) || a[1] - b[1]);
  for (const [c] of sorted) lines.push(`| ${c.action} | ${cell(c.path)} | ${c.class} |`);
  if (messages.length) lines.push("", "## Messages", "", ...messages.map((m) => `- ${defang(m)}`));
  lines.push(
    "",
    conflicted
      ? "## How to resolve\n\nEdit the files above to remove the conflict markers, then run `splice check` until it exits 0."
      : "## How to resolve\n\nNothing to do if CI is green; run `splice check` locally to confirm the repo matches the template.",
  );
  return { title, body: lines.join("\n"), result };
}

function main([reportPath, code, version]) {
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
    s = summarize(report, exitCode, version);
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
