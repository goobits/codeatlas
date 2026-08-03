# CodeAtlas

CodeAtlas maps public APIs, analyzes source reachability, and checks HTTP and
PostgreSQL contracts across JavaScript, TypeScript, Svelte, Python, and Rust
projects. Reports preserve unresolved and dynamic boundaries instead of
claiming certainty when the source graph is incomplete.

## Quick Start

```bash
npx @goobits/codeatlas scan .
npx @goobits/codeatlas scan packages/example --scope source --all --format json
npx @goobits/codeatlas lexicon packages/example --format json
npx @goobits/codeatlas audit .
npx @goobits/codeatlas audit packages/example --consumer-root .
npx @goobits/codeatlas ci . --workspace --fail-unused false --baseline public-api.json
npx @goobits/codeatlas diff public-api.json . --workspace --exact
npx @goobits/codeatlas dead-code . --format json
npx @goobits/codeatlas dead-code packages --workspace --format json
npx @goobits/codeatlas testing inventory . --workspace
npx @goobits/codeatlas testing impact . --workspace --changed packages/example/src/index.ts
npx @goobits/codeatlas testing witnesses . --workspace
npx @goobits/codeatlas context . --target src/main.rs
npx @goobits/codeatlas architecture compile architecture/root.atlas.yaml --source-root .
npx @goobits/codeatlas architecture source-check architecture/root.atlas.yaml --source-root . --repository . --check
npx @goobits/codeatlas architecture providers architecture/root.atlas.yaml --source-root . --capability example.capability.context
npx @goobits/codeatlas docs . --out docs/API-Reference.md
npx @goobits/codeatlas docs . --format html --out docs/API-Reference.html
npx @goobits/codeatlas postgres inventory .
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
| `scan`         | Show package API reachability or maintained source as a tree, Mermaid, or versioned JSON report |
| `lexicon`      | Report deterministic source-name collisions, structural aliases, repeated helpers, and terms |
| `audit`        | Report public exports with no detected local or explicitly scanned package consumers    |
| `dead-code`    | Classify source reachability, context-only code, and uncertain boundaries              |
| `testing`      | Inventory tests, select affected suites, and report public API witnesses               |
| `context`      | Return a bounded source graph slice for exact files or symbols                         |
| `architecture` | Compile declarations, query provider approvals, observe bindings, and evaluate conformance |
| `ci`           | Write a compact public API baseline and fail on configured audit findings              |
| `diff`         | Compare public APIs with compatibility or exact drift policy                           |
| `map`          | Generate a Mermaid dependency diagram                                                  |
| `docs`         | Generate deterministic Markdown or searchable HTML from public exports and source docs |
| `http`         | Inventory source routes, check/diff OpenAPI contracts, and run schema or transport fuzzing |
| `postgres`     | Discover, lint, replay, prepare, inventory, and diff PostgreSQL contracts               |

Run `codeatlas <command> --help` for command-specific options.

`audit --consumer-root <path>` and `ci --consumer-root <path>` count static
JavaScript, TypeScript, and Svelte imports, re-exports, and literal dynamic imports of
the scanned package from an external source tree. Maintained test and tooling
directories are included; the audited package itself, hidden directories,
dependencies, generated builds, and coverage output are excluded. The scan is
opt-in so a package-local audit stays bounded, and candidate files are filtered
by the package name before parsing. Namespace, default, and dynamically imported
package modules are handled conservatively because the exact member used may be
runtime-dependent. Public symbols required by another exported TypeScript
signature are treated as supporting API dependencies instead of duplicate
unused-public findings.

An explicit command is required. Repository-wide scan settings belong in
`codeatlas.json`; the former top-level flag interface has been removed.

`scan --scope api` is the default and follows configured entrypoints or
discovered package exports. `scan --scope source` removes that reachability
filter while preserving package export annotations, so `export_paths` still
distinguishes importable API from implementation-only symbols. Add `--all` when
the report should also include internal and private declarations. Source scans
honor repository ignore rules and exclude conventional test, dependency, and
generated-output directories unless the project explicitly disables default
ignores.

`lexicon` always inspects maintained source with internal and private symbols
included. Its advisory report flags exact same-name/different-shape concepts,
different-name/same-shape type candidates, and same-name/same-signature helper
families. Repeated identifier terms provide a deterministic vocabulary index;
no fuzzy or probabilistic naming guesses are used. Package exposure is derived
only from each symbol's `export_paths`, so an exported declaration in an
implementation-only file is not mislabeled as public API. Use text for a short
review or JSON for the complete term and public-symbol inventory.

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

Check workspace imports against package exports and accepted dependency
constraints without requiring a source commit or timestamp:

```bash
codeatlas architecture source-check \
  architecture/root.atlas.yaml \
  --source-root . \
  --repository . \
  --check \
  --out .codeatlas/source-conformance.json
