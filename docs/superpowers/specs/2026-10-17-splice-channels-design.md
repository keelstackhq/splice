# splice channels ("rigging") — Design Spec

Date: 2026-10-17 · Status: approved · Builds on: `2026-10-14-splice-design.md` (the main
spec) and `docs/template-format.md` (format 1, unchanged by this spec) · Plan:
`docs/superpowers/plans/2026-10-17-splice-04-channels.md`

Change note (2026-10-17, plan pre-flight rulings): §8 `update --all` now writes all sources'
files, then runs the deduplicated post once, then writes every lock (main + channels) last, so a
failure leaves every old lock (main spec §8.3/§8.6 lock-last rule; was: each source's lock after
its own files). Also: only `.splice/lock.toml` and `.splice/channels/**` are reserved paths (§4);
only `--only`/`--exclude` need `--channel` (§3); tracked channels run no migrations (§3); main
`update`/`add` check claims when channels exist (§5); the `rigging` `claude` piece has no
`CLAUDE.md` section (§7); berth joins the decisions migration (§10).

## 1. Goal

Distribute and update a repo's **agent layer** (agent instructions, `CLAUDE.md`/`AGENTS.md`
sections, `.claude/agents`, skills, the org-decisions snapshot) across many repos and harnesses,
with the same lock, ownership classes and 3-way update that splice already gives templates.
"rigging" is the product name of this feature; inside splice it is a **channel**.

A channel is an ordinary format-1 template (`splice.toml` + `pieces/`) applied to an existing repo
**next to** the repo's main template (or alone), with its own lock file, its own version, its own
piece selection and an optional per-repo path filter.

