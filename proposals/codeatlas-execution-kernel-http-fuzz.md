# Execution kernel and bounded HTTP fuzzing

Status: Proposed for first implementation authorization

Decision scope: Shared execution safety, immutable plans and receipts, enforced
isolation, and migration of existing HTTP fuzzing

Depends on: [`codeatlas-evidence-lifecycle-cli.md`](codeatlas-evidence-lifecycle-cli.md)
for the final public execution grammar. Phase 1 governance and self-audit can
start independently and in parallel with the CLI hard cut.

Unblocks: Callable fuzzing, PostgreSQL fuzzing, and performance evidence

## Decision

CodeAtlas will add one execution kernel and migrate existing HTTP fuzzing onto
it before adding another execution domain. This is the smallest acceptance unit
that proves the cross-domain safety contract against a real existing adapter.

The kernel owns:

- Immutable zero-call plans and exact execution receipts.
- Configuration and evidence digests.
- Finite resource budgets and pre-call permits.
- Rate and concurrency enforcement.
- Cancellation and partial outcomes.
- Sandbox capability discovery and enforcement.
- External scratch, private artifacts, and bounded captured output.
- Target classification and single-shot eligibility.
- Artifact identity/addressing, replay derivation, redaction, and cleanup
  leases.

HTTP continues to own OpenAPI interpretation, Schemathesis integration,
request/response oracles, stateful links, managed server lifecycle, and HTTP
report evidence.

This proposal does not add callable fuzzing, PostgreSQL parameter fuzzing,
performance curves, or cost-guided search. Those are independently reviewable
follow-ons that import the accepted kernel contract.

## Options considered

| Option | Fit | Tradeoff | Verdict |
|---|---|---|---|
| Keep HTTP-private execution safety | Smallest immediate diff | Every later domain must duplicate or migrate it | Reject |
| Build all fuzz/performance domains together | One program launch | Sandbox and ownership defects become buried in a very large review | Reject |
| Build an abstract kernel without a real adapter | Fast model work | Does not prove enforcement around an external engine | Reject |
| Kernel plus complete HTTP migration | Proves one shared contract end to end | Larger than a model-only phase but independently valuable | Adopt |

## Existing evidence and reuse

Reuse these current owners:

- `src/http/schemathesis/*` for generation, stateful execution, reduction, and
  normalized Schemathesis evidence.
- `src/http/target.rs` and `src/http/runtime.rs` for configured target and
  managed process lifecycle.
- `src/http/private_fs.rs` as the source behavior to rehome into the shared
  artifact owner, then delete.
- `src/environment.rs` and `src/source_index/environment.rs` for external state
  and cache conventions.
- `src/external_tool.rs` for exact tool discovery and fingerprints; the pinned
  provisioning pattern currently private to Schemathesis moves here before
  native fuzz engines or container helpers reuse it.
- `src/commands/output.rs` for output semantics where the artifact is
  explicitly caller-selected.

No current module owns a cross-domain plan, receipt, budget ledger, enforcing
proxy, sandbox capability, target classifier, artifact store, replay planner,
redaction policy, cleanup registry, or execution outcome, so `src/execution`
is a new cohesive owner rather than a parallel implementation.

The named ownership map is:

- `src/execution/target.rs`: target class, effect corroboration, required
  capabilities, and the single-shot eligibility decision.
- `src/execution/artifact.rs`: content identities, the external artifact store,
  typed reference resolution, digest linkage, retention, and byte ceilings.
- `src/execution/redaction.rs`: secret references, destination scoping,
  allowlisted environment, redaction verification, and fail-closed output.
- `src/execution/lease.rs`: acquired processes, proxies, containers, databases,
  scratch roots, release verification, and cleanup evidence.
- `src/execution/resource.rs`: shared elapsed/CPU/RSS/process sampling
  primitives; source-index telemetry and performance remain separate consumers.
- `src/fuzz/reproducer.rs`: one versioned reproducer envelope with a typed
  domain payload; kernel replay derives a new plan from it.
- `src/cli/execution.rs`: flattened common execution/fuzz limit arguments.

Domains contribute target/effect evidence, redaction patterns, cleanup actions,
and typed artifact payloads. They do not reimplement any decision or registry.

## Product contract

The safe flow is:

