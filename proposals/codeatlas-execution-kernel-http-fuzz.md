# Execution kernel and bounded HTTP fuzzing

Status: Accepted; implementation in progress

Decision scope: Shared execution safety, immutable plans and receipts, enforced
isolation, and migration of existing HTTP fuzzing

Depends on:

- [`codeatlas-evidence-lifecycle-cli.md`](codeatlas-evidence-lifecycle-cli.md)
  for the final public execution grammar
- [`codeatlas-published-schemas.md`](codeatlas-published-schemas.md) for the
  new-artifact registry and namespaced version contract

Phase 1 governance and self-audit can start independently and in parallel with
the CLI hard cut and schema publication.

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
request/response oracles, stateful links, the managed-server workload
description, and HTTP report evidence. The shared kernel owns the actual
managed process lifecycle.

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
- `src/http/target.rs` for configured target evidence. The host-direct
  `src/http/runtime.rs` lifecycle is deletion input for Phase 5, not a retained
  owner.
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
- External destinations, managed commands, and logical writable-scratch
  requirements; the executor assigns volatile physical roots after authorization.
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

Every envelope contains its kind, subject, single namespaced schema version,
canonical ID, content digest, parent/link digests, creation tool identity, and
bounded payload.
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

### Canonical execution-plan ID

The first execution artifact locks the byte contract for the namespace. The
current `ExecutionPlan` uses the namespaced schema string
`codeatlas.execution-plan/v2`. Version 2 adds canonical managed-image evidence;
the pre-v1 implementation hard-cuts from the earlier prospective v1 contract
without retaining a compatibility reader. Its identity is derived as follows:

1. Build the typed plan identity body containing the schema/subject/operation,
   exact target and evidence digests, workload, effects, required capabilities,
   destinations, tool/engine identities, policy digest, explicit ceilings, and
   eligibility decision.
2. Exclude only the derived `id` and `content_digest`. A plan contains no
   timestamp, local artifact path, checkout path, physical scratch path,
   display command, or other volatile metadata; identical accepted evidence
   must produce identical plan bytes.
3. Serialize the identity body with RFC 8785 JSON Canonicalization Scheme.
   Ordinary `serde_json` map ordering or the architecture subsystem's current
   restricted-integer helper is not assumed to be RFC 8785 conformance.
4. Hash these exact bytes:

   ```text
   atlas.codeatlas.dev/execution-plan/v2\n<RFC-8785-plan-bytes>
   ```

5. Serialize the digest as `sha256:<64 lowercase hex>` and the canonical ID as
   `plan_<same 64 lowercase hex>`.

The canonicalizer has one audited owner in `src/execution/artifact.rs` and must
pass the RFC 8785 published vectors plus CodeAtlas vectors for the domain
separator, Unicode, numeric serialization, nested key order, and field
exclusion. A value RFC 8785 cannot represent fails during zero-call planning.
Changing canonicalization, the domain separator, or identity-body membership
requires a new plan schema version and invalidates old cached authorization;
there is no fallback reader under the same version.

Every later artifact kind receives its own registered schema string and domain
separator. It may reuse the canonicalization primitive but cannot reuse the
execution-plan digest kind.

Each new artifact uses the published-schema registry's prospective artifact
mode. Its required schema-version string equals the schema `$id`, and the root
cannot carry a parallel API-version field. An unregistered schema file, invalid
namespace, reused ID, or payload drift under an unchanged version fails the
read-only registry tests.

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
      "processes": "deny",
      "container": {
        "executable": "/usr/bin/docker",
        "socket": "/var/run/docker.sock",
        "probe_image": "registry.example/codeatlas-probe@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
      }
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
production, unknown-effect, policy-exception, and uncontained-effect workloads
never qualify for single-shot execution even when their backing service is
disposable. Sandbox-contained mutation is eligible only when every other
preauthorization condition is corroborated.

## Budget enforcement

Budgets are pre-call permits, not post-run counters:

1. The caller reserves a permit before every target call.
2. The atomic ledger checks whole-run calls, rate, concurrency, cancellation,
   time, and resource state.
3. Failed and rejected calls are not refunded, closing retry-loop evasion.
4. A semaphore bounds in-flight work.
5. One monotonic admission clock spaces calls at the configured rate without a
   burst allowance.
6. No automatic retry exists unless it is explicit in the plan and consumes a
   new permit.
7. A hard limit cancels remaining work and produces `partial`.

The kernel owns one bounded asynchronous scheduler per execution. Domains do
not create private runtimes, thread pools, semaphores, or work queues. Network
connections may be reused and HTTP/2 streams may be multiplexed, but each
decoded logical call obtains its own permit and monotonic sequence number
before dispatch. Bounded semaphores and engine queues provide backpressure;
blocking engine, filesystem, and CPU work uses one separately bounded blocking
pool. Receipts serialize call evidence by permit sequence rather than completion
order, so runtime multiplexing cannot change canonical output. Structured
cancellation stops admission, drains owned tasks, and enters the reserved
cleanup path.

Before the first target call, the ledger carves a finite cleanup allowance out
of the same whole-run call and time ceilings. Setup, cases, retries, and
reduction cannot spend that allowance. Cleanup still acquires permits and may
never exceed the plan's global maximum; there is no hidden emergency budget.
If required cleanup cannot be bounded and reserved up front, execution blocks.
For plan schema v1, cleanup time is exactly 20 percent of `run_timeout_ms`,
clamped to 1 millisecond through 10 seconds. The plan-owned cleanup call count
must fit that window at the same paced call rate with time remaining to finish;
an arithmetically impossible reservation blocks before execution.

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

The managed hook carries a kernel-owned internal call-category header and
overwrites project adapter or authenticator attempts to claim it. The proxy
strips that control header before forwarding. Unsupported-method probes and
CodeAtlas authentication probes have exact categories. When Schemathesis does
not expose trustworthy stateful or reduction provenance, the call is still
counted but remains a `generated_case`; CodeAtlas never fabricates a more
specific subtype. Phase 3 target-proves this routing contract while public
execution remains blocked; Phase 5 connects the sandboxed engine, readiness,
and cleanup lifecycle to the same boundary.

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
HTTP `environment` and literal header values are explicitly non-secret semantic
evidence. `secret_environment` maps a target variable to an ambient reference,
and header `value_env` names an ambient reference; zero-call planning never
resolves either value.

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

Remote targets and targets with uncontained effects never qualify for
single-shot preauthorization.

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
table defines at least `target`, `target block`, `contract`, `case`, `call`,
`workload`, `plan`, `receipt`, `observation`, `measurement`, `optimization
candidate`, `hotspot`, `regression`, `failure`, `reproducer`, `effect`,
`capability`, `isolation`, `inventory`, `hint`, and `annotation key`. Each entry
records its exact meaning, owning module/report, required subject qualification,
and retired synonyms. Public schema fields, CLI terms, diagnostics, tests, and
proposals must use that table; external terms such as Schemathesis `example`
are translated at their adapter boundary. The target-block entry references the
superseding external core-plus-annotations contract without reproducing its
obsolete draft, and annotation keys reserve the `codeatlas.` producer
namespace defined by the published-schemas proposal.

## Phase 1: Governance and reproducible self-audit

Status: [x] Complete

Execution checkpoint (keep current across context restarts):

- [x] Proposal suite reviewed, consolidated, and committed as `6845a24`.
- [x] Expanded `AGENTS.md`, established the canonical lexicon, and added a
  strict Rust/JavaScript self-analysis config.
