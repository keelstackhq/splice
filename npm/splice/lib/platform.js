import { createRequire } from "node:module";
import { readFileSync } from "node:fs";
import { join } from "node:path";

const PLATFORMS = JSON.parse(readFileSync(new URL("../platforms.json", import.meta.url), "utf8"));

export class LauncherError extends Error {
  exitCode = 3;
}

export function packageFor(platform = process.platform, arch = process.arch) {
  const hit = PLATFORMS.find((p) => p.os === platform && p.cpu === arch);
  return hit ? { npm: hit.npm, bin: hit.bin } : null;
}

export function binaryPath({
  resolve = createRequire(import.meta.url).resolve,
  platform = process.platform,
  arch = process.arch,
  env = process.env,
} = {}) {
  if (env.SPLICE_BINARY) return env.SPLICE_BINARY;
  const pkg = packageFor(platform, arch);
  if (pkg) {
    try {
      return join(resolve(`${pkg.npm}/package.json`), "..", "bin", pkg.bin);
    } catch {
      // fall through to the error below
    }
  }
  const supported = PLATFORMS.map((p) => `${p.os}-${p.cpu}`).join(", ");
  const name = pkg ? pkg.npm : "none for this platform";
  throw new LauncherError(
    `splice: no prebuilt binary for ${platform}-${arch} (package ${name} is not installed); ` +
      `supported: ${supported}; download a release from https://github.com/keelstackhq/splice/releases ` +
      "or run `cargo install --locked --git https://github.com/keelstackhq/splice keel-splice`",
  );
}
