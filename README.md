# CodeAtlas

CodeAtlas is a local CLI for repository evidence and policy. It maps public
code APIs, classifies source reachability, inspects bounded dependency
neighborhoods, checks declared architecture, inventories tests, and analyzes
HTTP and PostgreSQL contracts.

CodeAtlas preserves unresolved and dynamic boundaries instead of turning
incomplete static evidence into false certainty. It does not mutate project
source or claim to replace runtime, integration, or property tests.

## Setup

CodeAtlas is used from its source repository and is not published. Clone it
over SSH, keep Cargo output outside the checkout, and build the locked source:

```bash
git clone git@github.com:goobits/codeatlas.git
cd codeatlas

export CARGO_TARGET_DIR="${XDG_CACHE_HOME:-$HOME/.cache}/codeatlas/target"
pnpm install --frozen-lockfile
cargo build --locked

"$CARGO_TARGET_DIR/debug/codeatlas" --help
```

The npm wrapper can run an exact local binary when a Node-based caller is more
convenient:

```bash
CODEATLAS_BINARY_PATH="$CARGO_TARGET_DIR/debug/codeatlas" node bin/codeatlas.js --version
```

Node.js 22 or newer is required by the wrapper. PostgreSQL live tests also
require `psql`. HTTP planning fingerprints the locked Schemathesis contract
without installing or starting it; managed execution fails closed unless the
required kernel enforcement is available.

## CLI

```text
codeatlas [--root <path>] [--config <path>] <command> <subject> [options]
```

`--root` defaults to the current directory. `--config` selects a specific
`codeatlas.json`; otherwise CodeAtlas reads the file at the selected root when
present.

| Command | Purpose |
| --- | --- |
| `scan code\|http\|postgres\|architecture\|tests` | Gather current subject evidence |
| `check code\|http\|postgres\|architecture\|tests` | Apply static rules and contract checks |
| `baseline code\|http\|postgres\|architecture` | Save reviewed comparison evidence |
| `diff code\|http\|postgres\|architecture` | Compare evidence with a baseline |
| `usage code\|tests` | Classify consumers or select affected tests |
| `inspect code\|architecture` | Explain an exact target and its bounded neighborhood |
| `lexicon code` | Report deterministic naming, structural, and declared conceptual overlap |
| `docs code` | Generate or check public API documentation |
| `fuzz http` | Persist or execute a bounded HTTP fuzz plan |
| `test postgres` | Replay migrations and prepare queries in a disposable database |
| `init postgres` | Discover and optionally write PostgreSQL configuration |

Run `codeatlas <command> <subject> --help` for the complete option set.

### Capability boundaries

| Evidence | JavaScript/TypeScript | Svelte | Python | Rust |
| --- | --- | --- | --- | --- |
| Public API scan and docs | yes | yes | yes | yes |
| Reachability, usage, context, and test impact | yes | yes | yes | yes |
| Static HTTP route inventory | yes | yes | yes | yes |
| Static PostgreSQL application-query extraction | yes | no | no | no |

HTTP fuzz planning operates at a configured transport boundary and makes zero
target calls. PostgreSQL live testing validates database contracts; it is not
SQL fuzzing. CodeAtlas does not yet fuzz SQL parameters or in-process code
APIs.

## Code Evidence

### Public and maintained surfaces

`scan code` follows configured entrypoints or discovered package exports by
default:

```bash
codeatlas --root packages/example scan code
codeatlas --root packages/example scan code --format json
```

Use a source scan to inspect every maintained source file. `--all` adds private
and internal declarations:

```bash
codeatlas --root packages/example scan code --scope source --all --format json
codeatlas --root packages/example scan code --scope source --format mermaid
```

Package exports remain attached to source-scope symbols, so the report still
distinguishes importable API from implementation-only declarations. Default
discovery excludes dependencies, generated output, conventional tests, and
fixture-data trees unless configuration explicitly selects them.

### Reachability and consumers

Use named contexts to describe production, test, and tooling roots:

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

`runtime` contexts follow executed dependencies. `public_surface` contexts
also expand exports from their roots. Test subjects record black-box intent;
they supplement observed graph evidence rather than replacing it.

