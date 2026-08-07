# Fuzzing and performance program

Status: Approved program; phased implementation in progress

Repository: CodeAtlas

Last reviewed: 2026-08-04

## Executive decision

CodeAtlas should support bounded, isolated fuzzing across callable code, HTTP,
and PostgreSQL plus evidence-backed performance tuning. The program keeps one
shared execution safety contract and domain-owned generators, harnesses, and
oracles.

This is not one all-or-nothing implementation proposal. It is a program index
for twelve independently reviewable proposals:

1. [`codeatlas-evidence-lifecycle-cli.md`](codeatlas-evidence-lifecycle-cli.md)
   hard-cuts the public CLI into a consistent evidence lifecycle.
2. [`codeatlas-published-schemas.md`](codeatlas-published-schemas.md) publishes
   drift-tested schemas for current reports and every new artifact.
3. [`codeatlas-hqa-seeding.md`](codeatlas-hqa-seeding.md) adds the CodeAtlas-only
   HQA application-inventory renderer without editing HQA.
4. [`codeatlas-execution-kernel-http-fuzz.md`](codeatlas-execution-kernel-http-fuzz.md)
   builds the execution kernel, proves a real sandbox, and migrates existing
   HTTP fuzzing.
5. [`codeatlas-structured-callable-evidence.md`](codeatlas-structured-callable-evidence.md)
   replaces signature heuristics with one four-language callable/effect model.
6. [`codeatlas-semantic-role-siblings.md`](codeatlas-semantic-role-siblings.md)
   adds bounded advisory evidence for conceptually duplicated sibling
   implementations.
7. [`codeatlas-code-fuzzing.md`](codeatlas-code-fuzzing.md) adds sandboxed
   Rust/Python/JavaScript/TypeScript callable fuzzing over accepted evidence.
8. [`codeatlas-postgres-fuzzing.md`](codeatlas-postgres-fuzzing.md) adds typed
   PostgreSQL parameter generation in the disposable database lifecycle.
9. [`codeatlas-subject-evidence-parity.md`](codeatlas-subject-evidence-parity.md)
   fills meaningful HTTP/PostgreSQL usage, inspection, documentation,
   initialization, and cross-subject lexicon gaps without forcing empty matrix
   cells.
10. [`codeatlas-performance-evidence.md`](codeatlas-performance-evidence.md)
   adds planned measurements, curves, baselines, regression gates, hotspot
   attribution, and self-performance evidence.
11. [`codeatlas-cost-guided-search.md`](codeatlas-cost-guided-search.md) is the
   separately gated bridge from accepted domain generators to explicit
   performance cost objectives.
12. [`codeatlas-source-impact.md`](codeatlas-source-impact.md) projects HQA
   retest hypotheses from the existing source graph and callable-effect model
   without creating target identity, graph edges, or a second analyzer.

The user authorized the full phased program. Code fuzzing, PostgreSQL fuzzing,
subject parity, profiler integration, and cost-guided search still retain their
technical continuation gates: authorization never turns a failed prerequisite
into a capability. This program edits CodeAtlas only; HQA, TypeMill, and any
neutral schema repository remain external owners.

## Review disposition

The external review of the original monolith was substantially correct.

### Accepted

- Split the original 6,250-9,300 LOC document at the kernel/HTTP boundary.
- Keep speculative cost-guided search outside base performance acceptance.
- Treat the sandbox as a first-class project with a capability matrix and a
  substantially larger estimate.
- Replace the TOML example with strict `codeatlas.json` JSON.
- Resolve `Observation` vocabulary as subject-qualified evidence captured from
  a current state or run, with separate numeric `Measurement` samples and no
  universal observation schema.
- Remove the invalid lexicon gate invocation from dogfood examples.
- Eliminate the top-level `observe` verb before performance work.
- Plan an explicit lexicon schema bump when structured callable JSON changes.
- Move durable TypeMill workflow instructions into `AGENTS.md`; it is not a
  CodeAtlas product dependency.
- Make the complete Cargo-backed self-dogfood checkpoint a Phase 1 gate.

### Adjusted

The plan/execute adoption concern is real, but the answer is not an unguarded
direct path. The kernel permits one-shot execution only for a checked-in,
preauthorized target that runtime evidence proves is fully local and disposable.
The command still persists a plan before the first call and invokes the exact
same plan executor. Remote, uncontained-effect, unknown, or incompletely
isolated targets never become single-shot. Those targets require an explicitly
reviewed plan ID and may still block; incompletely isolated targets block
unconditionally because review cannot supply a capability. Corroborated
sandbox-contained mutation is not a second exception path; it remains typed
effect evidence owned by the target classifier.

The sandbox consequence is also capability-based rather than a blanket macOS
ban. Planning works everywhere. Local execution works only where a verified
container or future native backend proves all required capabilities. A macOS or
Windows host without such a backend is plan-only; a verified container backend
may make it eligible.

### Rejected

- Shipping new commands beside old commands during migration.
- Preserving `--max-examples`, direct execution, or output-path aliases.
- Adding symmetry-only CLI subjects before their evidence models exist.
- Treating static complexity or type evidence as runtime correctness or hotspot
  proof.

### Accepted follow-up corrections

- PostgreSQL DML is always reviewed-plan execution; checked-in policy never
  makes a mutating workload single-shot.
- Reviewed authorization cannot waive isolation. Missing capabilities block
  code, HTTP, PostgreSQL, performance, and cost search before the first call.
- PostgreSQL depends on the verified sandbox gate and emits a versioned
  observation consumed by zero-call baseline/diff.
- Profiler integration has a continuation gate separate from base performance.
- The kernel owns reproducer-to-plan replay, typed artifact IDs/references, the
  complete shared limit flags, target classification, redaction, and cleanup
  leases.
- The OCI probe covers rootless and nested-container environments; the HTTPS
  proxy terminates TLS because CONNECT-level counting cannot bound requests.
- Cost search requires explicit metric-feedback/shrink extensions from domain
  adapters rather than assuming ordinary fuzz engines provide them.
- Kernel Phase 1 governance/self-audit can start alongside the CLI hard cut.

### Consolidated owners

- `src/fuzz/corpus.rs`: boundary descriptors, ordering, and deterministic
  pairwise selection; domains map/materialize their own types.
- `src/execution/target.rs`: target class, effect corroboration, capabilities,
  and single-shot eligibility.
- `src/execution/artifact.rs`: typed content identity, external store, reference
  resolution, digest linkage, retention, and byte limits.
- `src/cli/execution.rs`: flattened common execution/fuzz limit flags.
- `src/fuzz/reproducer.rs` plus kernel replay: one typed envelope and
  reproducer-to-plan derivation.
- `src/execution/redaction.rs`: secret scoping and fail-closed redaction.
- `src/execution/lease.rs`: process/proxy/container/database/scratch cleanup
  registration and verified release.
- `src/external_tool.rs`: pinned provisioning for every external engine/tool.
- `src/execution/resource.rs`: shared elapsed/CPU/RSS/process sampling
  primitives, without merging source telemetry and performance policy.

Callable, PostgreSQL, and OpenAPI contracts remain separate. Domain oracles,
psql versus typed query execution, and fuzz versus performance reports also
remain domain-owned.

### Accepted schema and interop follow-up

- Structured callable/effect evidence is its own acceptance unit because
  lexicon, witnesses, semantic sibling analysis, and fuzzing all consume it.
- New CodeAtlas artifacts publish generated, drift-tested schemas. Existing
  report version fields are not churned; new artifacts use one namespaced
  schema-version string.
- Execution-plan IDs are SHA-256 content identities over the exact
  `atlas.codeatlas.dev/execution-plan/v1\n` domain separator plus RFC
  8785-canonicalized identity bytes. The kernel proposal owns the byte-level
  contract and test vectors.
- Semantic-role sibling evidence compares only configured sibling sets, counts
  discrete corroborations, records mandatory counterevidence, treats a shared
  trait contract as nomination rather than proof, and can never gate.
- The CodeAtlas-side HQA renderer consumes the published application-inventory
  v1 schema. It preserves partial completeness, uses a globally unique
  contract-plus-operation route ID, includes source and OpenAPI evidence, and
  never treats detector-specific `pathPattern` text as an HQA regex.
- The obsolete source-target field draft is superseded by the external
  core-plus-annotations contract. CodeAtlas reserves registered `codeatlas.*`
  annotation keys and does not publish a competing schema.
- A future HQA-hints producer validates every embedded source target through
  the complete external contract and refuses duplicate `(kind, key)` pairs
  before serialization; free-form sibling evidence never weakens either rule.
- Any future TypeMill receipt verification must be closed over the exact plan
  and receipt alone. Moved-path digest verification remains an explicit
  dependency on TypeMill's publication audit rather than an assumed field.

### Accepted subject-parity follow-up

- Schedule `usage http|postgres`, `inspect http|postgres`,
  `docs http|postgres`, truthful `init code|http`, and
  `lexicon repository --subjects code,http,postgres` as one separate static
  evidence proposal.
- Extract one repository/workspace scope and strict config-edit owner before
  adding subject commands. HTTP and PostgreSQL do not reinterpret
  `--workspace` or clone PostgreSQL init's JSON insertion.
- Keep HTTP and PostgreSQL dependency graphs domain-owned while sharing bounded
  graph projection, ordering, pagination, and cursor mechanics with code
  inspection.
- Keep negative evidence honest: no known repository route consumer and no
  known static SQL touch are not universal “unused” claims.
- Add PostgreSQL docs as well as HTTP docs because migrations, static queries,
  catalog observations, and sourced comments provide a real deterministic
  evidence contract.
- Do not add `init architecture`, `test code|http`, universal domain graphs, or
  blank symmetry commands.

## Product outcome

The stable product families are:

```text
Evidence lifecycle
  scan code|http|postgres|architecture|tests|performance
  check code|http|postgres|architecture|tests|performance
  baseline code|http|postgres|architecture|performance
  diff code|http|postgres|architecture|performance

Focused evidence
  usage code|http|postgres|tests
  inspect code|http|postgres|architecture
  lexicon code|repository
  docs code|http|postgres
  init code|http|postgres

Runtime exploration
  fuzz code|http|postgres
  test postgres
```

HTTP/PostgreSQL usage, inspection, docs, cross-domain lexicon, and truthful
code/HTTP init are scheduled in the separate subject-parity proposal. They are
not created merely for matrix symmetry.

## Architectural decision

```text
strict config and CLI
        |
        v
ExecutionPlan<Workload>                 zero calls
        |
        v
ExecutionGuard
  +----------------+----------------+----------------+
  | sandbox        | budget ledger  | artifact store |
  | capabilities   | pre-call gate  | private paths  |
  +----------------+----------------+----------------+
        |
        v
domain adapter
  +---------------+---------------+----------------+
  | callable code | HTTP          | PostgreSQL     |
  | native engine | Schemathesis  | typed session  |
  +---------------+---------------+----------------+
        |
        +--------------------+
        |                    |
        v                    v
  fuzz report/reproducer   performance observation/curve
```

`src/execution` owns mechanics whose meaning is identical across domains:

- Plans, authorization, receipts, and content identities.
- Target classification and the single-shot eligibility decision.
- Artifact storage/addressing, redaction, cleanup leases, replay derivation,
  tool provisioning integration, and resource sampling.