Why: research (Theme 2, Sept 2026 threads) shows people sync skills and agent files across
20+ repos with scripts and symlinks; they ask for lockfile-style pinning ("hasn't moved without my
approval") and per-repo filtering. splice already has the lock, the classes and the merges.

First channels: `keelstackhq/decisions` (replaces `adopt/adopt.sh`) and the org-generic part of
hull's agent layer (§10).

### Non-goals

- New template syntax. Channels use the contract §4 subset unchanged.
- A registry, a skill marketplace, or symlink/global (`~/.claude`) installs. Repo files only.
- Cross-source rendering context: a channel never sees the main template's `pieces` or vars.
- Two sources owning one file. The only sharing is marker **sections** (§6).
- Settings merging into a file owned by another source (e.g. hull's `.claude/settings.json`).
- A separate `rigging` binary (decided: it lives in splice).
- Element-wise merges of hook arrays in `settings.json` (contract §5: arrays are values).

## 2. Concepts

| Term | Meaning |
|---|---|
| **source** | Something that writes files into the repo with a lock: the **main** template (`.splice/lock.toml`) or a **channel**. |
| **channel** | A template applied by `splice channel add`, named `^[a-z][a-z0-9-]*$` (default: the template's `name`), locked in `.splice/channels/<name>.toml`. |
| **harness** | An agent tool that reads repo files: Claude Code (`CLAUDE.md`, `.claude/`), Codex and others (`AGENTS.md`, `.agents/skills/`). Selected with **pieces**. |
| **filter** | Per-repo `--only`/`--exclude` globs on repo-root output paths, stored in the channel lock. |
| **section** | A marker-delimited block that a channel owns inside a Markdown file it does not own. |

A repo may have zero or one main template and any number of channels. A channel-only repo (no
`.splice/lock.toml`, e.g. splice, hoard, hoist) is valid.

## 3. CLI

New and extended commands (global flags `--json`, `--yes`, `-C` as in main spec §3):

| Command | Purpose |
|---|---|
| `splice channel add <source> [--as NAME] [--ref R \| --track BRANCH] [--pre] [--piece ID[:NAME]]… [--data K=V]… [--only G]… [--exclude G]… [--allow-dirty] [--force] [--no-post]` | Render the channel's base + listed pieces into the current repo, write `.splice/channels/NAME.toml`. |
| `splice channel remove <name> [--allow-dirty] [--force]` | Delete the channel's unedited owned/shared files, strip its sections, delete its lock last. |
| `splice add <piece-id> --channel NAME [--name N] [--data K=V]…` | Add a piece instance to a channel (e.g. a second harness). |
| `splice update --channel NAME [--to V] [--only G]… [--exclude G]…` | Update one channel. Any `--only`/`--exclude` given replaces **both** stored lists (`--only '**'` alone keeps everything). |
| `splice update --all [--allow-dirty] [--force] [--no-post]` | Update the main template (if any) to its latest version and every channel (§8). |
| `splice check [--channel NAME] [--offline]` | Without `--channel`: main template **and** every channel. |
| `splice diff --channel NAME [--to V]` · `splice explain <path>` | As in the main spec; `explain` searches every source. |

Unchanged: `splice update` without `--channel`/`--all` updates only the main template, exactly as
today (contract §9.14 keeps its meaning; `--to V` works alone for the main template).
`--only`/`--exclude` on `update` require `--channel`; `--all` conflicts with `--channel`, `--to`,
`--data`, `--only` and `--exclude` (exit 2). In a channel-only repo it exits 2 with
`no .splice/lock.toml; use --channel NAME or --all`.

`channel add` behaves like `update` with an empty base: it refuses a dirty tree (exit 2) unless
`--allow-dirty`, and plans every rendered file with base = absent (main spec §8.4 row "T only"):
owned → create, or drift if a different file is there (refused unless `--force`); shared → merge
with base = empty; seed → skip if present. A shared-file conflict gets diff3 markers and the lock
is still written (exit 1, as `update`). A failed post command exits 3 with no channel lock
written (the message says `no .splice/channels/NAME.toml was written`). `--track` with a branch
the source does not have is exit 3 naming the branch.

**Tracking a branch.** `--track BRANCH` pins the channel to the branch head at add time and makes
`update` move to the current head (main spec §4 records version `0.0.0+g<sha7>`; the lock's
`[channel].track` makes `update` resolve the target from the branch instead of needing `--to`).
This is for sources whose `main` is the release, like `keelstackhq/decisions` (D-002: only
owner-merged, settled decisions land there). `--ref` and `--track` are mutually exclusive (exit 2).
A tracked channel targets the branch head only (tags ignored; `--to` → exit 2), never compares
`0.0.0+g…` versions (no downgrade check; up to date = same subtree), and runs **no** migrations
(`[[remove]]`, steps): files the head no longer renders are removed by the "B only" rule.

**Bot.** The update Action (main spec §13) gains input `scope` (`main` | `all`, default `all` when
`.splice/channels/` holds a `*.toml` lock, else `main`) and runs `splice update --all --json --yes`; one PR.

## 4. Lock and contract delta

**Contract (`docs/template-format.md`): no change.** Contract §6 defines `.splice/lock.toml`;
splice keeps writing it byte-for-byte as today. Channel locks are a splice-only file that hull never
reads or writes, so D-003's joint-PR rule is not triggered. The decision is recorded as main spec
§10 item 15 ("channels are splice-only; contract §6 governs only `.splice/lock.toml`"; offered to
hull at the next `format` bump, not required).

Channel lock `.splice/channels/<name>.toml` = the contract §6 shape plus two optional tables:

```toml
format = 1
splice = "0.2.0"

[channel]                        # splice-only
name = "decisions"
track = "main"                   # absent = tag-based like the main template
only = []                        # repo-root globs; empty = everything
exclude = [".claude/skills/caveman/**"]

[template]
source = "gh:keelstackhq/decisions//channel"
version = "0.0.0+g3f2c1ab"
commit = "3f2c1ab…"

[vars]
[[pieces]]
id = "snapshot"
name = "snapshot"
vars = {}
[[pieces]]
id = "import-project"
name = "import-project"
vars = {}

[files]                          # as contract §6; filtered-out paths are not listed
"docs/agents/org-decisions.md" = { class = "owned", sha256 = "…" }

[sections]                       # splice-only; one entry per host file
"docs/agents/project.md" = { sha256 = "…", created = false }   # sha of the region body
```

A main lock with a `[channel]` or `[sections]` table, a channel lock without `[channel]` or whose
`name` differs from the file stem, or a file in `.splice/channels/` that is not `<valid-name>.toml`
is exit 3 ("corrupt lock", names the file). Names starting with `.` (atomic-write temp files left
by a crash) are ignored.
Each lock is written atomically (main spec §8.6) and only by its own source's operation.

**Template side.** A channel template is a format-1 template. The one addition is an optional
`[sections]` table in `piece.toml` (§6). splice already tolerates unknown manifest keys (main spec
§5); a template that uses `[sections]` sets `min_splice = "0.2.0"`, so older splices refuse it with
the normal message. `splice new`/`add` on the main template with a piece that declares
`[sections]` exits 3 ("sections are only applied by `splice channel add`"), so hull's template
can never depend on them.

**Path safety** (main spec §6): no source may render `.splice/lock.toml` (as today) or any path
under `.splice/channels/`. Other `.splice/` paths stay allowed (contract §7 ships
`.splice/migrations/*` as owned files).

## 5. Ownership and filters

Classes are exactly contract §5. Recommended classes for agent files in a channel:

| File | Class | Why |
|---|---|---|
| `.claude/agents/*.md`, `.claude/skills/<n>/**`, `.agents/skills/<n>/**`, `docs/agents/org-decisions.md` | owned | Org-wide; local edits are drift. A repo that wants its own variant excludes the path and keeps a local file. |
| `.claude/hooks/session-start.d/<channel>-*.sh` | owned | Run by hull's session-start hook; no `settings.json` edit needed. |
| Agent instructions in `AGENTS.md`/`CLAUDE.md` owned by nobody | section (§6) | Several sources and the user share these files. |
| Project notes (`docs/agents/project.md` body) | seed, or not shipped | The repo's own words. |
| `.claude/settings.json` | not shipped by channels | hull owns it; non-hull repos keep theirs. Hooks go to `session-start.d/` or are not needed (§10). |

**One owner per path.** Every operation loads every lock. A path rendered as owned or shared by a
source while another source's lock lists it in `[files]` is a **claim conflict**: nothing is
written, exit 2, message `path X is owned by <main|channel NAME>; exclude it with
--exclude X or remove it from one source`, naming the source that already holds the path (never the
one being changed). All sources are planned first (in memory) and checked
together, then applied. Seed vs seed: the first writer wins (seeds are written only if absent);
no error. A section host must not be in any lock's `[files]` (exit 2, same message). Main
`update` and `add` run the same check when `.splice/channels/` has locks (no channels: M2
behaviour). In `update --all`, a section host that an earlier source creates as a seed is planned
against that seed's bytes (`created = false`); every later plan sees the earlier sources' planned
writes (seeds, section host edits), so two channels' blocks in one host both survive.

**Filters.** `--only`/`--exclude` globs (globset, `literal_separator`, as main spec §7) match
**repo-root paths after `target`**, and section host paths. A path is kept if it matches some
`--only` glob (or `--only` is empty) and no `--exclude` glob. Filtered-out files are not written
and not locked. A filter matching nothing is a warning message, not an error. Changing the filter
on `update` plans newly excluded locked files as removals (deleted if unedited, else kept and
reported, main spec §8.4 last row) and newly included ones as creates.

**Selection by harness and topic uses pieces; fine-grained opt-out uses filters.** Pieces are the
template author's units (`claude`, `codex`, `skills`); filters are the repo owner's escape hatch
("everything except this skill"). `[[remove]]` in channel migrations works per channel (tagged
channels; tracked ones run no migrations, §3).

## 6. Sections (Markdown only)

A piece may declare sections:

```toml
[sections]                        # host path (before target) = fragment path under files/
"docs/agents/project.md" = "sections/org-decisions.md.jinja"
```

The fragment is rendered with the subset like any file (glob/class rules do not apply to it, and
it is not written as a file). Host and fragment must end in `.md` (template error otherwise). One
channel may declare at most one section per host (exit 3 if two of its pieces do).

In the host file the channel owns exactly the lines between

```
<!-- splice:<channel>:begin — managed by `splice update --channel <channel>`; edit the source -->

…rendered fragment…

<!-- splice:<channel>:end -->
```

Splice writes one blank line after the begin marker and one before the end marker, as Markdown
formatters (oxfmt, prettier) want. The block body is the text between the markers without
leading and trailing blank lines, ending in one newline; drift, adoption and the lock hash use
that body, so blocks without the blank lines (written before this layout) or with extra ones
next to the markers compare equal. Any other change inside the body is drift.

- **Insert** (`channel add`, or a section new in the target): host absent → created containing
  only the block; host present without markers → block appended after one blank line.
- **Update:** region body sha (LF-normalised) equals `[sections]` sha → replace the body;
  differs → drift (owned rules: refuse with exit 1 unless `--force`). Fragment unchanged → skip.
  Body already equals the new fragment (a rerun after a failed step, post or lock write) →
  record the new sha, nothing written, so the rerun converges without a restore.
- **Missing markers** for a locked section → drift (user deleted it; `--force` re-appends).
  Duplicate or unbalanced markers for one channel → exit 1 with the line numbers.
- **Removal** (section gone from target, filtered out, or `channel remove`): delete the block if
  unedited, else keep and report. A host left empty (whitespace only) by a removal is deleted
  only if splice created it (recorded as `created = true` in `[sections]`).
- **Body rules.** A fragment body is normalised to LF, loses leading and trailing blank lines and
  ends in a newline (empty stays empty);
  a CRLF host gets CRLF back. A body line that parses as a begin or end marker of any channel
  (`<!-- splice:<name>:begin …-->` or `<!-- splice:<name>:end -->`) is refused (exit 3, naming
  the channel and the line): it would corrupt this or another channel's block in the host.
  The begin marker matches exactly (`<!-- splice:<channel>:begin` then whitespace, ending in
  `-->`; `:beginX` does not), so a reworded tail still parses. A UTF-8 BOM before a marker on line
  1 is recognised and kept. Removing a block also drops the blank line insert added; a host that
  had no final newline gains one.
- An unchanged fragment is skipped even if the block was edited (the edit is reported only when
  the fragment changes). Existing markers whose body equals the fragment are adopted (recorded,
  nothing written). Broken markers are never fixed, not even with `--force`. Drift keeps the old
  `[sections]` entry in the lock.
- `--force` on existing markers that are not locked and whose body differs replaces the body and
  notes `HOST#CHANNEL: adopted differing block, replaced (--force)`. A gone section whose block
  was edited is kept, also with `--force`; a gone section whose markers are already missing needs
  no change. Both drop the `[sections]` entry: the block (if any) becomes unmanaged.
- A host that is not valid UTF-8 counts as broken markers only when the channel wants a section
  in it or has one locked there; otherwise the channel ignores it.
- Apply order per source: files, then section hosts (rewritten, or deleted when emptied and
  created by splice), then removals, migration steps, post, and the channel lock last. Every
  host is checked like a write target first (relative, no `..`, no symlink on the way), so a
  symlinked host is refused (exit 3) with nothing written. Fragments are re-checked as regular
  files below `files/` right before they are read.
- `check` reports a section as path `HOST#CHANNEL` with action `update`, class `owned` in
  `changes` (contract §8 shape; `#` cannot occur in a rendered path).
- `add --channel` refusing on section drift lists it in `changes` as path `HOST#CHANNEL`,
  action `conflict`, class `owned` (like owned-file drift, as main `add`) and writes nothing;
  `channel add` names drifted paths (files and `HOST#CHANNEL`) in `messages` only, as `update`.

Sections are deliberately owned, not 3-way merged: the user edits everything outside the block,
the source edits everything inside it. This is what `adopt.sh` step 3 does today, made lockable.

## 7. Rendering and multi-harness output

Rendering is main spec §6 unchanged, per source. A channel's context is its own `[vars]`,
`pieces`, `piece` and `template`; it has no access to the main template's.

**Harnesses are pieces, not a `harnesses` var.** The subset can only test `var` truthiness or
`"id" in pieces`, and lists are not vars, so piece presence is the natural switch:

```
rigging/ (example channel template)
  splice.toml        name = "rigging", base = "core", min_splice = "0.2.0"
  pieces/core/       harness-neutral: [sections] "AGENTS.md" = rules fragment
  pieces/claude/     .claude/agents/{explore,implementer,reviewer,ci-fixer}.md
                     (explore.md declares `name: Explore`, as hull ships it)
                     (no CLAUDE.md section: hull owns CLAUDE.md, which imports AGENTS.md)
  pieces/skills/     multiple = true; target = ".{{ dir }}/skills";
                     vars: dir = { pattern = "^(claude|agents)$" }; files/<skill>/SKILL.md
```

One skill source serves both harnesses through two instances. `--data` is one map per command
(plan 01 Task 8), so each instance is added by its own command:
`splice channel add <src> --piece skills:claude --data dir=claude`, then
`splice add skills --channel rigging --name agents --data dir=agents` →
`.claude/skills/<n>/SKILL.md` and `.agents/skills/<n>/SKILL.md`. No `/` inside a var, so path
segments stay portable (hull rule §9). Inside one file, `{% if "claude" in pieces %}` switches
harness-specific lines. Harness directory names are checked against each harness's documentation
when the channel template is written (they move; a channel release fixes them for every repo).

## 8. Update and check behaviour

- `update --channel N`: main spec §8.3 steps 1–8 applied to channel N's lock, plus the claim check
  across all sources and the section planner (§6). Target: `--to`, else latest stable tag, else
  the tracked branch head.
- `update --all`: plan main + every channel (sorted by name) in memory; run the claim check on the
  combined result; if any source has owned drift or broken markers without `--force`, write
  nothing (exit 1). Every write target of every source (files, section hosts, lock paths) is
  checked before the first write of any source; an unsafe one (symlink, `..`) exits 3 naming the
  source, with nothing written. Then, in that order: (1) every source's files, sections, removals and
  migration steps (main first, then channels by name); (2) the `[[post]]` commands of all sources
  **once**, deduplicated by exact `run` string, skipped if any source conflicted or `--no-post`;
  (3) every lock (main + channels), each written atomically, **last**. Post therefore sees the old
  locks, as in a single-source update. A failure in (1) or (2) writes no lock: every lock still
  records its old version; the message names the source (or the post command) and the
  `git checkout -- . && git clean -fd` recovery (main spec §8.3, §8.6). Conflicts: locks are
  still written (they record the targets), exit 1.
  Rulings: the claim check is conservative — each channel is checked against the earlier
  sources' planned claims plus the later sources' current claims, so moving a path between
  channels in one `--all` run exits 2 (do it with two `update --channel` runs, the releasing
  channel first). Each later plan sees the earlier sources' planned writes (files, seeds, section
  hosts, deletes), but migration steps run after all planning, so later plans do not see what
  steps change.
- `check`: union over sources of main spec §8.7 (owned drift, markers, update available) plus
  section drift. "Update available" for a channel: newer stable tag, or tracked head differs —
  and in both cases only if the channel's subdirectory tree differs
  (`git rev-parse <commit>:<subdir>`), so a template repo hosting a channel in a subdirectory
  does not report noise. Messages name the source: `channel decisions: update available
  3f2c1ab → 9e01d2c (main)`. Exit 1 if any source has a finding.
- `explain <path>`: checks every lock (and re-renders as main spec §8.9); adds message
  `source=main` or `source=channel:NAME` when the repo has channel locks (a repo without
  channels explains exactly as M2); a section host also lists `section=NAME` per block.
- `channel remove`: plan = every locked file and section as "B only". Unedited owned/shared files
  are deleted; seeds are never touched; a host splice created is deleted only when nothing else
  is left in it. Dirty tree → exit 2 unless `--allow-dirty`. An edited owned file or edited block
  is drift: exit 1, nothing written, drift in `messages` with empty `changes` (the `update`
  family); broken markers also refuse. `--force` keeps edited files and blocks and says so in
  `messages` (it never deletes them) and removes the rest. An edited shared file is kept with a
  message (no refusal). The channel lock is deleted last (and `.splice/channels/` when empty), so
  a failure leaves it and a re-run converges; the main lock is never touched.

JSON stays contract §8 (`ok`, `command`, `changes`, `messages`); `command` is `channel add`,
`channel remove`, `update`, etc. Source attribution travels in `messages`, never extra fields.

## 9. Errors and exit codes (contract §8, §9.8)

| Code | New cases |
|---|---|
| 1 | Section drift or broken markers; owned drift or conflicts in any source; channel update available (`check`). |
| 2 | Unknown channel name; name already used; bad name; `--ref` with `--track`; `--channel` and `--all` together; `update` without either in a channel-only repo; claim conflict between sources; section host owned by a source; dirty tree. |
| 3 | `[sections]` on the main template; non-`.md` host/fragment; two sections for one host in one channel; corrupt lock (`[channel]` in the main lock or missing in a channel lock); plus every existing template/env error per channel. |

Every message says how to fix it (e.g. the `--exclude` flag, the lock path, `--force`).

## 10. Migration

**keelstackhq/decisions → channel `decisions`** (replaces `adopt/adopt.sh`):

- Add `channel/` to the decisions repo: `splice.toml` (`name = "decisions"`, `base = "snapshot"`,
  `min_splice = "0.2.0"`), piece `snapshot` with `files/docs/agents/org-decisions.md` (owned; a
  copy of `DECISIONS.md` kept identical by `scripts/check-index.sh`, since templates may not
  contain symlinks), and pieces `import-project` (section in `docs/agents/project.md`, for
  hull-generated repos), `import-claude` (section in `CLAUDE.md`) and `import-agents` (section in
  `AGENTS.md`: "Read docs/agents/org-decisions.md").
- Freshness moves from the SessionStart hook to `--track main` + the update bot + `check`. The
  hook `sync-decisions.sh` is retired; `[[remove]]` cannot delete it (it was never locked), so the
  migration below deletes it by hand.
- Per adopted repo (hull, splice, hoard, example-app, keelstack, berth; one PR each; hoist only
  if the owner asks): delete
  `.claude/hooks/session-start.d/sync-decisions.sh` (or `.claude/hooks/sync-decisions.sh` and its
  `settings.json` entry), delete the hand-written "## Org decisions" block, then
  `splice channel add gh:keelstackhq/decisions//channel --track main --piece import-project`
  (or `import-claude`). The existing snapshot equals the render, so it adopts without `--force`.
  This needs splice 0.2.0 on npm (plan 03's release path) and is recorded as decision D-004 in
  keelstackhq/decisions.
- `adopt.sh` then prints a deprecation line pointing at `splice channel add`; it is deleted once
  every org repo has the channel.

**hull's agent layer → channel `rigging`.** Stack-specific files stay in hull's template because
they depend on hull's `pieces` (`CLAUDE.md` lists pieces; `session-start.sh` branches on `docs`;
skills `verify`, `add-piece`, `docs-lookup` run hull's commands; `settings.json` allows `pnpm`,
`turbo`). The org-generic files move to the channel: `.claude/agents/{explore,implementer,
reviewer,ci-fixer}.md` (hull's file names; `explore.md` has `name: Explore`) plus harness-neutral
rules as an `AGENTS.md` section. Hull-side changes (inside the contract, hull's own PR, starting
with a hull spec change the hull owner signs off; whether hull publishing `[sections]` needs a
contract note is the owner's D-003 call):

1. hull adds `rigging/` (a second template) and tags it with hull's tags; dogfood renders it with
   splice once splice 0.2 ships (hull's Nunjucks renderer never renders channels).
2. hull reclassifies `AGENTS.md` from owned to `seed` so channels can add sections to it.
3. A hull release removes the four agent files via `migrations/<v>.toml` `[[remove]]`; its README
   and generated `CLAUDE.md` tell users to run `splice update` then
   `splice channel add gh:keelstackhq/hull//rigging --piece claude`. A post or migration step
   cannot run it: steps run before the main lock is written, so the nested splice would see the
   old lock and report a claim conflict.

## 11. Testing

- **Fixtures:** `tests/fixtures/channel/v1`, `v2` (a channel template with `core` + section,
  `claude`, `skills` multiple with `target = ".{{ dir }}/skills"`, a `collide` piece that renders
  a path the main fixture owns, a v2 migration with `[[remove]]` and a changed fragment), plus
  `tests/fixtures/channel-b` (disjoint paths, the same post, a v2 step that can fail) for
  multi-channel tests. `template_repo` tags them; a `main` branch with two commits tests `--track`.
- **CLI tests** (`tests/cli_channel.rs`, assert_cmd + insta): add into a `new`-rendered repo and
  into a bare git repo; two harness instances from one skill source; `--only`/`--exclude` and a
  filter change on update; claim conflict → exit 2, nothing written; section insert, replace,
  drift (exit 1), deleted markers, `--force`, remove; `update --channel`, `update --all`
  (one source drifted → nothing written); `check` aggregation and message attribution; track
  branch advance → update available; subtree-unchanged → no update; `explain` source lines;
  `channel remove`; main `update` byte-identical behaviour with channels present.
- **Unit:** section parser (begin/end pairs, duplicates, CRLF hosts), filter matcher, lock
  round-trip with and without `[channel]`/`[sections]`, corrupt-lock detection.
- **Golden:** main v1 + channel v1, user edits in every class and inside/outside a section,
  `update --all` to v2; snapshot tree + report.
- **Conformance:** no new syntax, so the subset corpus is unchanged; add one corpus case for the
  section fragment (a `.md.jinja` with `{% if "claude" in pieces %}`) so hull's Nunjucks run
  covers it too.
- **Contract guard:** a test asserts `.splice/lock.toml` written by `new`/`update` in a repo with
  channels is byte-identical to one written without them.

## 12. Milestone

Milestone 4, after M2 (needs the planner, merges, atomic apply) and in parallel with M3; released
as splice 0.2.0. Plan tasks: lock types + `.splice/` path rule → source registry + claim check →
filters → `channel add`/`remove` → sections → `--channel`/`--all` on update/check/diff/explain →
`--track` + subtree compare → bot `scope` input → skill file + README → decisions and hull
channel templates (in their repos, after release).

## 13. Risks

- **Scope creep toward a package manager.** Mitigation: no dependency resolution between
  channels, no registry, no cross-source context; a channel is just a template with a lock.
- **Agents editing inside sections** create drift. The begin marker says how to change it, and
  `check` names the channel; the bundled splice skill tells agents to edit the source.
- **Harness path churn** (`.agents/skills`, Claude dirs). Handled once per channel release; pieces
  isolate it.
- **Tracking a branch weakens reproducibility.** The lock still pins the commit; `update` is the
  only thing that moves it, through a reviewed PR.
- **Private channel sources** need git credentials in the bot (same as private templates today).
- **Duplicated `DECISIONS.md`** in the decisions repo; guarded by `check-index.sh`.
- **Spec §10 item without hull's agreement**: harmless because hull never reads channel locks;
  revisit at the next contract `format` bump.