CodeAtlas also derives supported roots from package exports, executable
scripts, framework entrypoints, conventional tests, Rust targets, Python
project scripts, and configured HTTP or PostgreSQL tools. Dynamic imports,
reflection, macros, generated targets, unresolved aliases, and unsupported
syntax remain explicit analysis boundaries.
Configured directory aliases remain scoped to their owning project and never
suffix-match an unrelated workspace member.

```bash
codeatlas --root . usage code --workspace
codeatlas --root . usage code --workspace --format json --out usage.json
codeatlas --root . check code --workspace --gates-only
```

Text output prioritizes gating findings and groups advisories for triage. JSON
is the exhaustive machine-readable report. `check code` returns nonzero for
eligible findings and for a project whose `require_complete` assertion is not
satisfied.

Public consumer analysis is deliberately separate because external consumers
may be invisible:

```bash
codeatlas --root packages/library usage code --scope public
codeatlas --root packages/library usage code \
  --scope public \
  --consumer-root ../application
```

It recognizes static imports, re-exports, and literal dynamic imports from
JavaScript, TypeScript, and Svelte consumer trees. Namespace, default, and
runtime-dependent imports are handled conservatively.

#### Dead-code report v5

`usage code --format json` and `check code --format json` emit schema version
5. The report keeps project completeness separate from individual findings.

Project summaries include:

- `completeness`: `complete`, `partial`, or `unsupported`
- `completeness_reasons`: the exact boundary kind, effect, message, and source
  evidence that limits the project
- `require_complete`: whether incomplete evidence is a check failure
- deterministic file, language, and symbol counts

Every finding includes a stable `id`, optional exact `node_id`, contexts and
roots, confidence, source evidence, and these review fields:

| Field | Values | Meaning |
| --- | --- | --- |
| `evidence_class` | `direct` | High-confidence structural contract evidence |
|  | `inferred` | High-confidence reachability classification |
|  | `boundary_limited` | Public, dynamic, medium-confidence, or low-confidence evidence |
| `source_disposition` | `maintained`, `generated`, `fixture`, `test`, `tooling` | Source role inferred from the maintained path |
| `gates` | boolean | Whether this exact finding can fail `check code` |

Only high-confidence unreachable files, unused private symbols, workspace
export or source-bypass violations, and unresolved internal imports can gate.
Public symbols with no known consumer, dynamic boundaries, test-only code, and
tooling-only code remain visible evidence, not automatic deletion authority.

### Focused context

`inspect code` accepts one or more exact node IDs, repository-relative paths,
`project::path` selectors, or `path#symbol` selectors:

```bash
codeatlas --root . inspect code \
  core::src/compiler.rs#compile \
  packages/web/src/routes.ts \
  --depth 2 \
  --direction outgoing \
  --max-nodes 128 \
  --out context-page-1.json
```

Direction is `incoming`, `outgoing`, or `both`. The default is `both`.
Ambiguous project-relative targets fail with a qualification hint.

Context reports use schema version 3. Each page includes:

- `graph_digest`, `direction`, `depth`, and `max_nodes`
- `page_offset`, `remaining_nodes`, and omitted project, node, edge, context,
  and boundary counts
- an opaque `continuation` cursor when another page exists
- exact target resolutions and the page-owned graph evidence

Resume with the same targets, depth, direction, and node budget:

```bash
codeatlas --root . inspect code \
  core::src/compiler.rs#compile \
  packages/web/src/routes.ts \
  --depth 2 \
  --direction outgoing \
  --max-nodes 128 \
  --cursor '<continuation from context-page-1.json>' \
  --out context-page-2.json
```

The cursor binds the request and source-graph digest. A changed request is
rejected, and a changed graph makes the cursor stale. Combining every page by
stable identity reconstructs the complete directed slice. Source context is
kept separate from declared architecture because the graphs have different
authority and semantics.

### Test intelligence

Test analysis is read-only. It inventories and selects tests but never runs
package scripts:

```bash
codeatlas --root . tests inventory --workspace --format json
codeatlas --root . tests impact --workspace \
  --changed packages/brush/src/model.ts \
  --changed packages/paint/src/canvas.ts
codeatlas --root . tests impact --workspace
codeatlas --root . tests witnesses --workspace --format json
```