```text
resolve target and source evidence
    -> classify effects and required capabilities
    -> persist immutable zero-call plan
    -> authorize reviewed or preauthorized-isolated execution
    -> acquire sandbox and budget guards
    -> execute every call through a permit boundary
    -> clean resources
    -> persist immutable receipt and HTTP report
```

CodeAtlas remains read-only with respect to analyzed source. Plans, receipts,
reports, proxies, downloaded engines, caches, and scratch state default to a
private external CodeAtlas root. A caller may explicitly select an output file,
but target runtime state never writes into the checkout.

There is no force path, unlimited sentinel, declaration-only isolation bypass,
or unguarded raw target client.

## Shared execution model

The implementation is typed:

```text
ExecutionPlan<HttpFuzzWorkload>
ExecutionReceipt<HttpFuzzResult>
```

The generic envelope contains:

- Artifact version, operation, subject, and canonical plan ID.
- Workspace, source, config, target, contract, tool, and engine digests.
- Concrete seed and strategy identity.
- Required effects and isolation capabilities.
- External destinations, managed commands, and writable scratch roots.
- Common limits and HTTP-specific case/shrink limits.
- Expected calls by category where enumerable and a hard whole-run maximum.
- Authorization mode.

The receipt contains:

- Exact plan and result digests.
- Runtime isolation and environment fingerprints.
- Calls reserved and consumed by setup, readiness, authentication, generated
  case, stateful step, reduction, retry, validation, and cleanup.
- Peak rate, concurrency, elapsed time, CPU, RSS, process count, result bytes,
  output bytes, and artifact bytes where supported.
- Cleanup evidence.
- `passed`, `failed`, `partial`, `blocked`, or `cancelled` outcome.

Changed source, target, config, engine, tool, or policy evidence invalidates an
old plan. Budget exhaustion, incomplete cleanup, unavailable required
isolation, and interrupted execution can never be `passed`.

## Artifact identity and addressing

The kernel owns one content-addressed namespace for every persisted execution
artifact:

```text
plan_<digest>
receipt_<digest>
observation_<digest>
baseline_<digest>
reproducer_<digest>
report_<digest>
```

Every envelope contains its kind, subject, schema/API version, canonical ID,
content digest, parent/link digests, creation tool identity, and bounded payload.
Managed artifacts live under the external CodeAtlas state root. `--out` exports
the same envelope to one explicit file; it does not create a second identity.

One `ArtifactRef` resolver accepts either a strict typed ID or an explicit file
path. An ID resolves only through the managed external store. A file is loaded,
schema-checked, rehashed, and required to contain the matching canonical ID.
`--plan`, `--observation`, `--against`, and `--replay` all use this contract
rather than domain-owned lookup rules. Wrong-kind references fail before any
target call.

Observations and baselines remain domain-owned payloads. The shared store owns
only their identity, addressing, linkage, privacy, retention, and size rules.

## Configuration is strict JSON

The repository contract is `codeatlas.json`; there is no TOML configuration.
The proposed semantic shape is:

```json
{
  "execution": {
    "limits": {
      "max_calls": 100,
      "calls_per_second": 5,
      "max_concurrency": 1,
      "run_timeout_ms": 60000,
      "max_cpu_time_ms": 45000,
      "max_rss_bytes": 536870912,
      "max_call_result_bytes": 1048576,
      "max_output_bytes": 16777216,
      "max_processes": 8,
      "max_open_files": 128
    },
    "isolation": {
      "backend": "auto",
      "filesystem": "scratch_only",
      "network": "deny",
      "processes": "deny"
    }
  },
  "fuzz": {
    "limits": {
      "max_cases": 50,
      "max_shrinks": 100,
      "max_failures": 5,
      "case_timeout_ms": 3000
    }
  }
}
```

The exact schema is implemented in the existing strict config owner. Every
value is finite. Built-in profiles expand into explicit plan values so a saved
plan never depends on a profile name whose future meaning could drift. CLI
limits may tighten configured ceilings and may not raise them.

### Authoritative CLI limits

`src/cli/execution.rs` exposes one flattened `ExecutionLimitArgs` everywhere
execution is possible:

```text
--max-calls
--calls-per-second
--max-concurrency
--run-timeout-ms
--max-cpu-time-ms
--max-rss-bytes
--max-processes
--max-open-files
--max-call-result-bytes
--max-output-bytes
--max-artifact-bytes
```

`FuzzLimitArgs` flattens that struct and adds only strategy limits shared by
fuzz domains:

```text
--max-cases
--max-shrinks
--max-failures
--case-timeout-ms
```

