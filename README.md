# CodeAtlas

CodeAtlas maps public APIs and analyzes source reachability in JavaScript,
TypeScript, Svelte, Python, and Rust projects. Reports preserve unresolved and
dynamic boundaries instead of calling code dead when the source graph is
incomplete.

## Quick Start

```bash
npx @goobits/codeatlas scan .
npx @goobits/codeatlas audit .
npx @goobits/codeatlas dead-code . --format json
npx @goobits/codeatlas context . --target src/main.rs
npx @goobits/codeatlas architecture compile architecture/root.atlas.yaml --source-root .
npx @goobits/codeatlas architecture providers architecture/root.atlas.yaml --source-root . --capability example.capability.context
npx @goobits/codeatlas docs . --out docs/API-Reference.md
npx @goobits/codeatlas docs . --format html --out docs/API-Reference.html
```

Use `CODEATLAS_BINARY_PATH` to run a locally built binary through the npm
wrapper:

```bash
CODEATLAS_BINARY_PATH=/path/to/codeatlas npx @goobits/codeatlas --version
```

When a matching release archive is unavailable, the wrapper builds from the
locked Rust dependency graph with one Cargo job. Set `CODEATLAS_CARGO_JOBS` to
a positive integer to allow more parallel build work.

## Commands

| Command        | Purpose                                                                                |
| -------------- | -------------------------------------------------------------------------------------- |
| `scan`         | Show the public surface as a tree, Mermaid, or versioned JSON report                   |
| `audit`        | Report public exports with no detected repository consumers                            |
| `dead-code`    | Classify source reachability, context-only code, and uncertain boundaries              |
| `context`      | Return a bounded source graph slice for exact files or symbols                         |
| `architecture` | Compile declarations, query provider approvals, observe bindings, and evaluate conformance |
| `ci`           | Write a JSON baseline and fail on configured audit findings                            |
| `diff`         | Compare the current public symbols with a JSON baseline                                |
| `map`          | Generate a Mermaid dependency diagram                                                  |
| `docs`         | Generate deterministic Markdown or searchable HTML from public exports and source docs |
| `http`         | Inventory, check, diff, and schema-fuzz OpenAPI contracts                              |

Run `codeatlas <command> --help` for command-specific options.

An explicit command is required. Repository-wide scan settings belong in
`codeatlas.json`; the former top-level flag interface has been removed.

## Declared Architecture

`architecture compile` accepts one or more root `ArchitectureModule` files,
resolves exact digest-pinned local imports inside `--source-root`, validates the
closed v0.1 vocabulary, and emits a deterministic normalized graph plus its
generated lockfile.

```bash
codeatlas architecture compile \
  architecture/root.atlas.yaml \
  --source-root . \
  --mode governing \
  --out .codeatlas/architecture.json \
  --lock-out .codeatlas/architecture.lock.json
```

`governing` includes active accepted declarations only. `review` also includes
proposed and unresolved declarations, but remains non-governing. Restricted
YAML declarations are the editable authority. Generated graphs, lockfiles,
observations, and conformance reports are evidence and must not be edited by
hand.

Query owner-approved provider classifications for one capability:

```bash
codeatlas architecture providers \
  architecture/root.atlas.yaml \
  --source-root . \
  --capability example.capability.context \
  --approval-scope organization
```

This read-only query compiles the governing graph and returns only explicit
approved classifications in the requested scope. It validates the provider's
declared capability and contract relationships. It does not evaluate runtime
eligibility, select a provider, or authorize invocation.

Generate implementation evidence for accepted package and crate bindings:

```bash
codeatlas architecture observe \
  architecture/root.atlas.yaml \
  --source-root . \
  --repository . \
  --repository-id example.repository.source \
  --observation-id example.observation.current \
  --source-commit 0123456789abcdef0123456789abcdef01234567 \
  --observed-at 2026-07-23T00:00:00Z \
  --out .codeatlas/architecture-observation.json
```

