# xtask - Build Task Runner

**Purpose:** Custom cargo commands for lint, test, build, and running apps.

## COMMANDS

```bash
cargo xtask fmt [--check]
cargo xtask clippy [--locked]
cargo xtask lint [--check] [--locked]
cargo xtask test [--locked] [--lenient-desktop]
cargo xtask build [--release] [--locked]
cargo xtask release [--locked]
cargo xtask run-desktop [--release] [-- <args>]
cargo xtask run-cli -- <args>
cargo xtask coverage [--locked]
```

## CONVENTIONS

- Uses `clap` for CLI parsing
- `--locked` adds `--locked` to cargo commands
- `--lenient-desktop` ignores rca-desktop test failures (CI)
- Excludes `xtask` itself from workspace commands

## ANTI-PATTERNS

- **DON'T** bypass xtask - all dev workflows go through it
- **DON'T** add deps to xtask unless needed for build tooling
