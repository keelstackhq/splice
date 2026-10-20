// Fail a release when the tag and the Cargo.toml [package] version differ.
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

function packageVersion(cargoToml) {
  // A bare snippet with no [package] table is read as top-level keys.
  const hasPackage = /^\s*\[\s*package\s*\]/m.test(cargoToml);
  let inPackage = !hasPackage;
  for (const raw of cargoToml.split(/\r?\n/)) {
    const line = raw.trim();
    const table = line.match(/^\[\s*([^\]]+?)\s*\]\s*(#.*)?$/);
    if (table) {
      inPackage = table[1] === "package";
      continue;
    }
    if (!inPackage) continue;
    const m = line.match(/^version\s*=\s*"([^"]+)"/);
    if (m) return m[1];
  }
  return null;
}

export function checkVersion({ tag, cargoToml }) {
  if (!tag || !tag.startsWith("v")) {
    throw new Error(`tag ${tag} must start with "v"`);
  }
  const version = packageVersion(cargoToml);
  if (version === null) throw new Error("no [package] version in Cargo.toml");
  if (tag.slice(1) !== version) {
    throw new Error(`tag ${tag} does not match Cargo.toml ${version}`);
  }
  return version;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  try {
    const tag = process.argv[2] ?? process.env.GITHUB_REF_NAME;
    const cargoToml = readFileSync(process.argv[3] ?? "Cargo.toml", "utf8");
    console.log(checkVersion({ tag, cargoToml }));
  } catch (e) {
    console.error(`error: ${e.message}`);
    process.exit(1);
  }
}