- Call, rate, concurrency, time, CPU, RSS, process, descriptor, output, result,
  and artifact limits.
- Cancellation and partial outcomes.
- Sandbox capability discovery and enforcement.
- External scratch and private artifact persistence.

Domain owners retain their semantics:

- `src/http`: OpenAPI, operations, Schemathesis, stateful links, HTTP oracles.
- `src/postgres`: SQL/query/catalog evidence, database lifecycle, value/result
  semantics.
- `src/languages/*`: syntax, callable contracts, harnesses, native engines.
- `src/fuzz`: cases, shared boundary descriptors/pairwise selection, outcome
  taxonomy, reductions, and the typed reproducer envelope.
- `src/performance`: workloads, measurements, curves, regressions, attribution.
- `src/testing`: public API witnesses and test inventory/impact.

Base performance evidence depends on execution, not fuzz. The optional
cost-guided bridge waits for accepted domain fuzz adapters and shares only
explicit typed artifacts. There is no universal value engine, universal oracle,
or domain-neutral schema parser.

## Cross-program safety contract

Every executing capability follows these rules:

- Preview resolves and persists an immutable plan while making zero target
  calls.
- Every target interaction reserves a permit before it occurs.
- Setup, readiness, authentication, stateful, reduction, retry, sample, and
  cleanup work consumes finite budgets.
- A finite cleanup allowance is reserved inside the whole-run ceilings before
  execution; ordinary work cannot consume it and cleanup gets no hidden extra
  calls.
- Failed calls are not refunded.
- No unlimited value or force path exists.
- CLI limits only tighten checked-in ceilings.
- Budget exhaustion, incomplete cleanup, unavailable isolation, and
  interruption are never passing outcomes.
- The checkout and runtime root are read-only; only external scratch is
  writable.
- Network, processes, environment, and external destinations are denied by
  default and exact when allowed.
- No host home, control sockets, or ambient credentials enter the sandbox;
  only exact target-scoped secret references may be injected, and their values
  never enter artifacts or captured output.
- Missing runtime capabilities block execution before the first target call.
- A reviewed plan supplies authorization only and never substitutes for a
  missing runtime capability.
- Production APIs and databases are not fuzz targets.

Static effects help eligibility decisions but never replace runtime isolation.

## Execution authorization

Two authorization modes use one executor:

### Reviewed

```bash
codeatlas fuzz <subject> --target <id>
codeatlas fuzz <subject> --plan plan_ABC --execute
```

Required for remote, uncontained-effect, exceptional, or incompletely
preauthorized targets.

### Preauthorized isolated

```bash
codeatlas fuzz <subject> --target <id> --execute
codeatlas scan performance --target <id> --execute
```

Available only when checked-in policy and runtime capabilities prove a local,
disposable target. CodeAtlas still writes the immutable plan before the first
call and records the authorization mode. This is convenience over the same
path, not a compatibility executor.

The kernel target classifier owns this decision. Remote, production,
unknown-effect, policy-exception, and uncontained-effect workloads never
qualify for single-shot execution. PostgreSQL DML retains its stricter
reviewed-plan rule even when its effects are sandbox-contained.

## Determinism and honesty

CodeAtlas makes these deterministic for identical evidence:

- Exact target and contract resolution.
- Eligibility and block reasons.
- Effect and isolation requirements.
- Boundary corpus and ordering.
- Concrete seed, tool/engine identities, and hard ceilings.
- Canonical plan IDs, normalized report ordering, and capable replay.

Runtime timing, RSS, CPU, allocation, and profiler samples are measured, not
deterministic. Performance artifacts therefore include environment, cold/warm
state, size ladder, individual samples, robust aggregates, noise, and
capability evidence.

The lexicon distinguishes:

- **Observation:** subject-qualified evidence captured from a particular state
  or run; not a universal cross-domain schema.
- **Measurement:** one numeric performance sample.
- **Optimization candidate:** static reason to measure.
- **Hotspot:** runtime-attributed material cost.
- **Regression:** measured budget/curve degradation.
- **Failure:** a named runtime oracle was violated.

Types never prove semantic correctness. A wrong-answer claim requires an
invariant, roundtrip, model, reference, or differential oracle.

## Platform commitment

Portable support means scans and plans, not unsafe host execution.

| Environment | Static evidence and plans | Local managed execution |
|---|---:|---:|
| Verified container backend | Yes | Eligible for proven capabilities |
| Future verified native backend | Yes | Eligible for proven capabilities |
| macOS/Windows without verified backend | Yes | Blocked |
| Any incomplete/no backend | Yes | Blocked |

The kernel proposal commits to one verified OCI-compatible container backend
before follow-on code fuzzing. A backend name does not grant capability;
conformance tests prove mounts, scratch, network, process/resource limits, and
cleanup. Native backends remain future work until they pass the same suite.

## Artifact lifecycle

The kernel gives plans, receipts, observations, baselines, reproducers, and
reports typed content IDs (`plan_<digest>`, `observation_<digest>`, and so on).
One `ArtifactRef` resolver accepts a managed ID or explicit exported file,
rehashes it, and rejects wrong-kind/schema evidence before execution.

- `scan performance` and `test postgres` are the explicit live evidence owners.
- `check`, `baseline`, and `diff performance` consume observations and make zero
  workload calls.
- `baseline` and `diff postgres` consume a `test postgres` observation and make
  zero live database calls.
- Static code, HTTP, and architecture evidence may be gathered directly without
  target execution.
- Plans, receipts, private reports, harnesses, reproducers, profiles, caches,
  and temporary databases live under external state by default.
- Explicit `--out` always names one file.

## Dogfooding contract

Every child proposal records a before/after subset of:

```text
codeatlas scan code --scope source --all --format json
codeatlas check code
codeatlas usage code
codeatlas inspect code <exact-target>
codeatlas lexicon code --format json
codeatlas scan tests
codeatlas check tests
```

Later phases add `fuzz code|http|postgres` and `scan/check/baseline/diff
performance` against exact safe fixtures. CodeAtlas stays a binary crate; no
public library is manufactured for dogfooding. All caches, compilers, package
stores, harnesses, profiles, observations, and reports remain outside the
checkout.

The completed v1 release qualification fuzzes CodeAtlas itself across every
truthfully modeled, safely fuzzable public interface using disposable fixtures
and finite kernel budgets. It retains deterministic reproducers for crashes,
invariant violations, and performance cliffs, and unresolved qualifying
findings block release. Self-fuzzing grants no extra authority: source remains
read-only, writable state remains external, and adjacent deny directives remove
targets that must not run even under the verified sandbox.

The latest stable-lifecycle checkpoint records:

- 233 source files and 2,241 scan symbols.
- 2,628 lexicon symbols.
- 6 naming collisions and 4 shape aliases requiring classification.
- 41 callable candidates.
- Six test contexts and zero exported Rust symbols, expected for the current
  binary crate.

These counts are evidence to reproduce and explain, not frozen product budgets;
later accepted analysis changes may change them with an explicit disposition.

## Process contract

`AGENTS.md` owns repository workflow, including external generated state,
quality lenses, validation order, Git safety, dogfooding, and optional use of
the installed stable `mill` for supported semantic refactors. The product has
no TypeMill runtime dependency, version coupling, or fallback to an unstable
checkout.

## Dependency and authorization graph

```text
governance + CLI lifecycle hard cut
        |
        +--> published schemas --> CodeAtlas HQA renderer
        |                              (external HQA remains untouched)
        |
        +--> execution kernel + HTTP migration ------------------+
        |                                                        |
        +--> structured callable/effect evidence                 |
                     |                                           |
                     +--> semantic-role siblings                 |
                     |       (static advisory branch)            |
                     |                                           v
                     +---------------------------> callable code fuzzing
                                                            |
                                                            v
                                                PostgreSQL fuzzing
                                                            |
                                                            v
                                         HTTP/PostgreSQL subject parity
                                                            |
                                                            v
                                      performance evidence + attribution
                                                            |
                                                            v
                                           cost-guided isolated search
```

The recommended single-worker order is dependency driven, with one deliberate
gate-preemption rule. Finish the currently dirty structured-callable slice,
take the OCI backend to its locally verifiable boundary, and run the live
isolation suite as soon as a capable runner exists. While that external
capability is unavailable, continue only the independent static lane:
semantic-role siblings, the static PostgreSQL query contract, and the first
subject-parity foundation phase. After isolation passes, finish HTTP migration,
code fuzzing, PostgreSQL live execution and fuzzing, the observation-dependent
CLI cut, remaining subject parity, performance, separately gated profiler
attribution, and cost-guided search.

The static schema/HQA and structured-evidence/sibling branches do not require a
sandbox. Their position minimizes later artifact and adapter churn. PostgreSQL
consumes only the shared corpus descriptors established by code fuzzing, not
callable contracts or engines. Base performance could begin after the kernel,
but intentionally follows parity so its first attribution model is not
immediately replaced. Cost-guided work waits for explicit accepted
metric-feedback and threshold-preserving shrink capabilities.

## Canonical v1 completion tracker

Tracker date: 2026-08-07

This is the one cross-program ordering and progress tracker. Child proposals
remain the normative owners for behavior, acceptance evidence, and file
manifests. Their phase statuses are subordinate checkpoints and must be updated
with this section when a phase starts, completes, blocks, or changes order. Do
not create another program roadmap or copy these tasks into a scratch document.

Remaining-scope audit (2026-08-07): 103 open CodeAtlas implementation checks
remain across Phase 10A and Phases 11 through 21, including Phase 16A and the
deferred Phase 15 observation enrichment. Every incomplete child phase maps to exactly one
checklist below. Child proposals retain contract rationale and acceptance
criteria, but they do not own a second rolling task list.

For restart context, read this introduction, the current verdict, the order
rules, and only the first incomplete phase plus its active child. Completed
foundations and the verification log are audit history and do not need to be
reloaded unless a claim is under review. The immediate plan-only-host queue
through Phase 16, the live Phase 9 isolation gate, and Phase 10 HTTP migration
are complete. Phase 10A's measured build-topology work is next; then continue
numerically through the execution track, Phase 16A, performance, cost search,
and final signoff.

### Current verdict

The public grammar, schema registry, HQA renderer, immutable execution
artifacts, enforcing HTTP call budget, complete structured callable evidence,
semantic-sibling analysis and self-dogfood, and locally verifiable OCI
implementation are done. Repository scope, config editing, HTTP/PostgreSQL
usage and docs, truthful code/HTTP initialization, and the static HTTP and
PostgreSQL inspection graphs are also complete. One bounded projection owner
now serves code, HTTP, and PostgreSQL without merging their graph semantics.
The repository lexicon now relates bounded code, HTTP, and PostgreSQL naming
evidence through the canonical concept policy without claiming semantic
equivalence. Subject-parity release hardening is complete: the public matrix,
self-audit, domain fixture dogfood, one-owner consolidation searches, and
generated-state audit all pass.
The reproducible CodeAtlas-owned isolation probe, live rootful OCI gate, and
complete HTTP migration are done. GitHub run `31145328464` executed exact
commit `e9bdb71`; both managed HTTP profiles passed through the shared kernel,
the receipt consumed 179 of 256 permits at peak logical concurrency one, all
eight required capabilities including TLS interception were present, and every
lease released and verified. Rootless and nested states remain unclaimed rather
than extrapolated. The measured build-topology phase is now explicit because
the remaining implementation would otherwise keep paying one 92,000-line Rust
compilation unit; it preserves all evidence and isolation contracts rather than
becoming feature work. The source-impact producer design is accepted and reuses the existing source
graph, callable effects, source index, bounded projection, and completeness
vocabulary. Its implementation remains gated without making HQA graph
alignment a CodeAtlas dependency. On implementation progress rather than
proposal-design progress, the complete program is approximately 69 percent
done. That estimate is weighted by the
accepted implementation phases, not by raw checkbox count. One hundred percent
means every remaining tracker item is either verified and checked here or
removed through an accepted scope change; a child status line alone is not
completion evidence.