- [x] Cargo reproducibility exposed the enclosing TypeMill workspace; the
  explicit standalone CodeAtlas workspace boundary now passes Cargo tests.
- [x] The self-audit task covers scan, check, usage, exact inspect, lexicon,
  `scan tests`, and `check tests` with private external artifacts.
- [x] The stable testing and architecture CLI hard cuts pass focused, contract,
  and Cargo-backed dogfood verification.
- [x] Current findings are classified and the checkout has no generated
  residue.

Verified checkpoint, 2026-08-04 (stable lifecycle grammar):

- External root: `/tmp/codeatlas-xdo-cache.hTn1Nk`; no build, cache, or report
  directory exists under the checkout.
- Focused testing and architecture suites, the 15-case CLI contract suite,
  formatter checks, and artifact lifecycle acceptance tests passed through
  commits `dfb271b` and `7cbb342`.
- `node tasks/check-self.js` passed all seven stable reports under the external
  Cargo target: 233 files, 2,241 scan symbols, 2,628 lexicon symbols, one exact
  inspect target with five nodes/five edges, six test contexts, four scripts,
  and zero duplicate scripts.
- Code check/usage produced 100 non-gating advisories: 69 test-only nodes, 23
  explicit dynamic boundaries, six unresolved private-symbol usages, and two
  unresolved internal edges. None is evidence that source can be safely
  deleted; unresolved edges remain analyzer regression targets.
- Lexicon evidence contains six private domain-local name collisions, four
  intentional shape families, 41 callable candidates, and zero public exports
  (expected for the binary crate). The earlier duplicate diagnostic renderer
  was consolidated in the testing grammar phase rather than moved to a generic
  utility owner.

LOC: +550-800 / -20-50

Verify: Cargo-backed self scan/check/usage/inspect/lexicon/test evidence runs
with external state, current findings are classified, and no checkout artifact
or unrelated Git change exists.

```text
~ AGENTS.md
+ docs/concepts/lexicon.md
+ codeatlas.json
~ Cargo.toml
~ proposals/codeatlas-execution-kernel-http-fuzz.md
~ tasks/check-self.js
~ package.json
~ tests/cli_contract.rs
```

## Phase 2: Immutable plans, receipts, and private artifacts

Status: [x] Complete

Execution checklist:

- [x] Confirm existing owners and pin the minimum Phase 2 model surface.
- [x] Add strict execution/fuzz configuration and shared CLI limit arguments.
- [x] Add canonical artifact identity, private storage, typed references, and
  prospective schema registration.
- [x] Add the shared target classifier, redaction, leases, resource evidence,
  and non-passing incomplete outcomes.
- [x] Add zero-call target/replay planning and the reviewed execution entry
  contract without performing target work before later enforcement phases.
- [x] Pass focused and full checks, mutation/error-path proofs, CodeAtlas
  dogfood, generated-state audit, and a clean phase commit.

Current checkpoint, 2026-08-04:

- The exact CodeAtlas plan vector is
  `plan_acd56385b3cb19b6498051f28c60e4f90b3c7236b0d2daea21543d16177fcf2c`;
  RFC 8785, safe-integer, immutable-collision, tamper, and private-permission
  tests pass.
- Target and replay planning, reviewed execution, and stale-evidence rejection
  are target-observed zero-call paths. Reviewed execution currently emits a
  blocked receipt because proxy and isolation enforcement are continuation
  phases, not review-time exceptions.
- HTTP configuration now separates semantic literal `environment`/header
  values from reference-only `secret_environment`/`value_env` inputs. Planning
  never resolves ambient secret values, literal changes invalidate the plan,
  and ordinary rotation behind an unchanged reference does not alter plan
  identity. Phase 5 resolves and scopes those references only inside the
  authorized executor, where argument/output redaction can be proven against
  the actual values.
- Omitted HTTP seeds materialize deterministically from strategy and exact
  evidence, so repeated planning is byte-identical and every saved workload is
  replayable. Plan v1 also reserves canonical expected-call and logical scratch
  requirements; receipt v1 already carries creation-tool, runtime backend,
  environment, capability, rootless/nested, peak-rate, and peak-concurrency
  fields needed by the enforcing continuation phases.
- Managed-command evidence covers the engine, every server preparation step,
  the server, and the request adapter. Its digest is exact over owner,
  executable, arguments, and workspace-relative working directory, remains
  stable when the checkout moves, and rejects a working directory outside the
  project root.
- Artifact reads and file digests are streaming and bounded, reject mutation
  during collection, and share one metadata-stability predicate. The store
  rejects symlinked kind directories; artifact-link validation has one owner
  and rejects conflicting digests for one ID.
- Focused kernel/config/CLI/schema/security tests and warning-denying Clippy
  pass with all generated state under `/tmp/codeatlas-xdo-cache.hTn1Nk`.
- `pnpm check` passed: 15 Node tests, 324 passing Rust unit tests with two
  intentional ignores, all non-live integration suites, architecture/schema
  drift checks, formatter, warning-denying Clippy, self-audit, and package
  assembly. The two live integration cases remain explicitly ignored for their
  documented Phase 5 and opt-in PostgreSQL reasons.
- The final full-gate dogfood artifacts cover 255 files, 2,480 scan symbols,
  2,913 lexicon symbols, seven test contexts, three scripts, and zero duplicate
  scripts. The new Phase 2 paths have 21 non-gating findings: six conservative
  conditional-compilation boundaries, 11 test-only helpers, three analyzer
  reachability gaps for associated constructors that production code calls,
  and one Phase 3 JSON redaction verifier retained behind the reasoned module
  boundary. Exact inspection resolved the managed-command, artifact-link, and
  call-count owners under an explicit 80-node bound and reported 1,043 omitted
  nodes and 5,676 omitted edges rather than hiding the cap.
- Git diff checks and the generated-state audit pass; build, cache, state,
  report, and inspection artifacts remain outside the checkout.
- The disconnected HTTP executor is retained only for Phase 5 migration behind
  five reasoned module-level dead-code annotations; the obsolete live script
  is not advertised. Phase 5 must remove the annotations, delete the private
  filesystem owner, and reconnect a kernel-backed smoke suite.

Actual LOC: +6,122 / -200, including 1,312 generated schema lines and the
security, artifact, CLI, integration, and dogfood acceptance surface.

Verify: Target and replay preview make zero calls; RFC 8785 and CodeAtlas
domain-separation vectors pin canonical plan IDs; canonical artifact IDs and
typed references are stable; changed evidence refuses execution; the shared
classifier cannot preauthorize remote/effectful targets; artifacts are private,
bounded, redacted, and external; leases release on every outcome; incomplete
work cannot pass; every new artifact passes prospective schema registration
with one namespaced version and no parallel API version.

