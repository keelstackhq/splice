// Stage the npm packages for a release: one per platform plus the launcher (last).
import { chmodSync, copyFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = fileURLToPath(new URL("..", import.meta.url));
const REPO_RE = /^[A-Za-z0-9._-]+\/[A-Za-z0-9._-]+$/;

const writeJson = (path, value) => writeFileSync(path, `${JSON.stringify(value, null, 2)}\n`);

export function stage({ version, artifactsDir, outDir, repository }) {
  if (!REPO_RE.test(repository ?? "")) {
    throw new Error(`repository must look like owner/repo, got ${JSON.stringify(repository)}`);
  }
  const repo = { type: "git", url: `git+https://github.com/${repository}.git` };
  const license = join(ROOT, "LICENSE");
  const launcherSrc = join(ROOT, "npm", "splice");
  const platforms = JSON.parse(readFileSync(join(launcherSrc, "platforms.json"), "utf8"));
  const dirs = [];

  for (const p of platforms) {
    const src = join(artifactsDir, p.target, p.bin);
    if (!existsSync(src)) throw new Error(`missing binary for ${p.target}: ${src}`);
    const dir = join(outDir, p.npm.split("/")[1]);
    mkdirSync(join(dir, "bin"), { recursive: true });
    copyFileSync(src, join(dir, "bin", p.bin));
    if (p.os !== "win32") chmodSync(join(dir, "bin", p.bin), 0o755);
    copyFileSync(license, join(dir, "LICENSE"));
    writeJson(join(dir, "package.json"), {
      name: p.npm,
      version,
      description: `splice binary for ${p.os}-${p.cpu}`,
      license: "MIT",
      repository: repo,
      os: [p.os],
      cpu: [p.cpu],
      files: ["bin"],
    });
    dirs.push(dir);
  }

  const dir = join(outDir, "splice");
  for (const f of ["bin/splice.js", "lib/platform.js", "platforms.json", "README.md"]) {
    mkdirSync(dirname(join(dir, f)), { recursive: true });
    copyFileSync(join(launcherSrc, f), join(dir, f));
  }
  chmodSync(join(dir, "bin", "splice.js"), 0o755);
  copyFileSync(license, join(dir, "LICENSE"));
  const pkg = JSON.parse(readFileSync(join(launcherSrc, "package.json"), "utf8"));
  delete pkg.scripts;
  delete pkg.devDependencies;
  pkg.version = version;
  pkg.repository = repo;
  pkg.optionalDependencies = Object.fromEntries(platforms.map((p) => [p.npm, version]));
  writeJson(join(dir, "package.json"), pkg);
  dirs.push(dir);
  return dirs;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const arg = (name) => {
    const i = process.argv.indexOf(`--${name}`);
    if (i < 0 || !process.argv[i + 1]) {
      console.error(`npm-stage: missing --${name}`);
      process.exit(2);
    }
    return process.argv[i + 1];
  };
  try {
    const dirs = stage({
      version: arg("version"),
      artifactsDir: arg("artifacts"),
      outDir: arg("out"),
      repository: process.env.GITHUB_REPOSITORY,
    });
    console.log(dirs.join("\n"));
  } catch (e) {
    console.error(`npm-stage: ${e.message}`);
    process.exit(1);
  }
}
