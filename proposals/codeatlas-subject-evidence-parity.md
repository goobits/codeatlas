# HTTP and PostgreSQL evidence parity

Status: Accepted follow-on; static Phases 1-6 are complete. Optional
observation enrichment still waits for stable runtime artifact identities.

Decision scope: Repository scope, HTTP/PostgreSQL usage and inspection,
HTTP/PostgreSQL docs, truthful code/HTTP initialization, and cross-subject
lexicon evidence

Depends on:

- [`codeatlas-evidence-lifecycle-cli.md`](codeatlas-evidence-lifecycle-cli.md)
  for the final verb-subject grammar and file-output contract.
- [`codeatlas-execution-kernel-http-fuzz.md`](codeatlas-execution-kernel-http-fuzz.md)
  for typed artifact references when an optional observation enriches static
  PostgreSQL evidence.
- [`codeatlas-structured-callable-evidence.md`](codeatlas-structured-callable-evidence.md)
  and [`codeatlas-postgres-fuzzing.md`](codeatlas-postgres-fuzzing.md) Phase 1
  for static callable/query identities consumed by usage and docs.
- [`codeatlas-code-fuzzing.md`](codeatlas-code-fuzzing.md) and the live
  PostgreSQL phases for final report/observation identities consumed by
  inspection and repository lexicon evidence.

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

Status: [x] Complete

LOC: +1,304 / -528 (measured)

Verify: Single-project and pnpm-workspace code/test behavior remains exact;
HTTP/PostgreSQL contracts resolve through the same ordered member scope; local
config boundaries and digests are stable; init preview writes nothing; the
existing PostgreSQL initializer uses the one validated JSON edit path that
Phase 3 extends to code and HTTP.

Execution checkpoint (2026-08-05): the clean CodeAtlas HEAD was registered with
the installed stable Mill without writing source. Its capability report exposed
`verify` only and reported rename, move, extract, and transaction planning as
unavailable because no language backend was active. Phase 1 therefore uses
ordinary reviewed edits rather than an unreviewable TypeMill workaround.

Completion checkpoint (2026-08-05): `RepositoryScope` now owns project and
pnpm discovery, ordered members, immutable config snapshots and digests, code
contexts, and member-qualified HTTP/PostgreSQL contracts. Explicit project
configs and pnpm members reuse the same loaded snapshot; a regression changes
a local config after load and proves resolution retains the original evidence
without reopening it. One flattened CLI argument owns `--workspace`, and the
PostgreSQL initializer now delegates to the shared strict editor and shared
file-replacement owner. Ownership searches find one workspace resolver, one
CLI workspace flag, and one config insertion path.

The full `pnpm check` surface passed. Bounded self-dogfood scanned 284 files and
2,902 symbols, produced 370 advisory code findings with zero gates, resolved
the inspected output callable through the new filesystem owner, and emitted
the standard scan/check/usage/inspect/lexicon/test artifacts under the external
task root. That inspection caught and then verified the intentional provenance
change in `tasks/check-self.js`.

```text
+ src/config/repository.rs
+ src/config/edit.rs
+ src/cli/scope.rs
+ tests/repository_scope.rs
~ src/config/mod.rs
~ src/config/analysis.rs
~ src/filesystem.rs
~ src/cli/mod.rs
~ src/cli/baseline.rs
~ src/cli/check.rs
~ src/cli/diff.rs
~ src/cli/lexicon.rs
~ src/cli/scan.rs
~ src/cli/usage.rs
~ src/commands/architecture/source_check.rs
~ src/commands/dead_code.rs
~ src/commands/diff.rs
~ src/commands/lexicon.rs
~ src/commands/output.rs
~ src/commands/postgres.rs
~ src/commands/testing.rs
~ src/http/target/tests.rs
~ src/tests/dead_code/mod.rs
~ src/tests/dead_code/workspace.rs
~ src/tests/testing.rs
~ tasks/check-self.js
```

## Phase 2: HTTP and PostgreSQL usage evidence

Status: [x] Complete

LOC: +2,231 / -49 product and tests, plus 786 generated schema lines

Verify: Known route callers/tests and known query object touches resolve to
exact source evidence; external HTTP and dynamic SQL incompleteness remain
visible; no report uses `unused` without complete evidence; no target or live
database call occurs; repeated output is byte-identical.

