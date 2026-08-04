# Performance evidence, curves, and hotspots

Status: Accepted follow-on; base implementation waits for the execution kernel

Decision scope: Planned performance observations, fixed workload curves,
budgets, baselines, diffs, and profiler attribution

Depends on:

- [`codeatlas-evidence-lifecycle-cli.md`](codeatlas-evidence-lifecycle-cli.md)
- [`codeatlas-execution-kernel-http-fuzz.md`](codeatlas-execution-kernel-http-fuzz.md)

## Decision

CodeAtlas will add performance as an evidence subject in the existing lifecycle:

```text
scan performance       gather a planned current observation
check performance      apply configured performance gates
baseline performance   save a reviewed canonical observation
diff performance       compare an observation with a baseline
```

Performance does not introduce a top-level `observe` verb. An **observation** is
subject-qualified evidence captured from a particular state or run, not a
universal schema. A `PerformanceObservation` is this domain's immutable
artifact and contains many **measurements**, where a measurement is one numeric
sample for one workload/size/metric coordinate.

Fuzzing and performance share execution safety but remain different products:

- Fuzzing searches for correctness, robustness, and resource failures.
- Performance runs fixed workloads and size curves to measure cost.
- Cost-guided input search is a separately authorized follow-on; it does not
  turn timing noise into a correctness failure or hold up this base product.

## Options considered

| Option | Fit | Tradeoff | Verdict |
|---|---|---|---|
| Add performance fields to fuzz reports | Minimal new CLI | Conflates adversarial search with controlled measurement | Reject |
| Treat static complexity as performance | Fully deterministic and cheap | Produces candidates, not observed runtime cost | Reject as proof; retain as prioritization |
| Build a benchmark framework independent of execution | Familiar structure | Duplicates plans, sandboxing, budgets, artifacts, and environment identity | Reject |
| Performance workload model over the execution kernel | Shared safety with distinct measurement semantics | Requires honest noise/capability reporting | Adopt |

## Existing-first check

Reuse:

- `src/source_index/metrics.rs` field conventions and internal analysis timing,
  without making it the user-facing performance owner.
- Existing source graph identity, complexity, reachability, witnesses, and
  context slices for candidate ranking and frame attribution.
- Existing baseline/diff CLI families and artifact comparison conventions.
- The execution kernel for preview, authorization, isolation, budgets,
  cancellation, environment/tool fingerprints, artifacts, receipts, target
  classification, redaction, leases, and shared process-resource sampling.
- Existing CodeAtlas fixtures and self-check tasks as representative controlled
  workloads.

No current owner defines user-target workload sizes, samples, curves, noise,
regression budgets, or profiler attribution, so `src/performance` is a new
cohesive owner.

## Evidence vocabulary

- **Optimization candidate:** deterministic static evidence suggests that
  measurement may be valuable.
- **Workload:** exact target, dataset/factory, arguments, environment, and size
  axis executed by a plan.
- **Measurement:** one numeric sample at one workload/size/metric coordinate.
- **Performance observation:** immutable measurements and environment evidence
  produced by one exact execution plan.
- **Curve:** ordered aggregate measurements over a declared input-size axis.
- **Hotspot:** runtime profiler or attributable cost evidence shows material
  time/resource concentration.
- **Regression:** a compared metric or curve exceeds its explicit budget and
  noise tolerance.
- **Cost reproducer:** a minimized input that preserves a declared cost
  threshold under recorded evidence.

Static candidates, hotspots, regressions, and correctness failures are separate
report types and labels.

## Workload contract

A stable workload declares:

- Exact command, callable, HTTP operation, or PostgreSQL query target.
- Source/config/tool/engine/dataset digests.
- One meaningful size dimension and exact generation/fixture method.
- Explicit input-size ladder.
- Cold or warm cache state and preparation.
- Warmups and measured repetitions.
- Required isolation and external destinations.
- Maximum calls, rate, concurrency, time, CPU, RSS, process, output, result, and
  artifact bytes.
- Metrics required versus optional.
- Regression budgets and noise floor.

The initial portable workload is an isolated command with fixed input fixtures.
Callable, HTTP, and PostgreSQL workload factories are added only when the owning
domain can prove their size coordinate, effects, and cleanup.

The default deterministic size ladder may suggest zero, one, values around
powers of two, declared limits, and representative larger points, but the plan
materializes exact values. A hidden adaptive size sequence is not allowed in a
performance observation.

## CLI and no-hidden-execution contract

