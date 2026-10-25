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
| `splice update [--to VERSION \| --ref R] [--data K=V]… [--allow-dirty] [--force] [--no-post] [--channel NAME [--only GLOB]… [--exclude GLOB]…] [--all]` | Move to a newer template version (default: latest stable tag), or with `--ref` to a descendant git ref (§4). `--channel` updates one channel, `--all` the main template and every channel. |
| `splice check [--offline] [--channel NAME]` | Exit 1 on owned drift, leftover conflict markers, or an available update. |
| `splice diff [--to VERSION \| --ref R] [--channel NAME]` | Show what `update` would change, without writing. |
| `splice explain <path>` | Print class, owning piece instance and matching glob of a path. |
| `splice channel add <source> [--as NAME] [--ref R \| --track BRANCH] [--pre] [--piece ID[:NAME]]… [--data K=V]… [--only GLOB]… [--exclude GLOB]… [--allow-dirty] [--force] [--no-post]` | Render a template source into the repo as a named channel (channels spec §3). |
| `splice channel remove <name> [--allow-dirty] [--force]` | Delete the channel's unedited files, strip its sections, then delete its lock last (channels spec §8). |

`add` also takes `--channel NAME`. `--only`/`--exclude` need `--channel`; `--all` excludes
`--channel`, `--to`, `--ref`, `--data`, `--only` and `--exclude`; `--ref` excludes `--track` (on `channel add`)
and `--to` (on `update` and `diff`). `--to` keeps
working without `--channel` (main template). All channel commands are implemented.

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
`-C`. A relative local source typed on the command line is resolved against the current
directory, not against `-C`; a relative one found in a lock resolves against the repo root and is written back as it was (§10 item 18).

All git work shells out to the `git` CLI (present wherever repos live), so the user's credentials,
proxies and SSH config just work. Every call sets `-c core.autocrlf=false -c core.eol=lf` and
`GIT_TERMINAL_PROMPT=0`.

- **Tags:** `git ls-remote --tags <url>`; keep `refs/tags/v<semver>` (peeled `^{}` sha wins);
  default = highest stable version; `--pre` allows pre-releases.
- **`--ref R`:** a tag, branch or sha. A non-`v<semver>` ref records version `0.0.0+g<sha7>`
  (used by hull before its first tag), an **untagged version**. `update --ref R` (and `diff
  --ref R`) resolves R the same way and moves the main template or a `--channel` to it.