Domain CLI modules may add selectors and genuine domain ceilings such as rows,
response bytes, or input depth. They reference these shared structs rather than
relisting or re-parsing common flags.

## Preview and execution UX

Reviewed execution remains available everywhere:

```bash
codeatlas fuzz http --target local-api
codeatlas fuzz http --replay reproducer_ABC
codeatlas fuzz http --plan plan_ABC --execute
```

Target and reproducer inputs are both zero-call planning forms. Replay loads a
versioned `Reproducer<DomainPayload>`, validates its parent plan, source,
contract, target, tool/engine, seed, oracle, and result digests, then derives a
new immutable replay plan under current policy. Current ceilings may tighten
the saved run and may never expand it. Loading a reproducer does not call the
target, and replay never bypasses normal authorization or isolation.

A constrained single-shot form is allowed only for a checked-in target that is
both configured as preauthorized and proven at runtime to be fully local and
disposable:

```bash
codeatlas fuzz http --target local-api --execute
```

Single-shot execution is not a second executor. It performs the same steps in
one process:

1. Build and persist the immutable plan.
2. Verify source/config/target digests.
3. Verify every required isolation capability.
4. Verify there are no remote or uncontained effects.
5. Execute that exact persisted plan through the same kernel.
6. Record `authorization_mode: preauthorized_isolated` in the receipt.

It is unavailable for remote targets, unknown effects, missing capabilities,
raised CLI limits, or any target requiring a policy exception. Those require
the explicit reviewed plan ID. Configuration alone cannot make an unsafe
target eligible; runtime capabilities and destinations must corroborate it.

This acknowledges that consumers will automate local disposable runs while
preventing wrapper scripts from creating an unplanned execution path.

`src/execution/target.rs` makes the eligibility decision once. Domain adapters
submit typed evidence for locality, disposability, environment class,
destinations, effects, cleanup, and required capabilities. The classifier
returns the exact block/review/preauthorization reason. A reviewed plan grants
authorization only; it cannot waive a missing sandbox capability. Remote,
production, unknown-effect, policy-exception, and mutating/effectful workloads
never qualify for single-shot execution even when their backing service is
disposable.

## Budget enforcement

Budgets are pre-call permits, not post-run counters:

1. The caller reserves a permit before every target call.
2. The atomic ledger checks whole-run calls, rate, concurrency, cancellation,
   time, and resource state.
3. Failed and rejected calls are not refunded, closing retry-loop evasion.
4. A semaphore bounds in-flight work.
5. A token bucket bounds call rate.
6. No automatic retry exists unless it is explicit in the plan and consumes a
   new permit.
7. A hard limit cancels remaining work and produces `partial`.

Before the first target call, the ledger carves a finite cleanup allowance out
of the same whole-run call and time ceilings. Setup, cases, retries, and
reduction cannot spend that allowance. Cleanup still acquires permits and may
never exceed the plan's global maximum; there is no hidden emergency budget.
If required cleanup cannot be bounded and reserved up front, execution blocks.

Every acquired process, proxy, container, temporary database, scratch root, or
other managed resource registers an `ExecutionLease` before it becomes visible.
The lease stores its cleanup action, allowance, owner, and verification probe.
The runner releases leases in reverse acquisition order on success, failure,
cancellation, and interruption, and the receipt records each verified release.
A domain may implement a cleanup action; it may not implement a second lease
registry or claim success from an unverified drop/kill/remove call.

Schemathesis cannot be trusted as the sole counter because setup, shrinking,
stateful work, and future behavior may exceed its visible `max_examples`
setting. All Schemathesis traffic and CodeAtlas health/authentication probes
route through a local enforcing proxy. The fuzz sandbox can reach only that
proxy; the proxy forwards only to exact planned destinations.

HTTPS is terminated at the enforcing proxy, not counted at CONNECT-tunnel
level. CONNECT cannot count individual HTTP/1.1 keep-alive or multiplexed
HTTP/2 requests, so it cannot uphold `max_calls`. Each run uses an ephemeral
private CA trusted only inside the fuzz-client sandbox; the proxy validates the
upstream certificate, exact host, port, SNI, redirect, response size, and one
permit per decoded request. Certificate pinning, unsupported mutual TLS, or an
untrusted upstream blocks instead of falling back to an opaque tunnel or
certificate verification bypass.