```

The source check discovers pnpm workspace ownership, resolves JavaScript,
TypeScript, and Svelte imports, and reports three deterministic errors:
unexported workspace package imports, direct cross-package source bypasses,
and observed `depends_on` paths forbidden by accepted `no_path` or
`forbids_relation` constraints. Declared dependency constraints use only
production-reachable edges, so test and tooling imports do not invent runtime
architecture paths. Intrinsic export and source-bypass errors still apply in
every context. Aliases that resolve to the exact maintained
target of a declared package export remain valid. Repository-root tooling and
same-package implementation imports are not treated as cross-package API
violations. The report is deterministic and VCS-neutral.

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

Direct member-to-member source imports remain hard package-export gates.
Imports in either direction across the private workspace root remain visible
as non-gating repository-tooling advisories because the root is not an
independently consumable package contract. Architecture source conformance uses
the same boundary policy.

```json
{
	"projects": [
		{
			"id": "web",
			"root": ".",
			"languages": ["js", "ts", "svelte"],
			"require_complete": true,
			"contexts": {
				"application": {
					"role": "production",
					"scope": "runtime",
					"entrypoints": ["src/index.ts", "src/App.svelte"]
				},
				"public-api": {
					"role": "production",
					"scope": "public_surface",
					"entrypoints": ["src/public.ts"]
				},
				"unit-tests": {
					"role": "test",
					"entrypoints": ["src/**/*.test.ts"],
					"subjects": [
						{ "project": "web" },
						{ "source": "src/brushes/**" }
					]
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

Context roles (`production`, `test`, and `tooling`) describe who uses code.
Context scopes describe how roots are interpreted:

- `runtime` is the default. It follows executed imports and references, but
  does not count a declaration merely because a file exports it.
- `public_surface` expands exports from the configured root files, then follows
  their runtime dependencies. It does not turn exports of every imported module
  into public API.

JavaScript, TypeScript, and Svelte projects automatically add a production
`npm-package-exports` public-surface context when `package.json` exposes source
entries, including concrete files exposed through wildcard subpath exports.
Local source paths in npm `start` and `serve` lifecycle scripts also become
production roots in an `npm-package-runtime` context, as do `main` entrypoints
declared by `wrangler.toml`, `wrangler.json`, or `wrangler.jsonc`. Source entrypoints passed
to common bundler CLIs (`esbuild`, Rollup, and webpack) are production roots,
as are static `entry`, `entryPoints`, and `input` sources in conventional
esbuild, Rollup, tsup, Vite, and webpack config modules;
other package-script source paths and executable shebang modules become tooling
roots, including maintained scripts under normally ignored `build` directories.
Static local source files launched with Node child-process APIs are followed;
paths that depend on a generated runtime working directory remain explicit
dynamic boundaries rather than false missing-source gates. Local scripts
referenced by conventional `index.html` files, including imports in inline
module scripts, become production browser roots; scripts referenced by test
HTML and `test-harness.html` files become test roots. Conventional `*.test.*`,
`*.spec.*`, and test-config files become runtime roots in an
`ecmascript-tests` context.
Configured setup, teardown, and Svelte/Vite/Vitest alias replacement modules
are followed from those configs, including static `path.resolve(...)` values,
`alias`/`aliases` objects and arrays, and named replacement constants. Strings passed to
unknown package-resolution helpers are not guessed to be source paths. Ambient
`.d.ts` and declaration-only TypeScript
modules are classified as tooling declarations rather than runtime dead code.
Files such as `__tests__/support.ts` are scanned and followed when imported,
but are not roots merely because they live in a test directory. Explicit
contexts remain additive and can override automatic contexts by using their
names. Conventional nested fixture-data trees such as `tests/fixtures` and
`testdata` are excluded from broad discovery unless an explicit project path or
entrypoint selects them; scanning a fixture directory as the project root still
works normally.

`dead-code --workspace` preserves package ownership while applying the matching
local project from each member's `codeatlas.json`. Projects declared in the
workspace-root `codeatlas.json` may add settings for the root package or an
exact discovered member root. Other configured roots remain explicit projects
beside the discovered workspace, including nested Rust or migration owners.
When the workspace root is also a package, its non-member source is scanned as one
non-overlapping root project; member roots remain excluded from it. Packages
can therefore own their exceptional roots without duplicating one
workspace-wide configuration. Aggregate contexts that select only member-owned
sources are left to those member projects instead of becoming empty root
contexts. Nested pnpm workspaces are expanded recursively,
so packages embedded in a workspace member retain their own ownership instead
of being folded into the parent package's source graph.
Explicit aggregate projects follow the same ownership rule: when a nested
project root contains `codeatlas.json`, CodeAtlas inherits that root's
languages, contexts, assumptions, Rust settings, ignore policy, and
completeness requirement. The aggregate may add distinct contexts and assumed
roots, but conflicting copies fail configuration so package-owned analysis does
not silently drift.
Local source commands used by configured HTTP fuzz servers and request adapters
become test roots; source commands used to generate OpenAPI contracts become
tooling roots.

Medusa projects automatically root their conventional configuration,
instrumentation, API route, middleware, subscriber, and scheduled-job modules.
MikroORM configuration beneath a Medusa source tree is classified as tooling.

Each project may select `js`, `ts`, `svelte`, `py`, and `rs`. Rust projects can
also configure `rust.all_features` or an explicit `rust.features` list. Cargo
library targets use public-surface semantics; binaries, examples, benches,
build scripts, and tests use runtime semantics. Python PEP 621 projects derive
their public surface from import packages under configured setuptools roots or
the conventional `src`/root layout. Project scripts, executable Python shebang
modules, and conventional Python tests become production, tooling, and test
contexts automatically; an explicit context
with the same name overrides discovery. Python decorators that may register or
replace a declaration conservatively retain that symbol and its dependencies
while recording the dynamic boundary. Rust reachability honors `pub(crate)`,
`pub(super)`, and `pub(in path)` scope instead of treating restricted exports as public, follows
explicitly declared modules, and connects literal `include_str!` and
`include_bytes!` source dependencies.

Svelte reachability reads both module and instance scripts, preserves their
source spans, and connects JavaScript, TypeScript, and Svelte modules through
static imports, literal dynamic imports, bounded template imports, and Vite
globs. Resolved dynamic module namespaces conservatively retain their exported
symbols and private dependencies. Relative imports, TypeScript path aliases,
workspace-root Vite globs, and physical absolute paths beneath the workspace
can cross explicitly configured sibling project roots. Workspace root-absolute
imports into existing unscanned source remain explicit partial boundaries
rather than false missing-file gates. Standard `svelte-package` `src/lib` to
`dist` layouts map generated exports back to maintained source. Static worker,
worklet, and `importScripts`
dependencies are followed;
static Vite `?raw`, `?url`, and `?compose` imports of scanned ECMAScript source
retain a file dependency without treating that file's exported symbols as
executed. Resource imports of unscanned or non-source assets remain outside the
source graph without lowering analysis completeness. Svelte component symbols
remain conservative because markup-level
references are not yet a complete symbol graph. They are never emitted as
high-confidence unused-private findings.
SvelteKit route modules, pages, nested-app hooks, parameter matchers, and
service workers are discovered as framework-owned production roots. `$lib`
resolves to the nearest SvelteKit app's `src/lib`; generated `$types.js` imports
are framework boundaries rather than missing source files.
Generated, ignored, existing-but-excluded, and out-of-project relative source
imports remain visible as uncertainty advisories. Declared package imports into
conventional generated roots such as `dist`, `build`, and `pkg` are handled the
same way when those outputs have not been built. A genuinely missing in-project
source import remains a high-confidence gate.
Configured directory aliases stay scoped to their owning project when they do
not expose an index module; they never suffix-match an unrelated workspace
member.

The versioned dead-code report distinguishes unreachable private code,
test-only files and symbols, tooling-only code, unreferenced public APIs,
unresolved internal edges, and dynamic boundaries. A symbol in a
production-reachable file can therefore be reported as test-only when only test
roots reach that symbol. Context roots themselves are omitted from these
context-only findings to avoid listing every test file and test function.
Only high-confidence unreachable files, unused private symbols, unresolved
internal imports, and member-to-member package-export violations can fail
`dead-code --check`. Public APIs without repository consumers remain advisory
because external consumers may exist.
Set `require_complete` on a project only after its supported source and dynamic
boundaries are fully modeled. Report-only runs continue to preserve honest
partial or unsupported evidence; `dead-code --check` additionally fails closed
when any project carrying that requirement is not complete.

The dead-code JSON contract is schema version 4. Project summaries include
per-language file counts and the explicit completeness requirement. Each
finding includes a deterministic `id`, its exact `node_id` when one exists, and
the named context roots that support its classification. Finding IDs are stable
for the same structural source evidence and can drive bounded follow-up audits;
`node_id` can be passed directly to `context`. Scan, architecture, context, and
dead-code reports remain separate versioned contracts rather than one
all-purpose report.

Use `context` to retrieve only the nearby source facts needed for a task:

```bash
codeatlas context . \
  --target core::src/architecture/compiler.rs#compile \
  --target packages/web/src/routes.ts \
  --depth 2 \
  --max-nodes 128 \
  --out .codeatlas/context.json
```

Targets are exact source graph node IDs, `project::path` selectors,
repository-relative paths, or `path#symbol` selectors. Repeat `--target` to
resolve a batch into one bounded slice. Ambiguous project-relative paths fail
with a qualification hint, and exact reflexive edges are omitted as graph
noise. The schema-v2 result includes dependencies, dependents, visibility,
evidence, analysis boundaries, and an explicit truncation status.
The source context graph remains separate from the declared architecture graph
because the two graphs have different authority and semantics.

## Testing Intelligence

Testing analysis is read-only: CodeAtlas inventories and selects tests but never
executes package scripts or arbitrary repository commands.

```bash
codeatlas testing inventory . --workspace --format json
codeatlas testing impact . --workspace \
  --changed packages/brush/src/model.ts \
  --changed packages/paint/src/canvas.ts
codeatlas testing impact . --workspace --working-tree
codeatlas testing witnesses . --workspace --format json
```

`testing inventory` reports every analysis project, discovered test context and
root, test-related `package.json` script, recognized runner, conservative no-op
or allows-empty script, and exact duplicate test command. Runner detection is
evidence about a script string; it does not imply the command was executed
successfully.

`testing impact` follows the existing source graph from changed source to test
contexts. Each selection says whether it came from an observed dependency, a
declared project or source subject, or a conservative project/workspace
fallback. New, deleted, manifest, and unsupported paths fall back instead of
being presented as exact static selections. `selection_complete` is false when
any fallback was necessary. Changed paths are repository-relative, and
`--changed` can be repeated. `--working-tree` derives the changed set from
Git's staged, unstaged, and untracked paths; it remains read-only and cannot be
combined with explicit `--changed` arguments. Root workspace manifests,
toolchain files, lockfiles, and language project configs use workspace fallback
because they can affect every project.

`testing witnesses` evaluates public symbols reached from production
`public_surface` contexts. An observed witness identifies the exact test context
and root that statically reaches a symbol. A declaration remains visibly
`declared_only` until CodeAtlas observes that path. Missing witnesses become
`unwitnessed` only at high analysis confidence; incomplete source evidence is
reported as `unknown`. Test contexts with no observed public-API witness are
listed separately as detached from the public surface.

Test subjects are optional and valid only on contexts whose role is `test`.
`{ "project": "package-id" }` covers a named analysis project, including a
different workspace package. `{ "source": "src/domain/**" }` matches source
inside the test context's own project. Subjects express black-box intent and do
not replace observed graph evidence. Inventory, impact, and witness reports use
the separate versioned `codeatlas.testing/v1` data contract.

## Public API Baselines

Write one deterministic baseline for every public package in the nearest pnpm
workspace:

```bash
codeatlas ci . \
  --workspace \
  --fail-unused false \
  --baseline .codeatlas/baselines/public-api.json
```

Fail on additions, removals, export moves, signature changes, or visibility
changes:

```bash
codeatlas diff \
  .codeatlas/baselines/public-api.json \
  . \
  --workspace \
  --exact
```

The `codeatlas.public-api-baseline` schema stores only sorted package export
paths, stable symbol identities, and deterministic contract fingerprints.
Identity is the package, export path, symbol kind, and qualified name, so
source-file moves do not create drift and same-named exports on distinct
subpaths cannot overwrite each other. Overloads sharing one public identity
are retained as a sorted fingerprint set.

Without `--exact`, additions are reported but only removals and contract
changes fail. Exact mode is intended for repositories that require every API
change to update a reviewed baseline. `diff` continues to read full scan-report
baselines written by released CodeAtlas 0.7 versions.

## Documentation Checks

Generate the canonical reference:

```bash
codeatlas docs --config codeatlas.json
```

Fail CI when the committed reference is missing or stale:

```bash
codeatlas docs --config codeatlas.json --check
```

`diff` identifies each public binding by package, export path, kind, and
qualified name rather than declaration file. Non-package scans use the source
path as the public namespace. Additions are reported without failing;
removals and signature changes exit non-zero as breaking changes.

The JSON report contains `schema_version`, `tool_version`, package metadata,
public export paths, source signatures, structured documentation, imports, and
unused-public findings. Baselines must use the current report schema; CodeAtlas
does not carry legacy baseline readers.

## PostgreSQL Contracts

PostgreSQL analysis is a separate versioned domain. `postgres init <path>`
discovers conservative PostgreSQL evidence and prints a proposed explicit
contract; add `--write` to insert that property into `codeatlas.json` without
reformatting the rest of the file. Discovery requires PostgreSQL-specific SQL
or a PostgreSQL driver, so SQLite and generic data files do not become false
contracts.

```json
{
	"postgres": {
		"contracts": [
			{
				"id": "accounts-postgres",
				"bootstrap_sources": [
					{
						"path": "src/platform/db/schema.ts",
						"transaction": "always",
						"psql_meta_commands": "reject"
					}
				],
				"migration_sources": [
					{
						"path": "src/platform/db/migrations.ts",
						"transaction": "always",
						"psql_meta_commands": "reject",
						"recursive": false
					}
				],
				"query_roots": ["src"],
				"query_exclude_paths": ["src/integration"],
				"source_complete": true,
				"lint": {
					"pg_version": "17"
				}
			}
		],
		"targets": [
			{
				"id": "accounts-local",
				"contract": "accounts-postgres",
				"admin_url_env": "ACCOUNTS_CODEATLAS_POSTGRES_URL"
			}
		]
	}
}
```

Bootstrap and migration sources may be SQL files or directories. JavaScript and
TypeScript bootstrap files expose static schema SQL bindings; migration files
contain static `{ name, sql }` entries or project-relative `{ id, file }`
manifests whose referenced files end in `.sql`. Static SQL can be resolved through
relative imports and local workspace package exports. Calls that pass static
bindings through a runner's `bootstrapSql` property are discovered in runtime
order. Statically composed template migrations are resolved when every
interpolation is a local or workspace binding. Query roots inventory production
SQL files, PostgreSQL tagged templates, and SQL passed to supported database
calls such as `query`, `execute`, Prisma's parameterized `$queryRaw` and
`$executeRaw` tags, and the common pg-promise methods; conventional test sources
are excluded. Safe tagged-template values and pg-promise named value parameters
are normalized for PostgreSQL preparation. Identifier helpers, raw fragments,
lists, and unresolved interpolation remain explicit dynamic boundaries and are
never executed. Aliased, conditional, and nested tagged fragments—and templates
that mix tagged values with explicit PostgreSQL placeholders—are also kept
dynamic rather than being misrepresented as prepared queries. Reports contain
locations, counts, and SHA-256 digests—not raw SQL.

Use `query_exclude_paths` to partition known files or directories out of a
broad query root. Exclusions resolve relative to the configuration file, must
exist inside a configured query root, and can then be assigned as query roots
of a different contract. A contract with `depends_on` may intentionally omit
bootstrap and migration sources when it only validates integration queries
against schema owned by its dependency chain.

Directory sources remain recursive by default for backward compatibility. Set
`"recursive": false` when the production runner reads only direct files; this
keeps independently owned nested migration domains from being flattened
accidentally while existing contracts continue to inventory what they did
before this option existed.

Use `depends_on` when one independently tracked migration contract must be
replayed before another. CodeAtlas rejects missing or cyclic dependencies and
executes each dependency once in declared order, without flattening migration
names from separate runners.

Set migration semantics to match the real runner. `transaction` is `always` or
`never`; `unknown` remains visible and prevents live replay. Psql meta-commands
can be rejected, stripped when the application runner strips them, or declared
as psql-owned. Live replay refuses psql-owned directives rather than guessing
their side effects.

```bash
codeatlas postgres init .
codeatlas postgres init . --write
codeatlas postgres inventory . --out postgres-inventory.json
codeatlas postgres check . --out postgres-check.json

export ACCOUNTS_CODEATLAS_POSTGRES_URL='postgresql://postgres:password@127.0.0.1:5432/postgres'
codeatlas postgres test . --target accounts-local --out postgres-test.json
codeatlas postgres baseline . --target accounts-local --out postgres-baseline.json
codeatlas postgres diff postgres-baseline.json . --target accounts-local --out postgres-diff.json
```

`postgres check` runs the exact Squawk version shipped with the CodeAtlas npm
package. Squawk warnings stay visible but do not force unsafe edits to applied
migration history. A baseline records their structured identities, and a diff
gates newly introduced warnings while reporting resolved warnings as
informational. Squawk errors still gate immediately. `postgres test`
additionally requires `psql` and an admin URL supplied only through the
configured environment variable. It creates a bounded, uniquely named database
from `template0`, replays the selected contract and its dependencies with their
declared transaction semantics, and removes the database on success or failure.
It prepares supported static application queries so PostgreSQL checks their
relations, columns, operators, and parameter inference without executing
data-changing statements.

A clean live run can become a compact baseline. Baselines require
`source_complete: true`, zero gating findings, and PostgreSQL 13 or newer. A
diff requires the same contract and PostgreSQL server major. It gates migration
edits, removals, or insertions before applied names; lost static-query coverage;
removed or changed catalog objects; required columns without defaults; new
constraints; and unique indexes. Appended migrations and safe catalog
additions remain additive. Bootstrap source changes gate for explicit
upgrade-path review. Constraint-owned indexes are represented by their
constraint only, avoiding duplicate changes in the report.

## HTTP Contracts

HTTP contracts are a separate, versioned domain rather than part of the public
symbol scan. `http inventory <path>` works without configuration or an OpenAPI
document: it reports statically detected pages and HTTP endpoints, marks
endpoints as schema-missing, excludes conventional test sources, and stops at
nested project manifests. SvelteKit pages and server handlers, Medusa file-based
API routes, bounded Node pathname/direct-URL/prefix guards, Cloudflare-style
fetch path guards, and supported framework declarations retain their detector
and source evidence.
Genuinely dynamic dispatch can declare the otherwise unknowable transport
shape next to its implementation with `@codeatlas-http GET /items/{id}`; use
this narrow escape hatch only when static route detection cannot recover the
path.
Configured `source_include_paths` and `source_exclude_paths` bound API endpoint
contracts; page records remain complete so route and navigation tooling can
reuse the same inventory without duplicating filesystem discovery.

Add one or more OpenAPI 3.0 or 3.1 documents when exact request/response
contracts, conformance comparison, baselines, schema fuzzing, or stateful
workflows are needed:

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
        "source_include_operations": ["GET /health", "POST /v1/sessions"],
        "source_exclude_operations": ["GET /v1/internal-probe"],
        "source_complete": true
      }
    ],
    "fuzz": {
      "targets": [
        {
          "id": "public-local",
          "contract": "public-api",
          "base_url": "http://127.0.0.1:3443",
          "operations": [
            "GET /health",
            "POST /widgets/{id}"
          ],
          "expected_non_success_operations": ["GET /health"],
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
            "cwd": ".",
            "startup_timeout_seconds": 90,
            "prepare": [
              {
                "command": "node",
                "args": ["src/prepare-test-data.js"],
                "cwd": "."
              }
            ]
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
Exact operation filters use canonical `METHOD /path` keys when two methods on
the same source path belong to different contracts. Unlike path filters, these
are exact keys rather than globs. Operation filters apply after path filters
and may include `PAGE /path` inventory entries as well as HTTP endpoint methods.

```bash
codeatlas http inventory . --out source-routes.json
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
contract authority. Provider output streams into a private file with a 16 MiB
limit instead of being buffered without a bound. URL and target configuration
rejects credentials, non-HTTP schemes, and ambiguous fuzz base URLs containing
queries or fragments; provider URLs and headers are passed to the managed
fetcher over standard input rather than exposed in process arguments.

Without OpenAPI, `http check` emits one non-gating schema-missing warning while
retaining the source inventory. `http fuzz` automatically turns discovered
endpoints into a temporary source-transport contract after the target declares
its operation ownership. Use a reviewed string array to freeze an exact
allowlist, or `"operations": "contract"` to follow every endpoint retained by
the contract's source roots and include/exclude filters as that contract changes.
It exercises path serialization, arbitrary request bodies, unsupported methods,
and server-error handling without pretending that CodeAtlas inferred domain
fields, query parameters, authentication rules, or response schemas. Source-transport
reports are labeled `contractMode: "source_transport"`; the stateful profile
remains exclusive to explicit OpenAPI contracts. Curated source-transport
operations receive the same retained-evidence and positive-coverage gates as
curated OpenAPI operations. In a mixed configuration, `http baseline` stores
only schema-backed contracts, and `http diff` and baseline comparison ignore
new source-only contracts. A baselined contract that loses its schema remains a
breaking change. Baseline creation fails when no schema-backed contract exists.

With OpenAPI, `http check` also reports malformed path parameters, undefined security
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
foreground test server. CodeAtlas waits 30 seconds for owned servers to accept
connections by default. Set bounded `server.startup_timeout_seconds` (1–600)
for managed targets with slower cold starts. Optional `server.prepare` commands
run in order before the owned server and inherit the target's isolated
environment; use them for idempotent
local schema migrations or fixture preparation instead of project-specific
wrapper scripts. Schema-backed targets materialize their configured `file`,
`command`, `url`, or `target` provider into the private run directory, so the
service does not also need to expose a schema route. They run one standard
policy for negative-data rejection, response conformance, missing
authentication, unsupported methods, and unhandled server errors.
Source-transport targets run the narrower assertions that their static evidence
can support: known operations must not return an unhandled `500 Internal Server
Error`, and unsupported methods must be rejected with a 4xx client error. This
accepts intentional lifecycle statuses such as `503 Service Unavailable` and a
framework-level `400 Bad Request` when its request model cannot represent the
method. An explicit OpenAPI contract retains declared-status conformance plus
strict `405 Method Not Allowed` and `Allow` header conformance. Readiness
probes still run before fuzz cases. Source-transport runs also disable
schema-health warnings because their generated transport document intentionally
omits domain schemas and fixture values.
`standard` generates 75 examples per operation and `thorough` generates 750.
The additive `stateful` profile runs 25 scenarios against explicit OpenAPI
Links, rejects speculative link inference, and fails when it does not traverse
every selected link. Run `standard` and `stateful` to cover both isolated
request behavior and declared resource workflows. `--max-examples` provides a
focused local override. Every run prints its exact random seed; pass it back
through `--seed` to reproduce the generated sequence.
The target-owned `operations` selection is the authoritative fuzz boundary.
An explicit list rejects contract additions until reviewed; `"contract"`
explicitly delegates that boundary to the selected contract and its source filters.
`--operation "METHOD /path"` can only narrow that list for local debugging; it
cannot expand it. CodeAtlas validates every configured operation against the
selected contract, requires retained evidence for each selection, and keeps
all real sibling methods visible so an unsupported-method probe never calls a
different real operation. Focused reports are retained in distinct,
operation-specific directories beneath the selected profile, so one targeted
run does not erase evidence from another. Header values can be literal test
values or come from the target environment with `value_env`; do not commit real
secrets.
`expected_non_success_operations` declares target-persona operations whose
valid requests should be denied, such as outsider authorization probes. Every
entry must remain inside that target's operation boundary. A retained positive
4xx then satisfies positive coverage, while a run with only invalid generated
requests still fails a zero-tolerance `max_operations_without_success` gate.
CodeAtlas injects these headers through its private Schemathesis hook rather
than exposing their values in process arguments. Hook configuration lives in a
unique owner-only file for the run, is removed when its owner exits normally,
and is removed from the adapter process environment before the adapter starts.
A target may also declare a long-lived `request_adapter` command. CodeAtlas
sends each exact serialized request and each observed response over the versioned
`codeatlas.http-request-adapter/v2` JSONL protocol. Request messages include
the operation's active `securityParameters`. Replies may supply matching
`authentication` parameters independently from ordinary header, query-value,
and optional base64 body overrides. Query replies are maps whose string or
string-array values replace that name's generated values; `null` removes the
name. They cannot replace the URL's scheme, authority, or path. CodeAtlas
rejects undeclared authentication, prevents ordinary overrides from replacing
declared security parameters, and rejects an override of the exact negatively
generated parameter. When the engine identifies that exact query parameter,
an adapter may still replace other query values so long polls and other valid
fixtures stay deterministic. Overrides exist to supply valid fixtures or
credentials for the remaining components. When another header is negatively
generated, an adapter may still replace a configured static
credential header only while the request retains that exact placeholder value.
Authentication probes are marked in adapter messages and retain ordinary
fixture adaptation while never applying the adapter's dynamic authentication,
so sessions and credentials do not mask missing or invalid authentication.
Response observations let an application-owned adapter retain workflow
credentials for linked requests.
Coverage-phase scenarios that Schemathesis identifies as valid are normalized
to positive generation before adapter safety checks and coverage accounting,
including valid multipart objects whose raw engine mode is negative.
After the first exchange, adapters have 15 seconds to answer each message;
startup receives 90 seconds so an adapter may initialize local fixtures.
This keeps engine integration portable while each project reuses production
signing or token code without depending on Schemathesis. Each run stores its
report directory with owner-only permissions where the platform supports them.
An interrupted run stops its managed processes and discards raw exchange
evidence; the next run also clears any owned files left by an abnormal exit.
It retains sanitized NDJSON, a compact evidence-safe JUnit report, and a compact
versioned `codeatlas.http-fuzz/v2` summary; request and response bodies,
sensitive headers, and URL queries are removed from retained evidence. The
summary separates positive successes, declared non-success operations, negative
rejections, server errors, operations whose positive cases only reached
authentication rejection, and stateful link/scenario coverage. An operation
whose OpenAPI contract has no 2xx, 3xx, or `default` response satisfies the
coverage gate when a positive case reaches its declared client-error response;
privacy-preserving and deny-only endpoints therefore do not weaken the success
budget. Projects retain only their domain-owned
runtime fixture, optional explicit OpenAPI contract and Links, adapter, and
target configuration.

For complete standard and thorough runs—and focused runs backed by a configured
target allowlist—`positive_coverage` turns that evidence into a regression
gate. Set
`max_operations_without_success` to the current reviewed floor and ratchet it
down as contract examples and local fixtures improve; a newly uncovered
operation or a lost positive path then fails locally. Keep
`max_authentication_rejection_only_operations` at zero when the target supplies
working test credentials. Uncurated one-off OpenAPI `--operation` runs and the
additive stateful profile do not apply the aggregate budget. CodeAtlas's authentication probe
accepts 401, 403, and privacy-preserving 404 rejections.

This command covers HTTP operations exposed by the selected OpenAPI contract or
discovered in the selected source-transport boundary. It does not fuzz
in-process APIs such as paint engines or brush models; those need
language-native property tests over their public operations and invariants.

## Local Development

Run the complete local gate from this repository:

```bash
pnpm check
```

Local verification has three intentional layers:

- `pnpm test` is the repeatable default and runs the npm installer tests plus the
  fast Rust unit and executable-level integration tests.
- `pnpm run test:http-fuzz` runs the explicit managed Schemathesis smoke against
  a live fixture. It may provision the pinned Python toolchain on first use, so
  it stays out of the default test loop.
- `pnpm run self:check` dogfoods CodeAtlas's dead-code analysis against its own
  source and writes the detailed report to
  `$CARGO_TARGET_DIR/codeatlas-self-check.json`. The Cargo target must be an
  absolute path outside and disjoint from the checkout.

`pnpm check` composes the default tests with architecture-spec validation, Rust
formatting and linting, the dogfood scan, and the package-content check. Only
high-confidence dead code or unresolved internal imports fail the dogfood gate.

## Release Model

Git tags build native archives for Linux, macOS, and Windows and publish the npm
wrapper through npm trusted publishing with automatic provenance. If a matching
archive is unavailable, the wrapper builds from source with Cargo.

The `@goobits/codeatlas` npm package must trust the GitHub repository
`goobits/codeatlas` and workflow `release.yml` for `npm publish`. The workflow
uses GitHub OIDC and does not use a long-lived npm token.

The software is distributed under the terms in `LICENSE`.