Completion checkpoint (2026-08-05): `usage http` and `usage postgres` now
consume one resolved `RepositoryScope` and emit the registered
`codeatlas.http-usage/v1` and `codeatlas.postgres-usage/v1` artifacts. HTTP
loads only local OpenAPI files, never invokes command/URL/target providers,
joins semantic handler references with contextual static route literals, and
retains unmatched external declarations when an incomplete inventory cannot
validate them. Self-dogfood caught and removed a false-positive path where the
root route matched every unrelated `"/"` literal. PostgreSQL reuses the one SQL
lexer, extracts only statically supported schema definitions, keeps DDL from
counting as usage, and never resolves a configured live target.

The acceptance fixture proves byte-identical repeat runs, exact caller/test and
query-touch evidence, provider and database zero-call behavior, visible
dynamic/catalog/external incompleteness, no false `unused_*` vocabulary, and
strict rejection of unknown external operations only when the local inventory
is complete. The published-schema updater changed exactly the two new schema
files. Full `pnpm check` passed 403 Rust unit tests, 37 Rust integration tests,
15 Node tests, architecture-spec drift, formatting, warning-denying all-target
Clippy, the standard self-audit, and package assembly. Final HTTP self-dogfood
reported three discovered operations with byte-identical repeated output; the
PostgreSQL self-report truthfully contained no members because this repository
declares no PostgreSQL contract.

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

Status: [x] Complete

LOC: +3,603 / -499 product and tests (measured)

Verify: HTTP/PostgreSQL Markdown and HTML contain only sourced evidence,
surface missing descriptions/catalog evidence, and are deterministic; `--check`
never writes; init code/HTTP preview is zero-write; `--write` changes only the
selected strict config and refuses existing ownership.

- [x] Extract one presentation-only reference-document renderer, one reusable
  local HTTP inventory collector, and one subject-level static PostgreSQL
  schema owner before adding their second consumers. Preserve current code
  docs bytes and current public inventory schemas.
- [x] Collect OpenAPI descriptions and source-route locations in the same
  bounded HTTP parse/scan used by inventory; visibly label source-only or
  undocumented operations without inventing text or invoking a provider.
- [x] Reuse the collected PostgreSQL contracts, SQL lexer, query contracts,
  migrations, and static schema facts to render tables, columns, constraints,
  indexes, comments, parameters, and visible unavailable catalog evidence.
- [x] Add deterministic Markdown/HTML `docs http|postgres` commands with one
  explicit output path for `--check`; exact checks remain read-only.
- [x] Generalize the one strict config editor to subject-owned fragments, then
  add zero-write `init code|http` previews and exact `--write` behavior. HTTP
  discovery remains local-file/source-only, defaults `source_complete` false,
  and never proposes a target, URL, credential, secret, or effect policy.
- [x] Prove ownership refusal, concurrent-config protection, ambiguous OpenAPI
  refusal, bounded discovery, zero hidden calls, deterministic bytes, and
  unchanged source/config during preview through focused integration tests.
- [x] Run schema drift, CLI contract checks, the full required suite, bounded
  CodeAtlas self-dogfood, generated-state audit, checklist synchronization,
  and one scoped Phase 3 commit.

Completion checkpoint (2026-08-05): one presentation-only evidence document
model and renderer now serves HTTP and PostgreSQL without changing code-doc
bytes. One local HTTP contract collector serves inventory, usage, and docs;
one static PostgreSQL schema owner serves usage and docs, and the retired
usage-private schema parser is deleted. HTTP/OpenAPI and static PostgreSQL
descriptions remain sourced and bounded, absent evidence is visible, and docs
never invoke configured providers or a database. The shared strict config edit
path now owns deterministic preview/write for code, HTTP, and PostgreSQL;
ambiguous or already-owned configuration fails before mutation.

The final `pnpm check` passed 409 Rust unit tests, every non-live integration
suite, all Node tests, published-schema and architecture drift, formatting,
warning-denying all-target Clippy, self-audit, and package assembly. Bounded
self-dogfood scanned 299 files and 3,139 symbols, retained 417 advisory code
findings with zero gates, resolved the exact PostgreSQL docs builder into a
128-node bounded slice, and reported 12 test contexts. Repeated HTTP and
PostgreSQL Markdown was byte-identical and exact `--check` passed; the
repository truthfully produced zero HTTP operations and no PostgreSQL members.
Dogfooding exposed a route annotation embedded in this detector's own Rust
test fixture; the existing annotation owner now accepts markers only from
actual Rust, JavaScript/TypeScript/Svelte, or Python comments and regression
tests cover string, raw-string, template, and triple-string counterexamples.
No schema drift or generated checkout state remained.

