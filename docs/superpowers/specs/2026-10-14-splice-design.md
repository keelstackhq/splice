# splice — Design Spec

Date: 2026-10-14 · Status: draft for implementation · Contract: `docs/template-format.md` (format 1)

Names: GitHub org `keelstackhq` (tools for bootstrapping projects), repo `keelstackhq/splice`, binary
`splice`, Cargo package `keel-splice` (lib name `splice`; the crate `splice` is taken), npm launcher
`@keelstackhq/splice` with platform packages `@keelstackhq/splice-{linux-x64,linux-arm64,darwin-x64,darwin-arm64,win32-x64}`,
update bot `uses: keelstackhq/splice/action@v1`.

## 1. Goal

Bootstrap a repo from versioned template **pieces** (`splice new`, `splice add`) and keep it
current (`splice update`, `splice check`, `splice diff`), with a single static binary that runs
anywhere `git` runs. `pnpm dlx @keelstackhq/splice new gh:keelstackhq/hull//template` must work with no
Python and no Rust toolchain.

Why: no Rust tool does Copier-style 3-way update (cargo-generate and kickstart have no update;
baker is 2-way only; ffizer is unclear). Copier (9.19.1) is mature but needs Python and replays
old template versions under a new Copier, which breaks when the template language moves. splice
fixes that with a frozen, tiny rendering subset and a `min_splice` pin.

First consumer: `keelstackhq/hull` (opinionated monorepo template, built in parallel against the
same contract; it uses a Nunjucks dev renderer until splice ships).

### Non-goals (v1)

- Any template syntax beyond contract §4. No hooks beyond `[[post]]` and migration `[[step]]`.
- Element-wise array merges, comment-preserving JSON/YAML merges on real conflicts.
- A registry or hosted service. Templates are git repos or local directories.
- Built-in HTTP/TLS. All network access goes through the user's `git` CLI.

## 2. The contract is binding

`docs/template-format.md` defines layout, manifests, rendering subset, ownership classes, lock
file, migrations, JSON output and exit codes. This spec only adds what the contract leaves open
(§10 lists those decisions). If they conflict, the contract wins; change it only in both repos
(splice and hull) with a `format` bump.

## 3. CLI

Global flags on every command: `--json` (contract §8), `--yes` (no prompts, use defaults),
`-C <dir>` (repo root, default `.`). Prompts also never appear when stdin is not a TTY or with
`--json`. A missing value with no default is a usage error (exit 2) naming the variable and the
`--data` flag to pass.

| Command | Purpose |
|---|---|
| `splice new <source> [dest] [--ref R] [--pre] [--piece ID[:NAME]]… [--data K=V]… [--no-post] [--force]` | Render `base` + the listed pieces into `dest` (default `.`), write the lock. |
| `splice add <piece-id> [--name N] [--data K=V]… [--allow-dirty] [--no-post] [--force]` | Add a piece instance at the lock's template commit. |
| `splice update [--to VERSION] [--data K=V]… [--allow-dirty] [--force] [--no-post]` | Move to a newer template version (default: latest stable tag). |
| `splice check [--offline]` | Exit 1 on owned drift, leftover conflict markers, or an available update. |
| `splice diff [--to VERSION]` | Show what `update` would change, without writing. |
| `splice explain <path>` | Print class, owning piece instance and matching glob of a path. |

`--data K=V` sets a template var (`K`) or, for `new --piece`/`add`, a piece var. A key that
names no variable of the template or the selected pieces is a usage error (exit 2). On `update`,
`--data` sets only template vars new in the target: a var already in the lock or a piece var is
exit 2 (§8.3 step 4). `--piece ID:NAME`
names a `multiple` instance.

Human output is terse: one line per change (`create  apps/web/package.json  shared`), then one
summary line. Everything else goes to stderr. With `--json`, stdout holds exactly the one JSON
object; post-command and git output go to stderr.

### Exit codes (contract §8, applied per command)

