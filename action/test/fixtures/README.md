These JSON files are real `splice update --json --yes` output, captured by running the built
binary against `tests/fixtures/template` (v1 -> v2, piece `app:web`):

- `updated.json`  exit 0, clean update
- `conflict.json` exit 1, `.github/workflows/ci.yml` edited locally (ubuntu-latest -> ubuntu-22.04)
- `current.json`  exit 0, already up to date
- `drift.json`    exit 1, owned `apps/web/tsconfig.json` edited locally, `changes` empty
- `error.json`    exit 3, `--to 3.0.0` (fixture v3 needs a newer splice)