- `inventory` reports test contexts, roots, package scripts, recognized
  runners, no-op or allows-empty scripts, and duplicate commands.
- `impact` selects observed dependents and falls back conservatively for new,
  deleted, manifest, or unsupported paths. `selection_complete` exposes whether
  fallback was needed. Without `--changed`, it reads Git's staged, unstaged,
  and untracked paths. Explicit repeatable `--changed` values replace that
  default. Workspace manifests, lockfiles, toolchain files, and language project
  configuration use the conservative workspace fallback.
- `witnesses` distinguishes observed, declared-only, unwitnessed, unknown, and
  detached evidence for public symbols. Text output omits already-witnessed
  detail and bounds the remaining findings; JSON preserves the complete
  evidence contract.

All three use the separate `codeatlas.testing/v1` contract.

### Lexicon review

```bash
codeatlas --root . lexicon code
codeatlas --root . lexicon code --workspace --format json
```

Lexicon analysis scans maintained source with private symbols included. It
reports exact same-name/different-shape collisions, deterministic type-shape
candidates, callable contract candidates, repeated identifier terms, and
declared terminology policy. It also records package exposure.

Its programming-identifier grammar recognizes one bounded family of naming
constructions: `verb_object[_qualifier]`, `object[_qualifier]_actor`, and
`object[_qualifier]_result`. Thus `load_config` / `config_loader`,
`validate_request` / `request_validator`, and `resolve_path` /
`path_resolution` can be compared without permuting arbitrary words. Object and
qualifier order is preserved, and predicates (`is`, `has`, `can`, `supports`)
remain semantically distinct from actions. A grammar pair is reported only when
same-language, compatible symbol kinds also share a cross-file typed callable
contract, untyped callable shape, or structural type shape. Untyped evidence is
clearly lower confidence.

The built-in grammar uses a reviewed, closed programming morphology table for
actor/result forms of `build`, `collect`, `convert`, `format`, `load`, `parse`,
`plan`, `read`, `render`, `resolve`, `validate`, and `write`. Its safe
abbreviations are exactly `cfg`/`config`, `ctx`/`context`, `req`/`request`,
`resp`/`response`, and `repo`/`repository`. It does not use Porter stemming, a
general dictionary, or arbitrary token sorting. Projects may add bounded exact
rules without overriding built-ins:

```json
{
  "lexicon": {
    "grammar": {
      "abbreviations": [
        { "term": "svc", "expansion": "service" }
      ],
      "morphology": [
        { "term": "hydrator", "action": "hydrate", "role": "actor" }
      ]
    }
  }
}
```

Candidate generation is linear: each observed actor/result surface is compared
with one deterministic action-form anchor, never every spelling pair. Exact
same-name signatures carry direct structural evidence. Separate non-grammar
callable candidates require a typed contract shape, cohesive source scope, and
meaningful object or qualifier terms after the leading intent word. Untyped
name-only matches and unrelated type coincidences are omitted because they do
not provide enough evidence. CodeAtlas does not compare implementation bodies
or claim behavioral equivalence, so structural candidates remain advisory.
Results are read-only: they do not create gates, choose a refactor, authorize
deletion, or update a source dataset.

Project policy is the authority. A concept can own preferred terms, exact
aliases, and retired terms. `distinct_from` records that two declared concepts
are intentionally different; `never_suggest` suppresses one exact unowned or
partially owned term pair. Both exception forms require a durable reason.

```json
{
  "lexicon": {
    "concepts": [
      {
        "id": "request_handler",
        "preferred_terms": ["request handler"],
        "exact_aliases": ["controller"],
        "retired_terms": ["request processor"],
        "distinct_from": [
          {
            "concept": "event_listener",
            "reason": "Handlers own requests; listeners observe domain events."
          }
        ]
      },
      {
        "id": "event_listener",
        "preferred_terms": ["event listener"]
      }
    ],
    "never_suggest": [
      {
        "terms": ["record", "row"],
        "reason": "A record is a domain value; a row is storage in this project."
      }
    ]
  }
}
```