| Code | When |
|---|---|
| 0 | success; `check` found nothing |
| 1 | `check`: drift, markers or update available · `update`/`add`: conflicts written, or refused because of owned drift |
| 2 | usage: bad flags, missing var value or a value failing its `pattern`, dirty tree, non-empty destination or existing lock (`new`), unmet `requires`, downgrade, non-`multiple` piece added twice, duplicate instance name, no lock where one is needed |
| 3 | template or environment error: bad manifest, unknown `format`, `min_splice` too new, render error, unsafe path, git failure, failed migration step or post command |

clap parse errors still exit 2; when `--json` is in argv they are also reported as the JSON object
(`command` = the subcommand if known, else `""`).

## 4. Template sources and fetching

Source spec grammar: `gh:<owner>/<repo>[//<subdir>]` → `https://github.com/<owner>/<repo>.git`;
any `https://`, `ssh://`, `git@…` or `file://` URL with optional `//<subdir>` after the repo part;
or a local path (`./x`, `/x`, `../x`, `C:\x`) with optional `//<subdir>`. The lock stores the
spec as given (`gh:keelstackhq/hull//template`), except that a local path is stored as an absolute
path (`splice new tpl out` locks `/abs/tpl`), so later commands work from any directory and with
`-C`. A relative local source is resolved against the current directory, not against `-C`.

All git work shells out to the `git` CLI (present wherever repos live), so the user's credentials,
proxies and SSH config just work. Every call sets `-c core.autocrlf=false -c core.eol=lf` and
`GIT_TERMINAL_PROMPT=0`.

- **Tags:** `git ls-remote --tags <url>`; keep `refs/tags/v<semver>` (peeled `^{}` sha wins);
  default = highest stable version; `--pre` allows pre-releases.
- **`--ref R`:** a tag, branch or sha. A non-`v<semver>` ref records version `0.0.0+g<sha7>`
  (used by hull before its first tag); `update` from such a lock needs an explicit `--to`.
- **Cache:** `$SPLICE_CACHE_DIR` or `dirs::cache_dir()/splice`. One bare mirror per URL at
  `git/<sha256(url)[..16]>.git` (`git clone --mirror`, later `git fetch --tags --prune`), and one
  immutable extracted tree per commit at `trees/<sha256(url)[..16]>/<commit>/`, created with
  `git --git-dir <mirror> --work-tree <tmp> checkout <commit> -- .` using a temporary
  `GIT_INDEX_FILE`, then renamed into place. A present tree is reused with no network access.
- **Local paths** must be git repos (tests create them) and are used directly as the "mirror".
- A missing commit (force-pushed template) is a template error (exit 3) that names the commit.

## 5. Manifests and validation (contract §2, §3)

Loaded with `toml` + serde (`deny_unknown_fields` off: unknown keys warn, so newer templates stay
readable). Validation errors exit 3 and name the file and key.

- `format` must be `1`. `min_splice` must be ≤ the running version, else exit 3 with
  "template needs splice ≥ X; run `pnpm dlx @keelstackhq/splice@latest …`".
- `base` must name an existing piece with `multiple = false`.
- Piece `id` equals its directory name. `requires` ids must exist. No cycles needed (requires is
  a presence check, not an order).
- Var names match `^[a-z_][a-z0-9_]*$` and may not be `loop`, `self`, `range` or `namespace`
  (hull renderer rule; contract §9 does not list them yet, so they are decided here). A piece var
  may not reuse a template var name or the reserved names `pieces`, `piece`, `template`. `pattern` compiles with the `regex` crate (keep
  patterns to the JS/Rust common subset: no look-around, no backrefs). Defaults are literal (not
  rendered) and must match `pattern`.
- `[merge]` values are `json|yaml|toml|text`; a `[merge]` key must also match a `shared` glob.
- `[[post]].when` is required (a missing `when` is a validation error) and its values are
  `new|add|update`.
- Migration file names are `<semver>.toml`.

## 6. Rendering (contract §4)

**Subset gate.** Before MiniJinja sees a file, `subset::validate` scans every `{{ }}`, `{% %}` and
`{# #}` tag and rejects anything outside this grammar (exit 3, with file and line):