```text
+ Cargo.lock
~ Cargo.toml
+ schemas/codeatlas-execution-plan-v1.schema.json
+ schemas/codeatlas-execution-receipt-v1.schema.json
+ schemas/codeatlas-http-fuzz-workload-v1.schema.json
+ schemas/codeatlas-reproducer-v1.schema.json
+ src/config/execution.rs
+ src/config/fuzz.rs
+ src/execution/mod.rs
+ src/execution/model.rs
+ src/execution/policy.rs
+ src/execution/target.rs
+ src/execution/artifact.rs
+ src/execution/artifact/identity.rs
+ src/execution/artifact/store.rs
+ src/execution/redaction.rs
+ src/execution/lease.rs
+ src/execution/resource.rs
+ src/execution/runner.rs
+ src/fuzz/mod.rs
+ src/fuzz/model.rs
+ src/fuzz/reproducer.rs
+ src/cli/execution.rs
+ src/commands/fuzz.rs
+ src/http/planning.rs
+ tests/fuzz_plan.rs
~ README.md
~ docs/concepts/lexicon.md
~ package.json
~ src/published_schemas.rs
~ src/main.rs
~ src/config/mod.rs
~ src/config/http.rs
~ src/cli/fuzz.rs
~ src/cli/mod.rs
~ src/commands/http.rs
~ src/commands/mod.rs
~ src/environment.rs
~ src/external_tool.rs
~ src/http/mod.rs
~ src/http/model.rs
~ src/http/provider.rs
~ src/http/runtime.rs
~ src/http/schemathesis/mod.rs
~ src/http/schemathesis/request_adapter.rs
~ src/http/schemathesis/tests.rs
~ src/http/schemathesis/toolchain.rs
~ src/http/target.rs
~ src/http/target/tests.rs
~ src/http/transport_schema.rs
~ tests/http_cli.rs
~ proposals/codeatlas-execution-kernel-http-fuzz.md
~ proposals/codeatlas-fuzz-performance.md
```

## Phase 3: Pre-call budgets and enforcing HTTP proxy

Status: [x] Complete

Execution checklist:

- [x] Pin the one call-category, permit-sequence, cleanup-reservation, and
  cancellation contract against the Phase 2 artifacts.
- [x] Add the bounded shared scheduler and atomic pre-call budget ledger.
- [x] Add an exact-destination HTTP/HTTPS enforcing proxy with bounded
  request/response capture and per-decoded-request permits.
- [x] Pin and target-prove the Schemathesis, readiness, authentication,
  stateful, reduction, retry, validation, and cleanup routing contract while
  keeping public execution blocked until verified isolation exists.
- [x] Prove call and cancellation enforcement from the target side; prove
  rate, concurrency, backpressure, and deterministic sequencing at their
  budget/scheduler owners; then pass full checks, dogfood, residue audit, and
  a clean phase commit.

Verified checkpoint, 2026-08-04:

- `CallBudget::from_plan` is the sole production constructor. It derives the
  cleanup allowance from canonical expected-call evidence, admits every call
  through a finite pre-call permit, strictly paces admission without a burst,
  bounds concurrency, records sequence order independently of completion, and
  never refunds failed, rejected, timed-out, or cancelled work.
- The scheduler creates one Tokio runtime per execution. Its asynchronous
  workers and separately permitted blocking work are both bounded by the
  lesser of planned concurrency and observed host parallelism; process limits
  remain a distinct sandbox concern.
- The loopback TLS-terminating proxy supports HTTP/1.1 and HTTP/2, verifies
  upstream TLS and exact origin, strips hop-by-hop and kernel control headers,
  rejects off-origin redirects, bounds request and response bodies, and
  obtains one permit per decoded logical request. Target-side tests observed
  two calls—not `max_calls + 1`—zero oversized-request or untrusted-TLS calls,
  and prompt cancellation of an active call.
- The request-hook protocol is hard-cut to
  `codeatlas.http-request-adapter/v3`. The managed hook overwrites adapter and
  authenticator attempts to reclassify calls; unsupported-method and
  authentication probes have exact categories, while unavailable
  stateful/reduction provenance remains honestly classified as a generated
  case. A sandbox request cannot claim the cleanup reserve.
- All 34 focused execution tests and warning-denying Clippy pass. `pnpm check`
  passes 15 Node tests, 346 Rust unit tests with two intentional ignores, all
  non-live integration suites, schema/spec drift checks, formatter, Clippy,
  self-audit, and package assembly. Live HTTP and PostgreSQL cases retain
  their documented Phase 5 and opt-in reasons.
- Refreshed dogfood covers 258 files, 2,552 scan symbols, 3,022 lexicon
  symbols, seven test contexts, three scripts, and zero duplicate scripts.
  The execution owner has zero callable candidates, name collisions, or shape
  aliases. Its 51 non-gating findings are 12 explicit dynamic boundaries, 28
  test-only symbols, and 11 known associated/async reachability gaps; none is
  deletion evidence.
- Exact inspection resolved `CallBudget`, `EnforcingProxy`, and
  `ExecutionScheduler` into a bounded 120-node/1,179-edge slice with graph
  digest
  `sha256:f78cb5613c8c1a13c0fc746a1e03da4c8f149c8db19d6751ebf516354c79a780`;
  877 nodes, 4,074 edges, and six boundaries were explicitly omitted by the
  cap.
- Cargo resolves one Tokio version and the ring-backed TLS stack without an
  `aws-lc-rs` backend. `budget.rs` and `proxy.rs` remain cohesive invariant
  owners: their production halves own one ledger and one forwarding boundary,
  while their colocated halves are target-observing conformance tests. A split
  would separate the safety invariant from its proof without reducing product
  surface.
- Git diff and generated-state audits pass. TLS material is ephemeral only;
  no key, certificate, build, cache, report, or temporary artifact exists in
  the checkout.

Actual LOC: +2,938 / -108

Verify: The target never observes call `max_calls + 1`; deterministic budget
tests prove that rate and concurrency bursts remain bounded; setup, health,
authentication, stateful, reduction, retry, validation, and kernel-owned
cleanup work have one call vocabulary; HTTPS is terminated and counted per
decoded request with exact-host/upstream verification; bounded schedulers apply
backpressure; multiplexed completion order cannot change receipt bytes; budget
exhaustion is `partial`.

```text
+ Cargo.lock
~ Cargo.toml
+ src/execution/budget.rs
+ src/execution/proxy.rs
+ src/execution/scheduler.rs
~ src/execution/artifact.rs
~ src/execution/mod.rs
~ src/execution/model.rs
~ src/execution/runner.rs
~ src/http/schemathesis/hooks.py
~ src/http/schemathesis/request_adapter.rs
~ src/http/schemathesis/tests.rs
~ src/http/target.rs
~ src/http/target/tests.rs
~ tests/fixtures/http/request_adapter.py
~ README.md
```

## Phase 4: Verified isolation backend and capability matrix

Status: [x] Complete

Execution checklist:

- [x] Probe exact rootful, rootless, nested-container, user-namespace, cgroup,
  mount, network, runtime-socket, and resource-control capabilities without
  changing host state.
- [x] Pin one capability model and fail-closed backend-selection contract;
  absent or incomplete enforcement must block before the first target call.
- [x] Implement the OCI sandbox owner with read-only checkout/runtime mounts,
  one external writable scratch root, exact environment/process/network
  policy, bounded captured output, and no child-visible control socket.
- [x] Add target-observed conformance fixtures for mount and symlink escape,
  network, process, environment, CPU, RSS, PID, descriptor, output,
  interruption, cleanup, rootless, and nested-runtime behavior.
- [x] Connect capability evidence, resource sampling, leases, and receipts at
  the shared runner boundary without enabling HTTP execution before Phase 5.
- [x] Pass focused and full checks, dogfood, generated-state audit, and a clean
  phase commit; record unsupported hosts as plan-only rather than weakening
  the backend contract.

Starting checkpoint, 2026-08-04:

- Phase 3 is committed as `ff1c2af`; the worktree was clean before this
  checklist transition.
