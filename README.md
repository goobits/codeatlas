# codeatlas

Generate a high-density public surface map of a codebase. Code Atlas scans TypeScript, Python, and Rust to produce a compact report of exported symbols, routes, imports, and unused public APIs.

## Quick start

```bash
npx @goobits/codeatlas --help
npx @goobits/codeatlas . --format json --out .codeatlas --suggest --imports
```

## Common usage

```bash
# Compact console output
npx @goobits/codeatlas . --format compact

# Mermaid diagram
npx @goobits/codeatlas . --format mermaid --out .codeatlas

# JSON for CI diffs or scripts
npx @goobits/codeatlas . --format json --out .codeatlas --suggest --imports
```

Output files (when using `--out`):
- tree: `atlas.tree`
- mermaid: `atlas.mmd`
- compact: `atlas.txt`
- json: `atlas.json`

## Options

```
  -l, --languages <LANGUAGES>      Comma-separated list: ts, py, rs (default: all)
  -f, --format <FORMAT>            tree | mermaid | compact | json
  -o, --out <OUT>                  Output directory (prints to stdout if omitted)
      --include-types              Include type definitions/interfaces
      --include-private            Include private members
      --entrypoints <ENTRYPOINTS>  Audit mode entrypoints (comma-separated)
      --suggest                    Include unused_public analysis
      --imports                    Include import graph
      --no-default-ignore          Disable default ignore list
```

## Audit mode (entrypoints)

Audit mode limits the report to exports reachable from entrypoints.

```bash
npx @goobits/codeatlas src/index.ts --entrypoints src/index.ts --format json --out .codeatlas
```

## Default ignores

By default, Code Atlas ignores common build and test artifacts:

```
tests
tests/fixtures
__tests__
__test__
__mocks__
target
node_modules
dist
build
coverage
.git
```

Use `--no-default-ignore` to disable these filters.

## Environment variables

```
CODEATLAS_BINARY_PATH   Use a specific binary (skips download/build)
CODEATLAS_CACHE_DIR     Override binary cache location
CODEATLAS_REPO          Override GitHub repo for release assets (default: goobits/codeatlas)
CODEATLAS_DEBUG=1       Enable wrapper debug logging
```

## Notes

- If no prebuilt binary is available, Code Atlas falls back to `cargo build --release`.
- The CLI is designed for CI-friendly output and API surface regression checks.
