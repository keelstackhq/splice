# splice — Design Spec

Date: 2026-10-14 · Status: draft for implementation · Contract: `docs/template-format.md` (format 1)

Names: GitHub org and npm scope `keelstackhq` (tools for bootstrapping projects), repo `keelstackhq/splice`, binary
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
| `splice diff [--to VERSION]` | Show what `update` would change, without writing (never prompts, no dirty-tree check). |
| `splice explain <path>` | Print class, owning piece instance and matching glob of a path. |

`--data K=V` sets a template var (`K`) or, for `new --piece`/`add`, a piece var; for `update` it
sets a var that is new in the target (template var, or piece var for every instance lacking it). `--piece ID:NAME`
names a `multiple` instance.

Human output is terse: one line per change (`create  apps/web/package.json  shared`), then one
summary line. Everything else goes to stderr. With `--json`, stdout holds exactly the one JSON
object; post-command and git output go to stderr.

### Exit codes (contract §8, applied per command)

| Code | When |
|---|---|
| 0 | success; `check` found nothing |
| 1 | `check`: drift, markers or update available · `update`/`add`: conflicts written, or refused because of owned drift |
| 2 | usage: bad flags, missing var value, dirty tree, downgrade, non-`multiple` piece added twice, duplicate instance name, no lock where one is needed |
| 3 | template or environment error: bad manifest, unknown `format`, `min_splice` too new, render error, unsafe path, git failure, failed migration step or post command |

clap parse errors still exit 2; when `--json` is in argv they are also reported as the JSON object
(`command` = the subcommand if known, else `""`). `--help` and `--version` are not command runs: they
print clap's plain text and exit 0, even with `--json`. On a failed human-mode run stdout still
holds the summary line (`0 changes`); the message goes to stderr.

## 4. Template sources and fetching

Source spec grammar: `gh:<owner>/<repo>[//<subdir>]` → `https://github.com/<owner>/<repo>.git`;
any `https://`, `ssh://`, `git@…` or `file://` URL with optional `//<subdir>` after the repo part;
or a local path (`./x`, `/x`, `../x`, `C:\x`) with optional `//<subdir>`. The lock stores the
spec as given (`gh:keelstackhq/hull//template`), except that `new` rewrites a relative local path to
be relative to the new repo's root (§10 item 28); a relative local path in a lock always resolves
from the repo root (the `-C` dir), never from the process cwd.

All git work shells out to the `git` CLI (present wherever repos live), so the user's credentials,
proxies and SSH config just work. Every call sets `-c core.autocrlf=false -c core.eol=lf` and
`GIT_TERMINAL_PROMPT=0`.

- **Tags:** `git ls-remote --tags <url>`; keep `refs/tags/v<semver>` (peeled `^{}` sha wins);
  default = highest stable version; `--pre` allows pre-releases.
- **`--ref R`:** a tag, branch or sha. A non-`v<semver>` ref records version `0.0.0+g<sha7>`
  (used by hull before its first tag); `update` from such a lock needs an explicit `--to`.
- **Cache:** `$SPLICE_CACHE_DIR` or `dirs::cache_dir()/splice` (the OS temp dir + `splice` when the platform
  has no cache dir). One bare mirror per URL at
  `git/<sha256(url)[..16]>.git` (`git clone --mirror`, later `git fetch --tags --prune`), and one
  immutable extracted tree per commit at `trees/<sha256(url)[..16]>/<commit>/`, created with
  `git --git-dir <mirror> --work-tree <tmp> checkout <commit> -- .` using a temporary
  `GIT_INDEX_FILE`, then renamed into place. A present tree is reused with no network access.
- **Local paths** must be git repos (tests create them) and are used directly as the "mirror". A
  path that is not a repo (`.git` or a bare repo layout) is exit 3 `template source <spec> not
  found (resolved to <abs path>)`; git is never run on it, so it cannot discover a parent repo.
- A missing commit (force-pushed template) is a template error (exit 3) that names the commit.

## 5. Manifests and validation (contract §2, §3)

Loaded with `toml` + serde (`deny_unknown_fields` off: unknown keys warn, so newer templates stay
readable). Validation errors exit 3 and name the file and key.

- `format` must be `1`. `min_splice` must be ≤ the running version, else exit 3 with
  "template needs splice ≥ X; run `pnpm dlx @keelstackhq/splice@latest …`".
