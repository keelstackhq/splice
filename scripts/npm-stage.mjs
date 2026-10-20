// Build the npm package directories for a release.
// usage: node scripts/npm-stage.mjs <version> <artifacts-dir> <out-dir>
// <artifacts-dir>/<target>/<bin> holds each target's binary (splice or splice.exe).
import { chmodSync, copyFileSync, cpSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, relative, sep } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const DEFAULT_REPO = "keelstackhq/splice";
// Provenance must match the repo that publishes, so CI's GITHUB_REPOSITORY wins.
const repositoryFor = (env) => ({
  type: "git",
  url: `git+https://github.com/${env.GITHUB_REPOSITORY || DEFAULT_REPO}.git`,
});
const NO_SCRIPTS = ["preinstall", "install", "postinstall", "prepare"];

const writeJson = (file, value) => writeFileSync(file, JSON.stringify(value, null, 2) + "\n");

export function stage({ version, artifacts, out, env = process.env }) {
  const repository = repositoryFor(env);
  if (!version) throw new Error("version is required");
  const platforms = JSON.parse(readFileSync(join(root, "npm", "platforms.json"), "utf8"));
  rmSync(out, { recursive: true, force: true });
  mkdirSync(out, { recursive: true });

  const docs = (dir) => {
    // LICENSE lands with plan 03 Task 5; copy it as soon as the repo has one.
    for (const f of ["LICENSE", "README.md"]) {
      if (existsSync(join(root, f))) copyFileSync(join(root, f), join(dir, f));
    }
  };

  for (const p of platforms) {
    const src = join(artifacts, p.target, p.bin);
    if (!existsSync(src)) throw new Error(`missing binary for ${p.npm} (${p.target}): ${src}`);
    const dir = join(out, p.npm.split("/")[1]);
    mkdirSync(join(dir, "bin"), { recursive: true });
    const dest = join(dir, "bin", p.bin);
    copyFileSync(src, dest);
    if (p.os !== "win32") chmodSync(dest, 0o755);
    writeJson(join(dir, "package.json"), {
      name: p.npm,
      version,
      description: `splice binary for ${p.os}-${p.cpu}`,
      license: "MIT",
      repository,
      os: [p.os],
      cpu: [p.cpu],
      preferUnplugged: true,
      files: ["bin"],
      publishConfig: { access: "public" },
    });
    docs(dir);
  }

  const launcher = join(out, "splice");
  const launcherSrc = join(root, "npm", "splice");
  cpSync(launcherSrc, launcher, {
    recursive: true,
    filter: (s) => relative(launcherSrc, s).split(sep)[0] !== "test",
  });
  copyFileSync(join(root, "npm", "platforms.json"), join(launcher, "platforms.json"));
  const pkg = JSON.parse(readFileSync(join(root, "npm", "splice", "package.json"), "utf8"));
  pkg.version = version;
  pkg.repository = repository;
  pkg.optionalDependencies = Object.fromEntries(platforms.map((p) => [p.npm, version]));
  for (const k of NO_SCRIPTS) delete pkg.scripts?.[k];
  writeJson(join(launcher, "package.json"), pkg);
  docs(launcher);
  return out;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  try {
    const [version, artifacts, out] = process.argv.slice(2);
    if (!version || !artifacts || !out) throw new Error("usage: npm-stage.mjs <version> <artifacts-dir> <out-dir>");
    stage({ version, artifacts, out });
    console.log(`staged ${version} into ${out}`);
  } catch (e) {
    console.error(`npm-stage: ${e.message}`);
    process.exit(1);
  }
}