```text
+ src/http/docs.rs
+ src/http/repository.rs
+ src/postgres/docs.rs
+ src/postgres/static_schema.rs
+ tests/docs_http_postgres.rs
+ tests/init_cli.rs
- src/postgres/usage/schema.rs
~ src/cli/docs.rs
~ src/cli/init.rs
~ src/cli/mod.rs
~ src/commands/docs.rs
~ src/commands/mod.rs
~ src/commands/postgres.rs
~ src/config/edit.rs
~ src/config/http.rs
~ src/config/mod.rs
~ src/http/mod.rs
~ src/http/openapi/mod.rs
~ src/http/source.rs
~ src/http/usage.rs
~ src/languages/mod.rs
~ src/languages/registry.rs
~ src/postgres/mod.rs
~ src/postgres/source/mod.rs
~ src/postgres/source/query.rs
~ src/postgres/target/query/classification.rs
~ src/postgres/target/query/lexer.rs
~ src/postgres/usage.rs
~ src/outputs/reference.rs
~ src/outputs/markdown.rs
~ src/outputs/html.rs
~ tests/repository_scope.rs
```

## Phase 4: Shared bounded projection and domain inspection graphs

Status: [x] Static track complete; optional observation enrichment remains a
separately visible wait for accepted HTTP/PostgreSQL observation identities

LOC: +2,753 / -368 product and tests, plus 1,973 generated schema lines

Verify: Code context output remains exact after projection extraction; HTTP and
PostgreSQL exact targets return stable bounded graphs; depth/node limits,
ambiguous targets, cursors, digest invalidation, workspace ownership, and
optional wrong-kind/stale observations are covered; inspection makes zero
target calls.

Execution checkpoint (2026-08-05): the clean committed CodeAtlas workspace was
re-indexed by installed stable Mill 0.8.18, which advertised file moves. The
zero-write preview for moving `src/context_slice/pagination.rs` to the shared
inspection owner returned `target_not_supported` with no plan or source edit.
This exact transformation is therefore proceeding as ordinary reviewed
CodeAtlas edits; no TypeMill repository change is in scope.

Completion checkpoint (2026-08-05): code, HTTP, and PostgreSQL inspection now
share one bounded projection owner while retaining separate graph semantics.
The pre-extraction and post-extraction code-inspection fixtures are
byte-identical at
`be3f6dcdd070a354560832333148c57b5f2992393947762191ec1a8b5bc7f6c2`.
HTTP and PostgreSQL inspection resolve exact targets, reject sorted ambiguity,
page deterministically, invalidate stale cursors, retain member inventory and
source-graph digests, and make zero provider or database calls. Their two
namespaced schemas are registered and drift-tested.

Self-dogfood exposed two generic Rust resolution defects rather than inspection
defects: a same-named module could hide a callable re-export, and
`use path::{self, ...}` recorded the wrong local module alias. The shared Rust
resolver and fixture now cover both, glob imports no longer capture unrelated
absolute paths, the source-index algorithm is v9, and Rust module facts are v4
so warm caches cannot retain the old parse. Dedicated cold and warm checks are
byte-identical at
`28ee96c351e05bfba6245fc17aae3be91aee2a2701ce3fea66a00a191e7d1185`;
findings fell from 418 to 334 with zero gates and zero findings in the new
inspection paths. The full `pnpm check` passed 412 Rust unit tests with three
intentional ignores, every non-live integration suite, schema/spec drift,
warning-denying Clippy, self-audit, and package assembly. No checkout-local
generated state remains.

- [x] Extract one shared direction, limit validation, deterministic traversal,
  graph/request digest, cursor, and page-ownership primitive. Preserve the
  context-slice v5 schema and the exact pre-extraction fixture bytes.
- [x] Reuse one collected HTTP inventory plus one usage analysis to build
  contract, operation, schema, handler, caller, and test nodes with typed
  edges; resolve only exact operation keys or exact node IDs.