Compare the governing graph with that exact observation:

```bash
codeatlas architecture conform \
  architecture/root.atlas.yaml \
  --source-root . \
  --observation .codeatlas/architecture-observation.json \
  --conformance-id example.conformance.current \
  --as-of 2026-07-23T00:00:00Z \
  --check \
  --out .codeatlas/architecture-conformance.json
```

Policies are optional repeatable `--policy` inputs. They can authorize a
temporary deviation, but they never modify the governing graph. Callers supply
source commits and timestamps explicitly so generated evidence is
reproducible.

## Configuration

CodeAtlas automatically reads `codeatlas.json` from the scanned directory, or
an explicit file passed through `--config`. Unknown fields fail validation so a
misspelled setting cannot silently weaken a check.

```json
{
	"root": "packages/example",
	"languages": ["ts"],
	"docs": {
		"canonical_url": "https://example.com/api/",
		"declaration_contract": true,
		"description": "Example package API reference.",
		"home_url": "https://example.com/",
		"include_dependency_types": true,
		"public_name": "Example Browser SDK",
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
- `languages`: any of `js`, `ts`, `svelte`, `py`, or `rs`
- `entrypoints`: public source or declaration entrypoints used by scans and
  audits; omit this to follow discovered package exports
- `include_private`: include internal and private symbols
- `include_types`: include classes, interfaces, structs, and methods
- `no_default_ignore`: include normally ignored build and test directories
- `package_exports`: discover public entrypoints from `package.json` exports
- `projects`: source-reachability project roots with language-specific analysis
  and arbitrary named contexts
- `docs.include_dependency_types`: include local/workspace dependency contracts
  reachable from exported TypeScript signatures
- `docs.declaration_contract`: document the shipped `types` export instead of
  mapping declarations back to source. Referenced declarations that are needed
  to understand an exported signature appear separately as supporting types.
- `docs.public_name`: present one public product/module name instead of private
  implementation package paths in generated reference output
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
package consumers resolve. When narrowing documentation to one package subpath,
set `entrypoints` to that subpath's shipped declaration target, such as
`dist/hosted.d.ts`, rather than its source entrypoint.

Searchable HTML references link unambiguous type names to their definitions and
emit canonical, Open Graph, and Twitter metadata from the existing docs config.

All scan commands use discovered package exports by default. When an export
points to generated declarations or JavaScript, CodeAtlas reads TypeScript
`rootDir` and `outDir` from `tsconfig.build.json`, `tsconfig.lib.json`, or
`tsconfig.json` and maps that target back to its maintained source file.

## Source Reachability

Dead-code analysis uses named contexts whose roles determine whether reachable
code is used by production, tests, or tooling. Project names and context names
are arbitrary.

```json
{
	"projects": [
		{
			"id": "web",
			"root": ".",
			"languages": ["js", "ts", "svelte"],
			"contexts": {
				"application": {
					"role": "production",
					"entrypoints": ["src/index.ts", "src/App.svelte"]
				},
				"unit-tests": {
					"role": "test",
					"entrypoints": ["src/**/*.test.ts"]
				},
				"build-tools": {
					"role": "tooling",
					"entrypoints": ["scripts/**/*.ts"]
				}
			},
			"assume_reachable": ["src/runtime/plugins/**/*.ts"]
		}
	]
}
```

Each project may select `js`, `ts`, `svelte`, `py`, and `rs`. Rust projects can
also configure `rust.all_features` or an explicit `rust.features` list.

Svelte reachability reads both module and instance scripts, preserves their
source spans, and connects JavaScript, TypeScript, and Svelte modules through
static imports, literal dynamic imports, bounded template imports, and Vite
globs. Svelte component symbols remain conservative because markup-level
references are not yet a complete symbol graph. They are never emitted as
high-confidence unused-private findings.

The versioned dead-code report distinguishes unreachable private code,
test-only code, tooling-only code, unreferenced public APIs, unresolved
internal edges, and dynamic boundaries. Only high-confidence unreachable
files, unused private symbols, and unresolved internal imports can fail
`dead-code --check`. Public APIs without repository consumers remain advisory
because external consumers may exist.

The dead-code JSON contract is schema version 3. Project summaries include
per-language file counts, and each finding includes the exact named context
roots that support its classification. Scan, architecture, context, and
dead-code reports remain separate versioned contracts rather than one
all-purpose report.

Use `context` to retrieve only the nearby source facts needed for a task:

```bash
codeatlas context . \
  --target src/architecture/compiler.rs \
  --target src/architecture/compiler.rs#compile \
  --depth 2 \
  --max-nodes 128 \
  --out .codeatlas/context.json