```
expr     := path ( "|" ("lower"|"upper") )?          path := ident ("." ident)*
output   := "{{" ws expr ws "}}"
if       := "{%" ws ("if"|"elif") ws cond ws "%}"    cond := path | string ws "in" ws "pieces"
else     := "{%" ws ("else"|"endif"|"endfor") ws "%}"
for      := "{%" ws "for" ws ident ws "in" ws "pieces" ws "%}"
comment  := "{#" any "#}"
string   := '"' [a-z0-9-]+ '"'
```

Gate decisions (mirrored by hull's gate): `ident` is `[a-z_][a-z0-9_]*` and may not be one of
`true false none not and or in is loop self range namespace`; bare `pieces`, `piece` and `template`
are rejected as variables (use `"id" in pieces`, `for x in pieces`, or dotted access); a dotted
`path` may only be `piece.id`, `piece.name`, `template.name` or `template.version`; `else` is
allowed only once, inside `if` (never in `for`), and `elif` not after `else`.

No whitespace-control dashes (`{%-`), no `set`, `macro`, `include`, `raw`, operators or literals
other than the piece-id string. Path names (`{{ var }}` in a path segment) allow only `output`.

**Engine.** `minijinja::Environment::empty()` with exactly two filters (`lower`, `upper`),
`UndefinedBehavior::Strict`, auto-escape off, `keep_trailing_newline(true)` (Nunjucks keeps the
final newline; MiniJinja drops it by default). Default `trim_blocks`/`lstrip_blocks` (off) match
Nunjucks defaults.

**Context** per piece instance: template vars, the instance's vars, `pieces` (unique piece ids
present after the operation, in lock order), `piece` (`{id, name}`), `template` (`{name, version}`).
All var values are strings.

**Paths.** For each file under `pieces/<id>/files/` (walkdir, sorted): render each path segment,
strip a trailing `.jinja`, prefix the rendered `target`. The result must be relative, use `/`, and
contain no empty, `.` or `..` segment; `.splice/lock.toml` and everything under `.splice/channels/` are reserved (other `.splice/` paths, e.g. `.splice/migrations/*`, stay renderable). Violations exit 3. Two
sources producing the same path (within or across instances) exit 3.

**Contents.** `.jinja` files render as UTF-8 with `\n` endings (input `\r\n` is normalised).
Other files copy byte for byte. A rendered file that is empty or only whitespace is not written.

**Conformance corpus.** `tests/fixtures/subset/` holds `<case>.jinja` + `<case>.ctx.json` +
`<case>.out` triples covering every construct. hull runs the same corpus against Nunjucks; any
difference is a contract bug.

## 7. Ownership (contract §5)

Classification takes the file's path **relative to `files/`, after rendering and without
`.jinja`, before the `target` prefix**, and matches `owned`, then `shared`, then `seed` globs
(the contract's "first match wins" applied in that order); no match → `owned`. Globs use `globset`
with `literal_separator(true)` (`*` stays in one segment, `**` crosses them). `[merge]` keys
match the same path. A non-UTF-8 file matched as `shared` is a template error (binary files must
be `owned` or `seed`).

Per-file record: `RenderedFile { path, bytes, class, strategy, piece_id, piece_name, glob }`.

## 8. Operations

### 8.1 `new`

1. Resolve source + ref, fetch tree, load + validate manifests, check `min_splice`.
2. `dest` must be empty apart from `.git` unless `--force` (then existing owned/shared files are
   overwritten and seeds kept). A lock already present is exit 2.
3. Instances: `base` (name = `base`) then each `--piece` in order. Check `requires` against the
   final set, `multiple`, unique names.
4. Resolve vars (flags → prompt → default), validate `pattern`.
5. Render, write every file (seeds only when absent), `git init` if `dest` is not inside a git
   work tree, run `[[post]]` with `when` ∋ `new` (all instantiated pieces, identical commands once),
   then write the lock (§8.6) as the very last step. A failed post command (exit 3) therefore
   leaves the files but no lock; `new --force` redoes it.

Instance names: a `multiple` piece takes its name from `--piece ID:NAME`, else from its `name`
var if it declares one, else the piece id; a non-`multiple` piece's name is its id.

### 8.2 `add`

Uses the lock's template source and commit (no version change). Refuses a dirty tree or a
directory outside a git work tree (exit 2) unless `--allow-dirty`, as `update` (§8.3 step 1).
Validates `multiple`, unique name, `requires`. Because `pieces` changes, every instance is
re-rendered: base = render of the lock state, theirs = render with the new instance. The result
goes through the same planner as `update` (§8.4, no `[[remove]]` and no migration steps), so other
pieces' `{% if "x" in pieces %}` blocks update with full merge rules (an edited shared file is
merged). Unlike `update`, `add` never writes conflict markers: on owned drift or any conflict
(without `--force`) it writes nothing, not even the lock, and exits 1 listing the files as
`conflict` changes. With `--force`, owned drift is replaced and a conflicting file is kept as it
is (`skip`, "kept, merge by hand; the lock records the template's version"). Post commands
(`when` ∋ `add`, of every piece present after the add) run before the lock is written: a failing
one leaves the written files without a lock update (exit 3, with the restore hint), and re-running
the same `add` is safe because a file that already equals the new render is skipped.