- [x] Consolidate collected PostgreSQL source and static-schema evidence behind
  one repository owner shared by usage, docs, and inspection, then build
  contract, source, query, parameter, object, constraint/index, and callsite
  nodes with typed edges.
- [x] Add one flattened inspect target/projection argument owner and zero-call
  `inspect http|postgres` orchestration with deterministic JSON output.
- [x] Publish the two namespaced report schemas and prove exact targets,
  ambiguity, direction, depth/node budgets, pagination reconstruction, stale
  cursor rejection, workspace ownership, deterministic bytes, and zero hidden
  provider/database calls.
Deferred observation enrichment is tracked only by Phase 15 of the canonical
v1 completion tracker. It will consume optional HTTP/PostgreSQL observations
only through final typed artifact identities and reject wrong-kind or stale
evidence rather than creating a path-only or provisional artifact owner here.
- [x] Run the focused and full required checks, exact code-inspection byte
  comparison, bounded self-dogfood, generated-state audit, checklist sync, and
  one scoped static Phase 4 commit.

```text
+ src/inspection/mod.rs
+ src/inspection/model.rs
+ src/inspection/projection.rs
+ src/http/graph.rs
+ src/http/inspect.rs
+ src/postgres/graph.rs
+ src/postgres/graph/identity.rs
+ src/postgres/graph/model.rs
+ src/postgres/inspect.rs
+ src/postgres/repository.rs
+ tests/inspect_http_postgres.rs
+ tests/fixtures/dead-code/rust/src/internal/collision.rs
+ schemas/codeatlas-http-inspection-v1.schema.json
+ schemas/codeatlas-postgres-inspection-v1.schema.json
~ src/main.rs
~ src/context_slice/mod.rs
~ src/context_slice/model.rs
- src/context_slice/pagination.rs
~ src/context_slice/slice.rs
~ src/cli/inspect.rs
~ src/cli/mod.rs
~ src/commands/http.rs
~ src/commands/postgres.rs
~ src/http/mod.rs
~ src/http/usage.rs
~ src/languages/rust/parser.rs
~ src/languages/rust/reachability.rs
~ src/languages/rust/reachability/resolver.rs
~ src/source_index/mod.rs
~ src/postgres/docs.rs
~ src/postgres/mod.rs
~ src/postgres/usage.rs
~ src/published_schemas.rs
~ src/tests/dead_code/rust.rs
~ tests/fixtures/dead-code/rust/src/internal/consumer.rs
~ tests/fixtures/dead-code/rust/src/internal/model.rs
~ tests/fixtures/dead-code/rust/src/internal/mod.rs
~ tests/fixtures/dead-code/rust/src/lib.rs
```

## Phase 5: Cross-subject repository lexicon

Status: [x] Complete

LOC: +2,152 / -15 product, tests, and canonical lexicon, plus 455 generated
schema lines (measured)

Verify: Code/HTTP/PostgreSQL term evidence retains exact subject ownership and
provenance; configured subject selection is deterministic; unsupported or
incomplete domains stay visible; cross-subject candidates never assert
semantic equivalence without policy; focused `lexicon code` remains exact.

Completion checkpoint (2026-08-05): `lexicon repository` now runs one bounded
analysis over explicitly selected code, HTTP, and PostgreSQL evidence. The
domain adapters reuse the existing code scan, local HTTP repository collector,
static PostgreSQL repository collector, graph identities, and one canonical
normalization/concept-policy engine; they do not invoke providers, targets, or
databases. Every normalized term retains its observed source spelling, subject,
role, owner, exact target, source, confidence, and completeness. Relationships
claim only `related_evidence`, use stable content identities, retain at most
128 round-robin occurrence references without erasing a smaller subject, and
publish exact retained/omitted counts. The report also enforces finite seed,
input-byte, normalized-term, relationship, and JSON-output ceilings.

The focused acceptance surface passed 415 unit tests with two intentional
ignores plus 26 CLI/domain integration tests. Warning-denying all-target Clippy
and the full `pnpm check` surface passed, including 15 Node tests, every
non-live Rust integration suite, schema/spec drift, self-audit, and packaging.
Self-dogfood emitted 15,948 term records and one cross-subject relationship;
the 288-target relationship retained 128 references and reported 160 omitted.
Reordered subject selection was byte-identical, focused `lexicon code` remained
exact at `44ccd9540c24e73135763e3a00f70994f26e607c1fb5e09ce066d6804a527fee`,
and no checkout-local Cargo or Node state remained.