- Build, cache, state, conformance, and container writable data remain under
  `/tmp/codeatlas-xdo-cache.hTn1Nk`, never under the checkout.
- The current host is a nested Docker container on Linux 6.12 with a Docker
  client but no Docker, Podman, or containerd control socket. It has no
  effective capabilities, its cgroup v2 mount is read-only and undelegated,
  and user/mount namespace creation fails with `Operation not permitted`.
  This host is therefore plan-only; it may prove fail-closed selection and a
  fake-runtime boundary but cannot satisfy the live OCI conformance gate.

Verified partial checkpoint, 2026-08-04 (fail-closed capability selection):

- Configuration accepts only an absolute optional runtime executable, an
  absolute local socket, and an optional exact
  `repository@sha256:<lowercase-digest>` probe image. The client clears its
  environment and uses a private client root, so ambient contexts,
  credentials, and remote daemon selection cannot enter the probe.
- Runtime CLI and server metadata produce a content fingerprint and
  rootless/nested evidence only. A fake healthy runtime and digest-pinned
  image prove that declarations still yield zero capabilities until the
  target-observed conformance protocol passes.
- Missing or incomplete capability evidence reaches the shared runner as a
  blocked zero-call receipt. Its external scratch lease is released and
  verified, leaving the owner empty; reviewed authorization does not alter the
  result.
- Private file/directory handling now has one owner in
  `src/execution/private_fs.rs`. The artifact store and dormant HTTP adapter
  consume it, and the duplicate `src/http/private_fs.rs` is deleted ahead of
  Phase 5 rather than retained as parallel safety code.
- Forty focused execution tests and the zero-call planning/integration test
  pass. Formatting and test-target compilation pass with external state. The
  real OCI command path and live isolation conformance remain unverified and
  therefore grant no capabilities on this host.
- Bounded self-dogfood passes over 261 files, 2,581 scan symbols, and 3,058
  lexicon symbols with zero gates and no duplicate scripts. Exact inspection
  resolves the four new ownership boundaries into 120 nodes and 1,027 edges
  with graph digest
  `sha256:bdd8e457eea9b7ada38a7481ce679ef2286df140892df5796d3a58a0dcb1088e`;
  omitted counts remain explicit. The execution paths have zero callable,
  name-collision, or shape-alias candidates. Their 12 advisories are nine
  conditional/async analysis boundaries and three conservative reachability
  gaps; compiled production calls and focused tests corroborate the latter,
  so none is deletion evidence.

Locally verified implementation checkpoint, 2026-08-05:

- `src/execution/sandbox/container` now separates immutable launch-command,
  runtime-client, and conformance owners. The client clears its environment,
  uses an exact canonical executable and local Unix socket, refuses pulls,
  verifies the configured repository digest, and never exposes the runtime
  socket or client configuration to the child.
- Container construction and daemon inspection agree on a read-only root and
  checkout, one external writable scratch hierarchy, an isolated `/tmp`, no
  network, private PID/IPC namespaces, no added capabilities, no-new-privileges,
  the built-in seccomp policy, exact environment/command identity, and finite
  memory, CPU-time, process, descriptor, output, and elapsed ceilings.
- A strict nonce-bound conformance report maps each observed mount, traversal,
  symlink, network, process, ambient-environment, socket, resource, rootless,
  and nested-runtime result to one discrete capability. Unknown fields,
  changed limits, stale nonces, excess usage, altered daemon controls, and any
  failed observation withhold capability evidence rather than lowering the
  requirement.
- The bounded command primitive now combines stdout/stderr under one ceiling,
  responds to the shared cancellation ledger, kills and reaps on timeout,
  output exhaustion, or interruption, and records output consumption. SIGINT
  produces a cancelled non-passing receipt before the lease-owned cleanup
  path runs.
- A container lease is registered before creation. Successful asynchronous
  removal is recorded once; failed or unverifiable primary removal retains the
  lease and runs its bounded fallback. The regression fixture fails the first
  removal deliberately and proves the fallback removes the target before the
  scratch lease releases.
- The deterministic fake-runtime boundary passes four integration cases plus
  one explicit live-backend ignore. Forty-four focused execution tests,
  warning-denying Clippy, and the full repository check pass. The full check
  covers 15 Node tests, 370 passing Rust unit tests with two intentional
  ignores, all non-live integrations, architecture/schema drift, formatting,
  self-audit, and package assembly.
- Refreshed self-dogfood covers 275 files, 2,778 scan symbols, 3,302 lexicon
  symbols, eight test contexts, three scripts, and zero gates or duplicate
  scripts. The execution findings are conservative async/conditional
  boundaries, test-only symbols, and known reachability gaps; the new owner
  has no name collision, shape alias, or callable-duplication candidate.
- Exact inspection resolves the conformance evaluator, cleanup fallback, and
  cancellation registry into a bounded 120-node/1,006-edge slice with graph
  digest
  `sha256:6f00734c8fc337ef06dee0850173e5c809fec1cef523f92db67eeccbe403d7d8`;
  1,464 nodes, 9,054 edges, five contexts, and 11 boundaries remain explicitly
  omitted by the requested cap.
- The current host still has no usable outer OCI socket or effective namespace
  authority. An administrator probe confirmed that sudo root remains bounded
  without `CAP_SYS_ADMIN`, `CAP_NET_ADMIN`, or `CAP_SYS_RESOURCE`; cgroup v2 is
  read-only and user, mount, and network namespace creation is denied. A
  VFS-backed nested Docker daemon can initialize its API, but its first image
  layer fails to register with `unshare: operation not permitted`, so it cannot
  launch a target and grants no isolation evidence. The live test remains
  ignored with an exact operator-input contract. Phase 9 must run that same
  path against a digest-pinned probe image on a capable rootful, rootless, or
  nested runtime before Phase 5 execution is enabled.

Hosted gate preparation checkpoint, 2026-08-05:

- The task-owned nested QEMU runner was shut down cleanly and its two exact
  external state roots were removed. It is not retained as a fallback or a
  second backend path.
- `tasks/check-isolation-live.js` now owns one bounded transaction around the
  existing build task, a read-only unprivileged loopback registry, exact OCI
  import/publication coordinates, the baseline integration case, the
  destructive CPU/RSS/output/cancellation matrix, receipt export, evidence
  digests, and verified residue cleanup. It accepts selectors only; no
  arbitrary command can enter the runner.
- Built OCI manifest, loaded image ID, and published manifest are distinct
  evidence coordinates. The hosted proof records whether Docker preserved the
  manifest digest but does not fail or fabricate equality when a daemon
  normalizes media types during load/push.
- One BuildKit solve emits the canonical OCI-layout archive plus an optional,
  bounded Docker-import projection for runtimes whose `image load` does not
  consume OCI layout. The projection is not a second build or canonical
  artifact: its digest and byte count are recorded, it is removed and verified
  immediately after import, and it is retained in failed-run evidence only
  when import itself did not complete.
- The destructive matrix invokes `ContainerLaunchSpec`, `RuntimeClient`, the
  shared scheduler and cancellation ledger, `run_container_case`, and the
  existing lease registry. It does not create a test-only container executor.
  Probe modes and readiness markers have one typed owner in
  `codeatlas-isolation-conformance`.
- `.github/workflows/live-oci-isolation.yml` provides a manual-only,
  default-branch-only `ubuntu-24.04` runner with read-only repository
  permission, a finite job timeout, immutable official-action pins, exact
  digest-pinned Rust-musl, BuildKit, and registry images, and no
  caller-controlled command or secret
  input. The Docker socket remains host-side and is never forwarded to a child.
