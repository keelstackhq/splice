// Input validation and the `splice update` command line for the update-bot action.
// CLI (inputs arrive only through the environment):
//   node args.mjs validate   env VERSION, TO, BRANCH; exits 1 with a message on bad input
//   node args.mjs command    env TO; prints the splice arguments, one per line
import { fileURLToPath } from "node:url";

const SEMVER = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$/;

export function validateInputs({ version, to, branch }) {
  if (!["latest", "next", "none"].includes(version) && !SEMVER.test(version ?? "")) {
    throw new Error("`version` must be latest, next, none or a semver");
  }
  if (to && !SEMVER.test(to)) {
    throw new Error("`to` must be empty or a semver");
  }
  if (!branch) {
    throw new Error("`branch` must not be empty");
  }
}

export function command({ to }) {
  return ["update", "--json", "--yes", ...(to ? ["--to", to] : [])];
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const { VERSION, TO = "", BRANCH } = process.env;
  try {
    if (process.argv[2] === "validate") {
      validateInputs({ version: VERSION, to: TO, branch: BRANCH });
    } else if (process.argv[2] === "command") {
      process.stdout.write(command({ to: TO }).join("\n") + "\n");
    } else {
      throw new Error("usage: args.mjs validate|command");
    }
  } catch (err) {
    process.stderr.write(`error: ${err.message}\n`);
    process.exit(1);
  }
}
