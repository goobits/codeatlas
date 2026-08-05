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

The recommended single-worker order is: finish the proposal/schema contract
pass; publish current schemas; add the CodeAtlas HQA renderer; build the kernel
and migrate HTTP; complete the kernel-owned CLI normalization; land structured
callable evidence; dogfood semantic-role siblings; add code fuzzing; add
PostgreSQL fuzzing and finish its observation-dependent CLI cut; fill static
subject parity; then add performance and separately gated cost search.

The static schema/HQA and structured-evidence/sibling branches do not require a
sandbox. Their position minimizes later artifact and adapter churn. PostgreSQL
consumes only the shared corpus descriptors established by code fuzzing, not
callable contracts or engines. Base performance could begin after the kernel,
but intentionally follows parity so its first attribution model is not
immediately replaced. Cost-guided work waits for explicit accepted
metric-feedback and threshold-preserving shrink capabilities.

## Live execution checklist

Keep this program-level checkpoint synchronized whenever a child phase starts,
finishes, blocks, or changes order. The active child proposal holds its more
detailed checklist.

- [x] Original seven-document fuzz/performance suite corrected and committed as
  `6845a24`.
- [x] Governance, lexicon, standalone Cargo boundary, self-configuration, and
  external self-audit foundation.
- [~] Evidence lifecycle CLI: testing and architecture hard cuts plus shared
  execution flags are complete; PostgreSQL observation routing waits for its
  accepted owner.
- [x] Eleven-child proposal/schema convergence pass: four child proposals, RFC
  8785 plan identity, interop corrections, extracted callable ownership, links,
  and exact LOC arithmetic are internally verified.
- [x] Published CodeAtlas schemas and drift enforcement: 25 current roots,
  prospective artifact identity, and the CodeAtlas annotation namespace are
  complete.
- [x] CodeAtlas HQA application-inventory renderer; deterministic external-v1
  projection complete with no HQA repository edits.
- [~] Execution kernel: governance, immutable artifacts, and pre-call
  budget/proxy Phases 1-3 are complete; verified isolation Phase 4 is active,
  with HTTP migration next.
- [ ] Structured callable/effect evidence and lexicon v4.
- [ ] Advisory semantic-role sibling evidence and CodeAtlas dogfood.
- [ ] Callable code fuzzing and shared boundary corpus.
- [ ] PostgreSQL fuzzing and observation lifecycle.
- [ ] HTTP/PostgreSQL evidence parity.
- [ ] Performance curves, regressions, candidates, and gated profiler evidence.
- [ ] Cost-guided isolated search.
- [ ] Whole-program consolidation, dogfood, docs honesty, and release hardening.

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

Status: [~] Accepted; execution Phases 1-3 complete, isolation Phase 4 active

Projected LOC after measured Phase 3: +11,760-13,310 / -878-1,408

Verify: One sandbox backend passes the full isolation suite; HTTP requests
cannot exceed call/rate/resource ceilings; source remains read-only; plans,
receipts, cleanup, reviewed execution, and eligible one-shot execution pass.

```text
~ proposals/codeatlas-execution-kernel-http-fuzz.md
```

## Program stage 5: Structured callable evidence

Status: [ ] Accepted; independent of the sandbox track

LOC: +900-1,300 / -200-350

Verify: One four-language callable/effect contract feeds inspect, lexicon,
witnesses, sibling analysis, and fuzz planning; the display-signature heuristic
is deleted; lexicon/cache/schema identities are exact.

```text
~ proposals/codeatlas-structured-callable-evidence.md
```

## Program stage 6: Semantic-role siblings

Status: [ ] Accepted advisory analysis; follows structured callable evidence

LOC: +850-1,350 / -100-250

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

Status: [ ] Accepted; runs after the callable corpus foundation

LOC: +2,150-3,350 / -410-830

Verify: Catalog-backed parameters, blocked SQL effects, guarded typed session,
deterministic/adaptive values, disposable cleanup, replay, and zero-live-call
baseline/diff pass.

```text
~ proposals/codeatlas-postgres-fuzzing.md
```

## Program stage 9: HTTP and PostgreSQL evidence parity

Status: [ ] Accepted; runs after callable and PostgreSQL contract evidence

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