- One GitHub Actions Cargo cache has an exact compatibility prefix over OS,
  architecture, the `rustc -Vv` digest, both lockfiles, and both manifests,
  with the source revision as its immutable generation. A restore prefix may
  select only the newest generation under that complete compatibility
  identity; there is no cross-toolchain or cross-dependency restore. A
  generation is saveable only after the compiled isolation-test executable
  proves that expensive Cargo work exists, preventing an early empty entry
  from blocking a later useful save for the same compatibility class.
  Uncompressed state is capped at 6 GB, and the primary/matched generation,
  exact hit, restored/final/saved bytes, usefulness, and save outcome are
  reported. Cancelled or incomplete cache setup cannot save. The probe image
  retains `--no-cache`, so Cargo reuse cannot substitute for rebuilding the
  exact committed isolation payload.
- Focused Node contracts, the container-owner tests, five non-live isolation
  integrations, the probe suite, and both warning-denying Clippy surfaces pass
  with generated state directed outside the checkout. The two live tests
  compile and remain ignored locally by design.
- The rootful runtime input and complete target-observed matrix are resolved by
  the hosted attempts below. Phase 4 grants only the seven capabilities in the
  accepted artifact; HTTP workload execution remains deliberately disconnected
  until Phase 5 migrates the adapter through the proven kernel.

First hosted attempt, 2026-08-06:

- GitHub run `31080343512` used exact commit `00690aa`, Docker Engine 28.0.4,
  Buildx 0.35.0, the planned local socket, and the three digest-pinned images.
  Workflow setup, runtime discovery, bounded cache handling, cleanup, and
  evidence upload passed.
- The pinned BuildKit solve produced a valid 306,176-byte OCI-layout archive
  with manifest digest
  `sha256:2362a3d0f88392e712416e48942b3480a9d57236d3b61f6f12b9f3569734c0ab`.
  Docker rejected that layout at `image load` before any conformance case ran.
  The uploaded artifact digest is
  `sha256:b60c3e8e0b956823588b42c51e7924d5d980ff138c06f62257b85d1a8913537e`;
  it proves the failure boundary and successful registry/builder cleanup but
  grants no isolation capability.
- The narrow correction keeps the OCI artifact canonical and adds one Docker
  import projection to the same uncached solve. Focused local tests and
  workflow lint passed; the second attempt below exercised that correction.

Second hosted attempt, 2026-08-06:

- GitHub run `31081198694` used exact commit `b542923`, Docker Engine 28.0.4,
  Buildx 0.35.0, the planned local socket, and the three digest-pinned images.
  The one-solve Docker projection imported successfully, the probe was tagged
  and published through the temporary loopback registry, and the exact
  published digest passed inspection. This resolves the Phase 9B rootful
  runtime input without granting an isolation capability.
- The real baseline container launched and emitted blocked receipt
  `receipt_c675b070b5ceddc71874646f4e37c72f36d80a0626acf6aba3252c001b98d86b`.
  Both the container and scratch leases released and verified. Its strict
  evaluator stopped before the destructive matrix at one combined
  CPU/RSS/process/descriptor usage check, so all seven capabilities remained
  withheld and Phase 9C remains open.
- Audit found that the combined diagnostic hid the exact failing sample and
  that its `memory.peak <= memory.max` branch contradicts cgroup v2: the hard
  limit may be exceeded temporarily while `memory.peak` records that high-water
  mark. The correction retains exact `memory.max` equality and the destructive
  RSS proof, treats the peak as receipt evidence, and gives CPU, process, and
  descriptor overages exact observed/allowed diagnostics.
- The run restored a 214-byte immutable v1 cache entry, then spent 3 minutes 50
  seconds compiling 3,095,767,592 bytes of useful bounded Cargo state. The
  exact hit prevented that state from being saved. The v2 generation contract
  above makes source revision immutable beneath one complete compatibility
  prefix and refuses to save until the compiled isolation test exists, so an
  early empty generation cannot poison later work.
- Uploaded artifact `8959641326` is 320,226 bytes with digest
  `sha256:1703c210b13b7805662a79d8592762f01ce780c06650a3ab9f1756e7af96e857`.
  The task-local copy is under
  `/tmp/codeatlas-gha-phase9.GAkQVR/artifacts/github-run-31081198694`; it
  contains the canonical OCI archive, receipt, and bounded logs. No capability
  was granted.
- The local correction gate passes four focused evaluator contracts, five
  non-live isolation integrations, 420 root unit tests plus all non-live
  integrations, warning-denying Clippy, formatting, 14 hosted-task contracts,
  and workflow lint. Self-dogfood covers 327 files, 3,441 symbols, 2,923
  callable contracts, 353 non-gating advisories, three semantic-sibling sets
  with zero review candidates, 16 test contexts, seven scripts, and no
  duplicate scripts. All complete logs, reports, Cargo state, and audit output
  remain beneath `/tmp/codeatlas-gha-phase9.GAkQVR`.

Third hosted attempt, 2026-08-06:

- GitHub run `31083063180` was dispatched once from exact commit `69ae2cd` at
  2026-08-06T07:59:37Z after the API confirmed no active or completed run for
  that revision. Setup, runtime resolution, one-solve import/publication, real
  baseline launch, and verified cleanup passed again.
- The exact diagnostic identified `pids.peak = 7` with `pids.max = 1`. This did
  not prove seven escaped children: the target's exact process limit and
  unplanned-child denial both passed. The Linux cgroup contract counts kernel
  tasks and permits organizational attachment above `pids.max`; only new
  `fork`/`clone` work is guaranteed to fail at the boundary. Treating a
  historical high-water sample as a second enforcement oracle was therefore
  invalid for process evidence too.
- The follow-up consolidation makes every CPU, RSS, PID, and descriptor usage
  sample receipt evidence only. Exact target-observed limits, target-side
  denial/exhaustion checks, and the destructive live matrix remain the sole
  capability oracle. This removes the duplicate interpretation rather than
  accumulating resource-specific exceptions.
- The baseline emitted blocked receipt
  `receipt_54ba8f9811d92f48bdb31f6da6fb62094be3086e00e2a472472f3e197825caa2`;
  all capabilities remained withheld. Uploaded artifact `8960411261` is
  320,247 bytes with digest
  `sha256:09b167731f4c9e8a6e210c35e4bf430bece1bddbbe5c101daa4dd07234e3d267`.
  Its task-local artifact and one retrieved job log live under
  `/tmp/codeatlas-gha-phase9.GAkQVR/artifacts/github-run-31083063180` and
  `/tmp/codeatlas-gha-phase9.GAkQVR/logs`.
- The v2 cache miss finished with a useful 3,095,277,545-byte uncompressed
  generation and saved it successfully as a 702,923,486-byte GitHub cache.
  Future revisions with the same OS, architecture, Rust, manifests, and
  lockfiles can restore that generation through the exact compatibility
  prefix. No capability was granted and no fourth run is authorized until the
  follow-up passes every local check.
- The follow-up local gate passes two focused evaluator contracts, five
  non-live isolation integrations, 418 root unit tests plus all non-live
  integrations, warning-denying Clippy, and formatting. Self-dogfood covers
  327 files, 3,437 symbols, 2,919 callable contracts, 353 non-gating
  advisories, three semantic-sibling sets with zero review candidates, 16 test
  contexts, seven scripts, and no duplicate scripts. The reduced implementation
  removes the four-field usage validator instead of adding per-resource
  exceptions.

