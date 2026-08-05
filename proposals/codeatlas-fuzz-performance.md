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
for eleven independently reviewable proposals:

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
same plan executor. Remote, effectful, unknown, or incompletely isolated targets
never become single-shot. Remote/effectful/unknown targets require an explicitly
reviewed plan ID and may still block; incompletely isolated targets block
unconditionally because review cannot supply a capability.

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

Required for remote, effectful, exceptional, or incompletely preauthorized
targets.

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
unknown-effect, policy-exception, and mutating/effectful workloads—including
PostgreSQL DML—never qualify for single-shot execution.

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

Tracker date: 2026-08-05

This is the one cross-program ordering and progress tracker. Child proposals
remain the normative owners for behavior, acceptance evidence, and file
manifests. Their phase statuses are subordinate checkpoints and must be updated
with this section when a phase starts, completes, blocks, or changes order. Do
not create another program roadmap or copy these tasks into a scratch document.

### Current verdict

The public grammar, schema registry, HQA renderer, immutable execution
artifacts, enforcing HTTP call budget, complete structured callable evidence,
semantic-sibling analysis and self-dogfood, and locally verifiable OCI
implementation are done. The current host cannot grant a live OCI isolation
capability, so execution remains plan-only here and Phase 9 retains the hard
continuation gate. Work now follows the independent static lane through the
PostgreSQL query contract and repository-scope foundation. On implementation
progress rather than proposal-design progress, the complete program is
approximately 50 percent done.

### Order and ownership rules

1. Finish and commit the active dirty slice before changing product areas.
2. Shared contracts land before their second consumer. No child creates a
   private callable parser, corpus lattice, executor, artifact resolver,
   limiter, paginator, config editor, or metrics owner.
3. The OCI proof is the execution critical path. A capable runner preempts the
   independent static queue so the isolation gate closes as soon as possible.
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
Phase 4. This phase remains incomplete until Phase 9's live proof passes.

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

- [ ] Extend existing query inventory with stable query identity, placeholder
  order, statement class, parameter and result shapes, referenced objects,
  constraint evidence, effects, and exact block reasons.
- [ ] Keep SQL discovery in `src/postgres/source`, query policy in the
  PostgreSQL contract owner, and generic target/effect classification in the
  kernel owner.
- [ ] Classify dynamic SQL, DDL, transaction control, privileged operations,
  filesystem/program access, external links, and unknown functions as blocked
  before generated execution.
- [ ] Make DML checked-policy eligible but always reviewed-plan only, never
  single-shot, even for a local disposable target.
- [ ] Prove deterministic IDs, parameter order, constraints, effects, and
  eligibility against static or checked-in catalog fixtures with zero live
  calls.
- [ ] Publish any changed public schema, dogfood the static inventory, and
  commit PostgreSQL Phase 1.

### Phase 8: Build the repository-scope and config-edit foundation

Active child: [`codeatlas-subject-evidence-parity.md`](codeatlas-subject-evidence-parity.md)
Phase 1. This phase can start after the CLI hard cut and must not predeclare
the later observation-backed report shapes.

- [ ] Extract one ordered `RepositoryScope` for root/member ownership, config
  digests, code contexts, HTTP contracts, PostgreSQL contracts, and truthful
  discovery completeness.
- [ ] Flatten one `RepositoryScopeArgs` across code, tests, lexicon, HTTP, and
  PostgreSQL consumers without rescanning or reinterpreting `--workspace`.
- [ ] Extract one strict JSON config-edit owner from PostgreSQL init, with
  preview-first behavior, exact ownership refusal, reparse validation, and one
  selected-file write.
- [ ] Preserve single-project and pnpm-workspace code/test behavior while HTTP
  and PostgreSQL contracts resolve through the same ordered member scope.
- [ ] Prove generic config preview writes nothing, explicit insertion touches
  only one selected strict config, and every repository input is bounded and
  reused.
- [ ] Run focused repository-scope/config tests and dogfood, then commit
  subject-parity Phase 1 without advertising the later public commands.

### Phase 9: Pass the live OCI isolation continuation gate

Active child: execution kernel Phase 4. This task preempts Phases 5 through 8
as soon as an eligible runner is available.

- [ ] Resolve an exact rootful, rootless, or nested OCI runner with a local
  socket, digest-pinned probe image, external writable state, and no need to
  expose its control socket inside the child.
- [ ] Run target-observed mount, absolute-path, traversal, symlink, scratch,
  home, environment, network, subprocess, CPU, RSS, PID, descriptor, output,
  interruption, cancellation, and cleanup conformance cases.