### Order and ownership rules

1. Finish and commit the active dirty slice before changing product areas.
2. Shared contracts land before their second consumer. No child creates a
   private callable parser, corpus lattice, executor, artifact resolver,
   limiter, paginator, config editor, or metrics owner.
3. The measured build-topology phase follows the completed HTTP gate and
   precedes new execution adapters. It may improve iteration cost but may not
   alter product evidence or absorb the independently locked isolation probe.
4. Missing isolation always means plan-only. Static work may continue, but no
   HTTP, callable, PostgreSQL, performance, profiler, or cost workload runs.
5. PostgreSQL Phase 1 and subject-parity Phase 1 are static and may run before
   the live sandbox gate. Usage, docs, and inspection wait for the accepted
   observation identity so their first public schema is not knowingly replaced.
6. The evidence-lifecycle CLI closes only after `PostgresObservation` has one
   published owner. Baseline and diff never create that observation.
7. Profiler attribution and domain cost-search bridges start only after their
   named capability evidence passes. The full-program authorization does not
   turn an unproved backend feature into a capability.
8. Every public JSON change updates its model, namespaced identity, generated
   schema, fixture, drift check, and retired schema in one commit.
9. Every implementation phase ends with focused tests, applicable acceptance
   tests, bounded CodeAtlas self-dogfood, an external-state audit, staged and
   unstaged diff review, and one scoped commit. Full output stays under the
   external task root and only status, counts, digests, and short failure tails
   enter the working context.
10. Stable Mill is considered only at a clean committed HEAD and only when it
    advertises the exact deterministic refactor needed. It is never a phase
    dependency or a reason to delay ordinary reviewed edits.

### Phase 1: Finish structured callable effects and public evidence

Active child:
[`codeatlas-structured-callable-evidence.md`](codeatlas-structured-callable-evidence.md)
Phase 1.

- [x] Implement the bounded `src/analysis/effects.rs` owner over the existing
  source graph, with stable node, edge, effect, and work-queue ceilings.
- [x] Collect conservative direct filesystem, network, database, process,
  environment, time, randomness, ambient-state, and unsupported sink facts in
  the Rust, Python, JavaScript, and TypeScript adapters.
- [x] Represent unresolved dynamic call boundaries as explicit unknown evidence
  without claiming that the absence of a known sink proves purity.
- [x] Propagate known effects from callee to caller through resolved lexical
  edges with deterministic cycle handling, provenance, and ordering.
- [x] Attach the enriched contract once to the cached source snapshot and
  reject stale parser, graph, and analysis identities.
- [x] Extend the four-language conformance table for equivalent direct,
  propagated, unknown, receiver, overload, and block-reason behavior.
- [x] Generate scan v3 and context-slice v4 schemas, delete the replaced schema
  files, and update package drift assertions in the same slice.
- [x] Prove zero target calls, deterministic bytes, bounded failure behavior,
  external-only generated state, and exact target stability through focused
  tests and CodeAtlas dogfood.
- [x] Synchronize the child and umbrella checkpoints and commit callable Phase
  1 without unrelated paths.

### Phase 2: Migrate every callable consumer

Active child: structured callable evidence Phase 2.

- [x] Move lexicon callable shape and role evidence to `CallableContract`.
- [x] Move public API witness parameter and constructibility evidence to the
  same contract and existing symbol identity.
- [x] Confirm scan and inspect expose one shared serialized callable rather
  than a presentation-specific projection used as policy evidence.
- [x] Measure the lexicon JSON change. Bump to v4 only when bytes or meaning
  actually change, then regenerate the exact schema and fixtures.
- [x] Delete `src/lexicon/callable_contract.rs` and every display-signature
  policy parser, import, fallback, stale cache identity, and compatibility
  projection.
- [x] Search the repository for a second callable/type/effect owner and rehome
  any discovered logic before adding another consumer.
- [x] Run focused lexicon, witness, scan, inspect, schema, and CLI checks plus
  the bounded self-dogfood set.
- [x] Commit consumer migration and the retired heuristic as one clean Phase 2
  checkpoint.

### Phase 3: Harden and dogfood structured callable evidence

Active child: structured callable evidence Phase 3.

- [x] Add only exact, digest-bound CodeAtlas receiver, invariant, or known
  effect declarations that the self-dogfood corpus genuinely needs.
- [x] Classify CodeAtlas's own callable and effect findings, including unknown
  boundaries, rather than suppressing inconvenient evidence.
- [x] Confirm exact scan, witness, lexicon, and inspect target identities remain
  stable across cold and warm source-index runs.
- [x] Update the canonical lexicon, user-facing contract docs, dependent
  proposal assumptions, and self-audit task with the accepted evidence shape.
- [x] Verify no duplicate model, parser, cache identity, old schema, stale
  fixture, target call, or checkout-local generated state remains.
- [x] Run the required full checks and self-dogfood, synchronize statuses, and
  commit structured-callable Phase 3.

### Phase 4: Complete the locally verifiable OCI implementation

Active child: [`codeatlas-execution-kernel-http-fuzz.md`](codeatlas-execution-kernel-http-fuzz.md)
Phase 4. The accepted Phase 9 live proof closes this locally implemented phase.

- [x] Implement one OCI sandbox command owner with exact runtime fingerprint,
  digest-pinned image, cleared client environment, and no ambient context or
  credential lookup.
- [x] Enforce a read-only checkout and runtime, one external writable scratch
  root, path confinement, and symlink/traversal rejection in the planned mount
  set.
- [x] Enforce default-denied environment, process, and network policy with
  exact planned exceptions and no child-visible runtime control socket.
- [x] Bound captured output, elapsed time, CPU, RSS, processes, descriptors,
  result bytes, cancellation, and reserved cleanup through the shared runner.
- [x] Connect sandbox capability evidence, resource samples, cleanup leases,
  and non-passing partial outcomes to the canonical receipt.
- [x] Build target-observed conformance fixtures for every advertised
  capability without granting a capability from declarations or fake runtime
  health alone.
- [x] Prove command construction, fail-closed selection, zero-call blocking,
  lease release, and receipt evidence with deterministic fake-runtime tests on
  this plan-only host.
- [x] Record the still-unproved live matrix explicitly and commit a scoped
  implementation checkpoint only when all locally runnable checks pass.

### Phase 5: Add semantic-sibling configuration and evidence contracts

Active child: [`codeatlas-semantic-role-siblings.md`](codeatlas-semantic-role-siblings.md)
Phase 1. This static phase depends on Phase 3, not on OCI isolation.

Status: [x] Complete

- [x] Add strict, path-confined, nonoverlapping comparison-set configuration
  with finite nomination ceilings and exact validation diagnostics.
- [x] Define nomination, corroboration, counterevidence, disposition, omission,
  and provenance models without a score, probability, or gate field.
- [x] Exclude a shared trait or interface contract from its own corroboration
  evidence by construction.
- [x] Register the prospective lexicon report/schema transition and cover model
  ordering and bound behavior with the smallest fixture layer.
- [x] Run focused config/model/schema checks and commit the Phase 1 contract.

### Phase 6: Implement and dogfood semantic-role siblings

Active child: semantic-role siblings Phases 2 and 3.

Status: [x] Complete

- [x] Build bounded nomination indexes from contract roles, effects, named
  models, graph positions, and configured concepts without all-pairs scanning.
- [x] Evaluate each nomination with discrete independent corroborations and the
  complete mandatory counterevidence checklist.
- [x] Produce exact `review_candidate`, `separate_by_evidence`, and
  `inconclusive` dispositions with stable truncation and omission evidence.
- [x] Add deterministic JSON and text rendering, lexicon schema publication,
  report statistics, and permanently non-gating CLI behavior.
- [x] Declare CodeAtlas's language-adapter and HTTP-source-detector comparison
  sets and classify every bounded dogfood result.
- [x] Rehome only demonstrated shared helpers through their real owner. Keep
  intentional adapters separate when counterevidence supports the boundary.
- [x] Refuse Tier 2 body-skeleton work unless a named accepted counterexample
  proves Tier 1 cannot answer the required question.
- [x] Run focused and full dogfood, delete duplicate or speculative residue,
  and commit child Phases 2 and 3 as separate verified checkpoints.

### Phase 7: Land the static PostgreSQL query contract

Active child: [`codeatlas-postgres-fuzzing.md`](codeatlas-postgres-fuzzing.md)
Phase 1. This phase is static and does not open a database connection.

- [x] Extend existing query inventory with stable query identity, placeholder
  order, statement class, parameter and result shapes, referenced objects,
  constraint evidence, effects, and exact block reasons.
- [x] Keep SQL discovery in `src/postgres/source`, query policy in the
  PostgreSQL contract owner, and generic target/effect classification in the
  kernel owner.
- [x] Classify dynamic SQL, DDL, transaction control, privileged operations,
  filesystem/program access, external links, and unknown functions as blocked
  before generated execution.
- [x] Make DML checked-policy eligible but always reviewed-plan only, never
  single-shot, even for a local disposable target.
- [x] Prove deterministic IDs, parameter order, constraints, effects, and
  eligibility against static or checked-in catalog fixtures with zero live
  calls.
- [x] Publish any changed public schema, dogfood the static inventory, and
  commit PostgreSQL Phase 1.

### Phase 8: Build the repository-scope and config-edit foundation

Active child: [`codeatlas-subject-evidence-parity.md`](codeatlas-subject-evidence-parity.md)
Phase 1. This phase can start after the CLI hard cut and must not predeclare
the later observation-backed report shapes.

- [x] Extract one ordered `RepositoryScope` for root/member ownership, config
  digests, code contexts, HTTP contracts, PostgreSQL contracts, and truthful
  discovery completeness.
- [x] Flatten one `RepositoryScopeArgs` across current code, tests, and lexicon
  consumers; HTTP and PostgreSQL commands introduced later consume this same
  owner rather than rescanning or reinterpreting `--workspace`.
- [x] Extract one strict JSON config-edit owner from PostgreSQL init, with
  preview-first behavior, exact ownership refusal, reparse validation, and one
  selected-file write.
- [x] Preserve single-project and pnpm-workspace code/test behavior while HTTP
  and PostgreSQL contracts resolve through the same ordered member scope.
- [x] Prove generic config preview writes nothing, explicit insertion touches
  only one selected strict config, and every repository input is bounded and
  reused.
- [x] Run focused repository-scope/config tests and dogfood, then commit
  subject-parity Phase 1 without advertising the later public commands.