### 8.3 `update`

1. Read the lock. Refuse a dirty tree (`git status --porcelain` not empty) with exit 2 unless
   `--allow-dirty`; refuse outside a git work tree unless `--allow-dirty`. The clean tree is the
   recovery story: every failure can be undone with `git checkout -- . && git clean -fd`, and
   every error message after files were written says so.
2. Target = `--to` or latest stable tag. `--to` < lock version → exit 2. No `--to` and no stable
   tag newer than the lock (e.g. a prerelease lock, or a template with prerelease tags only) → exit 0, "up to date (no newer stable than
   X; use --to <tag> for a prerelease)", nothing written. Target = lock version → nothing to do
   (exit 0) unless the lock's commit differs (moved tag: message, then proceed).
3. Fetch both trees. Load the target manifest; check `format` and `min_splice` (exit 3).
4. Vars: keep lock values; prompt for vars new in the target; drop vars it no longer declares.
   `--data K=V` may set only template vars new in the target; `K` already in the lock → exit 2
   ("`K` is locked at `V`; update keeps lock values"), a piece var → exit 2 (piece vars come
   from `add` / `new --piece`), both before any write.
   A piece instance whose id no longer exists, or whose `requires` fails → exit 3 before writing.
5. **base** = render(lock commit, lock vars, lock pieces). **theirs** = render(target, new vars,
   same pieces). Both into memory (`RenderedTree`, a `BTreeMap<String, RenderedFile>`).
6. **Plan** (pure, in memory, §8.4). If any owned file drifted and `--force` is absent: write
   nothing, exit 1, list the drifted files.
7. **Apply:** write files, delete removals, run migration `[[step]]`s for versions A < v ≤ B in
   semver order (env `SPLICE_FROM_VERSION`, `SPLICE_TO_VERSION`), run `[[post]]` with
   `when` ∋ `update`, then write the lock atomically (§8.6) as the last step (as in §8.1 and §8.6).
   A failing step or post command exits 3 **before** the lock is written: the files are updated
   but the lock still records the old state, and re-running `update` is safe (after restoring,
   or at once with `--allow-dirty`: the re-run converges on the same result). The restore hint
   appears only once something was written (a refused target before the first write has none);
   a failed lock write says the lock still records the old version.
8. Conflicts: files carry diff3 markers, the lock still records the target, exit 1.

### 8.4 Planner rules

Inputs per path: base file B (render at lock), theirs T (target render), ours O (disk), lock entry
L. `sha(O) == L.sha256` means "unedited".

| Case | owned | shared | seed |
|---|---|---|---|
| B, T, O present | O unedited → replace with T (or skip if T = B); edited → drift (refuse, or replace with `--force`) | O = B → write T · T = B → keep O · else merge(strategy) → `merge` or `conflict` | skip |
| T only (new in target), O absent | create | create | create |
| T only, O present (user made it) | drift unless O = T | merge with base = empty | skip |
| B, T, O absent (user deleted) | drift (recreated with `--force`) | skip; keep deleted, lock records T | skip |
| B only (gone from target) or `[[remove]]` | delete if unedited, else keep + message | delete if O = B, else keep + message | never deleted |

"skip" for seeds means no change entry at all (nothing is reported for an existing seed).