- [ ] Verify every advertised capability comes from successful target-side
  evidence and that each failed or missing probe blocks before the first call.
- [ ] Verify rootless and nested behavior explicitly for every state the
  backend claims rather than extrapolating from one host mode.
- [ ] Record the runtime, client, server, image, kernel, cgroup, capability,
  fixture, and result digests needed to reproduce the matrix.
- [ ] Fix any backend or fixture defect and rerun the narrow failed case before
  rerunning the complete conformance matrix.
- [ ] Run full execution checks, CodeAtlas dogfood, and the generated-state
  audit, then mark and commit execution Phase 4 complete.

### Phase 10: Migrate HTTP fully onto the kernel

Active child: execution kernel Phases 5 and 6.

- [ ] Feed HTTP target, destination, authentication, readiness, stateful, and
  effect evidence into the shared target classifier.
- [ ] Route reviewed and eligible single-shot HTTP runs through the same
  persisted plan, sandbox, scheduler, permit ledger, TLS-terminating proxy,
  redactor, lease registry, and receipt path.
- [ ] Preserve positive, negative, boundary, unsupported-method, stateful, and
  exact OpenAPI operation behavior while enforcing finite calls and rates.
- [ ] Prove changed evidence refusal, managed/remote/production blocks,
  cancellation, incomplete cleanup, and budget exhaustion from the target
  side.
- [ ] Delete HTTP-private plan, budget, artifact, private-filesystem, direct
  executor, `max_examples`, and unsafe fallback owners rather than wrapping
  them.
- [ ] Synchronize public help, config, schemas, README, lexicon, tests, and
  self-audit commands with the one kernel path.
- [ ] Run focused HTTP/execution checks, the full required suite, dogfood, and
  the checkout-state audit, then commit Phases 5 and 6 separately.

### Phase 11: Build the shared corpus and callable harness foundation

Active child: [`codeatlas-code-fuzzing.md`](codeatlas-code-fuzzing.md) Phase 1.

- [ ] Define domain-neutral scalar and collection boundary descriptors,
  canonical ordering, finite depth/size limits, and deterministic pairwise
  selection in `src/fuzz/corpus.rs`.
- [ ] Map only supported `CallableContract` types and constructibility evidence
  into descriptors. Keep native value materialization domain-owned.
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
  tests, dogfood zero-call planning, and commit code-fuzz Phase 1.

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

- [ ] Implement truthful `init code` and `init http` proposals without URLs,
  secrets, execution targets, completeness claims, or effect policy.
- [ ] Implement `usage http` as known repository consumer evidence with
  visible external/dynamic incompleteness and no `unused_route` claim.
- [ ] Implement `usage postgres` as known static query touches with visible
  dynamic/catalog incompleteness and no `unused_table` or `unused_column`
  claim.
- [ ] Render deterministic sourced HTTP and PostgreSQL Markdown/HTML docs,
  including visible missing descriptions or catalog evidence, with zero hidden
  live calls.
- [ ] Prove docs `--check` never writes, init preview writes nothing, and an
  explicit init writes only one selected strict configuration file.
- [ ] Enrich PostgreSQL usage/docs only from an explicitly supplied exact
  observation reference, with no hidden target or database execution.
- [ ] Extract one generic bounded graph projection owner from code context
  slicing without merging code, HTTP, and PostgreSQL graph semantics.
- [ ] Keep existing code inspection bytes and cursor behavior exact after the
  shared projection extraction.
- [ ] Build HTTP contract, operation, schema, handler, caller, test, and
  optional fuzz-evidence graph nodes and typed edges.
- [ ] Build PostgreSQL contract, migration, query, parameter, table, column,
  constraint, callsite, and optional observation graph nodes and typed edges.
- [ ] Implement exact `inspect http` and `inspect postgres` target resolution,
  stable ambiguity errors, depth/node limits, digested cursors, and wrong-kind
  observation rejection.
- [ ] Prove inspection, usage, and docs remain bounded, deterministic,
  workspace-aware, and zero-call across cold and warm snapshots.
- [ ] Publish each final report shape once, run focused parity dogfood, and
  commit subject-parity Phases 2, 3, and 4 separately.

### Phase 16: Add the repository lexicon and harden subject parity

Active child: subject evidence parity Phases 5 and 6.

- [ ] Extract typed code, HTTP, and PostgreSQL term evidence with exact subject,
  owner, target, source, confidence, and completeness provenance.
- [ ] Reuse one normalization and concept-policy engine while keeping subject
  extraction in each domain adapter.
- [ ] Implement `lexicon repository --subjects code,http,postgres` as one
  bounded analysis, not subprocess composition or a code-command alias.