- `base` must name an existing piece with `multiple = false`.
- Piece `id` equals its directory name. `requires` ids must exist. No cycles needed (requires is
  a presence check, not an order).
- Var names match `^[a-z_][a-z0-9_]*$`; a piece var may not reuse a template var name, and no var
  (template or piece) may use the reserved names `pieces`, `piece`, `template`. `pattern`
  compiles with the `regex` crate (keep patterns to the JS/Rust common subset: no look-around, no backrefs). Defaults are literal (not
  rendered) and must match `pattern`.
- `[merge]` values are `json|yaml|toml|text`; a `[merge]` key must also match a `shared` glob.
- `[[post]].when` values are `new|add|update`.
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

No whitespace-control dashes (`{%-`), no `set`, `macro`, `include`, operators or literals
other than the piece-id string. `raw` outputs its body verbatim (§10 item 29); a stray `#}` is rejected (item 30). Path names
(`{{ var }}` in a path segment) allow only `output`.

**Engine.** `minijinja::Environment::empty()` with exactly two filters (`lower`, `upper`),
`UndefinedBehavior::Strict`, auto-escape off, `keep_trailing_newline(true)` (Nunjucks keeps the
final newline; MiniJinja drops it by default). Default `trim_blocks`/`lstrip_blocks` (off) match
Nunjucks defaults.

**Context** per piece instance: template vars, the instance's vars, `pieces` (unique piece ids
present after the operation, in lock order), `piece` (`{id, name}`), `template` (`{name, version}`).
All var values are strings.

**Paths.** For each file under `pieces/<id>/files/` (walkdir, sorted): render each path segment,
strip a trailing `.jinja`, prefix the rendered `target`. The result must be relative, use `/`, and
contain no empty, `.` or `..` segment; `.splice/lock.toml` is reserved. Violations exit 3. Two
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
   overwritten and seeds kept). A lock already present is exit 2, as is a `dest` that is a regular
   file or a `.splice` that is not a directory. Under `--force` a file whose bytes are already
   identical is omitted from the report (§9 omits unchanged files); a changed one is `update`,
   a kept seed is `skip`.
3. Instances: `base` (name = its id) then each `--piece` in order. Check `requires` against the
   final set, `multiple`, unique names.
4. Resolve vars (flags → prompt → default), validate `pattern`.
5. Render, write every file, write the lock (§8.6), `git init` if `dest` is not inside a git work
   tree, run `[[post]]` with `when` ∋ `new`.

Instance names: a `multiple` piece takes its name from `--piece ID:NAME`, else from its `name`
var if it declares one, else the piece id; a non-`multiple` piece's name is its id (so the base
instance is named after the base piece). `--piece ID:NAME` is also stored as the instance's `name`
var when the piece declares one. A `--data K=V` for a piece var applies to every instance of that
piece. Usage errors (exit 2): an unknown `--data` key, `--piece` naming the base piece, a repeated
non-`multiple` piece, a name given to a non-`multiple` piece, a duplicate (id, name).

### 8.2 `add`

Uses the lock's template source and commit (no version change). Validates `multiple`, unique
name, `requires`. Because `pieces` changes, every instance is re-rendered: base = render of the
lock state, theirs = render with the new instance. The result goes through the same planner as
`update` (§8.3), so other pieces' `{% if "x" in pieces %}` blocks update with full merge rules.
`add` first applies the §8.3 step 1 check (dirty tree or no git work tree → exit 2 unless
`--allow-dirty`), after the no-lock check.

Details of `add` on the full planner (§8.4), each path in base or theirs:
- Invalid input is exit 2 with tree and lock untouched: no lock, dirty tree, unknown piece, the
  base piece, a `multiple = false` piece already present or given `--name`, a missing `requires`,
  a duplicate (id, name), an unsafe name, a `--data` key that is not a var of the added piece
  (template vars come from the lock and cannot be changed by `add`).
- Any owned drift (an owned file edited or deleted locally) refuses the add like
  `update` (§10 item 20): nothing is written, exit 1, the files are listed; `--force` replaces them. Shared files
  merge by their strategy (`merge`) or get diff3 markers (`conflict`, exit 1); `--force` never
  overwrites a shared file. Seeds are created only when absent and never modified. A file only in
  base is deleted when unedited, else kept (`skip`) with the message `<path>: … no longer managed
  by splice`, and the new lock does not list it (§8.4).
