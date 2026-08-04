# HTTP and PostgreSQL evidence parity

Status: Accepted follow-on; implementation waits for stable callable and
PostgreSQL contract evidence

Decision scope: Repository scope, HTTP/PostgreSQL usage and inspection,
HTTP/PostgreSQL docs, truthful code/HTTP initialization, and cross-subject
lexicon evidence

Depends on:

- [`codeatlas-evidence-lifecycle-cli.md`](codeatlas-evidence-lifecycle-cli.md)
  for the final verb-subject grammar and file-output contract.
- [`codeatlas-execution-kernel-http-fuzz.md`](codeatlas-execution-kernel-http-fuzz.md)
  for typed artifact references when an optional observation enriches static
  PostgreSQL evidence.
- [`codeatlas-code-fuzzing.md`](codeatlas-code-fuzzing.md) and
  [`codeatlas-postgres-fuzzing.md`](codeatlas-postgres-fuzzing.md) for the final
  callable/query contract identities consumed by inspection and lexicon.

Unblocks: Richer performance attribution and repository-wide target selection

## Decision

CodeAtlas will fill the meaningful static-evidence gaps for HTTP and
PostgreSQL, but it will do so through one repository-scope contract and
domain-owned evidence graphs rather than by adding empty commands for visual
symmetry.

The scheduled public surface is:

```text
usage http|postgres
inspect http|postgres
docs http|postgres
init code|http
lexicon repository --subjects code,http,postgres
```

The existing `init postgres` and `lexicon code` remain distinct meaningful
operations. There is no `init architecture`: generating governing intent from
source would invent owner policy. There is no `test code|http`: callable and
HTTP live exploration is owned by `fuzz`.

These commands remain honest about incomplete evidence:

- A route with no repository caller is **unreferenced in the repository**, not
  unused by external consumers.
- A table or column with no statically resolved query touch is **untouched by
  known static queries**, not unused when dynamic SQL or external consumers
  may exist.
- Documentation renders only sourced descriptions, comments, types, schemas,
  locations, and completeness. It never writes plausible-sounding prose.
- Inspection projects a bounded graph that already exists; it never performs
  target calls or unbounded repository searches.
- Initialization proposes discovered facts and conservative defaults. It does
  not invent production URLs, secrets, completeness assertions, or effects.

## Options considered

| Option | Fit | Tradeoff | Verdict |
|---|---|---|---|
| Leave the sparse matrix permanently | Smallest product | Strands high-value evidence already collected by HTTP/PostgreSQL inventories | Reject |
| Add each command directly in its CLI/domain module | Fast first command | Duplicates workspace resolution, graph projection, config editing, and completeness rules | Reject |
| Create one universal code/HTTP/SQL graph and schema | Superficial reuse | Erases domain identities and makes weak edges look interchangeable | Reject |
| Share repository scope and bounded projection; keep domain graphs/reports | Reuses identical mechanics while preserving semantics | Requires a foundation phase before visible commands | Adopt |
| Fill every verb/subject cell | Rectangular help output | Creates meaningless commands and fabricated evidence | Reject |

## Existing-first check

The implementation extends current owners:

- `src/config/analysis.rs` already resolves configured projects and local
  member settings, but its reusable repository discovery is embedded in the
  code-analysis model.
- `src/package/workspace.rs` already discovers bounded pnpm workspace members.
- `src/commands/postgres.rs` already previews and safely inserts one strict JSON
  config property; its generic edit mechanics are currently trapped in the
  PostgreSQL command.
- `src/http/model.rs` and `src/http/source.rs` already expose operation keys,
  route source evidence, confidence, and completeness.
- `src/postgres/source/*` already exposes migrations, static query IDs,
  parameters, source locations, and partial/complete contract evidence.
- `src/context_slice/*` already implements deterministic depth/node limits and
  continuation, but it is coupled to the code source graph.
- `src/commands/docs.rs`, `src/outputs/reference.rs`, and
  `src/outputs/html.rs` already own code reference rendering and themes.
- `src/lexicon/*` already owns normalization, concept policy, provenance, and
  deterministic candidate ordering.