- [ ] Treat cross-subject term relationships as evidence only and require
  explicit policy or corroboration before semantic-equivalence claims.
- [ ] Publish the report/schema transition and preserve focused `lexicon code`
  behavior.
- [ ] Search for subject-private workspace discovery, config insertion,
  paginator, term normalization, hidden live calls, false unused labels,
  invented descriptions, and compatibility commands, then consolidate each
  real duplicate through its canonical owner.
- [ ] Run full checks and code/HTTP/PostgreSQL dogfood, update the public matrix
  and lexicon, audit external state, and commit Phases 5 and 6 separately.

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

- [ ] Replace the HQA sibling-checkout schema dependency with drift validation
  against the accepted neutral `agentspeak-contracts` application-inventory
  schema. Keep the CodeAtlas golden in this repository, vendor no external
  schema, remove the HQA-tree coupling, and make no HQA or neutral-repository
  edit from CodeAtlas.
- [ ] Add the CodeAtlas half of the cross-tool resolution conformance gate:
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
- [x] Implemented the complete local OCI command/runtime/conformance boundary,
  shared cancellation, resource evidence, and fallback cleanup path. The
  capable-host live proof remains the explicit Phase 9 gate.

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
- 2026-08-05: semantic-sibling Phase 1 passes strict config/model/schema/CLI
  checks and static self-dogfood; lexicon v5 is the sole published schema and
  shared-contract evidence cannot become corroboration.
- 2026-08-05: semantic-sibling Phase 2 passes bounded nomination and complete
  counterevidence tests, deterministic real-source CLI fixtures for all three
  dispositions, lexicon v5 drift checks, and warning-denying Clippy. Graph
  boundary evidence is indexed once and unconfigured lexicon runs retain their
  existing scan-only path.

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

Status: [x] Complete

LOC: +815 / -9

Verify: Source and OpenAPI routes render deterministically into the published
HQA v1 inventory with unique IDs, honest completeness, conservative dynamic
paths, no invented roles, and no HQA repository edit. Default CodeAtlas JSON
remains byte-identical; the golden validates against the exact external schema;
all repository checks and bounded CodeAtlas dogfood pass with zero gates.

```text
~ proposals/codeatlas-hqa-seeding.md
```

## Program stage 4: Execution kernel and HTTP migration

Status: [~] Accepted; execution Phases 1-3 and local Phase 4 complete, live isolation waits at Phase 9

Projected LOC after measured Phase 3: +11,760-13,310 / -878-1,408

Verify: One sandbox backend passes the full isolation suite; HTTP requests
cannot exceed call/rate/resource ceilings; source remains read-only; plans,
receipts, cleanup, reviewed execution, and eligible one-shot execution pass.

```text
~ proposals/codeatlas-execution-kernel-http-fuzz.md
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

Status: [ ] Accepted; waits for the sandbox gate

LOC: +2,400-3,800 / -430-880

Verify: Accepted callable/effect evidence, four-language parity, deterministic
boundary/replay, native engine adapters, automatic oracles, and CodeAtlas
self-fuzzing pass.

```text
~ proposals/codeatlas-code-fuzzing.md
```

## Program stage 8: PostgreSQL fuzzing

Status: [ ] Accepted; static Phase 1 is ready, live execution waits for
isolation, and generated cases wait for the callable corpus foundation

LOC: +2,150-3,350 / -410-830

Verify: Catalog-backed parameters, blocked SQL effects, guarded typed session,
deterministic/adaptive values, disposable cleanup, replay, and zero-live-call
baseline/diff pass.

```text
~ proposals/codeatlas-postgres-fuzzing.md
```

## Program stage 9: HTTP and PostgreSQL evidence parity

Status: [ ] Accepted; repository-scope Phase 1 is ready, later reports wait for
callable, PostgreSQL, and observation identities

LOC: +3,400-5,250 / -840-1,590

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

The full program is intentionally net higher because it adds a verified
sandbox, three execution domains, four language adapters, a typed database
client, and a performance evidence product. Each child has a final hardening
phase that removes replaced owners and refuses compatibility residue.

Total authored LOC: +21,936-31,416 / -4,183-7,813

Generated current-schema JSON: +7,706 LOC

## Layman's wins

- The work can be approved and shipped in safe pieces instead of one giant bet.
- One proven safety system prevents source writes and runaway calls everywhere.
- CodeAtlas can find crashes, bad database inputs, slow growth, and real
  hotspots while clearly stating what it cannot prove.
- No old command surface or duplicate implementation is carried forward.
