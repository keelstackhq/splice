// Input validation and the `splice update` command line for the update-bot action.
// CLI (inputs arrive only through the environment):
//   node args.mjs validate   env VERSION, TO, BRANCH, SCOPE; exits 1 with a message on bad input
//   node args.mjs command    env TO, SCOPE; prints the splice arguments, one per line
//                            (run in the repo: an empty SCOPE is `all` when .splice/channels/ has a *.toml)
import { readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";

export const SEMVER = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$/;

export function validateInputs({ version, to, branch, scope = "" }) {
  if (!["latest", "next", "none"].includes(version) && !SEMVER.test(version ?? "")) {
    throw new Error("`version` must be latest, next, none or a semver");
  }
  if (to && !SEMVER.test(to)) {
    throw new Error("`to` must be empty or a semver");
  }
  if (!["", "main", "all"].includes(scope)) {
    throw new Error("`scope` must be empty, main or all");
  }
  if (!branch) {
    throw new Error("`branch` must not be empty");
  }
}

export function scopeDefault({ channels }) {
  return channels.some((f) => f.endsWith(".toml")) ? "all" : "main";
}

export function command({ scope, to }) {
  if (scope === "all") {
    if (to) throw new Error("`to` needs `scope: main` (`--all` cannot be combined with `--to`)");
    return ["update", "--all", "--json", "--yes"];
  }
  return ["update", "--json", "--yes", ...(to ? ["--to", to] : [])];
}

function listChannels() {
  try {
    return readdirSync(".splice/channels");
  } catch {
    return [];
  }
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const { VERSION, TO = "", BRANCH, SCOPE = "" } = process.env;
  try {
    if (process.argv[2] === "validate") {
      validateInputs({ version: VERSION, to: TO, branch: BRANCH, scope: SCOPE });
    } else if (process.argv[2] === "command") {
      const scope = SCOPE || scopeDefault({ channels: listChannels() });
      process.stdout.write(command({ scope, to: TO }).join("\n") + "\n");
    } else {
      throw new Error("usage: args.mjs validate|command");
    }
  } catch (err) {
    process.stderr.write(`error: ${err.message}\n`);
    process.exit(1);
  }
}