No existing module owns cross-subject repository scope, HTTP consumer
evidence, PostgreSQL object-touch evidence, HTTP/PostgreSQL dependency graphs,
or repository lexicon inputs. New files below are focused owners, not parallel
versions of current behavior.

## Repository scope contract

`--workspace` cannot continue to mean “use code-specific pnpm discovery” while
HTTP and PostgreSQL independently resolve only root configuration. Before
adding their usage commands, CodeAtlas extracts one `RepositoryScope` from the
current project/workspace resolution.

The resolved value contains, in deterministic order:

- Repository root and selected member roots.
- Stable member IDs and repository-relative roots.
- Exact config file and digest for every member.
- Code analysis contexts plus HTTP and PostgreSQL contract ownership.
- Discovery kind and completeness/diagnostics.
- Nested boundaries and excluded roots.

The initial discovered workspace implementation remains truthful about its
current pnpm support. Explicit `projects` and local `codeatlas.json` files cover
other multi-project layouts until a separately tested Cargo/Python workspace
adapter exists. The public report records this capability; it does not imply
that “workspace” means every ecosystem merely because CodeAtlas parses their
source languages.

One flattened `RepositoryScopeArgs` owns `--workspace`. Code, tests, lexicon,
HTTP, and PostgreSQL consumers reuse it. Subject commands do not re-scan for
members or reinterpret local config.

## Shared config editing

`init` is preview-first. Without `--write`, it emits one deterministic proposed
config fragment and changes nothing. With `--write`, one strict JSON editor:

- Loads and validates the exact current file.
- Refuses an existing subject property instead of merging ambiguously.
- Applies one canonical insertion while preserving unrelated values.
- Re-parses the result through `CodeAtlasConfig` before replacement.
- Writes only the selected `codeatlas.json`; no source file is touched.

The generic editor moves from `src/commands/postgres.rs` to the config owner.
There is no second string-splicing path for code or HTTP.

`init code` may propose detected languages, package entrypoints, and explicit
analysis projects/contexts. `init http` may propose source roots, a discovered
OpenAPI file, and a stable contract ID. It defaults `source_complete` to false
and never proposes an execution target, base URL, authentication, secret, or
effect policy.

## Usage evidence

### HTTP

`usage http` joins an `HttpOperation`/`HttpSourceOperation` key to known static
repository evidence:

- Source handler declaration.
- Code call/import references that resolve to the handler or generated client.
- Test route strings, typed clients, request helpers, and declared witnesses
  with explicit confidence.
- OpenAPI links and operation IDs.

Its report includes per-operation known callers/tests, evidence locations,
classification, and completeness. The negative classification is
`no_known_repository_consumer`, never `unused_route`. An explicit external-use
declaration can explain an otherwise unreferenced public operation without
hiding the underlying count.

### PostgreSQL

`usage postgres` resolves known static query touches to tables and columns from
parsed SQL/query contracts. It reports:

- Query-to-table and query-to-column edges with source evidence.
- Migration-created objects with no known static query touch.
- Dynamic/unsupported SQL and its effect on completeness.
- Optional catalog objects from an exact `PostgresObservation` reference,
  loaded without a live database call.

The negative classification is `no_known_static_query_touch`, never
`unused_table` or `unused_column`. DDL references do not masquerade as runtime
query consumers.

Usage remains evidence, not a gate. Policy belongs in a future explicit
`check` rule only after completeness requirements and suppressions have stable
semantics.

## Documentation evidence

`docs http` renders operations, paths, parameters, bodies, responses, security
scheme references, source locations, and descriptions that exist in OpenAPI or
explicit config. Source-only routes render their known transport shape and a
visible missing-description state.

`docs postgres` renders contracts, migrations, static queries, parameters,
known tables/columns/constraints/indexes, source locations, and SQL/database
comments that are present in static evidence or an optional exact observation.
The command never starts PostgreSQL. Without catalog evidence it labels live
catalog sections unavailable rather than inferring them.

Both use the existing Markdown/HTML theme and output infrastructure. Each
domain owns its reference model and factual text; `src/outputs` owns format
only. `--check` compares generated bytes with one explicit output file and
never rewrites it.