### Phase 8A: Close CodeAtlas neutral-contract wiring

Active children: [`codeatlas-hqa-seeding.md`](codeatlas-hqa-seeding.md) and the
interop acceptance surface. CodeAtlas consumes the external contracts
read-only and never edits their repository.

- [x] Validate the complete resolution assertion document against
  `agentspeak-resolution-conformance-v1.schema.json` from the sibling
  `agentspeak-contracts` repository, including its cross-file source-target
  `$ref`, instead of checking only the schema-version string.
- [x] Remove the resolution test's implementation-ignore gate now that the
  schema exists, while retaining one explicit standalone-checkout diagnostic
  when the external contract repository is unavailable.
- [x] Verify the corrected `https://agentspeak.org/` source-target identity,
  digest-bound range semantics, and lowercase-alphanumeric annotation namespace
  through the external schema. Do not generate, vendor, or publish a CodeAtlas
  copy.
- [x] Keep the HQA application-inventory golden's neutral-schema migration
  visibly blocked until that schema is actually published by the contract
  owner; do not reconstruct it from HQA's implementation tree. The eventual
  migration is owned once in Phase 21.
- [x] Record that future hints producers must validate the full embedded target
  and reject duplicate `(kind, key)` pairs before serialization. Free-form
  evidence never weakens the target block.
- [x] Run the focused interop/schema tests, full contract drift checks, bounded
  CodeAtlas dogfood, and commit the CodeAtlas-only wiring without modifying
  HQA, TypeMill, or `agentspeak-contracts`.

### Phase 9A: Own the reproducible isolation probe

Active child: execution kernel Phase 4A. This locally verifiable work precedes
the external runtime input and remains incapable of granting execution by
itself.

- [x] Make one strict report model validate the host evaluator, real probe, and
  deterministic fake runtime without a second schema or evaluator.
- [x] Implement bounded probe modes for filesystem, environment, mount,
  network, process, CPU, RSS, descriptor, output, and cancellation evidence.
- [x] Run every intentional write attack against an external disposable
  sentinel workspace, never the analyzed checkout.
- [x] Add a digest-producing OCI recipe and build task whose targets, context,
  archives, and runtime state are required to live outside `/workspace`.
- [x] Prove strict input rejection, deterministic bytes, truthful negative
  evidence without isolation, and no capability grant from source or build
  success alone.
- [x] Run focused checks, CodeAtlas self-dogfood, one-owner searches, external
  state audit, synchronize the child checkpoint, and commit Phase 9A.

### Phase 9B: Resolve a capable OCI runner

Active child: execution kernel Phase 4. This task preempts Phases 5 through 8
as soon as an eligible runner is available.

Execution input: the manual `Live OCI isolation gate` workflow on GitHub's
fresh `ubuntu-24.04` runner. The current host has no outer runtime socket. Sudo
can start a VFS-backed nested Docker API, but the outer container's capability
bounding set omits `CAP_SYS_ADMIN`, `CAP_NET_ADMIN`, and `CAP_SYS_RESOURCE`;
cgroup v2 is read-only and user, mount, and network namespace creation is
denied. The nested daemon fails its first image-layer registration with
`unshare: operation not permitted` and cannot launch a target. Its task-owned
QEMU replacement was removed rather than retained as a slow second backend.
This host remains plan-only; daemon or workflow availability alone is not
isolation evidence.

Hosted attempt `31080343512` reached Docker 28.0.4 / Buildx 0.35.0, built and
uploaded the canonical OCI artifact, then failed before the first conformance
case because Docker `image load` did not accept the OCI-layout tar. Run
`31081198694` proved the one-solve Docker import projection, exact loopback
publication, real baseline launch, canonical blocked receipt, and verified
container/scratch cleanup. The baseline stopped at a combined resource-usage
check before the destructive matrix. Audit removed only the invalid cgroup-v2
`memory.peak <= memory.max` oracle, retained exact limit and destructive RSS
proofs, and made every remaining usage overage diagnostic exact. Run
`31083063180` then identified `pids.peak = 7` with exact `pids.max = 1` while
the target-side unplanned-child denial passed. The kernel contract permits
organizational attachment above a PID limit and counts kernel tasks, so the
high-water metric is not a fork/clone enforcement oracle. The follow-up keeps
all usage samples as receipt evidence and makes exact observed limits plus the
target-side denial/exhaustion matrix the one capability owner. No capability
was granted.

The second run also exposed an economy defect: a 214-byte immutable v1 cache
hit prevented the useful 3,095,767,592-byte Cargo target produced by its 3m50s
compile from being saved. The v2 cache uses one complete OS/architecture/Rust/
dependency compatibility prefix plus an immutable source-revision generation,
restores only within that exact class, and refuses to save until the compiled
isolation test proves useful state exists. This prevents empty-cache poisoning
without creating another cache owner. The third run proved the policy by saving
a useful 3,095,277,545-byte uncompressed generation (702,923,486 compressed)
after the baseline failure; the next revision can restore it through the exact
compatibility prefix.

- [x] Add one manual-only, default-branch-only, least-privilege hosted workflow
  with a finite timeout, immutable action pins, exact digest-pinned build,
  BuildKit, and registry images, external writable state, and no child-visible
  control socket.
- [x] Add one bounded orchestration task that builds the committed probe,
  publishes it only through a temporary loopback registry, invokes the real
  baseline and destructive cases, exports exact evidence, and verifies
  cleanup on every outcome.
- [x] Add one Cargo cache owner whose compatibility prefix contains runner
  OS/architecture, the `rustc -Vv` digest, both lockfiles, and both manifests,
  and whose immutable generation contains the source revision; require a
  compiled isolation-test marker, enforce a 6 GB uncompressed ceiling, report
  restore/save metrics, and keep OCI image building uncached.
- [x] Start that committed workflow on GitHub and resolve the actual exact
  rootful, rootless, or nested runtime, local socket, digest-pinned probe
  image, external writable state, and runtime metadata from the run itself.

### Phase 9C: Pass the live OCI isolation continuation gate

Active child: execution kernel Phase 4. The runtime and image resolved in Phase
9B are inputs, not evidence; only the target-observed matrix can close this
gate.

- [x] Run target-observed mount, absolute-path, traversal, symlink, scratch,
  home, environment, network, subprocess, CPU, RSS, PID, descriptor, output,
  interruption, cancellation, and cleanup conformance cases.
- [x] Verify every advertised capability comes from successful target-side
  evidence and that each failed or missing probe blocks before the first call.
- [x] Verify rootless and nested behavior explicitly for every state the
  backend claims rather than extrapolating from one host mode.
- [x] Record the runtime, client, server, image, kernel, cgroup, capability,
  fixture, result, uploaded-artifact, and cache-result evidence needed to
  reproduce the matrix.
- [x] Fix any backend or fixture defect and rerun the narrow failed case before
  rerunning the complete conformance matrix.
- [x] Run full execution checks, CodeAtlas dogfood, and the generated-state
  audit, then mark and commit execution Phase 4 complete.

Accepted checkpoint, 2026-08-06: GitHub run `31084275665` executed commit
`6d1e4f0` once on rootful, non-nested Docker 28.0.4 / Linux
6.17.0-1020-azure. The baseline and destructive matrix passed, all owned
cleanup verified, and the target granted exactly `read_only_checkout`,
`read_only_runtime`, `scratch_filesystem`, `network_allowlist`,
`process_allowlist`, `resource_limits`, and `cleanup_verification`. Artifact
`8960813530` has digest
`sha256:c7a5abfd1b0935df16927bca3f18560a42224f9535281622c46f4d95cf2bd53f`;
the compatible Cargo cache restored 3,095,277,545 bytes and saved a useful
5,009,304,839-byte generation. The child proposal owns the complete receipt,
runtime, image, cache, and attempt history. No rootless or nested claim was
made.

### Phase 10: Migrate HTTP fully onto the kernel

Active child: execution kernel Phases 5 and 6.

Continuation checkpoint, 2026-08-06: the complete locally green migration is
implementation commit `a7d4fa4`, dispatched at checkpoint revision `349214c`;
the full local gate and consolidation audit pass. The reattached GitHub App key
was moved to private external state, authenticated a clean seven-commit
fast-forward, and found no equivalent run before dispatch. Live OCI run
`31128013309` then failed before image build or any target call because two
human-readable image labels violated the existing runtime kebab-label contract.
The correction now derives both labels through that one validator and passes
the focused seven-case image-build test plus all 31 Node tests. Commit/push the
narrow fix and dispatch once for its new revision; do not start Phase 11 until
the target-observed managed HTTP evidence passes.

The correction is now commit `d07b7ac`; exact run `31128060350` was dispatched
once after zero-duplicate confirmation. It remains queued without an assigned
runner while GitHub's official status API reports a major Actions outage. Keep
that exact run alive and query it without redispatching; no paid runner time or
target call has started.

GitHub subsequently completed its unassigned job as cancelled with no failed
step or job log. A user-authorized rerun returned HTTP 201 after zero-active-run
confirmation and returned the same exact `d07b7ac` run record to queued at
2026-08-06T21:29:08Z. Actions still reports a major outage; query that run and
do not retry again while the external boundary remains.

After Actions recovered, the stale rerun could not be cancelled normally or
forcibly because GitHub said it had never queued. User-authorized fresh run
`31138827152` used exact audited remote revision `d330449`, excluding an
unrelated concurrent license commit, and reached a runner. Its probe image
built; the HTTP image then failed before sandbox or target work because Python
3.10 `compileall` has no `--quiet --force` options. The isolated correction uses
canonical `-q -f`, rejects the invalid spellings in the recipe test, removes a
test-only checkout-basename assumption, and passes the focused seven-case plus
complete 31-case Node suites. Commit/push only that correction from the
isolated worktree before another hosted attempt.

The correction is isolated commit `721d118`. Exact run `31139150366` built both
images and made 68 target-observed stateful calls, then the kernel stopped the
still-running Schemathesis process at its normal 110-second deadline and
verified every cleanup lease. Artifact `8979252406` has digest
`sha256:35dbce74f5e4cbaf816e439b7d315a4fec1aa125e8d2e700441e9a678ca3f27`.
An exact-engine local reproduction showed that Schemathesis's default six-step
state-machine search can spend hundreds of rejected attempts while seeking four
accepted scenarios. The existing HTTP generation-policy owner now sets three
stateful steps, while the kernel remains the only call/time/resource limiter;
the live fixture pins proven seeds 42 and 43. Focused policy, cancellation, all
root unit, and all non-live isolation checks pass. Commit and push this scoped
correction, then issue one duplicate-checked hosted run; do not begin Phase 11
until both stateful and standard evidence pass there.

The bounded-depth correction is commit `02fe111`. Exact run `31142619886`
built both images but made zero target calls: the live fixture's planning helper
and its explicit stateful/standard cases supplied `--seed` twice, so Clap
rejected the invocation before sandbox setup. Artifact `8980363790` is
68,425,282 bytes with verified digest
`sha256:042a5f8df5c1de9c7f3697f80ce56539e02f66caf39c6625e6f612f0077a5dbd`;
its cleanup after-state is empty. The helper now contributes a default seed
only for default plans and preserves an explicit caller seed exactly. The new
regression plus all 13 non-live isolation cases pass. Commit and push this
test-boundary correction, then issue one duplicate-checked hosted run; do not
begin Phase 11 until both managed HTTP profiles pass there.

