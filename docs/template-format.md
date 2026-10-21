# keelstack template format v1 (shared contract: splice ⇄ hull)

This file is identical in `keelstackhq/splice` and `keelstackhq/hull` (`docs/template-format.md`).
Change it only in both repos, in the same week. Bump `format` only when a change makes a valid
template invalid or renders it differently; syntax a template opts into is gated by `min_splice`.

## 1. Template repository layout

```
splice.toml                 # template manifest (required, at the template root)
pieces/<piece-id>/          # one directory per piece
  piece.toml                # piece manifest (required)
  files/                    # file tree copied into the target repo, relative to the repo root
migrations/<semver>.toml    # optional, run by `splice update` when crossing that version
```

A template version is a git tag `v<semver>` on the template repo. `splice` reads the template from
a git URL (`gh:keelstackhq/hull` = `https://github.com/keelstackhq/hull`), optionally with a subdirectory
(`gh:keelstackhq/hull//template`), or from a local path (for tests and template development).

## 2. `splice.toml`

```toml
format = 1                       # this contract's version; splice refuses unknown formats
name = "hull"
min_splice = "0.1.0"             # oldest splice that can render this template
base = "base"                    # piece applied by every `splice new`, exactly once

[vars]                           # template-wide variables, asked once per repo
scope = { prompt = "npm scope without @", default = "acme", pattern = "^[a-z0-9-]+$" }
```

## 3. `piece.toml`

```toml
id = "preact-app"
description = "Preact client app built with Vite+"
multiple = true                  # may be added more than once (each instance has a name)
requires = ["base"]              # other pieces that must be present
target = "apps/{{ name }}"       # optional; prefix applied to every path under files/

[vars]
name = { prompt = "app name", default = "web", pattern = "^[a-z][a-z0-9-]*$" }

[files]                          # ownership by glob, relative to files/ (first match wins)
owned  = ["vite.config.ts", "tsconfig.json"]
shared = ["package.json"]
seed   = ["src/**", "index.html"]
# a file matching no glob is `owned`

[merge]                          # strategy for shared files (default: "text")
"package.json" = "json"          # json | yaml | toml | text

[[post]]                         # commands run after `new`/`add`/`update`, in the repo root
run = "pnpm install --no-frozen-lockfile"
when = ["new", "add", "update"]
```

## 4. Rendering

- A file whose name ends in `.jinja` is rendered and written without the suffix. Other files are
  copied byte for byte (binary-safe).
