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
		"canonical_url": "https://example.com/api/",
		"declaration_contract": true,
		"description": "Example package API reference.",
		"home_url": "https://example.com/",
		"include_dependency_types": true,
		"require_descriptions": true,
		"theme": {
			"light": {
				"accent": "#6c3aed",
				"accent_text": "#5b21b6"
			},
			"dark": {
				"accent": "#a78bfa",
				"accent_text": "#c4b5fd"
			}
		},
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
- `docs.include_dependency_types`: include local/workspace dependency contracts
  reachable from exported TypeScript signatures
- `docs.declaration_contract`: document the shipped `types` export instead of
  mapping declarations back to source
- `docs.require_descriptions`: fail generation when a public symbol or member
  lacks source documentation
- `docs.title`, `docs.description`, `docs.home_url`, and `docs.canonical_url`:
  generated reference metadata and navigation
- `docs.theme.light` and `docs.theme.dark`: optional semantic color overrides
  for `background`, `surface`, `surface_muted`, `text`, `muted`, `border`,
  `accent`, `accent_text`, `code_background`, `code_text`,
  `warning_background`, and `warning_text`
- `docs.output`: generated reference ownership

For TypeScript packages, `docs` discovers `package.json` exports when explicit
entrypoints are absent. Source JSDoc is the documentation owner; CodeAtlas does
not synthesize descriptions for undocumented symbols. Dependency types are
opt-in so a package can keep a narrow reference or generate a complete facade
reference without copying contracts into the facade source.

Use declaration-contract mode for release documentation and compatibility
baselines. It makes the reference follow the same declaration entrypoint that
package consumers resolve.

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

`diff` identifies symbols by package, kind, and exported qualified name rather
than source file. Additions are reported without failing; removals, signature
changes, and removed export paths exit non-zero as breaking changes.

The JSON report contains `schema_version`, `tool_version`, package metadata,
public export paths, source signatures, structured documentation, routes,
imports, and unused-public findings. New report fields are additive and older
baselines remain readable.

## Release Model

Git tags build native archives for Linux, macOS, and Windows and publish the npm
wrapper through npm trusted publishing with automatic provenance. If a matching
archive is unavailable, the wrapper builds from source with Cargo.

The `@goobits/codeatlas` npm package must trust the GitHub repository
`goobits/codeatlas` and workflow `release.yml` for `npm publish`. The workflow
uses GitHub OIDC and does not use a long-lived npm token.

The software is distributed under the terms in `LICENSE`.