Exact run `31143159462` at revision `3f4b0f9` reached the stateful target and
consumed 70 calls, then ended correctly `partial` at 110,082 milliseconds with
all cleanup verified. Artifact `8980589056` is 68,427,576 bytes with
verified digest
`sha256:99f2e509245b891f9e71de5d9347f7b3e029aa682bd3bb7232b443a36f91231f`
and empty after-state logs. Its 128-descriptor value is isolation-probe
evidence, not the workload's live high-water sample; the resulting upstream
`Connection: close` hypothesis was not established.

Exact run `31144009982` at revision `1b840fc` retained that hypothesis and
failed identically after 70 calls and 110,067 milliseconds. Receipt
`receipt_a8f4f500fe0bdd36d267ced28c4b64b506d097c998e0d334b192d2b79a6a4573`
and artifact `8980887563` with digest
`sha256:d923a24dd44f44a06925449b4ed3184ce8102e369fc7db3d0245acd65cad4ff3`
record verified cleanup. Exact local engine and complete harness-plus-bridge
reproductions pass, isolating the enforcing proxy. Its accepted-connection
semaphore incorrectly reused logical `max_concurrency`; at one logical call,
an idle persistent connection prevented a second TLS connection from entering.
The current correction removes the disproved close behavior, retains
`max_concurrency` solely for call permits, and derives a finite transport
connection ceiling from the existing open-file budget. A target-observing
two-connection regression fails before and passes after the correction; all 11
proxy contracts and 13 non-live isolation cases pass. Finish the local gate,
commit and push the one-owner fix, then issue one duplicate-checked hosted run;
do not begin Phase 11 until both managed HTTP profiles pass there.

The correction is clean commit `e9bdb71`. Exact GitHub run `31145328464`
passed the target-observed live matrix: stateful coverage reached both selected
links, standard coverage exercised all three operations with zero server/check
failures, and passed receipt
`receipt_302d1788f81031b100add7d5409d145cfe6ddcd79f3f38558553832f7facc4df`
consumed 179 of 256 permits at peak logical concurrency one. It links report
`report_226e912a52858b7a24c197f52bc74a99ab3d3c70299bdef5b71221336b387012`,
records all eight required capabilities including TLS interception, and proves
every cleanup lease released and verified. Artifact `8981330764` is 68,432,294
bytes with verified digest
`sha256:6b2924e4b8ca6acf7581104406dfbe88e9d4fc3bb60792bd4ecf9bb6691d3548`;
execution and registry after-state logs are empty. The live gate is closed;
the post-fix closure evidence follows below.

Phase 5 closure is locally green after the live proof: 11 proxy contracts, 13
non-live isolation cases, 31 Node tests, 426 root unit tests plus every non-live
integration, probe/spec/schema drift, formatting, warning-denying Clippy, and
the bounded self-audit pass. The package guard rejected one task-created Python
bytecode cache; deleting that exact generated directory, restoring the external
Python cache root, and rerunning only the failed boundary produced the accepted
416-file package. Dogfood covers 331 files, 3,517 symbols, 2,969 callables, 278
non-gating findings, 4,203 lexicon symbols, three sibling sets with zero review
candidates, 16 test contexts, seven scripts, and no duplicate scripts. Exact
inspection resolves the new proxy limit owner into a bounded 80-node/452-edge
slice with all omissions explicit. One-owner and generated-state searches are
clean. Execution-kernel Phase 5 is complete; Phase 6 consolidation remains the
last Phase 10 item.

- [x] Feed HTTP target, destination, authentication, readiness, stateful, and
  effect evidence into the shared target classifier.
- [x] Route reviewed and eligible single-shot HTTP runs through the same
  persisted plan, sandbox, scheduler, permit ledger, TLS-terminating proxy,
  redactor, lease registry, and receipt path.
- [x] Preserve positive, negative, boundary, unsupported-method, stateful, and
  exact OpenAPI operation behavior while enforcing finite calls and rates.
- [x] Prove changed evidence refusal, managed/remote/production blocks,
  cancellation, incomplete cleanup, and budget exhaustion from the target
  side.
- [x] Delete HTTP-private plan, budget, artifact, private-filesystem, direct
  executor, `max_examples`, and unsafe fallback owners rather than wrapping
  them.
- [x] Synchronize public help, config, schemas, README, lexicon, tests, and
  self-audit commands with the one kernel path.
- [x] Run focused HTTP/execution checks, the full required suite, dogfood, and
  the checkout-state audit, then commit Phases 5 and 6 separately.

### Phase 10A: Extract measured build boundaries with TypeMill where proven

Active child:
[`codeatlas-build-topology.md`](codeatlas-build-topology.md) Phases 1 through 7.

- [x] Record the controlled no-op, app-edit, parser-edit, and test-build
  baseline with wall time, peak RSS, compiled packages, artifact bytes, one
  external Cargo target, and fixed job count.
- [x] Keep `codeatlas-isolation-conformance` excluded, independently locked,
  and digest-stable; add only explicit workspace members and reject a
  `crates/*` wildcard.
- [ ] At each move/rename batch, start from clean committed HEAD, inspect stable
  Mill capability evidence, preview and review one exact plan, and apply only
  when the plan contains the complete final imports, visibility, manifests,
  and deletions. Otherwise use an ordinary reviewed edit without partial Mill
  output.
- [ ] Extract `codeatlas-domain` with the existing language-neutral models and
  resolved analysis inputs; keep raw JSON config types and validation in the
  application config owner.
- [ ] Split only the evidence-document model/validation from
  `outputs::reference` into domain so HTTP/PostgreSQL no longer depend on
  renderers; keep API-reference presentation helpers in `outputs`.
- [ ] Remove the root domain module and every compatibility re-export, alias,
  duplicate resolved type, and retired import in the same phase.
- [ ] Pass domain tests, unchanged scan/inspect/schema bytes, HTTP/PostgreSQL
  docs fixtures, self-dogfood, and boundary searches before committing.
- [ ] Extract all Rust, Python, JavaScript/TypeScript, and Svelte adapters into
  one `codeatlas-languages` parity crate; do not delete or split a language.
- [ ] Pass every language/callable/effect/source-graph/fuzzability fixture and
  the neutral resolution-conformance gate through the new crate boundary.
- [ ] Remove parser dependencies unused by the root while retaining and naming
  PostgreSQL's legitimate domain-specific SWC visitor; do not invent a generic
  AST facade merely to remove that dependency.
- [ ] Re-run the exact Phase 1 lanes; require at least 20 percent app-edit wall
  time or peak-RSS improvement, independent language tests, and less than 10
  percent cold regression before touching execution topology.
- [ ] Extract the shared exact-tool leaf and replace execution's raw config and
  ambient state-root dependencies with one config-owned conversion into
  resolved runtime inputs.
- [ ] Extract `codeatlas-execution` as the sole plan/budget/artifact/sandbox/
  proxy/workload/cleanup/receipt owner, with no root facade or second kernel.
- [ ] Pass per-crate execution tests, every non-live and fake-runtime
  conformance case, canonical artifact vectors, root HTTP integration, schema
  drift, and the accepted live-proof identity.
- [ ] Re-run controlled build budgets, full checks, package assembly, neutral
  interop, complete CodeAtlas dogfood, one-owner searches, and generated-state
  audits; remove any extraction that misses the final 30-percent wall-time or
  25-percent RSS goal instead of waiving it.

### Phase 11: Build the shared corpus and callable fuzz foundation

Active child: [`codeatlas-code-fuzzing.md`](codeatlas-code-fuzzing.md) Phases
1A and 1B. Phase 1A is dependency-independent, static, and zero-call; Phase 1B
waits for Phases 9 and 10 and cannot be used to route around the live sandbox
gate.

Static checkpoint (Phase 1A; can proceed on a plan-only host):

- [x] Define domain-neutral scalar and collection boundary descriptors,
  canonical ordering, finite depth/size limits, and deterministic pairwise
  selection in `src/fuzz/corpus.rs`.
- [x] Map only supported `CallableContract` types and constructibility evidence
  into descriptors. Keep native value materialization domain-owned.
- [x] Account for every discovered public callable with receiver/factory,
  ordered-input, semantic-type, constructibility, result, effect, and oracle
  evidence or an exact deterministic block reason; silently omitted APIs fail
  the parity fixture.
- [x] Extend the one existing strict fuzz config owner with exact subject-shaped
  `code`, `http`, and `postgres` exclusions. Reject wildcards and a shared
  target mini-language; emit every denial as `blocked_by_policy` evidence.
- [x] Make config and interface exclusions monotonic: they may remove whole
  targets/cases but never grant safety, override detected/unknown effects, skip
  internal branches, waive review, or replace missing isolation.
- [x] Parse one source-adjacent
  `@codeatlas-fuzz deny: <bounded reason>` grammar through the
  existing Rust doc-comment, JavaScript/TypeScript JSDoc, and Python docstring
  adapters. Only `deny` exists; malformed or conflicting
  directives create `check code` findings and block planning.
- [x] Define `deny` as never fuzzing the exact target even with verified
  disposable isolation. Keep ordinary mutation/effect evidence in the existing
  callable and kernel target classifiers; reject a duplicative
  `requires_disposable` directive. Make the vocabulary subtractive-only: source
  comments may contract what runs but can never expand it, so no
  stale-comment `allow` path exists.
- [x] Permit effectful dependency substitution only through an explicit
  checked-in adapter with independently verifiable target, behavior, effects,
  and oracle evidence; otherwise keep the callable blocked.
- [x] Pass one attachment and payload conformance table across Rust, Python,
  JavaScript, TypeScript, and the SQL leading-comment convenience. Treat SQL
  exclusions as config-first and do not claim ORM or dynamic-query parity.
- [x] Register any public static report/schema changes, run the static
  acceptance surface and zero-call dogfood, then commit Phase 1A without
  advertising `fuzz code` execution.

Execution checkpoint (Phase 1B; waits for Phases 9 and 10):

- [ ] Supply the single planned `CODEATLAS_FUZZ=1` marker to sandboxed code
  harnesses (or one exact protocol-equivalent marker) as evidence, never as a
  safety boundary. When a target branches on it or skips an effect, label the
  run `alternate_behavior` and do not claim production-path coverage.
- [ ] Persist exact deterministic prefix, seed, engine fingerprint, scheduling,
  limits, evidence digests, and block reasons in the zero-call plan.
- [ ] Generate all harness, manifest, compiler, package, bytecode, corpus, and
  reproducer state under external scratch/cache roots with a read-only
  checkout.
- [ ] Use the shared reproducer envelope and kernel replay derivation with
  unchanged-evidence checks and no implicit execution.
- [ ] Require a pre-call permit for every case, retry, reduction, readiness,
  and cleanup action, with bounded watchdog and captured output.
- [ ] Prove path, symlink, home, `/tmp`, network, subprocess, resource, and
  cancellation escape behavior against controlled fixtures.
- [ ] Register report/reproducer schema changes, run foundation acceptance
  tests, dogfood zero-call planning, and commit code-fuzz Phase 1B.

### Phase 12: Add four native callable fuzz adapters and self-fuzzing

Active child: code fuzzing Phases 2 and 3.