```text
+ schemas/codeatlas-repository-lexicon-v1.schema.json
+ src/http/terms.rs
+ src/lexicon/code_terms.rs
+ src/lexicon/repository.rs
+ src/lexicon/subject_terms.rs
+ src/outputs/repository_lexicon.rs
+ src/postgres/terms.rs
+ tests/lexicon_repository.rs
~ docs/concepts/lexicon.md
~ src/cli/mod.rs
~ src/lexicon/mod.rs
~ src/cli/lexicon.rs
~ src/commands/lexicon.rs
~ src/http/mod.rs
~ src/http/openapi/mod.rs
~ src/lexicon/concepts.rs
~ src/outputs/mod.rs
~ src/postgres/graph.rs
~ src/postgres/graph/identity.rs
~ src/postgres/mod.rs
~ src/postgres/usage.rs
~ src/published_schemas.rs
```

## Phase 6: Consolidation, docs, and release hardening

Status: [x] Complete

LOC: +112 / -9 product configuration, tests, self-audit, and canonical public
documentation (measured)

Verify: Full required checks and dogfood pass; repository search finds no
subject-private workspace parser, duplicate config insertion, parallel graph
paginator, hidden live docs/usage/inspect call, `unused_route`,
`unused_table`, invented description, stale command matrix, compatibility
alias, or generated checkout residue.

Completion checkpoint (2026-08-05): repository searches and ownership review
found one `RepositoryScope` owner, one strict config editor, one bounded graph
projection/cursor owner, and one term-normalization owner. The static HTTP,
PostgreSQL, inspection, documentation, usage, and lexicon paths contain no
process, network, target, or database executor. Product output contains no
false `unused_route`, `unused_table`, or `unused_column` label, no retired
top-level command form, and no compatibility alias. Because the earlier phases
had already consolidated the real mechanics, this phase did not manufacture
deletions or a replacement abstraction merely to meet its forecast.

The repository self-config now asks semantic-role-sibling analysis to compare
the three repository-term adapters. It evaluated 50 nominations with the full
counterevidence checklist, retained no review candidate, and reported six
bounded omissions; the apparent HTTP/PostgreSQL member-term overlap remains
properly divided between thin adapters and the existing shared term core. The
expanded self-audit also validates the repository lexicon's exact subject set,
namespaced schema, evidence bound, retained/omitted arithmetic, and
evidence-only relationship claim.

The final self-audit scanned 314 files, 3,273 symbols, and 2,786 callables;
reported 338 advisory code findings with zero gates; analyzed three semantic
sibling sets with 362 evaluations and zero review candidates; and emitted
15,954 repository terms with one bounded cross-subject relationship. Focused
fixture dogfood exercised seven HTTP operations and 21 PostgreSQL objects from
13 queries, projected 10/55 HTTP/PostgreSQL nodes, rendered both references,
and passed exact `--check`. The full `pnpm check` surface passed 415 Rust unit
tests with two intentional ignores, every non-live integration suite, 15 Node
tests, schema/spec drift, formatting, warning-denying all-target Clippy,
self-audit, and package assembly. The public matrix matches executable help,
and no checkout-local Cargo, Node, or temporary state remains.

```text
~ README.md
~ codeatlas.json
~ src/config/mod.rs
~ proposals/codeatlas-fuzz-performance.md
~ proposals/codeatlas-subject-evidence-parity.md
~ tasks/check-self.js
```

The result is intentionally net higher because HTTP/PostgreSQL consumer graphs
and reference documentation are new product evidence. It must not be higher
because code, HTTP, and PostgreSQL retain separate workspace discovery, config
editing, pagination, or term-normalization mechanics.

Measured through Phase 6: +12,155 / -1,468 product, test, configuration, and
canonical-doc lines, plus 3,214 generated schema lines.

## Layman's wins

- CodeAtlas can show which routes and database objects the repository actually
  uses, while being honest about external and dynamic consumers it cannot see.
- HTTP and database contracts become inspectable and documentable with the same
  predictable grammar as code.
- Onboarding proposes safe config instead of making users hand-discover every
  source root.
- Cross-domain naming evidence connects `User`, `/users`, and `users` without
  pretending they are automatically the same thing.