At the CodeAtlas boundary the canonical terms are `case` and `max_cases`.
`example` and `max_examples` are translated only inside the Schemathesis
adapter and never retained as public aliases.

## Isolation contract

Static effect evidence informs planning but never replaces operating-system
enforcement. Managed target execution requires:

- Read-only checkout mount.
- Read-only runtime root.
- One external disposable scratch root as the only writable mount.
- Sandbox `/tmp`, language caches, package caches, and generated state under
  that scratch/cache root.
- No host home, ambient credentials, SSH agent, cloud metadata,
  container-management socket, or ambient secret environment. Only exact
  target-scoped secret references declared by policy may be injected.
- Network denied by default or restricted to exact proxy/target destinations.
- Child processes denied by default or restricted to exact planned tools.
- Bounded CPU, RSS, processes, file descriptors, stdout, stderr, response,
  result, and artifact bytes.
- Traversal and symlink attempts unable to escape mount confinement.
- Named cleanup ownership and post-run residue verification.

Environment-variable redirection alone does not satisfy this contract.
Secret values never enter plans, receipts, reports, command lines, or captured
output; the plan records only the reference and exact destination/header scope,
and execution fails closed when redaction or scoping cannot be proven.

### Capability matrix

Support is capability-based rather than promised from an operating-system name:

| Host/runtime state | Planning | Managed local execution | Remote disposable HTTP |
|---|---:|---:|---:|
| Verified container backend with all required mounts, network, process, and resource controls | Yes | Eligible | Eligible with reviewed external policy |
| Future verified native Linux backend | Yes | Eligible for capabilities it proves | Eligible with reviewed external policy |
| macOS or Windows without a verified container/native backend | Yes | Blocked | Blocked |
| Any host with an incomplete backend | Yes | Blocked | Blocked |
| Any host with no backend | Yes | Blocked | Blocked |

The first stable implementation target is one verified OCI-compatible container
backend selected through an exact executable and runtime capability probe. A
Docker- or Podman-like name is not sufficient; acceptance tests prove actual
read-only mounts, scratch confinement, network policy, process/resource limits,
and cleanup. Native backends are future adapters and are not advertised until
they pass the same conformance suite.

The probe explicitly distinguishes the host-side orchestrator boundary from
the sandbox. A rootful, rootless, or nested-container runtime may be used only
when its actual user namespace, cgroup, mount propagation, network, socket, and
resource-limit behavior passes. CodeAtlas may invoke an exact runtime CLI or
host-side socket while creating the sandbox; that control socket is never
mounted or forwarded into the child. A development container without a usable
host-side runtime is plan-only rather than silently privileged. Receipts record
rootless/nested state and the capabilities actually proven.

Consequences are explicit:

- Static scans and plan generation remain portable.
- A macOS or Windows machine without a verified backend is plan-only.
- No fallback silently runs target code on the host.
- Every plan and receipt names the backend and proven capabilities.

## Tool provisioning and resource evidence

`src/external_tool.rs` generalizes the current Schemathesis provisioning
pattern: exact source/version/digest, external cache root, install lock, size
ceiling, capability probe, and tool fingerprint. Schemathesis, container
helpers, and later cargo-fuzz, Hypothesis, fast-check, database clients, and
profilers consume this owner; no adapter gets a private downloader.

`src/execution/resource.rs` samples monotonic elapsed time and supported
CPU/RSS/process/descriptor evidence behind one capability-reporting primitive.
Execution receipts, internal source-index telemetry, and performance
observations may reuse the sampler while retaining separate schemas and policy.

## HTTP environment policy

HTTP method is not effect evidence. `GET` may mutate and a state-changing
operation may be safe when the entire target is disposable.

Eligible targets are:

- A managed server command inside the verified sandbox, with read-only source
  and disposable writable state.
- An exact remote disposable/staging target with an explicit host allowlist,
  reset/cleanup contract, finite budgets, and reviewed authorization.

Blocked targets are:

- Production-classified targets.
- Arbitrary execution-time URLs.
- Redirects outside the planned host set.
- Undeclared authentication or setup destinations.
- Managed commands without required sandbox capabilities.
- Runs whose managed process or disposable state cannot be cleaned up.

Remote and effectful targets never qualify for single-shot preauthorization.

## Dogfooding and validation

Phase 1 must reproduce the incomplete checkpoint with Cargo available and all
generated state external:

```text
codeatlas scan code --scope source --all --format json
codeatlas check code
codeatlas usage code
codeatlas inspect code <exact-target>
codeatlas lexicon code --format json
codeatlas scan tests
codeatlas check tests
```

Lexicon has no gate flag. Its findings are recorded and classified explicitly.

The execution conformance suite uses controlled targets that attempt:

- Call `max_calls + 1`.
- Bursts and excess parallel calls.
- Unplanned retries and redirects.
- Relative, absolute, traversal, symlink, checkout, home, and `/tmp` writes.
- Network access outside the proxy.
- Undeclared child processes.
- Output, response, CPU, RSS, PID, descriptor, and time exhaustion.
- Interruption during setup, execution, reduction, and cleanup.

The test observes the target side, proving rejected actions never arrived. A
receipt-only assertion is insufficient.

## Process ownership

Stable TypeMill use is an engineering workflow, not a CodeAtlas product
dependency. The durable preview/apply/receipt rule belongs in `AGENTS.md`.
Implementation follows that repository rule when the installed stable `mill`
advertises the required capability; detailed Mill steps do not remain in this
product proposal.

## Governance and vocabulary foundation

Phase 1 expands the terse CodeAtlas `AGENTS.md` with the parent repository's
durable engineering discipline, adapted to this product rather than copied
mechanically. The canonical operational entrypoint will own:

- Restart discipline: reread the active proposal, inspect status plus staged
  and unstaged diffs, and verify actual tools, state roots, and artifacts.
- The product boundary: CodeAtlas gathers read-only evidence and applies
  policy; it neither mutates consumer source nor absorbs TypeMill's mutation
  lifecycle.
- Current module ownership and dependency direction, including the execution,
  domain-adapter, fuzz, performance, testing, artifact, and rendering seams.
- One canonical naming grammar and hard removal of replaced pre-v1 vocabulary.
- Bounded scans, parse-once/index reuse, explicit concurrency and I/O limits,
  and the cache rule requiring an owner, key, invalidation, size ceiling, and
  hit/miss/eviction evidence.
- The conceptual-integrity, consolidation, performance, determinism,
  reliability, boundary, testability, observability, security, and simplicity
  review lenses.
- Deterministic high-value tests, external generated state, narrow-to-broad
  validation, dogfooding, and shared-worktree Git safety.
- Capability-based use of the installed stable `mill`, without matching it to
  an in-progress checkout version or making it a product dependency.

TypeMill-specific transaction, recovery, and mutation-stage rules stay in
TypeMill because CodeAtlas does not own those concepts.

`docs/concepts/lexicon.md` becomes the canonical vocabulary owner. Its initial
table defines at least `target`, `contract`, `case`, `call`, `workload`, `plan`,
`receipt`, `observation`, `measurement`, `optimization candidate`, `hotspot`,
`regression`, `failure`, `reproducer`, `effect`, `capability`, and `isolation`.
Each entry records its exact meaning, owning module/report, required subject
qualification, and retired synonyms. Public schema fields, CLI terms,
diagnostics, tests, and proposals must use that table; external terms such as
Schemathesis `example` are translated at their adapter boundary.

## Phase 1: Governance and reproducible self-audit

Status: [ ] Not started

LOC: +550-800 / -20-50

Verify: Cargo-backed self scan/check/usage/inspect/lexicon/test evidence runs
with external state, current findings are classified, and no checkout artifact
or unrelated Git change exists.

```text
~ AGENTS.md
+ docs/concepts/lexicon.md
+ codeatlas.json
~ proposals/codeatlas-execution-kernel-http-fuzz.md
~ tasks/check-self.js
~ package.json
~ tests/cli_contract.rs
```

## Phase 2: Immutable plans, receipts, and private artifacts

Status: [ ] Not started

LOC: +700-1,000 / -150-250

Verify: Target and replay preview make zero calls; canonical plan/artifact IDs
and typed references are stable; changed evidence refuses execution; the shared
classifier cannot preauthorize remote/effectful targets; artifacts are private,
bounded, redacted, and external; leases release on every outcome; incomplete
work cannot pass.

