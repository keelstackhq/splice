"use strict";
// Maps (process.platform, process.arch) to the platform package that ships the binary.
const fs = require("node:fs");
const path = require("node:path");

function loadPlatforms() {
  // Staged package: platforms.json sits next to lib/. In the repo it lives in npm/.
  for (const rel of [["..", "platforms.json"], ["..", "..", "platforms.json"]]) {
    const file = path.join(__dirname, ...rel);
    if (fs.existsSync(file)) return JSON.parse(fs.readFileSync(file, "utf8"));
  }
  throw new Error("platforms.json is missing from the @keelstack/splice package");
}

function packageFor(platform = process.platform, arch = process.arch) {
  const hit = loadPlatforms().find((p) => p.os === platform && p.cpu === arch);
  return hit ? { npm: hit.npm, bin: hit.bin } : null;
}

function supportedPairs() {
  return loadPlatforms().map((p) => `${p.os}-${p.cpu}`);
}

function binaryPath(resolve = require.resolve, platform = process.platform, arch = process.arch) {
  const pkg = packageFor(platform, arch);
  if (!pkg) {
    throw new Error(
      `splice has no prebuilt binary for ${platform}-${arch}.\n` +
        `Supported platforms: ${supportedPairs().join(", ")}.`,
    );
  }
  let manifest;
  try {
    manifest = resolve(`${pkg.npm}/package.json`);
  } catch {
    throw new Error(
      `The platform package ${pkg.npm} is not installed.\n` +
        `It is an optional dependency of @keelstack/splice; reinstall without --no-optional ` +
        `(or --omit=optional / --ignore-optional).\n` +
        `Supported platforms: ${supportedPairs().join(", ")}.`,
    );
  }
  return path.join(path.dirname(manifest), "bin", pkg.bin);
}

module.exports = { packageFor, binaryPath, supportedPairs };