Fourth hosted attempt and accepted live proof, 2026-08-06:

- GitHub run `31084275665` executed exact commit `6d1e4f0` once on the
  `ubuntu-24.04` hosted image. Docker Engine 28.0.4, Linux
  6.17.0-1020-azure, a cgroup-v2 digest, runtime version/info digests, and
  rootful/non-nested state are recorded in the accepted evidence; no rootless
  or nested capability is extrapolated.
- The baseline passed in 3.22 seconds and the destructive CPU, RSS, output, and
  cancellation matrix passed in 4.97 seconds. Target-side evidence granted
  exactly `read_only_checkout`, `read_only_runtime`, `scratch_filesystem`,
  `network_allowlist`, `process_allowlist`, `resource_limits`, and
  `cleanup_verification`. Container, registry, builder, import archive, image,
  tag, and scratch cleanup all verified with no owned residue.
- Receipt
  `receipt_9b381d56752e58ffc4ead8190ae8981ab71e24f30f5bfb596a22473d5594962a`
  records Docker environment digest
  `sha256:e9019e05081c59e30f4e2f6b7a494cd95fe22a5a1336fdc0a79734ec52f9f9c0`,
  29 ms CPU, 7,106,560 peak RSS bytes, six peak cgroup tasks, 32 peak open
  files, and verified leases. Its `blocked` outcome is the intended Phase 4
  result: isolation is proven, while HTTP workload execution remains
  disconnected until Phase 5.
- The 306,176-byte canonical OCI archive has digest
  `sha256:17d17ac135cfdbf89e0defbf614b9fa6c75e5c15710ae578dc1712b281e3972e`.
  BuildKit emitted manifest
  `sha256:ab9be7a7268a7179f2dc09752924ad8f690449c4683fe6a9daf54f3f9cdb9e59`;
  Docker honestly normalized publication to
  `sha256:b2491d5990bd4cd7e7e21b91011d75c8226f45038f6683f261f486205f8d364a`
  rather than fabricating digest equality.
- Artifact `8960813530` is 320,568 bytes with verified digest
  `sha256:c7a5abfd1b0935df16927bca3f18560a42224f9535281622c46f4d95cf2bd53f`.
  The receipt file digest is
  `sha256:52abdbbc0a186028ab1f84bce701fdf3a475a60854d047724ee2c69e1b5e2f3a`.
  One local copy and one job log live under
  `/tmp/codeatlas-gha-phase9.GAkQVR/artifacts/github-run-31084275665` and
  `/tmp/codeatlas-gha-phase9.GAkQVR/logs`.
- The compatible cache restored 3,095,277,545 uncompressed bytes from run 3 in
  about nine seconds. The completed matrix produced a useful 5,009,304,839-byte
  generation under the 6 GB ceiling and saved it successfully as a
  1,202,284,016-byte cache. OCI image construction remained uncached.

#### Phase 4A: CodeAtlas-owned isolation probe

Status: [x] Complete

The live gate must not depend on an opaque operator-supplied payload. CodeAtlas
owns one focused internal crate containing the deterministic Linux probe binary
and strict report model, plus a
digest-producing OCI image recipe. The host evaluator, real probe, and fake
runtime fixture are validated by that one report owner; a second schema or
independent evaluator is not acceptable.

The probe has a deliberately private surface. Its exact modes cover ordinary
confinement plus destructive CPU, memory, process, descriptor, output, and
cancellation cases needed by the live matrix. Building it writes the Cargo
target, OCI context/archive, and runtime cache only beneath caller-selected
external roots. The recipe requires digest-pinned build/runtime images and
prints the produced OCI manifest digest; it never publishes or contacts a
registry implicitly.

Filesystem conformance never attacks the analyzed checkout. The kernel creates
a disposable sentinel workspace under its external execution root, mounts that
fixture through the same read-only mount contract, and lets the probe attempt
absolute, traversal, and symlink writes there. Only after target-side evidence
and host-side inspection agree may the backend grant `read_only_checkout` for a
later workload. Probe source, a successful build, runtime metadata, and the
fake-runtime fixture grant no capability by themselves.

Execution checklist:

- [x] Extract one strict shared conformance-report owner and make the host
  evaluator, probe binary, and fake-runtime fixture validate against it.
- [x] Implement bounded deterministic probe modes for filesystem, environment,
  mount, network, process, resource, output, and cancellation observations.
- [x] Replace real-checkout write attacks with an external disposable sentinel
  workspace while preserving the exact inspected mount contract.
- [x] Add a pinned, network-explicit OCI recipe and external-only build task
  that emits the image manifest digest without publishing it.
- [x] Prove strict inputs, deterministic report bytes, truthful negative
  results on an unenforced host, and zero checkout-local generated state.
- [x] Run focused checks, bounded self-dogfood, one-owner searches, and commit
  this locally verifiable slice before requesting a capable runtime.

Verified checkpoint, 2026-08-05:

- `codeatlas-isolation-conformance` is the sole strict report and wire-constant
  owner. The host evaluator imports that model, the private Linux probe emits
  it, and the deterministic fake runtime validates the same nonce, mount, and
  sentinel evidence; the replaced evaluator-local report types are deleted.
- Focused probe modules own filesystem/mount, environment, network/process,
  resource, verification, and destructive workload concerns separately. Exact
  modes cover verification plus CPU, RSS, output, cancellation, and attempted
  unplanned-child execution without creating a second executor.
- The kernel creates a private disposable sentinel workspace beneath external
  execution state and mounts that read-only for absolute, traversal, and
  symlink attacks. Integration evidence proves the analyzed fixture checkout
  is never the attack mount.
- The OCI recipe requires a locally present digest-pinned musl build image and
  produces a static scratch image. The build task requires a clean commit,
  exact local runtime/socket, explicit network mode, external Cargo/output/log
  and runtime-data roots, finite elapsed/output capture, private logs, and a
  manifest digest; it never pulls, pushes, or overwrites implicitly. The actual
  OCI archive and digest
  remain Phase 9B inputs because this host has no capable runner.
- `tasks/storage.js` now owns symlink-aware external-path validation and private
  task-file writes. The duplicate self-audit writer was removed, and focused
  tests prove direct and symlinked checkout paths are rejected.
- CodeAtlas self-analysis exposed a false cross-crate graph caused by treating
  the standalone probe as the root crate. `codeatlas.json` now models the root
  and probe as two explicit projects; the strict config test pins that boundary.
  Repeated dogfood covers 324 files, 3,382 symbols, 2,872 callables, 16 test
  contexts, six scripts, 344 non-gating findings, and zero gates, duplicate
  scripts, or semantic-sibling review candidates.
- The broad repository gate passes 18 Node tests, 415 root unit tests with two
  intentional ignores, all non-live integration suites, four probe tests,
  both warning-denying Clippy runs, schema/spec drift, formatting, self-audit,
  and a 413-file package assembly. One-owner and generated-state searches are
  clean; all task state remains beneath `/tmp/codeatlas-xdo-cache.hTn1Nk`.

Actual LOC: +1,521 / -97.