```text
+ src/config/execution.rs
+ src/config/fuzz.rs
+ src/execution/mod.rs
+ src/execution/model.rs
+ src/execution/policy.rs
+ src/execution/target.rs
+ src/execution/artifact.rs
+ src/execution/redaction.rs
+ src/execution/lease.rs
+ src/execution/resource.rs
+ src/execution/runner.rs
+ src/fuzz/mod.rs
+ src/fuzz/model.rs
+ src/fuzz/report.rs
+ src/fuzz/reproducer.rs
+ src/commands/fuzz.rs
+ tests/fuzz_plan.rs
~ src/main.rs
~ src/config/mod.rs
~ src/cli/execution.rs
~ src/cli/fuzz.rs
~ src/environment.rs
~ src/external_tool.rs
```

## Phase 3: Pre-call budgets and enforcing HTTP proxy

Status: [ ] Not started

LOC: +700-1,050 / -100-200

Verify: The target never observes call `max_calls + 1`; rate and concurrency
bursts remain bounded; setup, health, authentication, stateful, reduction,
retry, and cleanup calls are counted; HTTPS is terminated and counted per
decoded request with exact-host/upstream verification; budget exhaustion is
`partial`.

```text
+ src/execution/budget.rs
+ src/execution/proxy.rs
+ tests/execution_policy.rs
~ src/execution/model.rs
~ src/execution/runner.rs
~ src/http/schemathesis/mod.rs
~ src/http/schemathesis/hooks.py
~ src/http/schemathesis/request_adapter.rs
~ src/http/schemathesis/tests.rs
~ tests/http_cli.rs
```

## Phase 4: Verified isolation backend and capability matrix

Status: [ ] Not started

LOC: +1,200-1,900 / -50-150

Verify: The backend conformance suite proves mount, scratch, symlink, network,
process, environment, CPU, RSS, PID, descriptor, output, interruption, and
cleanup guarantees across advertised rootful/rootless/nested states; the
control socket never enters the child; incomplete/no backend blocks before the
first target call; plan-only behavior remains portable.

```text
+ src/execution/isolation.rs
+ src/execution/sandbox/mod.rs
+ src/execution/sandbox/container.rs
+ tests/execution_isolation.rs
+ tests/fixtures/execution_isolation/
~ Cargo.toml
~ Cargo.lock
~ src/execution/model.rs
~ src/execution/runner.rs
~ src/config/execution.rs
~ src/environment.rs
~ src/external_tool.rs
~ package.json
```

This phase is a hard continuation gate. No later proposal may assume code
execution safety until one backend passes the suite on its advertised hosts.

## Phase 5: Complete HTTP migration

Status: [ ] Not started

LOC: +700-1,100 / -250-450

Verify: Existing positive, negative, boundary, unsupported-method, and stateful
coverage remains; domain evidence feeds the one target classifier;
managed/remote policy blocks are exact; reviewed and eligible single-shot modes
use the same plan executor; shared redaction and lease cleanup pass.

```text
~ src/commands/http.rs
~ src/cli/fuzz.rs
~ src/config/http.rs
~ src/http/mod.rs
~ src/http/model.rs
~ src/http/target.rs
~ src/http/runtime.rs
~ src/http/schemathesis/mod.rs
~ src/http/schemathesis/report/mod.rs
~ src/http/schemathesis/report/evidence.rs
~ src/http/schemathesis/report/summary.rs
~ src/http/schemathesis/report/tests.rs
~ src/http/schemathesis/tests.rs
~ tests/http_cli.rs
- src/http/private_fs.rs
```

## Phase 6: Consolidation, docs, and release hardening

Status: [ ] Not started

LOC: +250-450 / -250-450

Verify: Full required checks and self-dogfood pass; repository search finds no
HTTP-private plan/budget/artifact owner, `max_examples` public alias, direct
executor, unsafe fallback, stale docs, or generated checkout state.

```text
~ README.md
~ AGENTS.md
~ docs/concepts/lexicon.md
~ proposals/codeatlas-fuzz-performance.md
~ proposals/codeatlas-execution-kernel-http-fuzz.md
~ package.json
~ tasks/check-self.js
~ tests/execution_policy.rs
~ tests/execution_isolation.rs
~ tests/http_cli.rs
```

The implementation is intentionally net higher because it adds a real sandbox,
budget proxy, immutable artifacts, and security conformance tests. It must not
be higher because old HTTP-private and new shared mechanisms coexist.

Total LOC: +4,100-6,300 / -820-1,550

## Layman's wins

- HTTP fuzzing cannot silently spam a service or write into the source checkout.
- Every run has a visible maximum cost and a receipt proving what happened.
- Safe local targets remain convenient; risky targets require explicit review.
- Later code, database, and performance work reuse one proven safety system.