- Unchanged files are omitted from the report. Every path is checked for symlinks and directories
  first (exit 3, §10 item 19). The lock records the theirs render's hashes even when conflicts
  remain; a later `update` may treat a missing change as a user edit, so resolve the markers now.
- Conflicts: the other files and the lock are still written, exit 1, `[[post]]` is skipped (a
  message lists the commands). Otherwise `[[post]]` of the added piece with `when` ∋ `add` runs
  after the lock. There are no migration steps. Unknown lock keys are reported as messages.

### 8.3 `update`

1. Read the lock. Refuse a dirty tree (`git status --porcelain` not empty) with exit 2 unless
   `--allow-dirty`; refuse outside a git work tree unless `--allow-dirty`. The clean tree is the
   recovery story: every failure can be undone with `git checkout -- . && git clean -fd` run in the project
   directory, and every error message after files were written says so as `cd <dir> && git
   checkout -- . && git clean -fd` (in a monorepo subdir this restores only the project). That
   advice is given only when the tree was verified clean; with `--allow-dirty` on a dirty tree the
   message says git cannot separate the earlier changes (§10 item 25).
2. Target = `--to` or latest stable tag. Target < lock version → exit 2. Target = lock version →
   nothing to do (exit 0) unless the lock's commit differs (moved tag: message, then proceed).
3. Fetch both trees. Load the target manifest; check `format` and `min_splice` (exit 3).
4. Vars: keep lock values; prompt for vars new in the target; drop vars it no longer declares.
   A piece instance whose id no longer exists, or whose `requires` fails → exit 3 before writing.
5. **base** = render(lock commit, lock vars, lock pieces). **theirs** = render(target, new vars,
   same pieces). Both into memory (`RenderedTree`, a `BTreeMap<String, RenderedFile>`).
6. **Plan** (pure, in memory, §8.4). If any owned file drifted and `--force` is absent: write
   nothing, exit 1, list the drifted files.
7. **Apply:** write files, delete removals, run migration `[[step]]`s for versions A < v ≤ B in
   semver order (env `SPLICE_FROM_VERSION`, `SPLICE_TO_VERSION`), write the lock atomically
   (§8.6), then run `[[post]]` with `when` ∋ `update`. A failing step exits 3 **before** the lock
   is written; a failing post command exits 3 **after** it (the files and lock agree).
8. Conflicts: files carry diff3 markers, the lock still records the target, `[[post]]` is skipped (a message lists the commands),
   exit 1.

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

`[[remove]]` paths are rendered with each instance of `piece` (template vars, `pieces` and
`template` only when `piece` is absent; `template.version` is the lock version, §10 item 22) and
are repo-root relative. A `[[remove]]` path that the target still renders is ignored
with a warning. Renames are a removal plus an addition; a migration step may move user content
between them (it runs after both).

### 8.5 Merges (contract §5)

`merge3(strategy, base, ours, theirs) -> Merged { text, conflicts: Vec<String> }`.

- **text:** `diffy::MergeOptions` with `ConflictStyle::Diff3`; diffy labels the markers
  `||||||| original` and `>>>>>>> theirs`, so splice relabels them, only on marker lines inside
  conflict hunks, to the contract's `||||||| base` and `>>>>>>> template` (a marker length no
  input line uses tells diffy's delimiters from literal text). If ours contains any CRLF, all
  sides are normalised to LF for the merge and the result is all CRLF (an ours with mixed endings
  comes out all CRLF). With a missing final newline, markers start on their own line (diffy
  `IncompleteHunkStyle::Git`), so a conflicted file always ends with a newline, as in git.
- **json:** `serde_json` with `preserve_order`. **yaml:** `yaml_serde` (maintained fork of the
  archived `serde_yaml`, by the YAML organisation); multi-document files fall back to text.
  **toml:** `toml_edit::DocumentMut`, edited in place so comments and layout survive.
- Key-path rules exactly as contract §5: arrays are values, ours' key order kept, new keys
  appended to their parent, a key deleted on one side and changed on the other is a conflict.
- Limits of a real merge: JSON numbers keep their digits (`arbitrary_precision`) but exponents
  are normalised (`1e3` → `1e+3`). YAML expands aliases, drops anchors and a leading `---`,
  and re-flows sequences. TOML loses ours' comments on a replaced `[[x]]` array of tables.