- [ ] Implement one pinned Rust engine adapter with exact capability,
  deterministic-prefix, budget, replay, reduction, and oracle translation.
- [ ] Implement the equivalent pinned Python adapter without a private parser,
  limiter, cache, artifact, or execution path.
- [ ] Implement one shared JavaScript/TypeScript adapter boundary with exact
  language capability evidence and no duplicated engine provisioning.
- [ ] Pass one cross-language semantic-type, unknown, effect, harness, engine,
  and sandbox conformance table for every advertised feature.
- [ ] Normalize crashes, panics, exceptions, sanitizer findings, timeouts,
  resource limits, result-shape violations, forbidden effects, and cleanup
  failures without calling type validity a semantic oracle.
- [ ] Minimize only when the native adapter proves capable replay and preserve
  the exact named oracle and remaining budget.
- [ ] Fuzz real safe CodeAtlas CLI/config/report parsing boundaries without
  manufacturing a public library or changing internal visibility.
- [ ] Run every shipped CodeAtlas evidence feature against CodeAtlas during the
  phase, classify whether each result is useful, fix defects in its canonical
  owner, and rerun until the evidence is deterministic and actionable.
- [ ] Prove no source-local harness, dependency, cache, corpus, fake export,
  language-private budget, unsafe fallback, or compatibility alias remains.
- [ ] Run focused adapter suites, full checks, live isolated dogfood, and commit
  code-fuzz Phases 2 and 3 separately.

### Phase 13: Add guarded PostgreSQL execution and close the CLI lifecycle

Active children: PostgreSQL fuzzing Phase 2 and evidence-lifecycle CLI Phases 3
and 4.

- [ ] Add one persistent typed PostgreSQL client for generated parameters while
  retaining psql only for bootstrap, migrations, and psql meta-command rules.
- [ ] Plan `test postgres` with zero database calls and bind execution to exact
  query, catalog, target, tool, policy, and migration evidence.
- [ ] Run psql and the typed session inside the verified sandbox with a
  restricted role, exact network destination, shared permits, redaction,
  resource limits, and cleanup leases.
- [ ] Enforce parse, describe, execute, transaction, row, result-byte,
  connection, output, cancellation, and interruption ceilings from the
  database side.
- [ ] Create, migrate, exercise, close, drop, and verify one disposable database
  on every outcome, reserving cleanup capacity before ordinary work.
- [ ] Persist one complete namespaced `PostgresObservation` with a typed content
  ID, exact schema, source/catalog/tool digests, receipt linkage, and private
  artifact addressing.
- [ ] Make PostgreSQL baseline and diff load an exact `ArtifactRef`, reject
  wrong-kind or changed evidence, and make zero live database calls.
- [ ] Finish subject-neutral `--out`, `--format`, `--gates-only`, `--exact`, and
  flattened execution-limit semantics with parse-time rejection of invalid
  combinations.
- [ ] Remove every old tests/compile/observe spelling, duplicate output option,
  generation command, stale help example, and migration compatibility branch.
- [ ] Run live PostgreSQL cleanup tests, CLI contract tests, schema drift,
  self-dogfood, and full required checks, then commit PostgreSQL Phase 2 and CLI
  Phases 3 and 4 as separate scoped checkpoints.

### Phase 14: Add PostgreSQL fuzzing and harden the database boundary

Active child: PostgreSQL fuzzing Phases 3 and 4.

- [ ] Map catalog OIDs, domains, nullability, enums, lengths, precision, scale,
  temporal, JSON, byte, network, array, and supported composite evidence into
  shared corpus descriptors and PostgreSQL-native protocol values.
- [ ] Execute deterministic boundary and bounded pairwise cases before seeded
  adaptive cases, using one permit per SQL interaction.
- [ ] Distinguish expected rejection from SQLSTATE, result-shape, connection,
  resource, forbidden-effect, transaction, session, and cleanup failures.
- [ ] Reduce and replay only against unchanged source, catalog, target, tool,
  and policy evidence with the shared reproducer contract.
- [ ] Prove DML remains reviewed-only, every case rolls back, nontransactional
  residue disappears with database drop, and external effects stay blocked.
- [ ] Prove statement, call, rate, burst, concurrency, timeout, row,
  result-byte, output, cancellation, and cleanup limits from the database side.
- [ ] Dogfood controlled CodeAtlas PostgreSQL fixtures and classify every
  failure or unsupported type honestly.
- [ ] Delete any parallel query executor, corpus lattice, budget, artifact
  owner, `fuzz sql` alias, hidden live baseline/diff, or database residue.
- [ ] Run focused and full static/live gates, external-state audit, and commit
  PostgreSQL Phases 3 and 4 separately.

### Phase 15: Add HTTP and PostgreSQL usage, docs, init, and inspection

Active child: subject evidence parity Phases 2 through 4.

- [x] Implement truthful `init code` and `init http` proposals without URLs,
  secrets, execution targets, completeness claims, or effect policy.
- [x] Implement `usage http` as known repository consumer evidence with
  visible external/dynamic incompleteness and no `unused_route` claim.
- [x] Implement `usage postgres` as known static query touches with visible
  dynamic/catalog incompleteness and no `unused_table` or `unused_column`
  claim.
- [x] Render deterministic sourced HTTP and PostgreSQL Markdown/HTML docs,
  including visible missing descriptions or catalog evidence, with zero hidden
  live calls.
- [x] Prove docs `--check` never writes, init preview writes nothing, and an
  explicit init writes only one selected strict configuration file.
- [ ] Enrich PostgreSQL usage/docs only from an explicitly supplied exact
  observation reference, with no hidden target or database execution.
- [ ] Enrich HTTP and PostgreSQL inspection only from accepted typed fuzz or
  observation artifact references; reject wrong-kind, stale, or path-only
  evidence without changing the static graph path.
- [x] Extract one generic bounded graph projection owner from code context
  slicing without merging code, HTTP, and PostgreSQL graph semantics.
- [x] Keep existing code inspection bytes and cursor behavior exact after the
  shared projection extraction.
- [x] Build static HTTP contract, operation, schema, handler, caller, and test
  graph nodes and typed edges. Optional fuzz evidence waits for its accepted
  artifact identity.
- [x] Build static PostgreSQL contract, source, query, parameter, table,
  column, constraint/index, and callsite graph nodes and typed edges. Optional
  observation evidence waits for its accepted artifact identity.
- [x] Implement exact `inspect http` and `inspect postgres` target resolution,
  stable ambiguity errors, depth/node limits, and digested cursors. Wrong-kind
  observation rejection remains with the observation task above.
- [x] Prove inspection, usage, and docs remain bounded, deterministic,
  workspace-aware, and zero-call across cold and warm snapshots.
- [x] Publish each static report shape, run focused parity dogfood, and
  commit subject-parity Phases 2, 3, and 4 separately.

### Phase 16: Add the repository lexicon and harden subject parity

Active child: subject evidence parity Phases 5 and 6.

- [x] Extract typed code, HTTP, and PostgreSQL term evidence with exact subject,
  owner, target, source, confidence, and completeness provenance.
- [x] Reuse one normalization and concept-policy engine while keeping subject
  extraction in each domain adapter.
- [x] Implement `lexicon repository --subjects code,http,postgres` as one
  bounded analysis, not subprocess composition or a code-command alias.
- [x] Treat cross-subject term relationships as evidence only and require
  explicit policy or corroboration before semantic-equivalence claims.
- [x] Publish the report/schema transition and preserve focused `lexicon code`
  behavior.
- [x] Search for subject-private workspace discovery, config insertion,
  paginator, term normalization, hidden live calls, false unused labels,
  invented descriptions, and compatibility commands, then consolidate each
  real duplicate through its canonical owner.
- [x] Run full checks and code/HTTP/PostgreSQL dogfood, update the public matrix
  and lexicon, audit external state, and commit Phases 5 and 6 separately.

### Phase 16A: Add the gated source-impact projection for HQA

Active child: [`codeatlas-source-impact.md`](codeatlas-source-impact.md).

This follow-on starts only after Phase 8A and the live Phase 9 isolation proof.
Its schemas remain externally owned. CodeAtlas may write its proposal before
those gates close, but it does not implement or locally reconstruct an
unpublished family contract.

Blocked inputs for implementation: the live Phase 9 proof plus producer
acceptance of the external source-impact draft published at
`agentspeak-contracts` commit `7706a37`. The current hints and source-target
schemas are already usable. Graph alignment remains a separate HQA
continuation gate and does not block this producer. Proposal and field-level
contract review may start earlier, but neither grants runtime or schema
authority.

- [x] Write and accept one CodeAtlas-only source-impact proposal with explicit
  LOC, language/framework capability, schema, safety, and continuation gates.
  Treat `agentspeak.graph-alignment/v1` as a separate HQA dependency, not a
  CodeAtlas continuation gate or implied implementation.
- [ ] Define one typed `(surface hint, action) -> entry symbols` binding edge so
  shared handlers and action-specific bindings remain explicit. A symbol or
  source target alone is not the interaction identity.
- [ ] Project effects and unresolved boundaries only from the existing source
  graph and `CallableContract` evidence. Add no second effect walker, and point
  semantic-role-sibling dogfood at every new impact module.
- [ ] Scope named state reads and writes honestly. Extend the existing callable
  evidence owner with a typed state-access fact only if deterministic parity is
  proven across applicable languages; otherwise omit the field rather than
  deriving state identity from a coarse effect kind.
- [ ] Give Svelte and React surface-to-action mapping their own bounded adapter
  phase with exact capability and incompleteness evidence. Do not bury UI
  framework discovery inside the manifest renderer.
- [ ] Persist the explicit syntax, structure, and dependency manifests plus
  named reads, writes, effects, and boundaries, then digest their RFC
  8785-canonical bytes. Follow the external explainable-manifest convention;
  never emit an opaque-only comparison hash.
- [ ] Label every source-impact result as a hypothesis, invalidation hint, or
  retest hint. Never claim that a source change proves a runtime behavior
  change; HQA owns runtime confirmation.
- [ ] Review the exact `agentspeak.source-impact/v1` draft as the producer,
  return acceptance or field-level objections to its owner, and—once the
  relevant source-impact and hints schemas are accepted—bind them by sibling
  reference, validate full embedded source-target blocks, enforce unique
  `(kind, key)` hints, and vendor no copy. Graph alignment remains HQA-owned.
- [ ] Pass deterministic fixture, cross-language, framework, schema, sibling,
  boundedness, and CodeAtlas self-dogfood checks, then commit the proposal's
  independently reviewable phases.

### Phase 17: Add base performance observations, curves, and candidates

Active child: [`codeatlas-performance-evidence.md`](codeatlas-performance-evidence.md)
Phases 1 through 3.

- [ ] Define strict fixed-workload, size-ladder, cache-preparation, warmup,
  sample, metric-capability, noise, and regression configuration.
- [ ] Plan performance scans with zero workload calls and exact source, config,
  target, tool, environment, dataset, and policy evidence.
- [ ] Persist namespaced observations through the shared artifact store and
  make check, baseline, and diff consume them with zero workload calls.
- [ ] Execute fixed cold/warm workloads only through the verified sandbox,
  finite kernel budgets, shared resource sampling, leases, redaction, and
  receipts.
