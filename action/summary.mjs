// Decide the update-bot result from splice's JSON report, its exit code and the tree state.
// CLI: node summary.mjs <report.json> <exit-code>   (run inside the repo, after the update)
//   writes result, title and body-file to $GITHUB_OUTPUT; exits 1 with splice's messages on exit 2/3.
import { execFileSync } from "node:child_process";
import { appendFileSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { SEMVER } from "./args.mjs";

export function summarize(report, exitCode, { version, dirty }) {
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
  const base = `chore: splice update to ${version}`;
  const title = result === "conflicts" ? `[conflicts] ${base}` : base;
  const parts = [];
  const rows = report.changes ?? [];
  if (rows.length > 0) {
    const order = [];
    for (const c of rows) if (!order.includes(c.action)) order.push(c.action);
    const lines = ["| action | path | class |", "| --- | --- | --- |"];
    for (const action of order) {
      for (const c of rows.filter((r) => r.action === action)) {
        lines.push(`| ${action} | \`${c.path}\` | ${c.class} |`);
      }
    }
    parts.push(lines.join("\n"));
  }
  if (messages.length > 0) parts.push(messages.join("\n"));
  if (result === "conflicts") {
    parts.push("Resolve the conflict markers, then run `splice check --json`.");
  }
  return { title, body: parts.join("\n\n") + "\n", result };
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
    const version =
      exitCode === 0 || exitCode === 1
        ? parseLockVersion(readFileSync(".splice/lock.toml", "utf8"))
        : "unknown";
    const s = summarize(report, exitCode, { version, dirty });
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