`[[remove]]` paths are rendered with each instance of `piece` (template vars only when `piece` is
absent), and in both cases with `template` (name, target version) and `pieces`, as files are; they
are repo-root relative. A `[[remove]]` path that the target still renders is ignored
with a warning. Renames are a removal plus an addition; a migration step may move user content
between them (it runs after both).

### 8.5 Merges (contract §5)

`merge3(strategy, base, ours, theirs) -> Merged { text, conflicts: Vec<String> }`.

- **text:** `diffy::MergeOptions` with `ConflictStyle::Diff3`; diffy labels the markers
  `||||||| original` and `>>>>>>> theirs`, so splice relabels them, only on marker lines inside
  conflict hunks, to the contract's `||||||| base` and `>>>>>>> template`. CRLF in ours is
  normalised to LF for the merge and restored after.
- **json:** `serde_json` with `preserve_order`. **yaml:** `yaml_serde` (maintained fork of the
  archived `serde_yaml`, by the YAML organisation); multi-document files fall back to text.
  **toml:** `toml_edit::DocumentMut`, edited in place so comments and layout survive.
- Key-path rules exactly as contract §5: arrays are values, ours' key order kept, new keys
  appended to their parent, a key deleted on one side and changed on the other is a conflict.
- Fast paths before parsing: O = B → T verbatim; T = B or T = O → O verbatim.
- **json and yaml are text-first:** run the line-based text merge; if it is clean and its result
  still parses as the format, use it verbatim (layout, indentation and YAML comments survive).
  Only otherwise run the structured merge. On the structured path JSON is re-serialised with
  ours' indent (tabs or N spaces) and final newline; YAML is re-serialised by yaml_serde (two
  spaces), sequences re-indented to ours' style, and **comments may be lost** there.
- An empty or whitespace-only base counts as `{}` (json) or an empty mapping (yaml).
- YAML mapping keys must be strings: `1: a` is not coerced to `"1"`; such a file, like a
  multi-document one, is a parse error and falls back to text.
- A parse error in any side falls back to text.
- **Structured conflict → markers:** run the text merge; if it yields markers, use them; if it
  merges cleanly anyway, wrap the whole file in one diff3 block (ours / base / theirs), with the
  text merge's CRLF rule. Every conflict therefore leaves markers that `check` finds.
- **Text fallback guards:** a clean JSON text merge that introduces a duplicate key ours did not
  have, and a clean TOML text merge (a side did not parse) that no longer parses while ours does
  (e.g. a merge-introduced duplicate key), become one whole-file conflict block.

### 8.6 Lock writing (contract §6)

Built from the theirs tree: `[files]` lists every owned and shared path with the sha256 of the
**rendered** bytes; seeds are not listed. Written with `toml_edit` (stable order: files sorted)
to an exclusively created temp file in `.splice/` (never `lock.toml.tmp`), fsynced, then renamed over the lock and the directory fsynced. Always the
last step of `new`, `add` and `update`, after any post commands. `[[pieces]]` keeps instance order; `vars` is an inline
table.

### 8.7 `check`

Exit 1 if any of: an owned file in the lock whose disk sha differs (or is missing); a lock-listed
file containing a line that starts with `<<<<<<< ours`, `||||||| base`, `=======` inside such a
block, or `>>>>>>> template`; a newer stable tag than the lock version (skipped with `--offline`,
and a fetch failure is a message, not an error). Changes: drift → `update/owned`, markers →
`conflict/<the file's lock class>`. Every lock path is checked like a write target (relative, no
`..`, no symlink on the way) before any is read; an unsafe one is exit 3. Messages: `update available: 1.2.0 → 1.3.0`.

### 8.8 `diff`