## Bounded inspection graphs

The shared inspection primitive owns deterministic target resolution errors,
depth/node limits, ordering, truncation, and continuation cursors. It is generic
over a typed node/edge view and does not define cross-domain node meaning.
Existing code context slicing is refactored to consume this owner; there is not
one paginator for code and another for HTTP/PostgreSQL.

Domain graphs remain separate:

- HTTP nodes: contract, operation, schema, source handler, known caller, test,
  and optional fuzz observation/report. Edges name bindings, calls, witnesses,
  schemas, and evidence links.
- PostgreSQL nodes: contract, migration, query, parameter, table, column,
  constraint/index, source callsite, and optional observation. Edges name
  creation, alteration, touch, parameter binding, execution ownership, and
  evidence links.

`inspect http "GET /users/{id}"` and `inspect postgres <table|query-id>` resolve
exact subject targets, project a bounded subgraph, and emit a stable report.
Ambiguous targets fail with sorted candidates; no fuzzy “best guess” is made.

## Repository lexicon

`lexicon repository --subjects code,http,postgres` feeds one lexicon engine
with typed term evidence:

- Code symbols, callables, field/type names, and documentation terms.
- HTTP path segments, operation IDs, parameter/schema names, and sourced
  descriptions.
- PostgreSQL contract, schema, table, column, query, parameter, and comment
  terms.

Every term retains subject, owner, exact target, source, and confidence.
Normalization and concept policy remain in `src/lexicon`; domain adapters only
extract terms. The result may flag `User`, `/users`, and `users` as related
evidence, but it does not declare them semantically identical without a policy
or corroboration rule.

`lexicon code` remains the focused code-only command. `lexicon repository` is
not an alias and is not implemented as repeated subprocess calls. Structured
report changes receive one schema/algorithm identity bump with exact fixture
regeneration; no compatibility envelope remains.

## Determinism and performance

- Resolve repository scope once per command and reuse each member's source
  index and subject inventory.
- Parse each OpenAPI document, SQL source, manifest, and source input at most
  once per snapshot.
- Join operation/query/object identities with ordered maps and stable exact
  keys; do not use path-string heuristics when semantic evidence exists.
- Bound graph nodes, edges, term counts, description bytes, output bytes, and
  continuation state.
- Include source/index/config/contract digests in usage, inspection, docs, and
  lexicon reports so stale joins are detectable.
- Optional observations resolve through the kernel `ArtifactRef` owner and
  never trigger hidden execution.

## Phase 1: Repository scope and config-edit consolidation

Status: [ ] Not started

LOC: +450-700 / -180-300

Verify: Single-project and pnpm-workspace code/test behavior remains exact;
HTTP/PostgreSQL contracts resolve through the same ordered member scope; local
config boundaries and digests are stable; init preview writes nothing; all
three init subjects use one validated JSON edit path.

```text
+ src/config/repository.rs
+ src/config/edit.rs
+ src/cli/scope.rs
+ tests/repository_scope.rs
~ src/config/mod.rs
~ src/config/analysis.rs
~ src/package/workspace.rs
~ src/cli/init.rs
~ src/cli/usage.rs
~ src/commands/postgres.rs
~ tests/cli_contract.rs
```

## Phase 2: HTTP and PostgreSQL usage evidence

Status: [ ] Not started

LOC: +650-950 / -80-160

Verify: Known route callers/tests and known query object touches resolve to
exact source evidence; external HTTP and dynamic SQL incompleteness remain
visible; no report uses `unused` without complete evidence; no target or live
database call occurs; repeated output is byte-identical.

```text
+ src/http/usage.rs
+ src/postgres/usage.rs
+ tests/usage_http_postgres.rs
~ src/http/mod.rs
~ src/http/model.rs
~ src/postgres/mod.rs
~ src/postgres/model.rs
~ src/postgres/source/query.rs
~ src/cli/usage.rs
~ src/commands/http.rs
~ src/commands/postgres.rs
```

## Phase 3: Truthful docs and generalized init

Status: [ ] Not started

LOC: +700-1,050 / -100-220