- [ ] Record individual elapsed, CPU, RSS, and supported domain metrics plus
  robust aggregates, noise floors, comparability, and inconclusive states.
- [ ] Fit bounded size curves and gate only comparable, sufficiently stable
  regression evidence.
- [ ] Generate deterministic static optimization candidates from existing
  complexity, fan, allocation, blocking-I/O, query, and call-path facts without
  labeling them hotspots.
- [ ] Prove source-index telemetry remains a reused primitive rather than a
  second performance report owner.
- [ ] Run focused lifecycle/curve/candidate tests and isolated CodeAtlas
  performance dogfood, then commit Phases 1, 2, and 3 separately.

### Phase 18: Pass the profiler continuation gate and add attribution

Active child: performance evidence Phase 4.

Continuation gate: accepted Phase 17 measurements and one profiler backend
that proves the full sandbox, bounded-capture, cleanup, and overhead contract.

- [ ] Select one exact profiler backend only after Phase 17 records accepted
  base measurement evidence and the backend can satisfy the sandbox contract.
- [ ] Pin tool discovery, fingerprint, target/environment support, bounded
  capture, output size, cleanup, and unsupported capability behavior.
- [ ] Measure profiler overhead and rootless/nested-container behavior instead
  of assuming ordinary measurement comparability.
- [ ] Map frames to exact source targets where proven, retain unmapped frames,
  and report attribution confidence and completeness.
- [ ] Label a location a hotspot only when material runtime evidence supports
  the claim. Keep static candidates and missing metrics visibly distinct.
- [ ] Run focused profiler capability/attribution tests, isolated dogfood,
  schema drift, and external-state checks, then commit Phase 4.

### Phase 19: Establish CodeAtlas performance baselines

Active child: performance evidence Phase 5.

- [ ] Reproduce the Phase 8A warm exact-target `inspect code` latency lead with
  named cold/warm observations before optimizing it. Keep it an optimization
  candidate until accepted runtime attribution proves a hotspot.
- [ ] Define representative CodeAtlas scan, check, usage, inspect, lexicon,
  tests, and accepted HTTP/PostgreSQL fixture workloads with recorded file,
  symbol, byte, and dataset scale.
- [ ] Capture reviewed cold and warm observations and baseline artifacts on a
  named reproducible environment.
- [ ] Verify optimized paths preserve canonical static evidence, plan IDs,
  decisions, errors, reports, receipts, and artifact digests.
- [ ] Remove hidden execution, duplicate metrics owners, false hotspot labels,
  unbounded profiles, stale observe vocabulary, and checkout-generated state.
- [ ] Run full performance and repository checks, update docs and self-audit,
  and commit Phase 5.

### Phase 20: Add the separately gated cost-search bridge

Active child: [`codeatlas-cost-guided-search.md`](codeatlas-cost-guided-search.md)
Phases 1 through 3.

Continuation gate: accepted performance observations plus at least one domain
adapter that explicitly proves external metric feedback, threshold-preserving
reduction, and replay.

- [ ] Prove at least one accepted domain adapter has a typed
  `CostSearchCapability` for external metric feedback, threshold-preserving
  reduction, and replay. Do not infer it from ordinary fuzz support.
- [ ] For callable engines, state exact metric feedback, deterministic or
  sampled objective, shrink, replay, and unsupported capability evidence.
- [ ] For PostgreSQL, export typed supported rows, buffers, planner, and
  execution metrics before enabling the corresponding bridge.
- [ ] Define validated cost objectives, search-plan identity, candidate order,
  remaining-budget accounting, confirmation, variance, and inconclusive
  outcomes over accepted performance observations.
- [ ] Keep target and reproducer forms zero-call until the exact persisted plan
  is executed through the kernel.
- [ ] Charge every candidate, retry, confirmation, reduction, and cleanup
  interaction to finite permits without consuming the reserved cleanup share.
- [ ] Reuse existing code/PostgreSQL contracts, corpora, materializers,
  shrinkers, runners, metrics, reproducer envelope, and artifact resolver
  without creating another generator or execution loop.
- [ ] Preserve the named cost threshold during reduction and never relabel a
  cost result as a correctness failure without a separate violated oracle.
- [ ] Dogfood controlled parser and PostgreSQL fixtures, remove duplicate or
  stale paths, run full checks, and commit cost-search Phases 1, 2, and 3
  separately.

### Phase 21: Whole-program v1 consolidation and signoff

External dependency for the first item: the neutral contract owner must
publish the accepted application-inventory schema. CodeAtlas will consume it
read-only and will not reconstruct or vendor it.

- [ ] Replace the HQA sibling-checkout schema dependency with drift validation
  against the accepted neutral `agentspeak-contracts` application-inventory
  schema. Keep the CodeAtlas golden in this repository, vendor no external
  schema, remove the HQA-tree coupling, and make no HQA or neutral-repository
  edit from CodeAtlas.
  CodeAtlas now resolves only the neutral schema path; the gate remains open
  because that external schema is not yet present in `agentspeak-contracts`.
- [x] Add the CodeAtlas half of the cross-tool resolution conformance gate:
  one checked-in repository fixture with an exact symbol, resolved consumer
  owners, and explicit unresolved counterexamples; generate its evidence from
  the existing source graph rather than a second resolver, retain a versioned
  published artifact, and drift-test the exact normalized consumer set. The
  external TypeMill half must prove that a rename plan edits the target plus
  exactly those resolved consumer paths and never an unresolved path; any set
  difference is a named conformance failure. No CodeAtlas phase edits TypeMill.
- [ ] Confirm every child status, phase checklist, schema version, CLI example,
  LOC record, and dependency statement matches the implemented repository.
- [ ] Regenerate every published schema externally and prove exact registry,
  fixture, package, and drift-test agreement with no retired schema shipped.
- [ ] Verify the final CLI subject matrix and reject every retired command,
  alias, force path, unlimited sentinel, duplicate output flag, and hidden live
  lifecycle action.
- [ ] Create one concise `docs/upgrade-guide.md` for user-visible hard cuts,
  then remove internal legacy compatibility code, readers, aliases, fallbacks,
  and schema shims. Explicitly resolve `digest_legacy_graph`, the context-slice
  direction schema shim, and the Mermaid imports fallback. An external
  dependency's required `legacy` module name is boundary vocabulary, not a
  CodeAtlas compatibility path.
- [ ] Search for and remove second owners of callable evidence, effects, corpus
  primitives, plans, target classification, limits, sandboxing, artifacts,
  replay, redaction, cleanup, tools, resource sampling, graph projection,
  config edits, query execution, and performance metrics.
- [ ] Run target-observed isolation, call-budget, filesystem, network, process,
  resource, cancellation, interruption, and cleanup conformance across HTTP,
  callable, PostgreSQL, performance, profiler, and cost-search capabilities.
- [ ] Reprove that plan-only hosts block before the first target call and that
  review never substitutes for a missing capability.
- [ ] Run the complete static CodeAtlas dogfood surface and classify all gates,
  candidates, sibling evaluations, unknown boundaries, omissions, and target
  identities honestly.
- [ ] Run isolated HTTP, callable, PostgreSQL, performance, profiler, and cost
  dogfood only against exact disposable fixtures and retain plan, receipt,
  observation, baseline, report, and reproducer digests.
- [ ] Audit README, AGENTS.md, the canonical lexicon, architecture docs,
  package tasks, and every proposal for truthful current behavior and one
  preferred vocabulary.
- [ ] Verify builds, caches, package stores, harnesses, corpora, databases,
  profiles, observations, temporary files, and reports never appeared under
  `/workspace`.
- [ ] Run formatting, package checks, focused integration suites, the complete
  required test suite, schema drift, self-audit, and release-build dogfood with
  concise externally stored logs.
- [ ] Inspect Git status plus staged and unstaged diffs, confirm only intended
  source and docs remain, commit the final hardening slice, and record the
  clean v1 evidence checkpoint.

### Completed foundations

- [x] Corrected and committed the original proposal suite as `6845a24`.
- [x] Established governance, the canonical lexicon, standalone Cargo
  ownership, external generated-state policy, and reproducible self-audit.
- [x] Hard-cut testing and architecture into the evidence lifecycle.
- [x] Published and drift-tested the current CodeAtlas schemas and annotation
  namespace.
- [x] Added the deterministic CodeAtlas HQA application-inventory renderer
  without modifying HQA.
- [x] Added immutable plans, receipts, typed private artifacts, canonical RFC
  8785 plan identity, replay ownership, target classification, redaction,
  leases, resource evidence, and shared CLI limit arguments.
- [x] Added finite pre-call budgets, the bounded scheduler, and the enforcing
  TLS-terminating HTTP proxy with target-observed call accounting.
- [x] Proved fail-closed capability selection and plan-only receipts on the
  current host without granting an unverified sandbox capability.
- [x] Mapped callable owners, pinned the language-neutral model, and emitted
  complete Rust, Python, JavaScript, and TypeScript contracts with propagated
  effect evidence and conformance coverage; every consumer now uses that model
  and the retired display-signature heuristic is deleted.
- [x] Implemented the complete OCI command/runtime/conformance boundary,
  shared cancellation, resource evidence, and fallback cleanup path, then
  passed the target-observed rootful hosted matrix with all seven advertised
  capabilities and verified cleanup.

### Verification log

- 2026-08-04: proposal/schema convergence, published schemas, and HQA renderer
  completed and committed with external generated state.
- 2026-08-04: execution Phases 1 through 3 completed; fail-closed Phase 4
  checkpoint committed through `c4fac1b`.
- 2026-08-05: structured-callable Phases 1 through 3 completed through
  `cf97b73`; consumers migrated and the replaced lexicon heuristic was deleted.
- 2026-08-05: all locally verifiable OCI work passes focused/full checks and
  target-observed fake-runtime dogfood; the host remains plan-only and the
  live OCI test is retained as Phase 9's hard gate.
- 2026-08-05: Phase 9A adds the CodeAtlas-owned strict isolation probe and
  digest-producing OCI recipe. The host evaluator, real probe, and fake runtime
  consume one report model; write attacks use an external disposable sentinel
  workspace; 18 Node tests, 415 root unit tests, four probe tests, both Clippy
  gates, zero-gate self-dogfood over 324 files, and package assembly pass. The
  current host still cannot build or run the live OCI matrix, so Phase 9B is
  the next continuation gate.
- 2026-08-05: commit `5622c87` adds one reusable Docker task boundary and one
  live runner transaction that exercises the baseline plus destructive
  CPU/RSS/output/cancellation modes through the existing container executor
  and lease registry. Focused Node, container, integration, probe, and Clippy
  checks pass. The manual hosted workflow adds exact image/action pins and one
  bounded exact-identity Cargo cache; its unrun source remains input, not
  capability evidence.
- 2026-08-05: semantic-sibling Phase 1 passes strict config/model/schema/CLI
  checks and static self-dogfood; lexicon v5 is the sole published schema and
  shared-contract evidence cannot become corroboration.
- 2026-08-05: semantic-sibling Phase 2 passes bounded nomination and complete
  counterevidence tests, deterministic real-source CLI fixtures for all three
  dispositions, lexicon v5 drift checks, and warning-denying Clippy. Graph
  boundary evidence is indexed once and unconfigured lexicon runs retain their
  existing scan-only path.