Runs steps 2–6 of `update` without writing (no dirty-tree check, no `--data`), and prints a
unified diff (`diffy::create_patch`, labels `a/<path>` and `b/<path>`, `/dev/null` for a new
file) from the disk to the planned bytes per written file (a conflict shows its markers), plus
`delete <path>` per removal and `Binary file <path> differs` for non-UTF-8 content. Notes and
owned drift go to stderr as messages. `--json` returns the planned `changes` (equal to
`update --json`'s on the same repo when `update` would not refuse) and the messages. Drift and conflicts are reported, not
failures: exit 0; a downgrade `--to` is exit 2 as on `update`.

### 8.9 `explain`

Re-renders the lock state from the cached tree (offline after the first fetch) and prints
`<path>  <class>  <piece-id>/<instance-name>  glob=<glob|default>  merge=<strategy>` on stdout
(nothing on stderr); `--json`: one change `{path, action: "skip", class}` and messages
`piece=…`, `glob=…`, `merge=…`. An
unmanaged path → ok, no changes, message `unmanaged`. `merge=` is the `[merge]` strategy for
shared files and `none` for owned and seed files. Paths are normalised first (`./` prefix, `\`
separators); absolute paths and `..` are a usage error.

## 9. JSON output (contract §8)

`Report { ok, command, changes: [Change { path, action, class }], messages }`, serialised with
exactly those keys. `ok` is false for exit codes ≠ 0. Changes are sorted by path; unchanged files
are omitted. An exit 3 after files were written (a failed write, migration step, post command or
lock write in `update` or `add`) still lists the changes applied so far; the last message is the
error with the restore hint.

## 10. Decisions on gaps in the contract

These decisions are now part of the contract itself (`docs/template-format.md` §9). If this section and §9 ever differ, §9 wins.

These are splice's reading of points the contract leaves open. They go to hull for agreement and
into the contract at its next `format` bump.

1. Globs and `[merge]` keys match the rendered path relative to `files/`, minus `.jinja`, before
   `target`; `*` does not cross `/`.
2. Dotted access (`piece.name`, `template.version`, loop var) counts as `{{ var }}`.
3. `if` conditions are only `var` or `"id" in pieces`; no `not`, no comparisons. All vars are
   strings (no boolean type); `{% if pieces %}` is rejected (JS treats `[]` as truthy).
4. No whitespace control and no auto-escaping; final newline kept.
5. `pieces` is the unique id list. A non-`multiple` instance's name is its id.
6. `[[remove]].path` is repo-root relative. Removed `shared` files follow the owned rule (deleted
   only when unedited).
7. `update` writes a seed file that is new in the target and absent on disk; it never modifies one.
8. Exit 1 also covers `update`/`add` conflicts and owned-drift refusal; failed steps/post exit 3.
9. The lock has no owning piece per file, so `explain` re-renders; JSON has no extra fields, so
   `diff --json` carries no patch text.
10. Subset gate (task 5 review): keyword literals (`true false none not and or in is`) and
    `loop self range namespace` are never variables; bare `pieces`/`piece`/`template` are rejected;
    dotted access is limited to `piece.id|name`, `template.name|version`; `else` only inside `if`.
    These go to hull and the contract at the next `format` bump.

11. Merge formatting (task 3 review): json/yaml merge text-first (§8.5) so the common case of
    disjoint edits never rewrites the user's file; hull's `pnpm-workspace.yaml` is the reference
    case (`tests/fixtures/merge/`).

12. Class changes between base and target (plan 02 task 4): owned or shared → seed leaves the
    file untouched and drops it from the lock (`skip` + message); seed → owned or shared creates
    the file when absent, keeps it when it equals the target, else is a `conflict` with the
    whole file in one diff3 block (never drift, also with `--force`); owned ↔ shared follows the
    target class's rules with the lock hash and base render as they are. A file no longer
    rendered, or in `[[remove]]`, is deleted when its hash matches the lock or it equals the base
    render; a `[[remove]]` path not in the lock is kept with a message. These go to hull and the
    contract at the next `format` bump.
13. `update` inputs (plan 02 task 6 review): `--data` on `update` sets only template vars new in
    the target; a locked var or a piece var is exit 2, so `--data` never silently loses to the
    lock. Without `--to`, a latest stable tag older than the lock (prerelease lock) is "up to
    date" (exit 0), not a downgrade; an explicit older `--to` stays exit 2. Errors carry the
    `git checkout -- . && git clean -fd` hint only after a write.

14. `add` on the full planner (plan 02 task 9): the same dirty-tree refusal (exit 2) and
    `--allow-dirty` as `update`; owned drift or a conflict writes nothing (exit 1, never markers);
    `--force` replaces owned drift and keeps conflicting files as they are. A file no longer
    rendered after the add follows §8.4 (deleted when unedited, else kept with a message that
    does not mention the lock, which no longer lists it). `diff` exits 0 on drift and conflicts.

## 11. Testing

- Unit tests next to each module; `cargo nextest run`.
- `tests/fixtures/template/v1`, `v2`, `v3` are template trees; `tests/common::template_repo(&[..])`
  turns them into a temp git repo with tags `v1.0.0`, `v2.0.0`, `v3.0.0` (v3 has
  `min_splice = "99.0.0"`). The fixture exercises every contract feature (§ table in plan 01).
- CLI tests with `assert_cmd`, `predicates`, `insta` snapshots of JSON reports and of rendered
  trees (path + sha + text). Tests set `SPLICE_CACHE_DIR` to a temp dir, `GIT_CONFIG_GLOBAL` to
  an empty file and author/committer env vars.
- Golden update test: `new` at v1, user edits on every class, `update` to v2, snapshot tree and
  report; plus one test per planner table cell.
- Conformance corpus (§6).

## 12. Distribution

- Release targets: `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl` (static),
  `x86_64-apple-darwin`, `aarch64-apple-darwin`, `x86_64-pc-windows-msvc`. No TLS or network
  crates at all (git does the fetching), so no OpenSSL anywhere.
- GitHub release per tag `v<semver>`: archives `splice-<target>.tar.gz` (`.zip` for Windows) and
  `SHA256SUMS`.
- npm: `@keelstackhq/splice` = a tiny JS launcher (`bin/splice.js`) with the five platform
  packages as exact-version `optionalDependencies`; each platform package declares `os`/`cpu`
  and ships the binary in `bin/`. No install scripts (pnpm 10 blocks them by default); the
  launcher resolves `@keelstackhq/splice-<platform>-<arch>/package.json`, spawns the binary with
  inherited stdio and forwards the exit code. Not cargo-dist's npm installer.

## 13. Update bot

Composite action `action/action.yml` (`uses: keelstackhq/splice/action@v1`). Inputs: `version`
(splice npm version, default `latest`), `to` (template version, default latest), `token`
(default `github.token`), `branch` (default `splice/update`), `labels` (default `splice`).
Steps: install via `npm i -g @keelstackhq/splice@<version>`, run `splice update --json --yes`,
and if anything changed commit to `branch`, `git push --force-with-lease`, then open or edit the
single PR with the `gh` CLI (body = summary table from the JSON; title gets `[conflicts]` on
exit 1). If the repo is already current and a bot PR is open, close it.

`gh` over `peter-evans/create-pull-request`: `gh` is preinstalled on GitHub runners, adds no
third-party action to pin and audit, and finding/editing one PR by head branch is two commands.

Docs note: PRs opened with `GITHUB_TOKEN` do not trigger `pull_request` workflows; pass a GitHub
App or fine-grained token as `token` to get CI on the bot PR.

Renovate alternative (documented, not shipped): a `customManagers` regex entry on
`.splice/lock.toml` (`version = "…"`, datasource `github-tags`) plus `postUpgradeTasks`
running `splice update --to {{newVersion}} --yes`. `postUpgradeTasks` only run on self-hosted
Renovate with `allowedCommands`; without them Renovate would only bump the version string,
which breaks the lock, so the Action is the default.

## 14. Agent use

`--json` everywhere, terse human output, `splice explain <path>` as cheap context. The repo ships
`skills/splice/SKILL.md` (Claude Code skill): always pass `--json`, read `changes`, resolve
conflicts by editing marker blocks then `splice check --json`, never hand-edit
`.splice/lock.toml`, use `explain` before editing a generated file (owned → change the template
instead).

## 15. Milestones

1. **Render + new** — sources, fetching, manifests, subset, render, classify, `new`, `add`
   (reduced planner), lock, `--json`, `explain`, fixture template.
2. **Update + check** — planner, merges, migrations, post, atomic apply, `update`, `check`,
   `diff`, `add` on the full planner, golden v1→v2 tests.
3. **Release + action** — CI, release workflow, npm launcher, update-bot Action, skill file,
   product README.
