// Release version gate: tag == Cargo.toml == npm launcher version == its optionalDependencies pins.
import { readFileSync, existsSync, appendFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const SEMVER = /^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$/;

function cargoVersion(cargoToml) {
  const section = cargoToml.match(/^\[package\]\s*$([\s\S]*?)(?=^\[|(?![\s\S]))/m);
  const m = section && section[1].match(/^version\s*=\s*"([^"]+)"/m);
  if (!m) throw new Error("Cargo.toml has no [package] version");
  return m[1];
}

function actionPin(actionYml) {
  const m = actionYml.match(/^ {2}version:\s*\n(?: {4}[^\n]*\n)*? {4}default:\s*"?([^"\n]+?)"?\s*$/m);
  if (!m) throw new Error("action/action.yml has no `version` input default");
  return m[1];
}

export function checkVersion({ tag, cargoToml, launcherPkg, platforms, actionYml }) {
  const version = cargoVersion(cargoToml);
  if (actionYml !== undefined) {
    // The Action installs this exact splice, so a release must bump the pin with the version.
    const pin = actionPin(actionYml);
    if (pin !== version) {
      throw new Error(`action/action.yml pins splice ${pin}, expected ${version}`);
    }
  }
  if (tag !== null && tag !== undefined) {
    if (!tag.startsWith("v") || !SEMVER.test(tag.slice(1))) {
      throw new Error(`tag ${tag} is not a release tag (expected v<major>.<minor>.<patch>[-pre])`);
    }
    if (tag.slice(1) !== version) {
      throw new Error(`tag ${tag} does not match Cargo.toml ${version}`);
    }
  }
  if (launcherPkg) {
    if (launcherPkg.version !== version) {
      throw new Error(`npm/splice/package.json version ${launcherPkg.version} does not match Cargo.toml ${version}`);
    }
    const deps = launcherPkg.optionalDependencies ?? {};
    const list =
      platforms ??
      JSON.parse(readFileSync(new URL("../npm/splice/platforms.json", import.meta.url), "utf8"));
    const missing = list.map((p) => p.npm).filter((n) => !(n in deps));
    if (missing.length) {
      throw new Error(`npm/splice/package.json is missing optionalDependencies: ${missing.join(", ")}`);
    }
    for (const [name, pin] of Object.entries(deps)) {
      if (pin !== version) {
        throw new Error(`npm/splice/package.json pins ${name} to ${pin}, expected ${version}`);
      }
    }
  }
  return version;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const i = process.argv.indexOf("--tag");
  const tag = i > 0 ? process.argv[i + 1] : null;
  const pkgPath = new URL("../npm/splice/package.json", import.meta.url);
  const launcherPkg = existsSync(pkgPath) ? JSON.parse(readFileSync(pkgPath, "utf8")) : undefined;
  try {
    const version = checkVersion({
      tag,
      cargoToml: readFileSync(new URL("../Cargo.toml", import.meta.url), "utf8"),
      launcherPkg,
      actionYml: readFileSync(new URL("../action/action.yml", import.meta.url), "utf8"),
    });
    console.log(version);
    if (process.env.GITHUB_OUTPUT) appendFileSync(process.env.GITHUB_OUTPUT, `version=${version}\n`);
  } catch (e) {
    console.error(`check-version: ${e.message}`);
    process.exit(1);
  }
}