```bash
# Preview: persists a plan and makes zero workload calls.
codeatlas scan performance --target scan-self

# Reviewed execution: produces an immutable observation.
codeatlas scan performance --plan plan_ABC --execute

# Eligible preauthorized local/disposable execution uses the same plan path.
codeatlas scan performance --target scan-self --execute

# These consume artifacts and make zero workload calls.
codeatlas check performance --observation observation_ABC
codeatlas baseline performance --observation observation_ABC
codeatlas diff performance \
  --against baseline_ABC \
  --observation observation_DEF
```

Single-shot execution follows the kernel's preauthorized-isolated contract.
Remote or effectful workloads require reviewed exact authorization and still
must satisfy every required capability. Incompletely isolated workloads remain
blocked; a reviewed plan cannot turn missing isolation into permission.

Baseline never runs a benchmark, and diff never silently samples the current
checkout. This makes automation cost visible and prevents comparisons from
using two unreviewed environments.

Observation and baseline arguments use the kernel's typed `ArtifactRef`; the
performance domain defines payload schemas but not an ID namespace, file
resolver, or content store.

## Sampling and curve evidence

Each observation records:

- Operating system, architecture, CPU identity, logical CPU count, available
  memory, and relevant environment digest.
- CodeAtlas, target, runtime, compiler/interpreter, profiler, and external tool
  versions.
- Source, config, dataset, plan, and isolation digests.
- Cold/warm state and cache preparation evidence.
- Warmup/sample count and deterministic order.
- Wall time, CPU time, throughput, peak RSS, output/result bytes, and calls.
- Allocation counts/bytes, I/O, query rows, or other metrics only when a
  capability reports them.
- Individual samples plus median and robust dispersion.
- Noise floor, threshold, and comparison decision.

Elapsed/CPU/RSS/process collection consumes `src/execution/resource.rs`.
Performance owns workload semantics, samples, statistics, curves, and gates;
it does not fork the operating-system sampler or reuse internal source-index
telemetry as a public report.

Timing and memory samples are not byte-deterministic. The plan and workload are
deterministic; the report is honest about measurement variance. A single
unfingerprinted timing cannot gate.

Curve analysis can report:

- Absolute and normalized throughput/cost.
- Median and dispersion per size.
- Local and overall slope estimates.
- Knees or discontinuities supported by enough points.
- Declared asymptotic/ratio budget where applicable.
- Missing/noisy/incomparable evidence.

The initial gate should prefer explicit absolute or relative budgets on
representative points. Complexity inference remains evidence, not a proof of
algorithmic complexity.

## Static prioritization

CodeAtlas may rank where measurement is valuable using deterministic existing
or extended evidence:

- Fan-in/fan-out and reachability.
- Public API and test witnesses.
- Complexity, nesting, and source size.
- Repeated scans, parses, allocations, clones, I/O, queries, and broad loops.
- Change concentration.
- Known blocking operations on broad paths.

The ranking includes reason and source evidence. It is called an optimization
candidate and never a hotspot.

## Profiler attribution

Profiler integration is capability-based and optional. The portable base
product still records time, CPU, RSS, throughput, calls, and byte metrics.

When an accepted profiler exists:

- The plan records its exact tool/version and required privileges.
- Capture is bounded by time and artifact size.
- Raw paths, addresses, process IDs, and nondeterministic frames are normalized.
- Frames map to stable SourceGraph node IDs where exact source identity exists.
- Attribution records mapped/unmapped sample counts and confidence.
- A location becomes a hotspot only when material measured/profiler evidence
  supports it.

Missing profiler support is explicit capability evidence, not a failed runtime
or fabricated attribution.

Profiler integration has its own continuation gate after fixed measurements are
accepted. No backend is authorized merely by accepting base performance. A
candidate backend must name its privileges, container/rootless support,
normalization, sampling overhead, artifact ceiling, and conformance evidence
before Phase 4 begins.

## Follow-on boundary

[`codeatlas-cost-guided-search.md`](codeatlas-cost-guided-search.md) separately
proposes adaptive input search after this performance model and the applicable
domain fuzz adapters are accepted. This proposal exports typed workload,
measurement, observation, curve, and cost-metric contracts; it does not import
fuzz generators or authorize adaptive search.

## CodeAtlas dogfooding

Initial self workloads use fixed fixtures and size ladders for:

- `scan code` API and source scopes.
- `check code`.
- `lexicon code`.
- `inspect code` exact targets.
- `scan tests` and `check tests`.
- Selected HTTP and PostgreSQL fixtures after their live contracts are accepted.

The checkout is read-only. Source indexes, compilers, package managers, Python
bytecode, Node caches, profiler output, observations, baselines, and temporary
datasets resolve outside it.

Every hot-path optimization must preserve canonical static artifacts, plans,
decisions, errors, reports, and receipts. A before/after observation names its
dataset, item/byte scale, cold/warm state, environment, and budget.

## Acceptance gates

- Preview makes zero workload calls and exact execution rejects changed
  evidence.
