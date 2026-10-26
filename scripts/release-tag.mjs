// Release tag decision for a push to main: which tag `v<version>` this run releases, and whether
// it must create it first (see `releaseTag`).
import { execFileSync } from "node:child_process";
import { appendFileSync, readFileSync, existsSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { checkVersion } from "./check-version.mjs";

/**
 * `remoteTags` is `git ls-remote --tags` output (plain and peeled `^{}` lines). Returns the tag and
 * whether to create it: a missing tag is created; a tag on `sha` (the run's own commit, e.g. after
 * "Re-run all jobs") is released again without creating it; a tag on another commit releases
 * nothing (`tag: ""`).
 */
export function releaseTag({ version, remoteTags, sha }) {
  const tag = `v${version}`;
  const ref = `refs/tags/${tag}`;
  let plain = null;
  let peeled = null;
  for (const line of remoteTags.split("\n")) {
    const [commit, name] = line.split("\t");
    if (name === ref) plain = commit;
    if (name === `${ref}^{}`) peeled = commit;
  }
  const at = peeled ?? plain;
  if (at === null) return { tag, create: true };
  if (sha && at === sha) return { tag, create: false };
  return { tag: "", create: false };
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
    const sha = process.env.GITHUB_SHA ?? "";
    const { tag, create } = releaseTag({ version, remoteTags, sha });
    console.log(
      create
        ? `${tag} is not tagged yet: tag and release it`
        : tag
          ? `${tag} already tags this commit: release it again (a re-run)`
          : `v${version} is already tagged on another commit: nothing to release`,
    );
    if (process.env.GITHUB_OUTPUT) {
      appendFileSync(process.env.GITHUB_OUTPUT, `version=${version}\ntag=${tag}\ncreate=${create}\n`);
    }
  } catch (e) {
    console.error(`release-tag: ${e.message}`);
    process.exit(1);
  }
}