- Path segments may contain `{{ var }}`.
- **Allowed syntax (portable subset):** `{{ var }}`, `{{ var | lower }}`, `{{ var | upper }}`,
  `{% if var %}…{% elif … %}…{% else %}…{% endif %}`, `{% if "piece-id" in pieces %}`,
  `{% for p in pieces %}…{% endfor %}`, comments `{# … #}` and `{% raw %}…{% endraw %}` (§9.23). Nothing else: no macros, includes,
  custom filters or extensions. MiniJinja (splice) and Nunjucks (hull's dev renderer) must render
  this subset identically.
- Context: all `[vars]` of the template and of the piece instance, plus `pieces` (list of piece ids
  present in the repo after the operation), `piece` (`{id, name}` of the current instance) and
  `template` (`{name, version}`).
- Output uses `\n` line endings. A rendered `.jinja` file that ends up empty (only whitespace) is not
  written; a copied non-`.jinja` file is written even when empty.

## 5. Ownership classes

| Class | `new` / `add` | `update` | User edits |
|---|---|---|---|
| `owned` | written | replaced by the new render | `splice check` reports drift (exit 1); `update` refuses unless `--force` |
| `shared` | written | 3-way merge: base = old render, theirs = new render, ours = file on disk | allowed; merged |
| `seed` | written if absent | never touched | the user's file |

Structured merges (`json`, `yaml`, `toml`) merge by key path: a key changed only by the template
takes the template's value; a key changed only by the user keeps the user's value; a key changed by
both to different values is a conflict. Arrays are values (no element merge). Key order of `ours`
is kept; new keys from `theirs` are appended. `text` uses a line-based 3-way merge with diff3
markers (`<<<<<<< ours`, `||||||| base`, `=======`, `>>>>>>> template`).

## 6. Lock file `.splice/lock.toml` (in the target repo, committed)

```toml
format = 1
splice = "0.1.0"

[template]
source = "gh:keelstackhq/hull//template"
version = "1.2.0"
commit = "3f2c…"                 # full sha of the rendered tag

[vars]
scope = "acme"

[[pieces]]
id = "preact-app"
name = "web"
vars = { name = "web" }

[files]                          # sha256 of each owned/shared file as rendered (not as on disk)
"apps/web/vite.config.ts" = { class = "owned", sha256 = "…" }
"apps/web/package.json"   = { class = "shared", sha256 = "…" }
```

Only `splice` writes this file. Seed files are not listed.

## 7. Migrations `migrations/<semver>.toml`

```toml
[[step]]
run = "node .splice/migrations/1.2.0-rename-config.mjs"   # file shipped by the template as owned
description = "Rename vite.config.mts to vite.config.ts"

[[remove]]                        # files the new version no longer ships
path = "apps/{{ name }}/.eslintrc.json"
piece = "preact-app"
```

`update` from version A to B runs every migration with A < version ≤ B in semver order, after the
files are written and before `[[post]]` commands. Removed owned files are deleted only if their
on-disk hash equals the lock hash; otherwise they are reported.

## 8. Machine output

Every command accepts `--json` and then prints exactly one JSON object to stdout:
`{"ok": bool, "command": str, "changes": [{"path", "action": "create|update|merge|conflict|delete|skip", "class"}], "messages": [str]}`.
Exit codes: 0 ok, 1 drift/conflict/update available (`check`), 2 usage error, 3 template error.

## 9. Clarifications (binding, both repos)

1. **Glob matching.** `[files]` globs and `[merge]` keys match the rendered path relative to `files/`, without the `.jinja` suffix and before `target` is applied. `*` stays within one path segment; `**` crosses segments.
2. **Conditions.** `if` and `elif` may test only `var` (truthy = non-empty string) or `"piece-id" in pieces`. `not`, `and`, `or` and comparisons are not allowed. All vars are strings. `{% if pieces %}` is rejected.
3. **Dotted access.** `{{ piece.name }}`, `{{ piece.id }}`, `{{ template.name }}` and `{{ template.version }}` count as `{{ var }}`.
4. **Whitespace.** No whitespace control (`{%-`/`-%}`), no auto-escaping, and the file's final newline is kept exactly as in the source.
5. **`pieces` list.** `pieces` holds unique piece ids in the order they were added, even when a piece has several instances. A piece with `multiple = false` has `name` equal to its `id`.
6. **Removals.** `[[remove]].path` is relative to the repo root after rendering. Removed `shared` files follow the same rule as removed owned files: they are deleted only if unchanged since the lock, otherwise reported.
7. **New seed files on update.** A seed file that first appears in the target version is created only if it does not exist yet.
8. **Exit codes for every command.** 1 = conflict, drift refusal or available update; 2 = usage error; 3 = template error, failed migration step or failed post command.
9. **`explain` and `diff`.** The lock does not record which piece owns a file, so `explain` re-renders the template to find it. `diff --json` uses the §8 object only; patch text goes to stdout only without `--json`.
10. **Glob precedence.** A path is matched against the `owned` globs, then `shared`, then `seed`. The first match wins, and no match means `owned`.
11. **Re-render on `add`.** `add` re-renders every existing piece instance with the new `pieces` list and applies the §5 rules (owned replaced, shared merged, seed skipped). Root files that depend on other pieces therefore live in `base` and use `{% if "id" in pieces %}`.
12. **Undefined variables** are template errors (exit 3). Templates never rely on an undefined variable rendering as empty.
13. **File modes are not carried.** Every file is written as a regular, non-executable file, and templates invoke scripts through their interpreter (`bash path`, `node path`). `or` stays forbidden: use `if/elif` branches with identical bodies.
14. **Minimal CLI both repos rely on:** `splice new <source> [dest] --piece <id>[:<name>]… --data <key>=<value>… --yes --json`, `splice add <id> [--name <name>] --data <key>=<value>… --yes --json`, `splice update [--to <version>] --yes --json`, `splice check --json`.
15. **Required and default keys.** `[[post]].when` is required (no default). A piece's `description` defaults to `""` and a var's `prompt` defaults to the var name.
16. **Reserved names.** No template var or piece var may be named `pieces`, `piece` or `template`. Bare `{{ pieces }}`, `{{ piece }}` and `{{ template }}` are rejected (MiniJinja and Nunjucks print lists differently); use a dotted path (§9.3) or `for`.
17. **Identifiers.** A var, loop variable or dotted segment is ASCII `[a-z_][a-z0-9_]*`. It may not be `true`, `false`, `none`, `not`, `and`, `or`, `in`, `is`, `loop`, `self`, `range` or `namespace`.
18. **`else` and comments.** `{% else %}` is allowed only inside `if`, and no `elif` follows an `else`. A comment may not start or end with `-` or `+` (`{#-`, `-#}`).
19. **Whitespace inside tags** is optional space, tab, CR or LF, also around `|`: `{{var|lower}}` and `{{ var | lower }}` are both valid.
20. **No symlinks** anywhere in a template: not under `files/`, not `files/` itself, `splice.toml`, `piece.toml`, `pieces/`, `pieces/<id>`, `migrations/` or a migration file.
21. **Portable paths.** Each rendered path segment, and each piece id and instance name, is non-empty, not `.` or `..`, has no `/`, `\`, `:` or control characters (U+0000–U+001F, U+007F), does not end in `.` or a space, is not a Windows reserved name (`CON`, `PRN`, `AUX`, `NUL`, `COM1`…`COM9`, `LPT1`…`LPT9`, with or without an extension) and is not `.git` (any case). Within one source (the main template, or one channel), two rendered paths equal ignoring case, also as directory prefixes, collide (exit 3). This applies to literal file names as well as to var values.
22. **`[[remove]]` paths** render with `template.version` = the version being updated from; `pieces` and `template` are in scope with or without `piece`.
23. **Raw blocks.** `{% raw %}…{% endraw %}` outputs its body verbatim; tags inside are not parsed. Neither tag takes whitespace control (`-`, `+`). The body may not contain another `raw` tag in any spelling, and only `{% endraw %}` with optional space, tab, CR or LF inside the braces closes it (MiniJinja and Nunjucks disagree on nesting and on other spellings). Raw is allowed wherever text is, also inside `if`/`for` bodies, but never in a path segment. An unterminated `raw` or a stray `endraw` is exit 3. A template that uses raw sets `min_splice = "0.2.0"` or later.
24. **Stray `#}`.** A `#}` in plain text, outside a comment and outside a raw body, is exit 3 naming file and line (Nunjucks fails on it, MiniJinja prints it). Write it inside a raw block. This also applies to path segments.
25. **Template commands run with POSIX `sh -c`** on every OS: the `run` strings of `[[post]]` and `[[step]]` are POSIX shell. On Windows splice uses `sh` from `PATH`, else Git for Windows' `sh.exe`; no `sh` is exit 3.
26. **`[sections]` (channel templates only).** A piece may map a host Markdown path (relative to the piece `target`) to a fragment under `files/`: `[sections] "AGENTS.md" = "sections/rules.md"`. The fragment is not written as a file; splice keeps it between `<!-- splice:<channel>:begin … -->` and `<!-- splice:<channel>:end -->` markers in the host file. Only templates added with `splice channel add` may use it (`new` and `add` of a main template refuse it, exit 3), and they set `min_splice = "0.2.0"` or later.
