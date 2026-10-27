# splice — contract rule 9.27 (empty re-render removes an owned file)

Spec: hull `docs/superpowers/specs/2026-10-27-secrets-and-ci-providers-design.md` S§6.1, S§10. Contract
change: `docs/template-format.md` gets rule 9.27 with the exact text of S§10, byte-identical to hull's
HU-8, in the same week (D-003). `format` stays 1 (D-005); templates using it set `min_splice = "0.3.0"`.

## SP-1: implement 9.27

Files: `docs/template-format.md`, `src/planner.rs` (on `add`/`update`, an owned/shared path that the
new render produces as empty → planned as `delete` under the §7 removal rule: only if the on-disk hash
equals the lock hash, else a `skip` change with a message), lock writer (drop the entry), `check`
(an owned file missing on disk whose render at the lock commit is empty is not drift), `tests/
empty_rerender.rs`, splice spec section for update/check.
- Tests first: template with `ci.yml.jinja` guarded by `{% if "off" in pieces %}{% else %}x{% endif %}`;
  `new` writes it; `add off` deletes it and drops it from the lock; same with a user edit → kept,
  reported, exit 0 with a message; `check` after the delete → 0; `--json` change action `delete`.
- Commit: `feat: rule 9.27 — empty re-render removes owned file`
- After merge: owner tags splice 0.3.0 (D-008: tag-only repos are tagged by the owner).