- **Untagged versions are never ordered by semver** (semver orders build metadata as text, so
  `0.0.0+ga7ebb3f` < `0.0.0+gd25ab5e` says nothing about the commits). Whenever the lock version
  or the target is untagged, and for every `--ref`, "newer" means a git descendant:
  `git merge-base --is-ancestor <lock commit> <target commit>` (§10 item 21). A target that is
  not the lock commit or a descendant is exit 2 (`… is not a descendant of the locked commit
  <sha7>; splice does not downgrade (pass --force to update anyway)`); `--force` proceeds.
  Without `--to`/`--ref`, an untagged lock moves to the highest stable tag whose commit is a
  descendant of the lock commit (not the lock commit itself); none → up to date (exit 0, "up to
  date (no stable tag descends from the locked commit <sha7>; use --ref <branch|sha> for an
  untagged commit)"). `--to` with build metadata (`--to 0.0.0+ga7ebb3f`) is exit 2 and names
  `--ref`. Two tagged versions compare by semver as before, also from `--to`.
- **Cache:** `$SPLICE_CACHE_DIR` or `dirs::cache_dir()/splice`. One bare mirror per URL at
  `git/<sha256(url)[..16]>.git` (`git clone --mirror`, later `git fetch --tags --prune`), and one
  immutable extracted tree per commit at `trees/<sha256(url)[..16]>/<commit>/`, created with
  `git --git-dir <mirror> --work-tree <tmp> checkout <commit> -- .` using a temporary
  `GIT_INDEX_FILE`, then renamed into place. A present tree is reused with no network access.
- **Tracked channels** (`channel add --track BRANCH`, channels spec §3): `update --channel`
  resolves the branch head (tags ignored) and records `0.0.0+g<sha7>`. Tracked versions are never
  compared (semver orders build metadata as text): no downgrade check, and no migrations
  (`[[remove]]`, steps) run; files the head no longer renders go by the "B only" rule. `--to` on
  a tracked channel exits 2 (`channel N tracks BRANCH; re-add it with --ref to pin a version`),
  and so does `--ref`.
- **Subtree compare** (channels only): a target commit whose tree at the source's `//subdir`
  (`git rev-parse <commit>:<subdir>`, the commit's root tree without one) equals the lock
  commit's is up to date (exit 0, nothing written), for tagged and tracked channels. The mirror
  is fetched only when it lacks the commit.
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
- `[sections]` (a splice-only piece key, see the channels spec §6) maps a host path, relative to
  the piece's `target`, to a fragment file below `files/`. The host ends in `.md`, the fragment in
  `.md` or `.md.jinja`; the fragment must exist and is not rendered as a file. `new`, `add` and
  `update` of the main template refuse a piece that declares `[sections]` (exit 3): only
  `splice channel add` applies them.
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
raw      := "{%" ws "raw" ws "%}" any "{%" ws "endraw" ws "%}"
comment  := "{#" any "#}"
string   := '"' [a-z0-9-]+ '"'
```

Gate decisions (mirrored by hull's gate): `ident` is `[a-z_][a-z0-9_]*` and may not be one of
`true false none not and or in is loop self range namespace`; bare `pieces`, `piece` and `template`
are rejected as variables (use `"id" in pieces`, `for x in pieces`, or dotted access); a dotted
`path` may only be `piece.id`, `piece.name`, `template.name` or `template.version`; `else` is
allowed only once, inside `if` (never in `for`), and `elif` not after `else`.

No whitespace-control dashes (`{%-`), no `set`, `macro`, `include`, operators or literals
other than the piece-id string. `raw` outputs its body verbatim (§10 item 16); a stray `#}` is
rejected (item 17). Path names (`{{ var }}` in a path segment) allow only `output`.

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
2. Target = `--to`, `--ref` or latest stable tag. `--to` < lock version → exit 2; with an
   untagged lock or target, or with `--ref`, the check is git ancestry instead (§4, §10 item 21). No `--to` and no stable
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
   semver order (an untagged side selects by ancestry, §10 item 21; env `SPLICE_FROM_VERSION`, `SPLICE_TO_VERSION`), run `[[post]]` with
   `when` ∋ `update`, then write the lock atomically (§8.6) as the last step (as in §8.1 and §8.6).
   A failing step or post command exits 3 **before** the lock is written: the files are updated
   but the lock still records the old state, and re-running `update` is safe (after restoring,
   or at once with `--allow-dirty`: the re-run converges on the same result). The restore hint
   appears only once something was written (a refused target before the first write has none);
   a failed lock write says the lock still records the old version.
8. Conflicts: files carry diff3 markers, the lock still records the target, exit 1.

Channels (channels spec §5, §8): with channel locks in `.splice/channels/`, plain `update` also
runs the one-owner claim check, with the main target's owned/shared files checked last against
every channel's lock: a path a channel already holds exits 2 (`path X is owned by channel NAME;
…`) before anything is written. Without channel locks it behaves exactly as above. In a repo
with channel locks but no `.splice/lock.toml`, plain `update` exits 2 with `no .splice/lock.toml;
use --channel NAME or --all`. `update --channel NAME` runs steps 1–8 on that channel's lock (its
own vars and pieces only; `--to` allowed, `--data` exit 2, §10 item 15; a downgrade message
names the channel lock), with base filtered by the stored
`--only`/`--exclude` and theirs by the new ones (either flag replaces both stored lists; a filter
change alone also updates at the same version), the claim check, the section planner (section
drift and broken markers refuse like owned drift, broken markers even with `--force`), and only
that channel's lock written last; `.splice/lock.toml` is never touched. Tracked channels and
the subtree compare: §4.

`update --all` (G12, channels spec §8) runs step 1 once, then plans the main template (if
locked; latest target) and every channel (by name) in memory; each later plan sees the earlier
sources' planned writes, so a section host another source seeds in the same run is planned
against that seed (`created = false`) and two channels' blocks in one host both survive. Then,
in order: the claim check on all planned results (exit 2); drift or broken markers in any source
→ nothing written, exit 1, each line prefixed `main: ` / `channel NAME: `; every changed source's
files, sections, removals and steps (main first); the `[[post]]` commands (`when` ∋ `update`) of
all changed sources once, deduplicated by exact `run` (first occurrence), skipped on conflicts
or `--no-post`; every changed lock last, each atomically (§8.6). A failure before the locks
writes none: the message names the source (`main: `, `channel NAME: `) or the post command,
says "no lock was written, every lock still records its old version" and gives the
`git checkout -- . && git clean -fd` recovery. A failed lock write names
the locks written, the one not confirmed and those not written; a re-run converges. An
up-to-date main lock is not rewritten. The report has a `source=…` message per changed source,
`NAME: up to date …` per unchanged one. The conservative claim check and the steps-after-planning
rule: channels spec §8 (`update --all` rulings).

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
absent), and in both cases with `template` (name, and the version updated FROM, contract §9.22)
and `pieces`, as files are; they
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
block, or `>>>>>>> template`; a newer stable tag than the lock version (for an untagged lock: a stable tag whose commit
descends from the lock commit, §4; skipped with `--offline`,
and a fetch failure is a message, not an error). Changes: drift → `update/owned`, markers →
`conflict/<the file's lock class>`. Every lock path is checked like a write target (relative, no
`..`, no symlink on the way) before any is read; an unsafe one is exit 3. Messages: `update available: 1.2.0 → 1.3.0`.

With channels (channels spec): without `--channel`, `check` covers the main lock (if there is
one; a channel-only repo is fine) and then every channel lock by name; `--channel NAME` checks
only that channel (unknown name: exit 2; a corrupt channel lock: exit 3). Main changes and
messages are exactly as above, with no prefix. Per channel: owned drift and conflict markers as
for the main lock (messages prefixed `channel NAME: `, plus one summary line); section drift
(a locked block edited, deleted, missing its host or with broken markers, or a non-UTF-8 host)
as the change `HOST#NAME`, `update`, `owned` (broken markers also give a message that says how
to fix them). Section paths are always `HOST#NAME`. Drift follows each command family: `check` reports it as
the change `update`/`owned` (the gate fails); the `add` family (`add`, `add --channel`, `channel
add`) refuses with the change `conflict`/`owned` plus a message, nothing written; the `update`
family (`update`, `update --channel`, `update --all`) refuses with messages only and no changes,
as M2 `update`; `diff` names it in a message and exits 0. Update available (skipped with `--offline`; a lookup failure is a
message): a newer stable tag, or for a tracked channel a branch head, whose template subtree
differs from the locked commit; message `channel NAME: update available 1.0.0 → 2.0.0 (tag)` (an untagged channel lock: the
highest descendant stable tag, §4), or
`… <old7> → <new7> (BRANCH)` with short shas. Read-only, one JSON object.

### 8.8 `diff`

Runs steps 2–6 of `update` without writing (no dirty-tree check, no `--data`), and prints a
unified diff (`diffy::create_patch`, labels `a/<path>` and `b/<path>`, `/dev/null` for a new
file) from the disk to the planned bytes per written file (a conflict shows its markers), plus
`delete <path>` per removal and `Binary file <path> differs` for non-UTF-8 content. Notes and
owned drift go to stderr as messages. `--json` returns the planned `changes` (equal to
`update --json`'s on the same repo when `update` would not refuse) and the messages. Drift and conflicts are reported, not
failures: exit 0; a downgrade `--to` or a non-descendant `--ref` is exit 2 as on `update` (`diff`
has no `--force`).

`diff --channel NAME` runs the same on that channel's lock: file patches plus one patch per
section host (labels `a/HOST`, `b/HOST`; `delete HOST` when the host goes), nothing written. Its
`--json` changes (files, and `HOST#NAME` per section) equal those of `update --channel NAME
--json` when `update` would not refuse. Drift, section drift and broken markers are messages.

### 8.9 `explain`

Re-renders the lock state from the cached tree (offline after the first fetch) and prints
`<path>  <class>  <piece-id>/<instance-name>  glob=<glob|default>  merge=<strategy>` on stdout
(nothing on stderr); `--json`: one change `{path, action: "skip", class}` and messages
`piece=…`, `glob=…`, `merge=…`. An
unmanaged path → ok, no changes, message `unmanaged`. `merge=` is the `[merge]` strategy for
shared files and `none` for owned and seed files. Paths are normalised first (`./` prefix, `\`
separators); absolute paths and `..` are a usage error.

With channels, `explain` searches the main lock, then every channel by name (the first hit owns
the path; one owner per path) and adds the message `source=main` or `source=channel:NAME` (the
human line gains a trailing `source=` column; `source=` and `section=` appear as messages only
with `--json`, like `piece=`). A path that is a section host in a channel's `[sections]` also
gets `section=NAME` per channel (a human line `PATH  section=NAME`); if no source owns the file
it is `unmanaged` plus those lines. A channel-only repo works (no main lock needed).

## 9. JSON output (contract §8)

`Report { ok, command, changes: [Change { path, action, class }], messages }`, serialised with
exactly those keys. `ok` is false for exit codes ≠ 0. Changes are sorted by path; unchanged files
are omitted. An exit 3 after files were written (a failed write, migration step, post command or
lock write in `update` or `add`) still lists the changes applied so far; the last message is the
error with the restore hint.

## 10. Decisions on gaps in the contract

These are splice's reading of points the contract leaves open. Items 1–9 are contract §9.1–§9.9.
Contract §9 now also holds items 10 (§9.16–§9.18), 15 (only the `[sections]` key and its
`min_splice` rule, §9.26), 16 (raw, §9.23), 17
(stray `#}`, §9.24) and 19 (POSIX sh, §9.25), and the rules of item 20 (§9.15, §9.19–§9.22).
Items 11–14 and 18 stay splice's own decisions.

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
    Now contract §9.16–§9.18 (also: a comment may not start or end with `-` or `+`).

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

15. `[sections]` (channels plan 04 task 5): splice < 0.2.0 ignores the key (unknown keys warn) and
    would render the fragment as an owned file, so a template that uses `[sections]` sets
    `min_splice = "0.2.0"` (the crate version moves to 0.2.0 in plan 04 task 21). Fragments may not
    be symlinks or sit below a symlinked directory. `add` checks the locked pieces and the asked
    piece for `[sections]` (exit 3) before it reports an unknown piece (exit 2). `update --channel`
    takes no `--data` (exit 2): a channel's vars are set at `channel add`; vars new in a later
    channel version take their defaults or prompts. Only the `[sections]` key and its `min_splice`
    rule are contract §9.26 (an older `min_splice` is exit 3 at load); G1–G17 below stay
    splice-only.

    Channel rulings (G1–G17; the channels spec has the detail, `docs/superpowers/specs/
    2026-10-17-splice-channels-design.md`). Splice-only; offered to hull at the next `format`
    bump, not required (G1; this item, not "item 10", records it).
    - G1: the channel decision is recorded here as item 15.
    - G2: every source reserves `.splice/lock.toml` and `.splice/channels/**`; other `.splice/` paths stay renderable.
    - G3: `--track` follows the branch head, ignores tags, skips version comparisons and migrations; `--to` on a tracked channel is exit 2.
    - G4: `update --channel` is "up to date" when the target subtree id equals the lock commit's.
    - G5: a `[sections]` key is a host path (`.md`) relative to the piece `target`; the value is a fragment under `files/`, excluded from the tree.
    - G6: section changes are reported as `HOST#CHANNEL`, class `owned`, in every command.
    - G7: a block body is the fragment without leading and trailing blank lines, with one trailing newline, in the host's line ending; splice writes `begin`, a blank line, the body, a blank line, `end` (Markdown formatters add those blank lines). Reading (drift, adopt, update, check) takes the text between the markers, drops leading and trailing blank (whitespace-only) lines and ends it in one newline, so the earlier layout without blank lines and formatter-touched blocks compare equal; the lock hash is over that LF body. Known edge: a fragment that itself starts or ends with blank lines, locked before this rule, reads as drift in `check` (and is kept, not removed, when its section goes) until the next `update` relocks it; no shipped fragment does this.
    - G8: an unchanged fragment is skipped even if the block was edited; an existing block equal to the fragment is adopted, else drift.
    - G9: `--force` never fixes duplicate or unbalanced markers.
    - G10: a seed path another source locks is dropped from this source's plan.
    - G11: `channel add` follows `update` on shared conflicts (markers, lock written, exit 1); `add --channel` follows `add` (writes nothing).
    - G12: `update --all` writes all sources' files, runs deduplicated post commands once, then writes every lock last.
    - G13: a corrupt channel lock or a stray file in `.splice/channels/` is exit 3 naming the file; names starting with `.` are ignored.
    - G14: only `--only`/`--exclude` need `--channel`; `--to` works alone; `--all` excludes `--channel`, `--to`, `--data`, `--only`, `--exclude`.
    - G15: every template using `[sections]` (or raw) sets `min_splice = "0.2.0"` or later, fixtures included; an older `min_splice` is exit 3 at load (contract §9.23, §9.26). The crate is 0.2.0.
    - G16: `--only`/`--exclude` on `update --channel` replace both stored lists.
    - G17: in `update --all`, a section host's disk view includes an earlier source's planned seed write.
    - Drift reporting follows each command family (`add` family: `conflict`/`owned` changes; `update` family: messages; `check`: `update`/`owned`; `diff`: messages, exit 0).

16. `{% raw %}` ... `{% endraw %}` (now contract §9.23). The body is output
    verbatim (after §6 CRLF normalisation) and may be anything except the text of an `endraw`
    tag; `{{`, `{%` and `{#` inside are neither parsed nor validated, and the body's lines still
    count for error line numbers. No whitespace control (`-`, `+`) on either tag; `ws` inside the
    braces is space, tab, CR or LF. An unterminated raw is exit 3 naming the line where it
    opened; a stray `endraw` is exit 3. `raw` is allowed anywhere text is (also inside `if`/`for`
    bodies) and does not affect their balancing; it is not allowed in a path segment. MiniJinja
    ends a raw at the first `endraw` while Nunjucks nests raw blocks, and the two read dashes and
    exotic whitespace (vertical tab, NBSP) in the `endraw` tag differently, so the gate also
    rejects, inside a raw body, an inner `raw` tag (any spelling) and any `endraw` spelling other
    than the strict one. A template using raw sets `min_splice` to a splice with raw support
    (0.2.0 and later; a `.jinja` file with a raw block under an older `min_splice` is exit 3 at
    load); there is no `format` bump. Corpus cases: `raw-braces`,
    `raw-final-newline`, `raw-line-edges`, `raw-tags`.
17. A `#}` in plain text (outside a comment and outside a raw body) is exit 3 naming file:line
    (now contract §9.24): Nunjucks' lexer throws "unexpected end of comment" on it
    while MiniJinja prints it, so the gate rejects it. Inside a raw body it stays allowed. It also
    applies to path segments. **Behaviour change:** splice <= 0.2.0-pre accepted a literal `#}` in
    plain template text (for example the shell `${#}`); from this release it is exit 3. Write it
    inside `{% raw %}...{% endraw %}` (item 16).
18. Local sources (ported from PR #1, adapted). `new`, `add` and `channel add` already lock an
    absolute path (§4), so a relative `../tpl` given on the command line never reaches a lock. A
    relative local source found in a lock anyway (hand-written, or from another splice that stores
    it relative to the repo) resolves against the repo root (the `-C` dir), never the process cwd,
    and becomes absolute before any git call; this holds for the main lock and every channel
    lock. `update` and `add` (and `channel update`) write such a lock back with the relative spec
    it had; only `new` and `channel add` store an absolute path. A local path is canonicalised leniently
    (made absolute, `.` and `..` collapsed, symlinks followed before `..` for the longest existing ancestor, a Windows `\\?\` prefix removed), for
    the stored absolute path and for the cache key, so the key and the stored path do not change
    when the source folder is later deleted or renamed, nor with Windows short names. The cache
    key of an existing source is unchanged on Unix; on Windows the key used to hash a `\\?\C:\...`
    path, so the first fetch of a local source there after this change is a one-time refetch.
19. Template commands run with POSIX `sh -c` on every OS (ported from PR #1, adapted): the `run`
    strings of `[[post]]` and `[[step]]` are shell text written by template authors (hull writes
    POSIX), so `cmd /C` on Windows would break them. On Windows splice uses the first `sh.exe` or
    `sh` on `PATH`, else Git for Windows' `sh.exe`, found by walking up from `git --exec-path`
    to the first `bin\sh.exe` or `usr\bin\sh.exe`. No sh found is exit 3 and the message names
    the missing `sh`. Now contract §9.25.
20. Rules added with contract §9.15–§9.26 (splice 0.2.0). §9.15: a piece's `description` defaults
    to `""`, a var's `prompt` to the var name. §9.19: whitespace inside tags is only space, tab,
    CR and LF (NBSP, form feed, vertical tab are exit 3). §9.20: a symlinked `splice.toml`,
    `piece.toml`, `pieces/`, `pieces/<id>`, `migrations/` or migration file is exit 3 (a symlinked
    piece dir is never skipped). §9.21: every rendered segment, piece id and instance name is
    checked as hull's `segmentProblem` does (no `:` or control characters, no trailing `.` or
    space, no Windows reserved name with or without an extension, not `.git` in any case); two
    rendered paths of one source equal ignoring case, also as directory prefixes, are exit 3
    naming both (paths of different sources are not compared). An invalid instance name is exit 2
    when the user chose it (`--piece id:name`, `add --name`, `--data name=`) and exit 3 when it is
    the piece's own default. §9.22: `[[remove]]` paths render with `template.version` = the
    version updated from. The from-version template is only the merge base of `update` (and
    `channel update`): it loads and renders without the §9.15–§9.26 gates an older template may
    break (the raw/`[sections]` `min_splice` floor, keyword var names, piece-id and path segment
    rules, case collisions); symlinks stay rejected and the target stays strict. A locked
    instance name the target rejects is exit 3 naming the lock. Duplicates and case collisions
    count a `.jinja` that renders empty (as hull's buildPlan does).
21. Untagged versions and `update --ref` (canary finding F1: a repo rendered from hull at an
    untagged commit could not be updated). The contract defines versions as `v<semver>` tags
    only; `0.0.0+g<sha7>` is splice's own record of an untagged commit (§4), so ordering it is
    splice's decision, not a contract change. Rules, applied to the main lock and every channel
    lock alike:
    - Untagged = a version with build metadata. splice never compares an untagged version with
      `<`/`>`; it asks git (`merge-base --is-ancestor`) whether the target commit is the lock
      commit or a descendant of it. `--ref` always uses ancestry, also to a tag; `--to` between
      two tagged versions keeps the semver check (no `--force` override, item 13).
    - Not a descendant → exit 2 before any write; `update --force` proceeds with a message (the
      same flag that replaces owned drift; both are "I know, overwrite"). Same commit → up to date.
    - No `--to`/`--ref` from an untagged lock: the highest stable tag that descends from the lock
      commit, else up to date; `check` reports an update only for such a tag. So the update bot
      never proposes a tag older than the commit the repo was rendered from (F1: v0.2.2 from a
      lock at a later untagged commit).
    - `--to` with build metadata is exit 2 and names `--ref <sha7>`; `--ref` with `--to`, `--all`
      or a tracked channel is exit 2.
    - Migrations (contract: A < v ≤ B in semver order) when either side is untagged: migration
      `v` runs when tag `v<v>` exists, its commit is an ancestor of (or is) the target commit and
      is not an ancestor of (nor is) the lock commit; selected migrations still run in semver
      order. A migration without its tag does not run then. Tracked channels still run none (B5).
      Two tagged sides select by semver exactly as the contract says.
    - The lock shape is unchanged: `version = "0.0.0+g<sha7>"` and the full `commit`.

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
- GitHub release per tag `v<semver>` (workflow filter `v[0-9]+.[0-9]+.[0-9]+*`, so the Action's
  major tag `v1` never releases): archives `splice-<target>.tar.gz` (`.zip` for Windows), each
  with `splice[.exe]`, `LICENSE` and `README.md`, and `SHA256SUMS`. The tag, `Cargo.toml` and
  `npm/splice/package.json` (version and every platform pin) must agree, or the release stops
  before building. A version with `-` is a GitHub prerelease.
- Builds run in unprivileged jobs; only the release job (`contents: write`) and the npm job
  (`id-token: write`, environment `release`) hold privileges, and neither builds.
- npm: `@keelstackhq/splice` = a tiny JS launcher (`bin/splice.js`) with the five platform
  packages as exact-version `optionalDependencies`; each platform package declares `os`/`cpu`
  and ships the binary in `bin/`. No install scripts (pnpm blocks them by default). Not
  cargo-dist's npm installer. Every package carries `repository` (the GitHub repo) and
  `license`. Published with npm trusted publishing (OIDC) and provenance, no npm token; a
  version with `-` publishes under the dist-tag `next`, else `latest`; an already published
  version is skipped.
- Launcher: resolves `@keelstackhq/splice-<platform>-<arch>/package.json` (or uses
  `SPLICE_BINARY` when set), spawns the binary without a shell with inherited stdio, and forwards
  the exit code unchanged. A signal death is re-raised on the launcher. No prebuilt binary for the
  platform, or a binary that cannot start, is **exit 3** (environment error): the message names
  the `<platform>-<arch>` pair, the supported pairs, the release page and
  `cargo install --locked --git https://github.com/keelstackhq/splice keel-splice`.

## 13. Update bot

Composite action `action/action.yml` (`uses: keelstackhq/splice/action@v1`). Inputs: `version`
(splice npm version: `latest` (default), `next`, a semver, or `none` = use the `splice` already on
`PATH`), `to` (template version, default latest), `scope` (`main` | `all`; default `all` when
`.splice/channels/` holds a `*.toml`, else `main`), `token` (default `github.token`), `branch`
(default `splice/update`), `labels` (default `splice`), `working-directory` (default `.`).
Outputs: `result` and `pr-url`. Inputs reach the scripts only through environment variables and
are validated (`version`, `to` as semver, `branch` with `git check-ref-format`); `to` with
`scope: all` is an input error (`--all` excludes `--to`, §3).

Steps: install via `npm i -g @keelstackhq/splice@<version>` (unless `none`), run
`splice update --json --yes` (`scope: all`: `splice update --all --json --yes`), then decide the
`result` from the exit code and `git status`:

| Exit | Tree | `result` | Bot does |
|---|---|---|---|
| 0 | clean | `current` | close an open bot PR with a comment and delete its branch |
| 0 | changed | `updated` | commit to `branch`, push, open or edit the single PR |
| 1 | changed | `conflicts` | same, title prefixed `[conflicts]`, body lists the conflicted paths |
| 1 | clean | `drift` | fail with splice's messages (owned or section drift); no push |
| 2, 3 | — | — | fail with splice's messages |

The push fetches the remote bot branch first, then `git push --force-with-lease`; it
always authenticates with `token` (`gh auth setup-git`; the push resets `http.https://github.com/.extraheader`, so credentials left by checkout are ignored). `token` is also passed as `GH_TOKEN` to the update step, so it authenticates the template fetches of `splice update` (private `gh:` templates) too. The PR body
is a summary table from the JSON. For `scope: all` it has one table per source under a heading
(`main`, then each channel): rows carry no source, so a `HOST#NAME` section row belongs to channel
`NAME` and any other path to the source whose lock (current or at `HEAD`) lists it, else `main`.
The title is `chore: splice update to <version>`, without the version when there is no main lock.
`scope: main` in a channel-only repo is left to splice (exit 2, the job fails with its message).
Labels are added after the PR exists; a label failure is a warning.
The job needs `permissions: contents: write, pull-requests: write`. `action-selftest.yml`
(`workflow_dispatch`) runs the Action three times against a local bare origin with a stub `gh`
and `version: none`: `updated`, `updated` again (the bot branch is rewritten), then `drift`.

`gh` over `peter-evans/create-pull-request`: `gh` is preinstalled on GitHub runners, adds no
third-party action to pin and audit, and finding/editing one PR by head branch is two commands.

Docs note: PRs and pushes made with `GITHUB_TOKEN` do not trigger `pull_request` workflows; pass a
GitHub App or fine-grained token as `token` to get CI on the bot PR, and check out with
`persist-credentials: false`.

Renovate alternative (documented, not shipped): a `customManagers` regex entry on
`.splice/lock.toml` (`version = "…"`, datasource `github-tags`) plus `postUpgradeTasks`
running `splice update --to {{newVersion}} --yes`. `postUpgradeTasks` only run on self-hosted
Renovate with `allowedCommands`; without them Renovate would only bump the version string,
which breaks the lock, so the Action is the default.

## 14. Agent use

`--json` everywhere, terse human output, `splice explain <path>` as cheap context. The repo ships
`skills/splice/SKILL.md` (Claude Code skill), triggered by `.splice/lock.toml` or files in
`.splice/channels/` (channel-only repos have no main lock): always pass `--json`, read
`changes`, resolve conflicts by editing marker blocks then `splice check --json`, never hand-edit
`.splice/lock.toml` or a channel lock, use `explain` before editing a generated file (owned →
change the template instead; `source=channel:NAME` → change that channel), never edit inside a
channel's section block.

## 15. Milestones

1. **Render + new** — sources, fetching, manifests, subset, render, classify, `new`, `add`
   (reduced planner), lock, `--json`, `explain`, fixture template.
2. **Update + check** — planner, merges, migrations, post, atomic apply, `update`, `check`,
   `diff`, `add` on the full planner, golden v1→v2 tests.
3. **Release + action** (done) — CI, release workflow, npm launcher, update-bot Action, skill file,
   product README. The first release itself is Blocked on human steps (docs/PROGRESS.md).
4. **Channels** — extra templates next to the main one with their own locks, `[sections]`
   blocks in shared Markdown files, filters, `--track`, `--channel`/`--all` on `update`, `add`,
   `check`, `diff`, `explain`, `channel add|remove`, version 0.2.0 (channels spec; plan 04).
   The npm release waits on the Blocked first-release steps of milestone 3 (docs/PROGRESS.md).