```text
+ crates/isolation-conformance/Cargo.toml
+ crates/isolation-conformance/Cargo.lock
+ crates/isolation-conformance/src/lib.rs
+ crates/isolation-conformance/src/main.rs
+ crates/isolation-conformance/src/boundary.rs
+ crates/isolation-conformance/src/environment.rs
+ crates/isolation-conformance/src/filesystem.rs
+ crates/isolation-conformance/src/resource.rs
+ crates/isolation-conformance/src/verify.rs
+ crates/isolation-conformance/src/workload.rs
+ crates/isolation-conformance/tests/cli.rs
+ containers/isolation-conformance/Containerfile
+ tasks/build-isolation-probe.js
+ tests/isolation-probe-build.test.js
~ Cargo.toml
~ Cargo.lock
~ codeatlas.json
~ package.json
~ src/config/mod.rs
~ src/execution/sandbox/container.rs
~ src/execution/sandbox/container/command.rs
~ src/execution/sandbox/container/conformance.rs
~ tasks/check-package.js
~ tasks/check-self.js
~ tasks/storage.js
~ tests/execution_isolation.rs
~ tests/fixtures/execution_isolation/fake_runtime.py
~ tests/storage.test.js
~ README.md
~ docs/concepts/lexicon.md
```

This local implementation slice is +1,521 / -97 source, configuration,
documentation, task, and test lines before this checkpoint text. Together with
the earlier fail-closed Phase 4 slice, the measured Phase 4 implementation is
+2,696 / -268; the extra surface is the
strict daemon/target conformance and failure-path suite, not a second executor.

Original estimate: +1,200-1,900 / -50-150. The measured difference is
explained above and is accepted only for the conformance and failure-path
evidence required by this hard gate.

Verify: The backend conformance suite proves mount, scratch, symlink, network,
process, environment, CPU, RSS, PID, descriptor, output, interruption, and
cleanup guarantees across advertised rootful/rootless/nested states; the
control socket never enters the child; incomplete/no backend blocks before the
first target call; plan-only behavior remains portable.

```text
+ src/execution/isolation.rs
+ src/execution/private_fs.rs
+ src/execution/sandbox/mod.rs
+ src/execution/sandbox/container.rs
+ .github/workflows/live-oci-isolation.yml
+ tasks/check-isolation-live.js
+ tasks/container-runtime.js
+ tests/execution_isolation.rs
+ tests/isolation-live.test.js
+ tests/fixtures/execution_isolation/
~ Cargo.toml
~ Cargo.lock
~ src/execution/model.rs
~ src/execution/artifact/store.rs
~ src/execution/runner.rs
~ src/config/execution.rs
~ src/environment.rs
~ src/external_tool.rs
~ src/http/mod.rs
~ src/http/schemathesis/report/evidence.rs
~ src/http/schemathesis/request_adapter.rs
~ src/http/transport_schema.rs
~ tests/fuzz_plan.rs
~ README.md
~ proposals/codeatlas-fuzz-performance.md
- src/http/private_fs.rs
~ package.json
```

This phase is a hard continuation gate. No later proposal may assume code
execution safety until one backend passes the suite on its advertised hosts.

## Phase 5: Complete HTTP migration

Status: [~] In progress

Starting checkpoint, 2026-08-06:

- Phase 4 is closed by accepted hosted run `31084275665`; exact commit
  `6d1e4f0` grants the seven rootful OCI capabilities while keeping the HTTP
  workload deliberately disconnected.
- The Phase 5 implementation starts from clean commit `1500b64`. Generated
  state and complete command logs remain under
  `/tmp/codeatlas-gha-phase9.GAkQVR`; previously passed Phase 4 checks and the
  paid hosted matrix will not be repeated unless this phase changes their
  owning contract.
- The migration must replace the disconnected runner path and host-direct
  `OwnedHttpServer`/Schemathesis process path with one kernel-owned workload
  lifecycle. It may not treat successful conformance as permission to launch
  an unisolated host process or create a second proxy, budget, lease, or
  artifact owner.

Phase 5 closes one previously implicit transport decision before enabling a
call. HTTP configuration owns one optional exact digest-pinned
`http.fuzz.image`. Planning remains zero-call when it is absent, but execution
blocks before isolation setup; no host-process fallback exists. When present,
the plan carries the image as shared managed-image evidence. The image contains
the exact Schemathesis runtime and any managed-target runtime the checked-in
commands need. A managed target that needs extra dependencies derives its image
from the standard HTTP workload recipe and pins the resulting manifest digest.

`--schemathesis` selects an absolute executable path inside that image. It does
not authorize or fingerprint a host executable. Runtime version evidence must
match the planned engine identity before the first target call. This is a
pre-v1 hard cut: the retired host toolchain and direct process route are
deleted, not retained behind an alias.

The workload container retains `--network none`. A kernel-owned private Unix
socket under the leased scratch root carries raw TLS bytes between an
in-container loopback relay and the existing TLS-terminating enforcing proxy.
For a managed target, the same container starts the exact prepare/server
commands and exposes the server to that proxy through a second private Unix
socket. The container cannot reach a Docker network, host gateway, target, or
internet destination directly; the runtime control socket never enters it.
This transport is a new adapter for the existing proxy and scheduler, not a
second call counter or executor.

`http.fuzz.targets[].preauthorized` is subtractive authorization evidence, not
a bypass. Only a local managed server whose effects are contained in the same
verified disposable sandbox may become `preauthorized_isolated`; remote,
production, unknown, or uncontained effects still require review or block.
Ordinary contained mutation therefore remains target-classification evidence,
not a second annotation or safety directive.

`report_dir` is retired. Scratch reports are sanitized and bounded, then the
typed HTTP report is persisted through the shared content-addressed artifact
store and linked from the receipt. Nothing writes into the analyzed checkout,
and no HTTP-private report directory becomes a second artifact owner.

Because managed-image evidence is a new canonical plan-identity member, Phase
5 bumps the execution plan and domain separator to v2 and deletes the
prospective v1 schema. Rewriting the v1 bytes or retaining a fallback reader
would violate the artifact identity contract and create pre-release legacy
cruft.

Phase 5 also hard-cuts HTTP contract discovery to bounded file-backed OpenAPI
evidence. The retired command, URL, target, and object-wrapped file providers
could start host processes or make target calls from `scan http` without an
execution plan. They cannot coexist honestly with zero-call planning and the
one-kernel rule. Consumers materialize generated or remote OpenAPI evidence to
a file before CodeAtlas reads it; `openapi` is therefore one path value, and
static evidence commands never execute a configured provider. The obsolete
target `openapi_path` setting is removed because the workload mounts the exact
file whose digest was planned. This is a pre-v1 hard cut with a README upgrade
note, not a compatibility reader or a second plan ceremony for static scans.

Execution checklist:

- [x] Add strict workload-image and preauthorization configuration, shared
  managed-image plan evidence, schema drift coverage, and zero-call planning
  tests; retire host-path Schemathesis semantics and `report_dir`.
- [x] Add one network-none workload launch contract plus strict private
  in-container harness protocol; reuse the verified container client,
  scheduler, resource limits, redactor, and lease registry.
- [x] Extend the enforcing proxy with one Unix-listener transport and add the
  managed-server reverse bridge without duplicating request policy or call
  accounting.
- [x] Split Schemathesis into deterministic scratch preparation and result
  collection, run its engine and managed commands only through the workload
  container, and delete the direct host runtime/toolchain path.
- [x] Persist one typed execution report, link it to the exact plan and
  receipt, and prove bounded secret-free output plus non-passing incomplete
  cleanup and budget outcomes.