- 2026-08-05: PostgreSQL static Phase 1 passes focused query and CLI tests, all
  29 schema drift tests, the full repository check, and two byte-identical
  zero-call fixture scans. The v2 contract records bounded placeholders,
  catalog/type shapes, statement/effect evidence, and exact eligibility; the
  old v1 schemas and duplicate baseline-query projection are removed.
- 2026-08-05: CodeAtlas's neutral resolution-conformance half passes against
  `agentspeak-contracts`: three runtime consumers, two test witnesses, and one
  named dynamic-import boundary agree exactly. The fixture exposed and now
  guards `.e2e.ts` test classification and extensionless computed-import
  incompleteness without adding another resolver or vendored contract.
- 2026-08-05: callable-fuzz Phase 1A passes the Rust, Python, JavaScript,
  TypeScript, and SQL directive table; exact public-callable accounting;
  canonical bounded corpus; code/HTTP/PostgreSQL exclusions; all 30 published
  schemas; 399 unit tests; CLI suites; warning-denying Clippy; package audit;
  and complete zero-call CodeAtlas dogfood. The self-scan found 2,581 callable
  contracts and zero gates. A single warm engineering probe measured
  `usage code` at 1.197 seconds and `check code` at 1.226 seconds with RSS
  unavailable; it is recorded as phase evidence, not a performance claim.
- 2026-08-05: subject-parity Phases 2 through static Phase 4 are committed
  through `2efbe11` and pass deterministic
  HTTP/PostgreSQL usage, docs, init, and bounded inspection acceptance. Shared
  projection extraction preserves the exact code-inspection checkpoint bytes.
  Dogfood corrected Rust module/callable collisions, grouped `self` imports,
  glob resolution, and both graph and parser-fact cache identities; cold and
  warm checks are byte-identical with 334 advisories, zero gates, and no
  findings in the new inspection paths. Optional observation enrichment remains
  explicitly gated on the accepted artifact identities.
- 2026-08-05: the neutral resolution-conformance gate consumes
  `agentspeak-contracts` `ab62f51`, validates the complete assertion through
  its source-target `$ref`, and passes with three runtime consumers, two test
  witnesses, and one unresolved dynamic-import boundary. The ordinary test
  path is no longer ignored; an absent default sibling checkout produces an
  explicit standalone diagnostic. Corrected `agentspeak.org` identities,
  digest-bound range wording, annotation namespace constraints, set uniqueness,
  unresolved path uniqueness, and cross-set disjointness are checked without a
  vendored schema. Bounded self-dogfood reports 308 files, 3,206 scan symbols,
  337 advisory code findings, and zero gates. Its warm exact-target inspection
  latency is retained as an unclassified Phase 19 performance lead.
- 2026-08-06: hosted run `31084275665` executes exact commit `6d1e4f0` and
  closes Phase 9. The baseline and destructive matrix pass, exact target-side
  evidence grants seven capabilities, every owned lease verifies cleanup, the
  uploaded artifact digest is recorded above, and the compatible Cargo cache
  restores and saves useful bounded state. HTTP workload execution remains
  disconnected until Phase 10 rather than bypassing the accepted migration.
- 2026-08-06: Phase 10A Phase 1 pins the monolithic build baseline at exact
  commit `8147807` with two Cargo jobs and external state. Three unchanged
  checks vary by 4.0 percent; controlled app, parser, test-build, and offline
  clean-target lanes record wall time, peak RSS, compiled packages, and
  artifact bytes while mtime probes preserve exact source digests.
- 2026-08-06: the Phase 10A domain scaffold adds one explicit workspace member
  while preserving the standalone probe manifest and lock. The focused
  topology test, locked offline workspace check, and 418-file package audit
  pass before the TypeMill move preview.

## Existing-first check

The suite reuses current HTTP Schemathesis/runtime code, PostgreSQL source and
disposable lifecycle, testing witnesses, language parsers/reachability, source
graph identity, source-index metrics, external tool/cache owners, output
helpers, and baseline/diff families. New modules exist only for product
contracts with no current owner.

No child may create a second plan, target classifier, budget, sandbox, artifact
store/resolver, common limit parser, reproducer envelope, replay path, redaction
engine, cleanup registry, tool provisioner, resource sampler, callable parser,
published-schema registry, annotation namespace, HQA route scanner, query
executor, performance metrics owner, or public command alias.

## Program stage 1: Evidence lifecycle CLI

Status: [~] Accepted; testing and architecture complete, shared artifacts next

LOC: +970-1,650 / -610-1,060

Verify: New verb-subject commands preserve evidence; old `tests`, `compile`,
and `observe` commands reject; outputs/artifacts have one meaning; no
compatibility routing remains.

```text
~ proposals/codeatlas-evidence-lifecycle-cli.md
```

## Program stage 2: Published schemas

Status: [x] Complete

LOC: +1,463 / -226 authored; +7,706 generated schema JSON

Verify: Every current public JSON root has one generated schema and exact drift
test; new-artifact version and CodeAtlas annotation namespace rules are pinned;
existing report bytes do not churn.

```text
~ proposals/codeatlas-published-schemas.md
```

## Program stage 3: CodeAtlas HQA seeding

Status: [~] Renderer complete; neutral-contract wiring remains in Phase 8A

LOC: +815 / -9

Verify: Source and OpenAPI routes render deterministically into the published
HQA v1 inventory with unique IDs, honest completeness, conservative dynamic
paths, no invented roles, and no HQA repository edit. Default CodeAtlas JSON
remains byte-identical. The renderer's golden currently validates against HQA's
accepted schema; Phase 8A/21 must move that drift gate to the neutral owner once
the application-inventory schema exists there. All repository checks and
bounded CodeAtlas dogfood pass with zero gates.

```text
~ proposals/codeatlas-hqa-seeding.md
```

## Program stage 4: Execution kernel and HTTP migration

Status: [x] Complete

Projected LOC after measured Phase 3: +11,760-13,310 / -878-1,408

Verify: One sandbox backend passes the full isolation suite; HTTP requests
cannot exceed call/rate/resource ceilings; source remains read-only; plans,
receipts, cleanup, reviewed execution, and eligible one-shot execution pass.

```text
~ proposals/codeatlas-execution-kernel-http-fuzz.md
```

## Program stage 4A: Measured build topology

Status: [ ] Accepted; implementation follows the completed HTTP hardening gate

Physical move/edit LOC: +29,670-31,860 / -28,450-30,370; expected net authored
surface +700-1,300

Verify: Controlled build lanes improve app-edit wall time by at least 30
percent or peak RSS by at least 25 percent without more than 10 percent cold
regression; domain, language, and execution boundaries pass independently; no
facade, duplicate owner, wildcard workspace member, or probe lock/digest change
remains.

```text
+ proposals/codeatlas-build-topology.md
```

## Program stage 5: Structured callable evidence

Status: [x] Complete

LOC: +900-1,300 / -200-350

Verify: One four-language callable/effect contract feeds inspect, lexicon,
witnesses, sibling analysis, and fuzz planning; the display-signature heuristic
is deleted; lexicon/cache/schema identities are exact.

```text
~ proposals/codeatlas-structured-callable-evidence.md
```

## Program stage 6: Semantic-role siblings

Status: [x] Complete

Measured LOC: +3,446 / -273 authored; +1,205 / -848 generated schema

Verify: Only configured sibling sets are compared; discrete corroboration and
mandatory counterevidence are exact; shared trait contracts are not proof;
results never gate; both CodeAtlas dogfood corpora are reviewed.

```text
~ proposals/codeatlas-semantic-role-siblings.md
```

## Program stage 7: Callable code fuzzing

Status: [~] Accepted; static Phase 1A is complete, harness execution waits for
the sandbox gate

Projected authored LOC: +4,564-5,614 / -639-1,019

Verify: Accepted callable/effect evidence, four-language parity, deterministic
boundary/replay, native engine adapters, automatic oracles, and CodeAtlas
self-fuzzing pass.

```text
~ proposals/codeatlas-code-fuzzing.md
```

## Program stage 8: PostgreSQL fuzzing

Status: [~] Accepted; static Phase 1 and the shared corpus foundation are
complete, while live execution and generated cases wait for isolation and the
PostgreSQL adapters

LOC: +4,025-4,925 / -469-819 authored; Phase 1 additionally replaced
+3,544 / -1,778 generated schema lines

Verify: Catalog-backed parameters, blocked SQL effects, guarded typed session,
deterministic/adaptive values, disposable cleanup, replay, and zero-live-call
baseline/diff pass.

```text
~ proposals/codeatlas-postgres-fuzzing.md
```

## Program stage 9: HTTP and PostgreSQL evidence parity

Status: [~] Accepted; repository-scope, HTTP/PostgreSQL usage, truthful docs,
generalized initialization, and static bounded inspection Phases 1-4 are
complete; repository lexicon is next while artifact-backed enrichment remains
deferred

LOC: +8,738-9,688 / -1,556-1,986 product and tests, plus generated schemas

Verify: One repository scope feeds code/HTTP/PostgreSQL; route and database
usage claims expose completeness; docs contain only sourced facts; exact
inspection graphs are bounded; init preview is zero-write; repository lexicon
retains subject provenance.

```text
~ proposals/codeatlas-subject-evidence-parity.md
```

## Program stage 10: Performance evidence and attribution

Status: [ ] Accepted; recommended after subject dependency graphs

LOC: +2,150-3,450 / -430-910

Verify: Planned measurements, cold/warm curves, zero-call check/baseline/diff,
noise-aware regression, static candidate honesty, and self-performance pass;
profiler confidence is required only after its separate continuation gate.

```text
~ proposals/codeatlas-performance-evidence.md
```

## Program stage 11: Cost-guided isolated search

Status: [ ] Accepted but technically gated by performance and domain adapters

LOC: +500-800 / -100-250

Verify: Typed cost objectives reuse accepted generators and metrics, all calls
remain budgeted, noisy outcomes are inconclusive, reductions replay, and cost
results are never mislabeled as correctness failures.

```text
~ proposals/codeatlas-cost-guided-search.md
```

## Program stage 12: HQA source-impact projection

Status: [~] Accepted design; implementation waits for Phase 9 and the external
source-impact contract

LOC: +2,550-3,850 / -350-800

Verify: One bounded source-graph projection emits deterministic hints and
source-impact hypotheses with explicit framework completeness, full external
schema convergence, no HQA target IDs or runtime edges, and no parallel parser,
effect walker, cache, graph, or command family.

```text
~ proposals/codeatlas-source-impact.md
```

The full program is intentionally net higher because it adds a verified
sandbox, three execution domains, four language adapters, a typed database
client, and a performance evidence product. Each child has a final hardening
phase that removes replaced owners and refuses compatibility residue.

Total authored LOC: +28,525-38,655 / -4,801-8,741

Generated current-schema JSON at the Phase 11A checkpoint: 15,805 LOC across
30 registered files

## Layman's wins

- The work can be approved and shipped in safe pieces instead of one giant bet.
- One proven safety system prevents source writes and runaway calls everywhere.
- CodeAtlas can find crashes, bad database inputs, slow growth, and real
  hotspots while clearly stating what it cannot prove.
- No old command surface or duplicate implementation is carried forward.
