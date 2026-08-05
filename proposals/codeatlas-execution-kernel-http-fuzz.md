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

The first execution artifact locks the byte contract for the namespace. An
`ExecutionPlan` uses the namespaced schema string
`codeatlas.execution-plan/v1`. Its identity is derived as follows:

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
   atlas.codeatlas.dev/execution-plan/v1\n<RFC-8785-plan-bytes>
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

Status: [~] In progress

Execution checklist:

- [x] Probe exact rootful, rootless, nested-container, user-namespace, cgroup,
  mount, network, runtime-socket, and resource-control capabilities without
  changing host state.
- [x] Pin one capability model and fail-closed backend-selection contract;
  absent or incomplete enforcement must block before the first target call.
- [ ] Implement the OCI sandbox owner with read-only checkout/runtime mounts,
  one external writable scratch root, exact environment/process/network
  policy, bounded captured output, and no child-visible control socket.
- [ ] Add target-observed conformance fixtures for mount and symlink escape,
  network, process, environment, CPU, RSS, PID, descriptor, output,
  interruption, cleanup, rootless, and nested-runtime behavior.
- [ ] Connect capability evidence, resource sampling, leases, and receipts at
  the shared runner boundary without enabling HTTP execution before Phase 5.
- [ ] Pass focused and full checks, dogfood, generated-state audit, and a clean
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

LOC: +1,200-1,900 / -50-150

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
+ tests/execution_isolation.rs
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

Projected total LOC after the measured Phase 3 result: +11,760-13,310 /
-878-1,408.

## Layman's wins

- HTTP fuzzing cannot silently spam a service or write into the source checkout.
- Every run has a visible maximum cost and a receipt proving what happened.
- Safe local targets remain convenient; risky targets require explicit review.
- Later code, database, and performance work reuse one proven safety system.