- Fast paths before parsing: O = B → T verbatim; T = B → O verbatim. JSON/YAML are re-serialised
  only on a real merge (indent detected from ours: tabs or N spaces; trailing newline kept).
- A parse error in any side falls back to text.
- **Structured conflict → markers:** run the text merge; if it yields markers, use them; if it
  merges cleanly anyway, wrap the whole file in one diff3 block (ours / base / theirs). Every
  conflict therefore leaves markers that `check` finds.

### 8.6 Lock writing (contract §6)

Built from the theirs tree: `[files]` lists every owned and shared path with the sha256 of the
**rendered** bytes; seeds are not listed. Written with `toml_edit` (stable order: files sorted)
to `.splice/lock.toml.tmp` in the same directory, fsynced, then renamed over the lock. Always the
last write of `new`, `add` and `update`. `[[pieces]]` keeps instance order; `vars` is an inline
table.

### 8.7 `check`

Exit 1 if any of: an owned file in the lock whose disk sha differs (or is missing); a lock-listed
file containing a line that starts with `<<<<<<< ours`, `||||||| base`, `=======` inside such a
block, or `>>>>>>> template`; a newer stable tag than the lock version (skipped with `--offline`,
and a fetch failure is a message, not an error). Changes: drift → `update/owned`, markers →
`conflict/shared`. Messages: `update available: 1.2.0 → 1.3.0`.

`check` reads only the lock, the disk and (without `--offline`) the tag list: it never fetches or
checks out the template, so `--offline` makes no git call at all, and it never writes. Details in
§10 item 24.

### 8.8 `diff`

Runs steps 2–6 of `update` without writing, and prints a unified diff (`diffy::create_patch`)
per changed file (`--- a/<path>` / `+++ b/<path>`, `/dev/null` for a side that does not exist, line endings kept,
non-UTF-8 content as `Binary files … differ`;
a conflicted file shows its diff3 markers) plus one line per removal. `--json` returns the planned
`changes` and messages only, the same as `update --json` would (§10 item 9), except under owned
drift, where `update` refuses with no changes and `diff` still lists the planned ones. It writes no file, no
lock and nothing else, needs no clean tree, and exits 0 even when `update` would conflict or
refuse. It never prompts: a var that is new in the target takes its default, and one without a
default is a usage error (exit 2) that names the var and points to `splice update --data`. Owned
drift is listed in a message (`update would refuse`), the other planned changes are still shown.

### 8.9 `explain`

Re-renders the lock state from the cached tree (offline after the first fetch) and prints
`<path>  <class>  <piece-id>/<instance-name>  glob=<glob|default>  merge=<strategy>`; `--json`:
one change `{path, action: "skip", class}` and messages `piece=…`, `glob=…`, `merge=…`. An
unmanaged path → ok, no changes, message `unmanaged`.

## 9. JSON output (contract §8)

`Report { ok, command, changes: [Change { path, action, class }], messages }`, serialised with
exactly those keys. `ok` is false for exit codes ≠ 0. Changes are sorted by path; unchanged files
are omitted.

## 10. Decisions on gaps in the contract

Items 1–9 are now part of the contract itself (`docs/template-format.md` §9); if they and §9 ever
differ, §9 wins. Item 10 onward are pending hull agreement and not yet in the contract.

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
10. `[[post]].when` is required (no default; a missing `when` is exit 3); piece `description`
    defaults to "" and var `prompt` defaults to the var name.
11. A bare `{{ pieces }}`, `{{ piece }}` or `{{ template }}` is rejected (list/map output differs
    by renderer); use a dotted path or `for`.
12. `ident` is ASCII `[a-z_][a-z0-9_]*`, not `true|false|none|not|and|or|in|is`, and not the
    roots `loop|self|range|namespace` (also as a `for` variable).
13. `{% else %}` is valid only inside `if`; `elif` after `else` is rejected. A comment may not
    start or end with `-` or `+` (whitespace control).
14. `ws` inside tags is optional space, tab, CR or LF, also around `|`.
15. A symlink anywhere under a piece's `files/` (or `files/` itself), or as `splice.toml`,
    `piece.toml`, `pieces/`, a `pieces/<id>` dir, `migrations/` or a migration file, is a template
    error (exit 3). A `//subdir` that resolves through a symlink outside the extracted tree is
    rejected at fetch. Rendering never reads outside the template root.
