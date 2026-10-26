// Release tag decision for a push to main: the tag `v<version>` to create, or "" when it exists.
import { execFileSync } from "node:child_process";
import { appendFileSync, readFileSync, existsSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { checkVersion } from "./check-version.mjs";

/** `remoteTags` is `git ls-remote --tags` output; a tag counts when listed, peeled or not. */
export function releaseTag({ version, remoteTags }) {
  const tag = `v${version}`;
  const refs = remoteTags
    .split("\n")
    .map((line) => line.split("\t")[1])
    .filter(Boolean)
    .map((ref) => ref.replace(/\^\{\}$/, ""));
  return refs.includes(`refs/tags/${tag}`) ? "" : tag;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  try {
    const pkgPath = new URL("../npm/splice/package.json", import.meta.url);
    const version = checkVersion({
      tag: null,
      cargoToml: readFileSync(new URL("../Cargo.toml", import.meta.url), "utf8"),
      launcherPkg: existsSync(pkgPath) ? JSON.parse(readFileSync(pkgPath, "utf8")) : undefined,
    });
    const remoteTags = execFileSync("git", ["ls-remote", "--tags", "origin", `refs/tags/v${version}*`], {
      encoding: "utf8",
    });
    const tag = releaseTag({ version, remoteTags });
    console.log(tag ? `${tag} is not tagged yet: release it` : `v${version} is already tagged: nothing to release`);
    if (process.env.GITHUB_OUTPUT) appendFileSync(process.env.GITHUB_OUTPUT, `version=${version}\ntag=${tag}\n`);
  } catch (e) {
    console.error(`release-tag: ${e.message}`);
    process.exit(1);
  }
}