Verify: HTTP/PostgreSQL Markdown and HTML contain only sourced evidence,
surface missing descriptions/catalog evidence, and are deterministic; `--check`
never writes; init code/HTTP preview is zero-write; `--write` changes only the
selected strict config and refuses existing ownership.

```text
+ src/http/docs.rs
+ src/postgres/docs.rs
+ tests/docs_http_postgres.rs
+ tests/init_cli.rs
~ src/cli/docs.rs
~ src/cli/init.rs
~ src/commands/docs.rs
~ src/commands/http.rs
~ src/commands/postgres.rs
~ src/config/http.rs
~ src/outputs/reference.rs
~ src/outputs/html.rs
~ tests/cli_contract.rs
```

## Phase 4: Shared bounded projection and domain inspection graphs

Status: [ ] Not started

LOC: +900-1,400 / -220-400

Verify: Code context output remains exact after projection extraction; HTTP and
PostgreSQL exact targets return stable bounded graphs; depth/node limits,
ambiguous targets, cursors, digest invalidation, workspace ownership, and
optional wrong-kind/stale observations are covered; inspection makes zero
target calls.

```text
+ src/inspection/mod.rs
+ src/inspection/model.rs
+ src/inspection/projection.rs
+ src/http/graph.rs
+ src/http/inspect.rs
+ src/postgres/graph.rs
+ src/postgres/inspect.rs
+ tests/inspect_http_postgres.rs
~ src/main.rs
~ src/context_slice/mod.rs
~ src/context_slice/model.rs
~ src/context_slice/pagination.rs
~ src/context_slice/slice.rs
~ src/cli/inspect.rs
~ src/commands/context_slice.rs
~ src/commands/http.rs
~ src/commands/postgres.rs
~ tests/cli_contract.rs
```

## Phase 5: Cross-subject repository lexicon

Status: [ ] Not started

LOC: +500-800 / -80-160

Verify: Code/HTTP/PostgreSQL term evidence retains exact subject ownership and
provenance; configured subject selection is deterministic; unsupported or
incomplete domains stay visible; cross-subject candidates never assert
semantic equivalence without policy; focused `lexicon code` remains exact.

```text
+ src/lexicon/repository.rs
+ src/lexicon/subject_terms.rs
+ tests/lexicon_repository.rs
~ src/lexicon/mod.rs
~ src/lexicon/model.rs
~ src/lexicon/analyze.rs
~ src/cli/lexicon.rs
~ src/commands/lexicon.rs
~ src/http/mod.rs
~ src/postgres/mod.rs
~ tests/cli_contract.rs
~ docs/concepts/lexicon.md
```

## Phase 6: Consolidation, docs, and release hardening

Status: [ ] Not started

LOC: +200-350 / -180-350

Verify: Full required checks and dogfood pass; repository search finds no
subject-private workspace parser, duplicate config insertion, parallel graph
paginator, hidden live docs/usage/inspect call, `unused_route`,
`unused_table`, invented description, stale command matrix, compatibility
alias, or generated checkout residue.

```text
~ README.md
~ AGENTS.md
~ docs/concepts/lexicon.md
~ proposals/codeatlas-fuzz-performance.md
~ proposals/codeatlas-subject-evidence-parity.md
~ tasks/check-self.js
~ tests/cli_contract.rs
~ tests/repository_scope.rs
~ tests/usage_http_postgres.rs
~ tests/docs_http_postgres.rs
~ tests/inspect_http_postgres.rs
~ tests/lexicon_repository.rs
```

The result is intentionally net higher because HTTP/PostgreSQL consumer graphs
and reference documentation are new product evidence. It must not be higher
because code, HTTP, and PostgreSQL retain separate workspace discovery, config
editing, pagination, or term-normalization mechanics.

Total LOC: +3,400-5,250 / -840-1,590

## Layman's wins

- CodeAtlas can show which routes and database objects the repository actually
  uses, while being honest about external and dynamic consumers it cannot see.
- HTTP and database contracts become inspectable and documentable with the same
  predictable grammar as code.
- Onboarding proposes safe config instead of making users hand-discover every
  source root.
- Cross-domain naming evidence connects `User`, `/users`, and `users` without
  pretending they are automatically the same thing.