16. A non-`.jinja` file that is empty is still written; only rendered files are dropped when blank.
17. Each rendered path segment must be non-empty, not `.`/`..`, free of `/`, `\`, `:` and ASCII
    control characters, not end in `.` or space, and not be a Windows reserved name (`CON`, `PRN`,
    `AUX`, `NUL`, `COM1-9`, `LPT1-9`, case-insensitive, with or without extension). The lock path
    is reserved case-insensitively. A segment equal to `.git` (any case) is rejected anywhere in a
    rendered path, including one produced by a var or a `target`. Two rendered paths that are equal
    ignoring case are a collision (exit 3, naming both paths and their pieces), because they would
    clash on case-insensitive filesystems. The same holds for directory prefixes (`Apps/x` and
    `apps/y`), and for a file whose name equals a directory's name ignoring case.
18. Piece ids and instance names follow item 17's segment rule.
19. `new --force` (and later `add`/`update`) refuses to write through a symlink below the
    destination or over a directory: template/environment error "unsafe path" (exit 3), checked
    for every file before anything is written.
20. `add` runs the full planner of `update` and therefore its §8.3 step 1 check: a dirty tree or
    a missing git work tree is exit 2 unless `--allow-dirty` (§3). Owned drift refuses the add
    (exit 1, nothing written) like `update`.
21. `explain <path>` normalises `\` to `/` and drops `./` and empty segments. An absolute path, a
    drive prefix or any `..` segment cannot name a rendered file, so it is `unmanaged` (ok, exit 0,
    message `unmanaged (not a repo-relative path)`), not a usage error. The human line replaces the
    generic change line on stdout, followed by the usual summary line (messages stay on stderr);
    lock warnings are messages, as in `add`.
22. `[[remove]]` paths render with `template.version` = the lock (base) version, since they name
    files rendered at that version; `pieces` and `template` are in scope with or without `piece`.
23. `update` details the spec leaves open: (a) the dirty-tree check comes first, so a repo at the
    target version still needs a clean tree (or `--allow-dirty`) before "up to date". "A git work
    tree" means `-C` is anywhere inside one (a project in a monorepo subdirectory is fine), and
    only changes at or below `-C` count (`git status --porcelain -- .`), so unrelated changes
    elsewhere in the repo do not block; (b) `--data K=V` may only name a var that is new in the
    target (template var, or piece var of an instance lacking it); any other key is exit 2, also
    when the repo is already up to date, since lock values are kept. A new piece var given with
    `--data` applies to every instance that lacks it (as in `new`); prompting asks once per
    instance; (c) `--to` accepts `1.2.3` or `v1.2.3`; (d) owned-drift refusal is a report with no
    changes, one message per file, and a last message `nothing was written`; (e) lock and template
    warnings are report messages, before any other message; (f) an unsafe path (symlink or
    directory in the way) found while reading the disk is exit 3 with `unsafe path: …`, before any
    write; (g) a moved tag (same version, other commit) proceeds with a message naming both
    commits; (h) a bare `update` (no `--to`) whose latest stable tag is older than the lock (a lock
    at a newer prerelease) is "up to date", exit 0; only an explicit `--to` below the lock is a
    downgrade (exit 2); (i) a lock from `new --ref` has version `0.0.0+g<sha>`: `update` says the
    lock came from a ref and suggests `--to`, instead of the moved-tag message.

24. `check` details the spec leaves open: (a) every lock-listed file is scanned for markers (`find_markers`: any `<<<<<<< ours`, and any
    stray `||||||| base` or `>>>>>>> template` line, count; `=======` only inside a block), owned
    and shared alike (a seed is not in the lock, so a stray or real marker block there never
    counts); the change's class is the lock's class; (b) an owned file is drift when missing or
    its sha differs, one change `update/owned` per path (an owned file with drift is not also
    scanned for markers); a missing shared file is not a finding; (c) the update test compares the
    newest stable tag with the lock version by semver precedence (a lock at `0.0.0+g<sha>`, from
    `--ref`, therefore reports "update available" whenever a stable tag exists); any error while listing tags
    (offline remote, deleted repo, bad source) is a message `could not look for updates: …`, not
    an error; (d) lock warnings are report messages, first; (e) no dirty-tree check (read-only);
    a symlink or directory on a lock path is exit 3 `unsafe path` as in `update`; a non-UTF-8
    file is not scanned for markers; (f) findings are exit 1 with `ok: false` and the full report.

25. The restore advice of §8.3 step 1 is printed only when the tree was verified clean before the
    update (also under `--allow-dirty`, when it happened to be clean), because `git checkout -- .
    && git clean -fd` would destroy uncommitted user work. On a tree that had uncommitted changes
    (or could not be checked) the message says so: git cannot separate them from the update's
    changes, inspect `git status` and `git diff` by hand. Outside a git work tree it says there is
    no git undo.
26. `update` and `add` refuse (exit 1, nothing written, report with no changes) when a lock-listed
    shared file that the plan would merge still contains conflict markers on disk (`find_markers`):
    message `resolve conflict markers in <path> first (splice check)`, then `nothing was written`.
    `--force` does not override it; `diff` reports it as a message.
27. Update bot and owned drift: `update --json` exiting 1 with no `conflict` change is owned drift
    (nothing written). The bot fails the job and prints splice's messages instead of opening a
    `[conflicts]` PR, which would have no diff. Only exit 1 with at least one `conflict` change
    gives a `[conflicts]` PR.
28. Local template sources. A relative local path in the lock resolves against the repo root (the
    `-C` dir), not the process cwd, so every command works from inside the generated repo. `new`
    therefore stores a relative local source rewritten relative to `dest` (`./tpl` from `$T` with
    dest `demo` is `../tpl`; `//subdir` is kept; a result that does not start with `..` gets a
    leading `./`, so it can never start with `-`). Both paths are canonicalised first (symlinks
    followed before `..`, as the filesystem does). When no relative path exists (Windows: another
    drive) the absolute canonical path is stored, without a `\\?\` prefix. "As given" in §4 means exactly that: absolute paths, URLs, `gh:` and scp
    specs are stored unchanged. A missing or non-repo local source is exit 3 `template source <spec>
    not found (resolved to <abs path>)`; the lock-relative path is checked before any git call.

29. `{% raw %}` … `{% endraw %}` (pending contract, like 10-28). The body is output verbatim (after §6 CRLF normalisation) and may
    be anything except the text of an `endraw` tag; `{{`, `{%` and `{#` inside are neither parsed
    nor validated, and the body's lines still count for error line numbers. No whitespace
    control (`-`, `+`) on either tag; `ws` inside the braces follows item 14. An unterminated raw
    is exit 3 naming the line where it opened; a stray `endraw` is exit 3. `raw` is a block
    statement allowed anywhere text is (also inside `if`/`for` bodies) and does not affect their
    balancing; it is not allowed in a path segment. Because MiniJinja ends a raw at the first
    `endraw` while Nunjucks nests raw blocks, and Nunjucks and MiniJinja read dashes and exotic
    whitespace (vertical tab, NBSP) in the `endraw` tag differently, the gate also rejects, inside a
    raw body, an inner `raw` tag (any spelling) and any `endraw` spelling other than the strict
    one; this deviates from "an inner `{% raw %}` is just text" so both renderers stay identical.
    Versioning: a template using raw needs a splice with raw support, so it sets `min_splice` to
    that version (0.1.0, the first release, already has it); there is no `format` bump.
30. A `#}` in plain text (outside a comment and outside a raw body) is exit 3 naming file:line
    (pending contract): Nunjucks' lexer throws "unexpected end of comment" on it while MiniJinja
    prints it, so the gate rejects it. Inside a raw body it stays allowed. It also applies to path
    segments.

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
(default `github.token`), `branch` (default `splice/update`), `labels` (default `splice`),
`working-directory` (default `.`). Outputs: `result` (`updated|conflicts|current`) and `pr-url`.
`version: local` skips the npm install and uses the `splice` on PATH (testing only; the self-test
uses it). The push and the `gh` calls use `token`: the push passes it as a one-command
`http.<server>/.extraheader` override (resetting the checkout credential, never writing
`.git/config`). `GITHUB_TOKEN` cannot push `.github/workflows/*` changes and its PRs trigger no CI,
so consumers pass a GitHub App or fine-grained PAT (contents, pull-requests, workflows write).
Open/refresh/close logic lives in `action/publish.sh` (tested with stub `gh`/`git`). Owned drift
(exit 1 with no conflicted file, nothing written) and exit 2/3 fail the job with splice's
messages; no PR is opened (decision 27 in §10). Steps: install via `npm i -g @keelstackhq/splice@<version>`, run `splice update --json --yes`,
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