- `check`, `baseline`, and `diff performance` make zero workload calls.
- Workloads, size ladders, warmups, samples, and cache preparation are explicit.
- Runtime calls and resources cannot exceed kernel ceilings.
- Observations record environment, individual samples, aggregate statistics,
  and noise.
- Incomparable/noisy evidence cannot be a passing regression decision.
- Static candidates are never rendered as hotspots.
- Hotspot attribution includes mapped/unmapped evidence and confidence.
- Missing optional profiler metrics remain visible capabilities.
- Base performance acceptance does not preauthorize a profiler backend; Phase
  4 starts only after its explicit continuation gate.
- Self-dogfood produces no checkout-generated state.

## Phase 1: Performance artifacts and lifecycle CLI

Status: [ ] Not started

LOC: +600-900 / -100-200

Verify: Preview and exact execution artifact contracts, observation identity,
zero-call check/baseline/diff, config validation, changed-evidence refusal, and
CLI grammar tests pass.

```text
+ src/config/performance.rs
+ src/performance/mod.rs
+ src/performance/model.rs
+ src/commands/performance.rs
+ tests/performance_cli.rs
~ src/main.rs
~ src/config/mod.rs
~ src/cli/mod.rs
~ src/cli/scan.rs
~ src/cli/check.rs
~ src/cli/baseline.rs
~ src/cli/diff.rs
~ src/commands/mod.rs
~ src/execution/artifact.rs
~ src/execution/model.rs
~ codeatlas.json
```

## Phase 2: Fixed workloads, sampling, and curves

Status: [ ] Not started

LOC: +700-1,100 / -80-160

Verify: Fixed command workloads and size ladders, cold/warm preparation,
warmups/samples, metric capability evidence, robust aggregates, noise floors,
curve decisions, cancellation, and resource budgets pass on deterministic
fixtures.

```text
+ src/performance/measure.rs
+ src/performance/curve.rs
+ tests/fixtures/performance/
~ src/performance/model.rs
~ src/commands/performance.rs
~ src/execution/resource.rs
~ src/execution/runner.rs
~ src/source_index/metrics.rs
~ tests/performance_cli.rs
~ codeatlas.json
```

## Phase 3: Static optimization candidates

Status: [ ] Not started

LOC: +350-550 / -30-80

Verify: Candidate reasons and ranking are deterministic, evidence-linked, and
never rendered as measured hotspots; existing source metrics are reused without
becoming a second performance report owner.

```text
+ src/performance/candidates.rs
~ src/performance/model.rs
~ src/commands/performance.rs
~ src/domain/source_graph.rs
~ src/context_slice/model.rs
~ src/source_index/metrics.rs
~ tests/performance_cli.rs
```

## Phase 4: Gated profiler attribution

Status: [ ] Waiting for accepted Phase 2 measurement evidence and explicit
profiler-backend authorization

LOC: +250-450 / -20-70

Verify: The accepted profiler capability and bounded capture are explicit;
container/rootless behavior and overhead are measured; mapped/unmapped frames
and confidence are reported; locations are never called hotspots without
material runtime evidence.

```text
+ src/performance/attribution.rs
~ Cargo.toml
~ Cargo.lock
~ src/performance/model.rs
~ src/performance/measure.rs
~ src/commands/performance.rs
~ src/execution/resource.rs
~ tests/performance_cli.rs
```

## Phase 5: Self-performance baselines and release hardening

Status: [ ] Not started

LOC: +250-450 / -200-400

Verify: Representative CodeAtlas self workloads produce reviewed cold/warm
observations; full checks pass; no top-level observe verb, hidden execution,
duplicate metrics owner, unbounded profiler artifact, false hotspot label,
stale docs, or checkout-generated state remains.

```text
~ README.md
~ AGENTS.md
~ docs/concepts/lexicon.md
~ codeatlas.json
~ proposals/codeatlas-performance-evidence.md
~ proposals/codeatlas-fuzz-performance.md
~ tasks/check-self.js
~ package.json
~ src/performance/mod.rs
~ src/performance/model.rs
~ tests/performance_cli.rs
```

The implementation is intentionally net higher because it adds a controlled
measurement product, curve evidence, optional profiler attribution, and
regression testing. It must not duplicate the execution kernel, internal source
index telemetry, or fuzz correctness models.

Total LOC: +2,150-3,450 / -430-910

## Layman's wins

- CodeAtlas shows which code is actually slow instead of only guessing from
  complexity.
- Comparisons use reviewed observations, so baseline and diff cannot secretly
  rerun expensive workloads.
- Results include enough environment and noise evidence to avoid false alarms.
- A separate follow-on can later search for worst-case inputs without delaying
  trustworthy measurements and hotspot evidence.
