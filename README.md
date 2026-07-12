# CodeAtlas

CodeAtlas maps the public API surface of TypeScript, JavaScript, Svelte, Python,
and Rust projects. It produces deterministic JSON, Markdown, dependency trees,
Mermaid diagrams, and unused-export audits from source code and package exports.

## Quick Start

```bash
npx @goobits/codeatlas scan .
npx @goobits/codeatlas audit .
npx @goobits/codeatlas docs . --out docs/API-Reference.md
npx @goobits/codeatlas docs . --format html --out docs/API-Reference.html
```

Use `CODEATLAS_BINARY_PATH` to run a locally built binary through the npm
wrapper:

```bash
CODEATLAS_BINARY_PATH=/path/to/codeatlas npx @goobits/codeatlas --version
```

## Commands

| Command | Purpose |
| --- | --- |
| `scan` | Show the public surface as a tree, Mermaid, or versioned JSON report |
| `audit` | Report public exports with no detected consumers |
| `ci` | Write a JSON baseline and fail on configured audit findings |
| `diff` | Compare the current public symbols with a JSON baseline |
| `map` | Generate a Mermaid dependency diagram |
| `docs` | Generate deterministic Markdown or searchable HTML from public exports and source docs |

Run `codeatlas <command> --help` for command-specific options.

## Configuration

CodeAtlas automatically reads `codeatlas.json` from the scanned directory, or
an explicit file passed through `--config`. Unknown fields fail validation so a
misspelled setting cannot silently weaken a check.

```json
{
	"root": "packages/example",
	"languages": ["ts"],
	"entrypoints": ["src/index.ts"],
	"docs": {
		"title": "Example API Reference",
		"output": "docs/API-Reference.md"
	}
}
```

Paths in the config are relative to the config file. Supported fields are:

- `root`: project or package root
- `languages`: any of `ts`, `py`, `rs`, or `svelte`
- `entrypoints`: public source entrypoints used by scans and audits
- `include_private`: include internal and private symbols
- `include_types`: include classes, interfaces, structs, and methods
- `no_default_ignore`: include normally ignored build and test directories
- `package_exports`: discover public entrypoints from `package.json` exports
- `docs.title` and `docs.output`: generated Markdown ownership

For TypeScript packages, `docs` discovers `package.json` exports when explicit
entrypoints are absent. Source JSDoc is the documentation owner; CodeAtlas does
not synthesize descriptions for undocumented symbols.

All scan commands use discovered package exports by default. When an export
points to generated declarations or JavaScript, CodeAtlas reads TypeScript
`rootDir` and `outDir` from `tsconfig.build.json`, `tsconfig.lib.json`, or
`tsconfig.json` and maps that target back to its maintained source file.

## Documentation Checks

Generate the canonical reference:

```bash
codeatlas docs --config codeatlas.json
```

Fail CI when the committed reference is missing or stale:

```bash
codeatlas docs --config codeatlas.json --check
```

The JSON report contains `schema_version`, `tool_version`, package metadata,
public export paths, source signatures, structured documentation, routes,
imports, and unused-public findings. New report fields are additive and older
baselines remain readable.

## Release Model

Git tags build native archives for Linux, macOS, and Windows and publish the npm
wrapper with provenance. If a matching archive is unavailable, the wrapper
builds from source with Cargo.

Required release secret:

```text
NPM_TOKEN
```

The software is distributed under the terms in `LICENSE`.