```

Targets are exact source graph node IDs, repository-relative paths, or
`path#symbol` selectors. The result includes dependencies, dependents,
visibility, evidence, analysis boundaries, and an explicit truncation status.
The source context graph remains separate from the declared architecture graph
because the two graphs have different authority and semantics.

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
public export paths, source signatures, structured documentation, imports, and
unused-public findings. New report fields are additive and older baselines
remain readable.

## HTTP Contracts

HTTP contracts are a separate, versioned domain rather than part of the public
symbol scan. Configure one or more OpenAPI 3.0 or 3.1 documents and their
domain-owned source roots:

```json
{
  "http": {
    "contracts": [
      {
        "id": "public-api",
        "openapi": {
          "kind": "target",
          "target": "public-local"
        },
        "source_roots": ["src/http"],
        "source_include_paths": ["/v1/**", "/health"],
        "source_exclude_paths": ["/internal/**"],
        "source_complete": true
      }
    ],
    "fuzz": {
      "targets": [
        {
          "id": "public-local",
          "contract": "public-api",
          "base_url": "http://127.0.0.1:3443",
          "environment": {
            "NODE_ENV": "test",
            "PORT": "3443"
          },
          "headers": [
            {
              "name": "Authorization",
              "value_env": "LOCAL_API_TOKEN"
            }
          ],
          "report_dir": ".codeatlas/reports",
          "server": {
            "command": "node",
            "args": ["src/test-server.js"],
            "cwd": "."
          },
          "positive_coverage": {
            "max_operations_without_success": 0,
            "max_authentication_rejection_only_operations": 0
          },
          "suppress_health_checks": ["filter_too_much"]
        }
      ]
    }
  }
}
```

`source_complete` is an explicit project assertion. Leave it false when routes
may be registered dynamically; CodeAtlas will preserve that uncertainty rather
than turn incomplete static detection into false CI failures.
Path filters partition mixed public/internal source files without creating
duplicate route inventories.

```bash
codeatlas http inventory --config codeatlas.json --out http-inventory.json
codeatlas http baseline --config codeatlas.json --out http-baseline.json
codeatlas http check --config codeatlas.json
codeatlas http check --config codeatlas.json --baseline http-baseline.json
codeatlas http diff http-baseline.json --config codeatlas.json
codeatlas http fuzz --config codeatlas.json --target public-local
codeatlas http fuzz --config codeatlas.json --target public-local --profile stateful
codeatlas http fuzz --config codeatlas.json --target public-local --profile thorough
codeatlas http fuzz --config codeatlas.json --target public-local --seed 42
codeatlas http fuzz --config codeatlas.json --target public-local --operation "POST /widgets/{id}"
```

`openapi` accepts a file path shorthand or a provider object with `kind` set to
`file`, `command`, `url`, or `target`. A target provider starts the matching
owned fuzz target and reads its runtime OpenAPI endpoint, removing the need for
project-specific scratch-file wrappers. `--openapi <path>` remains a deliberate
one-off file override, repeated once per configured contract. The inventory
normalizes operations, authentication requirements, parameters, request
content, response content, and referenced schema digests. CodeAtlas compares
that evidence; runtime schema libraries and the OpenAPI document remain the
contract authority.