Terms are matched exactly after case, punctuation, separator, and identifier
word-boundary normalization. A term may belong to only one concept. A
`distinct_from` declaration is symmetric even when it is written on only one
concept. Contradictory, duplicate, unknown, or reasonless declarations fail
before source scanning.

Evidence precedence is project policy, exact normalized concepts, local
programming grammar/morphology, then pinned CSO relations. Project declarations
are authoritative. Grammar and provider results remain explainable advisories;
each JSON finding lists its canonical grammar, every abbreviation/morphology
rewrite, compatible kind, and exact structural corroboration. An exact
`distinct_from` or `never_suggest` rule always suppresses later evidence.

#### Offline thesaurus evidence

Optional sources are pinned offline evidence, never authority. Every provider
manifest declares its version, SHA-256 digest, license, attribution, upstream
URL, format, and whether the local data is complete or filtered. Missing files,
changed bytes, malformed records, and unsupported schemas fail the command
instead of silently producing a partial report.

[Computer Science Ontology (CSO)](https://cso.kmi.open.ac.uk/) is the primary
supported programming/domain source. CSO 3.5 contains about 15,000 topics and
166,000 relationships and is licensed
[CC BY 4.0](https://cso.kmi.open.ac.uk/faq). CodeAtlas reads the official,
extracted CSV directly and considers only `preferentialEquivalent` and
`relatedEquivalent`; hierarchy and contribution edges are not synonym
evidence. CSO itself defines `relatedEquivalent` as contextual equivalence,
not `skos:exactMatch`, so every sourced result remains advisory.

Source acquisition and refresh stay outside analysis. This reproducible example
pins the CSO 3.5 release archive and the extracted CSV bytes used by the
provider:

```bash
lexicon_source_root=/opt/codeatlas/lexicon
mkdir -p "$lexicon_source_root"
curl -fsSL \
  https://cso.kmi.open.ac.uk/download/version-3.5/CSO.3.5.csv.zip \
  -o "$lexicon_source_root/CSO.3.5.csv.zip"
printf '%s  %s\n' \
  5b16a3902e90b704bc90536034665022b2b3d074c7bf3fbf4291e5d6cc0aae20 \
  "$lexicon_source_root/CSO.3.5.csv.zip" | sha256sum --check
unzip -p "$lexicon_source_root/CSO.3.5.csv.zip" \
  > "$lexicon_source_root/CSO.3.5.csv"
printf '%s  %s\n' \
  564fb62dcc638c655bd9936247f45d740417e5786f6892f0341f606cfbbba98f \
  "$lexicon_source_root/CSO.3.5.csv" | sha256sum --check
```

```json
{
  "lexicon": {
    "providers": [
      {
        "id": "cso",
        "tier": "domain",
        "format": "cso_csv",
        "coverage": "complete",
        "version": "3.5",
        "path": "/opt/codeatlas/lexicon/CSO.3.5.csv",
        "sha256": "sha256:564fb62dcc638c655bd9936247f45d740417e5786f6892f0341f606cfbbba98f",
        "license": "CC-BY-4.0",
        "attribution": "Computer Science Ontology, Knowledge Media Institute, The Open University",
        "url": "https://cso.kmi.open.ac.uk/downloads"
      }
    ]
  }
}
```

Relative provider paths resolve from the selected `codeatlas.json`. CodeAtlas
does not distribute CSO, download it during analysis, or refresh a pin.

`relations_json_v1` is the small pluggable format for a versioned domain source
or a filtered general thesaurus. It has a closed schema:

```json
{
  "schema_version": 1,
  "relations": [
    {
      "subject": "language model",
      "relation": "synonym",
      "object": "language models"
    }
  ]
}
```

Domain manifests may use `preferential_equivalent`, `related_equivalent`, or
`synonym`. General manifests may use only `synonym`, require at least one domain
provider, and are indexed only when the exact normalized pair already has
domain evidence. Thus a normal thesaurus can corroborate a programming result
but cannot originate one. [Open English WordNet
2025](https://github.com/globalwordnet/english-wordnet/releases) is the
recommended general source: it is versioned and CC BY 4.0, but its roughly
120,000 sense-specific synsets are deliberately not embedded or guessed across
code identifiers. Prepare a small attributed relation file in a separate source
refresh process, mark its coverage `filtered`, and pin its resulting bytes.

JSON reports use lexicon schema version 3. They expose deterministic candidate
IDs and ordering, source manifests and record counts, evidence relation and
direction, available source ranges for observed symbols, project/domain tiers,
qualitative confidence, stable rules and reasons, preferred terms when
declared, the exact built-in/configured grammar-rule counts, applied
suppressions, and the exact config key for permanently dismissing an advisory
candidate. Text output shows the same review surface in compact form; JSON is
the complete contract.

### Public API baselines

```bash
codeatlas --root . baseline code \
  --workspace \
  --out .codeatlas/baselines/public-api.json

codeatlas --root . diff code \
  --workspace \
  --against .codeatlas/baselines/public-api.json \
  --exact
```

The compact baseline stores stable public identities and deterministic
contract fingerprints. Without `--exact`, additions are reported while
removals and contract changes fail. A fingerprint proves that a contract
changed, not whether that language-specific change is source-compatible, so
changed contracts are labeled `REVIEW` and remain fail-closed until reviewed or
checked by a purpose-built compatibility tool. Exact mode also fails on
additions and export moves. Baseline creation is explicit; checks never update
reviewed evidence.

### API documentation

```bash
codeatlas --root . --config codeatlas.json docs code
codeatlas --root . --config codeatlas.json docs code \
  --format html \
  --out docs/API-Reference.html
codeatlas --root . --config codeatlas.json docs code --check
```

Source documentation remains the description owner. CodeAtlas does not invent
missing descriptions. `declaration_contract` follows the shipped declaration
entrypoint, while `include_dependency_types` includes supporting local types
needed to understand exported signatures. Documentation configuration also
accepts home and canonical URLs, a public display name, description checks, and
light or dark semantic color overrides.

```json
{
  "docs": {
    "title": "Example API Reference",
    "description": "Public contracts for the Example package.",
    "public_name": "Example SDK",
    "declaration_contract": true,
    "include_dependency_types": true,
    "require_descriptions": true,
    "output": "docs/API-Reference.md"
  }
}
```

## Declared Architecture

Restricted YAML is the sole editable architecture authority. Generated graphs,
lockfiles, observations, and conformance reports are evidence and must not be
edited by hand.

Save a canonical compilation baseline for one or more root modules:

```bash
codeatlas baseline architecture \
  architecture/root.atlas.yaml \
  --source-root . \
  --mode governing \
  --out .codeatlas/architecture.json \
  --lock-out .codeatlas/architecture.lock.json
```

`governing` includes active accepted declarations. `review` also includes
proposed and unresolved declarations but remains non-governing.

Check current imports and accepted dependency constraints:

```bash
codeatlas --root . check architecture \
  architecture/root.atlas.yaml \
  --source-root . \
  --out .codeatlas/source-conformance.json
```

The source check reports unexported workspace imports, direct cross-package
source bypasses, and dependency paths forbidden by accepted architecture.

Generate reproducible binding evidence and compare it with the governing graph:

```bash
codeatlas --root . scan architecture \
  architecture/root.atlas.yaml \
  --source-root . \
  --repository-id example.repository.source \
  --observation-id example.observation.current \
  --source-commit 0123456789abcdef0123456789abcdef01234567 \
  --observed-at 2026-07-23T00:00:00Z \
  --out .codeatlas/architecture-observation.json

codeatlas --root . diff architecture \
  --against .codeatlas/architecture.json \
  --observation .codeatlas/architecture-observation.json \
  --conformance-id example.conformance.current \
  --as-of 2026-07-23T00:00:00Z \
  --out .codeatlas/architecture-conformance.json
```

The caller supplies commit and time metadata explicitly. Diff loads the exact
saved governing graph; it does not silently recompile current declarations.
A review-mode baseline is inspectable but cannot govern conformance. Optional
repeatable `--policy` inputs may change a finding's disposition but never the
governing graph.

Query an approved provider classification:

```bash
codeatlas inspect architecture \
  capability:example.capability.context \
  architecture/root.atlas.yaml \
  --source-root . \
  --approval-scope organization
```

This projection does not evaluate runtime eligibility, select a provider, or
authorize invocation. The accepted architecture language and trust boundaries
are specified in [`spec/architecture/v0.1/`](spec/architecture/v0.1/README.md).

## HTTP Contracts

`scan http` inventories supported source routes without configuration. Add an
OpenAPI 3.0 or 3.1 contract for request and response schemas, conformance,
baselines, and schema-backed fuzzing.

```json
{
  "http": {
    "contracts": [
      {
        "id": "public-api",
        "openapi": "openapi.json",
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
          "operations": ["GET /health", "POST /v1/sessions"],
          "environment": {
            "NODE_ENV": "test",
            "PORT": "3443"
          },
          "secret_environment": {
            "API_TOKEN": "LOCAL_API_TOKEN"
          },
          "server": {
            "command": "node",
            "args": ["src/test-server.js"],
            "cwd": "."
          },
          "positive_coverage": {
            "max_operations_without_success": 0,
            "max_authentication_rejection_only_operations": 0
          }
        }
      ]
    }
  }
}
```

Leave `source_complete` false when runtime registration can escape static
discovery. Exact operation filters use canonical `METHOD /path` keys. Dynamic
dispatch may declare a narrow literal route with
`@codeatlas-http GET /items/{id}` when static recovery is impossible.

```bash
codeatlas --root . scan http --out http-inventory.json
codeatlas --root . scan http --format hqa-inventory --out hqa-routes.json
codeatlas --root . check http
codeatlas --root . baseline http --out http-baseline.json
codeatlas --root . diff http --against http-baseline.json
codeatlas --root . fuzz http --target public-local --seed 42
codeatlas --root . fuzz http --target public-local --profile stateful
# After reviewing the emitted plan:
codeatlas --root . fuzz http --plan plan_ABC --execute
```

The target and `--replay` forms only gather current evidence and persist an
immutable content-addressed plan under the external state root. `--plan ...
--execute` revalidates that exact plan before any target work. Missing proxy or
isolation enforcement produces a blocked zero-call receipt; review never
waives a missing capability. Set `CODEATLAS_STATE_DIR` to choose the external
private artifact base.

The `hqa-inventory` format projects the same bounded source and OpenAPI union
into HQA application-inventory v1. Endpoint and OpenAPI-only operations are
probe-only; source pages remain explorable. Dynamic `{parameter}` paths use a
navigable static prefix, while detector-specific path patterns survive only as
provenance tags. Partial source completeness stays explicit, and CodeAtlas
never invents HQA roles, readiness targets, or transitions.

Without OpenAPI, `check http` preserves the source inventory and reports
schema absence without pretending a schema exists. Source-transport fuzzing
can check known operations for unhandled server errors and unsupported methods.
Explicit OpenAPI adds schema conformance, authentication probes, declared
status checks, and optional stateful traversal through OpenAPI Links.

The target's operation list is the fuzz authority. `--operation` may narrow it
for local diagnosis but cannot expand it. Every plan contains its concrete
seed and finite limits. Plans and receipts exclude secret values; future run
reports also exclude request and response bodies, sensitive headers, and URL
query values.

HTTP configuration also supports:

- OpenAPI providers from a file, command, URL, or configured fuzz target
- exact source operation filters after path filters
- an explicit operation list or `"operations": "contract"`
- literal non-secret process environment plus `secret_environment` mappings
  from target variable name to ambient secret-reference name
- literal test headers or `value_env` secret references; planning records the
  reference and does not require or persist its value
- expected non-success operations and positive-coverage budgets
- ordered `server.prepare` commands before an owned local server starts
- a long-lived request adapter over the
  `codeatlas.http-request-adapter/v3` JSONL protocol for project-owned fixture,
  signing, and authentication logic

The profile ceilings are 75 cases for `standard`, 750 for `thorough`, and 25
stateful cases across explicit OpenAPI Links. Checked-in `fuzz.limits.max_cases`
(50 by default) remains the hard ceiling, and `--max-cases` may only tighten
it. The retained `codeatlas.http-fuzz/v2` summary separates positive successes,
expected denials, negative rejections, server errors, authentication-only
results, and stateful coverage; the execution-kernel migration reconnects its
producer without preserving a direct executor.

## PostgreSQL Contracts

`init postgres` discovers conservative PostgreSQL evidence and prints proposed
configuration. `--write` is the only form that edits `codeatlas.json`.

```json
{
  "postgres": {
    "contracts": [
      {
        "id": "accounts",
        "bootstrap_sources": [
          {
            "path": "src/db/schema.sql",
            "transaction": "always",
            "psql_meta_commands": "reject"
          }
        ],
        "migration_sources": [
          {
            "path": "src/db/migrations",
            "transaction": "always",
            "psql_meta_commands": "reject",
            "recursive": false
          }
        ],
        "query_roots": ["src"],
        "source_complete": true,
        "lint": {
          "pg_version": "17"
        }
      }
    ],
    "targets": [
      {
        "id": "accounts-local",
        "contract": "accounts",
        "admin_url_env": "ACCOUNTS_CODEATLAS_POSTGRES_URL"
      }
    ]
  }
}
```

Static inventory resolves supported SQL files, migration manifests, tagged
templates, and database calls. Unresolved interpolation, identifier helpers,
raw fragments, and dynamic SQL remain visible boundaries and are never
executed.

```bash
codeatlas --root . init postgres
codeatlas --root . scan postgres --out postgres-inventory.json
codeatlas --root . check postgres --out postgres-check.json

export ACCOUNTS_CODEATLAS_POSTGRES_URL='postgresql://postgres:password@127.0.0.1:5432/postgres'
codeatlas --root . test postgres --target accounts-local --out postgres-test.json
codeatlas --root . baseline postgres --target accounts-local --out postgres-baseline.json
codeatlas --root . diff postgres \
  --against postgres-baseline.json \
  --target accounts-local \
  --out postgres-diff.json
```

`check postgres` is static and runs the pinned Squawk version. `test postgres`
creates a bounded database from `template0`, replays dependencies and
migrations with declared transaction semantics, prepares supported static
queries, and removes the database on success or failure. It does not execute
data-changing application queries.

Baselines require complete source evidence, a clean live replay, and a
supported server version. Diffs gate edited or reordered applied migrations,
lost query coverage, breaking catalog changes, required columns without
defaults, new constraints, and unique indexes. Safe appended migrations and
additive catalog changes remain additive.

## Published JSON Schemas

Every stable JSON report root has a generated Draft 2020-12 schema in
[`schemas/`](schemas/). The schemas come from the same Rust models and serde
attributes that write the reports. Normal tests regenerate them in memory and
fail on byte drift; they never rewrite the checkout.

After an intentional report-contract change, update the registered files with
an external Cargo target:

```bash
schema_cache_root="$(mktemp -d /tmp/codeatlas-schema-cache.XXXXXX)"
export CARGO_TARGET_DIR="$schema_cache_root/cargo-target"
pnpm run schemas:write
```

Existing reports retain their shipped integer and API version fields. Every
new artifact instead uses one `codeatlas.<lower-kebab-kind>/v<positive-integer>`
schema-version string and no parallel API version. CodeAtlas annotation keys
are registered in the [canonical lexicon](docs/concepts/lexicon.md) before use.
External schemas, including the pending source-target contract, are not
vendored or re-published here. Schema publication adds packaged files, not a
runtime `schemas` command.

## Configuration Rules

Configuration is strict JSON. Unknown fields fail validation so spelling
errors cannot silently weaken analysis. Paths are relative to the config file.

Common top-level fields are:

- `root`: project root
- `languages`: any of `js`, `ts`, `svelte`, `py`, or `rs`
- `entrypoints`: explicit public or runtime roots
- `include_private` and `include_types`: code scan detail
- `no_default_ignore`: include normally ignored source classes
- `package_exports`: discover package entrypoints from `package.json`
- `projects`: named reachability projects and contexts
- `execution`: finite call, rate, concurrency, time, memory, process, output,
  artifact, and isolation ceilings
- `fuzz`: finite case, shrink, failure, and per-case time ceilings shared by
  fuzz subjects
- `docs`, `http`, and `postgres`: domain-specific contracts

CLI limit flags may only tighten their checked-in values. Zero, unlimited
sentinels, and command-line increases are rejected. The built-in defaults are
materialized into every saved plan, so a later default change cannot alter a
reviewed artifact.

Package exports are enabled by default. TypeScript declaration or JavaScript
export targets are mapped back to maintained source when the project's
TypeScript output configuration makes that mapping exact.

## Source Index

Source-graph analysis uses a bounded external index by default. Set
`CODEATLAS_SOURCE_INDEX=0`, `false`, or `off` to disable it.

| Variable | Behavior |
| --- | --- |
| `CODEATLAS_SOURCE_INDEX_DIR` | Overrides the index root. The path must be absolute and disjoint from every analyzed project. |
| `CODEATLAS_CACHE_DIR` | Supplies the cache base when no source-index root is set. Otherwise CodeAtlas uses the platform or XDG cache location. |
| `CODEATLAS_STATE_DIR` | Supplies the external base for private content-addressed plans, receipts, and reproducers. The resulting execution root must be disjoint from the analyzed workspace. |
| `CODEATLAS_SOURCE_INDEX_MAX_BYTES` | Sets the byte limit. The default is 512 MiB; accepted values range from 16 MiB through 16 GiB. |
| `CODEATLAS_METRICS=1` | Writes one source-index metrics record as JSON to stderr after each source-graph analysis. |

The default index root is `codeatlas/source-index/v1` below the selected cache
base. CodeAtlas rejects a root that contains an analyzed project or is contained
by one.

The whole-graph key covers resolved project configuration, maintained source
and control-file contents, the source-graph schema, and the analysis algorithm.
An unchanged key reuses the complete graph. Content-addressed parser facts are
also reused per file. A changed key still rebuilds the global graph, but that
rebuild can reuse eligible facts for unchanged files.

Corrupt entries and entries with an invalid format or algorithm version are
removed and rebuilt. There is no legacy cache reader. Successful reads refresh
the entry's eviction timestamp, and pruning removes the least recently used
entries after each run when the configured limit is exceeded.

The metrics record reports graph and parser-fact hits and misses, input files
and bytes, writes and written bytes, current and maximum cache bytes,
`elapsed_ms`, RSS when available, and any untracked inputs.

Local release-build measurements for the initial implementation were:

| Workload | Cold | Warm | Output |
| --- | ---: | ---: | --- |
| CodeAtlas self-inspection | 22.525 s | 0.254 s | Identical SHA-256 |
| Goobits code check | 119.070 s | 11.199 s | Identical SHA-256 |

These measurements show that identical warm reruns are fast for the measured
checkouts. They are not universal performance guarantees. Changed runs reuse
eligible parser facts but still rebuild the global graph, so their speedup is
smaller and workload-dependent.

## Evidence Posture

CodeAtlas distinguishes direct structural evidence, inferred reachability, and
boundary-limited suspicion. Treat its output as evidence to verify:

- Public "no known consumer" findings are advisory because outside consumers
  may exist.
- Reflection, plugins, decorators, macros, dynamic imports, generated code,
  and dynamic SQL can limit completeness.
- A bounded context page is not a completeness claim unless all continuation
  pages are consumed.
- Static checks can replace redundant structural assertions, not behavioral
  tests whose contract is runtime behavior.

## Local Development

Keep Cargo output outside the checkout, then run the complete local gate:

```bash
export CARGO_TARGET_DIR="${XDG_CACHE_HOME:-$HOME/.cache}/codeatlas/target"
pnpm check
```

Useful focused checks:

```bash
pnpm test
pnpm run spec:check
pnpm run self:check
pnpm run test:postgres-live
```

`pnpm test` runs wrapper tests and the default Rust suite. The PostgreSQL live
smoke is explicit because it requires a local service. HTTP execution remains
blocked until the kernel enforcement and migrated smoke suite are available.
`pnpm run self:check` writes its report below `CARGO_TARGET_DIR`.

Verification is local. The repository does not use automatic hosted CI, and no
hosted workflow should be dispatched as part of ordinary development.

## License

CodeAtlas is distributed under the terms in [LICENSE](LICENSE).