- [ ] Connect reviewed and eligible single-shot CLI paths to the same runner;
  prove stale evidence, remote/production policy, target-side call ceilings,
  cancellation, and cleanup behavior with fake-runtime and live fixtures.
- [ ] Run focused checks, the full required suite, CodeAtlas self-dogfood,
  one-owner searches, and the checkout-state audit; synchronize the tracker
  and commit Phase 5 before beginning Phase 6.

Contract checkpoint, 2026-08-06:

- Execution plans hard-cut to `codeatlas.execution-plan/v2` with domain
  separator `atlas.codeatlas.dev/execution-plan/v2`; the exact empty-image
  vector is
  `plan_bcc4125edebc92b104807e3f3cd823c019f544dec28c8d66671106bc188f9856`.
  The prospective v1 schema is deleted rather than accepted with changed
  identity bytes.
- HTTP workloads hard-cut to `codeatlas.http-fuzz-workload/v3` and carry one
  normalized absolute executable path inside the exact digest-pinned workload
  image. `--schemathesis` is no longer interpreted or fingerprinted as a host
  path. Planning remains zero-call without an image, while execution will
  block at the shared workload boundary added next.
- `http.fuzz.image` and target-local `preauthorized` are strict configuration;
  `report_dir` is removed. Managed-image evidence has one kernel model and
  exact owner/reference/manifest validation. Target effects distinguish
  absent, sandbox-contained, uncontained, and unknown evidence; ordinary
  contained mutation does not grow a second policy directive.
- Schema generation/drift validation, 421 binary unit tests, and the zero-call
  target/replay integration test pass from the external Cargo root. The old
  disconnected host runtime remains only as deletion input for the fourth
  checklist item; it is not reachable from the plan contract.
- Next exact action: add the one kernel-owned network-none workload launch and
  strict private harness protocol by extending the verified container client,
  scheduler, redactor, and lease owners—without adding an HTTP-private runner.

Locally verified migration checkpoint, 2026-08-06:

- One strict `codeatlas.container-workload/v1` protocol now carries the exact
  preparation, delegated adapter, managed service, and fuzz-engine commands
  through the verified network-none OCI owner. The harness validates one
  environment and command owner set, waits within the planned deadline until
  the managed service accepts a loopback TCP connection, and only then starts
  the workload. Delegated adapter commands remain plan-accounted while the
  exact Schemathesis hook launches them; no second path mapper or command
  evidence owner remains.
- The existing enforcing proxy now accepts one private Unix-socket client
  transport and one kernel-owned managed-server reverse bridge. TLS
  termination, exact destination policy, permits, rate/concurrency limits,
  response ceilings, redaction, cancellation, and call evidence remain in the
  pre-existing owners. The container receives neither network access nor the
  runtime control socket.
- Schemathesis is split into a deterministic scratch adapter and typed report
  collector. The host-direct runtime, provider, environment, JUnit, and
  tool-provisioning execution paths are deleted. Static HTTP discovery accepts
  only bounded file-backed OpenAPI evidence, and the new
  `codeatlas.http-fuzz-report/v1` artifact is persisted through the shared
  store and linked to the exact plan and receipt.
- The deterministic fake-runtime boundary proves the same executor for
  reviewed and preauthorized-isolated runs, exact command ownership including
  the request adapter, target-observed calls, finite budgets, report linkage,
  cancellation, and verified cleanup. Twelve non-live execution-isolation
  cases pass; the one real OCI case remains ignored locally because this host
  has no runtime socket.
- One generic container-image build owner now performs a single clean-HEAD,
  digest-pinned BuildKit transaction for the probe and standard HTTP workload
  images. Thin domain recipes contribute only their contexts, pinned base
  images, arguments, and byte ceilings. The existing manual hosted workflow
  builds, imports, publishes, exercises, records, and cleans both images; it
  does not create a second paid workflow or builder.
- The full local repository gate passed in 184 seconds: 30 Node tests; 426
  root unit tests with three intentional live/interop ignores; every non-live
  integration; four probe tests; both warning-denying Clippy surfaces; schema
  and architecture drift; formatting; self-audit; and a 416-file package.
  Self-dogfood covers 331 files, 3,513 scan symbols, 2,967 callables, 4,199
  lexicon symbols, three sibling comparison sets with zero review candidates,
  16 test contexts, seven scripts, zero gates, and zero duplicate scripts.
- Repository searches find no HTTP direct process launch, private executor,
  retired report schema, public `max_examples`, compatibility provider, or
  duplicate image-builder implementation. Task-generated package-manager
  residue was moved back under the external task root. A pre-existing ignored
  4.9 GB `target/` directory has uncertain ownership and is deliberately
  preserved rather than deleted.
- Next exact action: commit this locally green candidate, push that exact clean
  revision, confirm there is no equivalent active or completed workflow run,
  and dispatch the existing live OCI gate once. Phase 5 cannot close until its
  target-observed managed HTTP standard/stateful evidence and cleanup artifact
  pass on that capable runner.

Remote continuation checkpoint, 2026-08-06:

- The locally green Phase 5 implementation is commit `a7d4fa4`; checkpoint-only
  documentation produces exact dispatched revision `349214c`. Its push-bound
  range contains no private key, token, credential file, generated binary,
  large blob, or unexpected mode change.
- The reattached `goobits-build-dispatcher` key was moved immediately from the
  checkout to a mode-0600 external task directory. App ID `4343570` minted one
  short-lived installation token with the expected contents/actions/workflows
  permissions; no secret entered Git, command output, or a child workload.
- The authenticated fetch proved a clean seven-commit fast-forward from
  `6d1e4f0` to `349214c`. Duplicate detection found zero active or completed run
  for that revision, the existing workflow dispatch returned HTTP 204, and
  GitHub run `31128013309` entered `in_progress` at 2026-08-06T21:00:46Z.
- Next exact action: query run `31128013309` without redispatching. On success,
  retrieve and validate its bounded evidence artifact; on failure, retrieve
  only the failed job/step and short log tail before changing code.

First Phase 5 hosted attempt, 2026-08-06:

- GitHub run `31128013309` executed exact revision `349214c` on Docker 28.0.4
  and restored the useful 5,009,304,839-byte compatible Cargo cache. It failed
  in the live-matrix step before either image build, sandbox launch, or target
  call with the exact diagnostic `Runtime log label is invalid`.
- The 4,762-byte failure artifact is `8974870774` with digest
  `sha256:d8e5de33ec301a6f25792654bccddab5f470413ce3a15cd80fd17c22a13864db`.
  Builder, registry, and workload work had not begun, so the run consumed no
  target budget and created no target residue.
- The generic builder derived runtime log labels from two human-readable image
  labels containing spaces. The correction reuses the one existing
  `requireRuntimeLogLabel` owner, makes both domain recipe labels canonical
  lowercase kebab values, and adds a pure specification regression that rejects
  the invalid form locally. No second sanitizer or compatibility spelling is
  retained.
- The focused seven-case image-build test and complete 31-case Node suite pass.
  Next exact action: commit and push the narrow correction, confirm no run for
  that new revision, and dispatch the existing workflow once.

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

Projected total LOC after the measured Phase 3 result: +11,760-13,310 /
-878-1,408.

## Layman's wins

- HTTP fuzzing cannot silently spam a service or write into the source checkout.
- Every run has a visible maximum cost and a receipt proving what happened.
- Safe local targets remain convenient; risky targets require explicit review.
- Later code, database, and performance work reuse one proven safety system.
