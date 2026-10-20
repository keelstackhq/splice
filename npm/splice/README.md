# @keelstackhq/splice

Launcher for the `splice` binary. The prebuilt binary comes from the platform package that npm
installs for your OS and CPU (`@keelstackhq/splice-<os>-<cpu>`); there are no install scripts.

```bash
pnpm dlx @keelstackhq/splice new gh:keelstackhq/hull//template
```

Unsupported platform: download a release from https://github.com/keelstackhq/splice/releases or
run `cargo install --locked --git https://github.com/keelstackhq/splice keel-splice`.
Set `SPLICE_BINARY` to run a specific binary.

Docs: https://github.com/keelstackhq/splice