`http check` also reports malformed path parameters, undefined security
schemes, missing success/error responses, missing request/response schemas,
unconstrained object or array shapes, and JavaScript regex flags accidentally
serialized into OpenAPI patterns. Static source evidence names the detector
that found a route; it does not claim that a generic `createRoute` call belongs
to a particular framework. `http baseline` stores only the normalized
behavioral contract, keeping source locations and tool metadata out of long-term
snapshots.

`http fuzz` manages a content-addressed Schemathesis toolchain in the CodeAtlas
cache. Its source-owned requirements lock pins every transitive package and
requires package hashes, so a fresh machine cannot silently resolve a different
fuzzer stack. Managed provisioning requires Python 3.10 or newer; set
`CODEATLAS_PYTHON` when `python3` is not the intended interpreter. CodeAtlas
also asks that exact CLI to load its bundled hook before starting the optional
foreground test server. It then waits for the OpenAPI document and runs one
standard policy for negative-data rejection, response conformance, missing
authentication, unsupported methods, and unhandled server errors.
`standard` generates 75 examples per operation and `thorough` generates 750.
The additive `stateful` profile runs 25 scenarios against explicit OpenAPI
Links, rejects speculative link inference, and fails when it does not traverse
every selected link. Run `standard` and `stateful` to cover both isolated
request behavior and declared resource workflows. `--max-examples` provides a
focused local override. Every run prints its exact random seed; pass it back
through `--seed` to reproduce the generated sequence.
`--operation "METHOD /path"` narrows a local debugging run without exposing
Schemathesis-specific filters. Header values can be literal test values or come
from the target environment with `value_env`; do not commit real secrets.
CodeAtlas injects these headers through its private Schemathesis hook rather
than exposing their values in process arguments. A target may also declare a
long-lived `request_adapter` command. CodeAtlas sends each exact serialized
request and each observed response over the versioned
`codeatlas.http-request-adapter/v1` JSONL protocol. Request replies supply
header and optional base64 body overrides before transport; response
observations let an
application-owned adapter retain workflow credentials for linked requests.
This keeps engine integration portable while each project reuses production
signing or token code without depending on Schemathesis. Each run stores its
report directory with owner-only permissions where the platform supports them.
It retains sanitized NDJSON, a compact evidence-safe JUnit report, and a compact
versioned `codeatlas.http-fuzz/v1` summary; request and response bodies,
sensitive headers, and URL queries are removed from retained evidence. The
summary separates positive successes, negative rejections, server errors,
operations whose positive cases only reached authentication rejection, and
stateful link/scenario coverage. Projects retain only their domain-owned
runtime fixture, explicit OpenAPI Links, adapter, and target configuration.

For complete standard and thorough runs, `positive_coverage` turns that evidence
into a regression gate without copying an operation allowlist. Set
`max_operations_without_success` to the current reviewed floor and ratchet it
down as contract examples and local fixtures improve; a newly uncovered
operation or a lost positive path then fails locally. Keep
`max_authentication_rejection_only_operations` at zero when the target supplies
working test credentials. Focused `--operation` runs and the additive stateful
profile do not apply the aggregate budget. CodeAtlas's authentication probe
accepts 401, 403, and privacy-preserving 404 rejections.

This command covers HTTP operations exposed by the selected OpenAPI contract.
It does not fuzz in-process APIs such as paint engines or brush models; those
need language-native property tests over their own public operations and
invariants.

## Release Model

Git tags build native archives for Linux, macOS, and Windows and publish the npm
wrapper through npm trusted publishing with automatic provenance. If a matching
archive is unavailable, the wrapper builds from source with Cargo.

The `@goobits/codeatlas` npm package must trust the GitHub repository
`goobits/codeatlas` and workflow `release.yml` for `npm publish`. The workflow
uses GitHub OIDC and does not use a long-lived npm token.

The software is distributed under the terms in `LICENSE`.
