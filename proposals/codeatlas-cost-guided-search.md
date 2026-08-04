# Cost-guided isolated input search

Status: Speculative follow-on; independently authorized after all dependencies

Decision scope: Adaptive input search and reduction against an explicit
performance cost objective

Depends on:

- [`codeatlas-evidence-lifecycle-cli.md`](codeatlas-evidence-lifecycle-cli.md)
- [`codeatlas-execution-kernel-http-fuzz.md`](codeatlas-execution-kernel-http-fuzz.md)
- [`codeatlas-performance-evidence.md`](codeatlas-performance-evidence.md)
- The accepted domain adapter being searched: initially
  [`codeatlas-code-fuzzing.md`](codeatlas-code-fuzzing.md) or
  [`codeatlas-postgres-fuzzing.md`](codeatlas-postgres-fuzzing.md)

The base domain proposals are necessary but not sufficient. Before Phase 2, an
adapter needs a separately verified `CostSearchCapability`: callable engines
must accept external metric feedback and threshold-preserving shrink/replay;
PostgreSQL must export typed `PostgresCostEvidence` for each supported
rows/buffers/planner/execution metric. Ordinary failure/coverage fuzzing or
query execution does not imply either extension.

## Decision

CodeAtlas may later search for inputs that preserve or maximize an explicit
cost objective, but only as a bridge over already accepted contracts:

- The execution kernel owns plans, limits, isolation, permits, and receipts.
- The code or PostgreSQL fuzz adapter owns constructible inputs, mutation,
  shrinking, and replay.
- Performance owns cost metrics, sampling requirements, thresholds,
  environment identity, and confirmation evidence.

The feature extends `scan performance`; it does not add a top-level verb or
reinterpret a correctness fuzz failure:

```bash
# Zero-call preview of a checked-in workload and cost objective.
codeatlas scan performance --target parser-cost --search worst-case

# Zero-call preview of an exact saved cost reproducer.
codeatlas scan performance --replay cost_reproducer_ABC.json

# Reviewed execution through the exact persisted plan.
codeatlas scan performance --plan plan_ABC --execute
```

Target and reproducer forms without `--execute` only create a plan. Loading a
cost reproducer never invokes the target implicitly and uses the kernel's
shared reproducer-to-plan derivation.

Eligible preauthorized local/disposable execution may use the kernel's same
single-shot form only when the shared classifier approves it. Remote,
effectful, or noisy searches require reviewed authorization and every required
capability. Incompletely isolated searches remain blocked; review cannot waive
missing isolation.

## Why this is separate

Fixed measurements answer “how expensive is this declared workload?” Adaptive
search answers “which supported input makes this declared cost especially
large?” They share evidence but have different determinism, budgets, review
risk, and stopping rules.

Keeping search separate means performance baselines, curves, gates, and
hotspots can ship without accepting speculative generator integration. It also
prevents a search result from being mislabeled as a correctness failure.

## Options considered

| Option | Fit | Tradeoff | Verdict |
|---|---|---|---|
| Put cost fields in ordinary fuzz reports | Reuses current search command | Conflates robustness failures with expensive-but-correct behavior | Reject |
| Make performance own value generators | One apparent owner | Duplicates domain constructibility and shrink semantics | Reject |
| Add a new `optimize` or `search` verb | Explicit operation | Expands the grammar for one strategy rather than one evidence lifecycle | Reject |
| Extend planned performance scans through typed domain search adapters | Reuses all three correct owners | Requires accepted dependencies and strict artifact seams | Adopt |

## Cost objective contract

A checked-in objective records:

- Exact workload, target, input contract, source, config, engine, tool, and
  dataset identities.
- Metric and unit, direction, threshold, and minimum meaningful delta.
- Whether the metric is deterministic, sampled, or profiler-derived.
- Warm/cold preparation and confirmation sample requirements.
- Search strategy, seed, worker count, input size/depth bounds, and stopping
  rule.
- Whole-run calls, cases, reductions, rate, concurrency, time, CPU, RSS,
  process, output, result, and artifact ceilings.
- Isolation capabilities, external destinations, and cleanup owner.

CLI values may select a checked-in objective and tighten its ceilings. They do
not invent or override a metric or threshold, enable an effect, or turn an
unisolated target into an eligible one.

## Eligible metrics

Prefer stable cost evidence before noisy time:

- Declared work units or instruction/sample counts.
- Allocation count or bytes where an accepted capability measures them.
- Output or result bytes.
- Recursion, traversal, or graph depth.
- PostgreSQL rows, buffers, or a clearly labeled planner/execution metric.
- Robust elapsed or CPU time only under a controlled environment with repeated
  confirmation and an explicit noise floor.

Static complexity, an estimated query cost, or input size may prioritize a
search but is not silently presented as measured wall time. Unsupported metrics
produce capability evidence rather than fallback guesses.

## Search and confirmation

The ordered search is deterministic where the adapter and metric permit it:

1. Start with the domain's deterministic boundary corpus.
2. Reserve the global cleanup allowance and acquire a permit before every
   candidate or confirmation call.
3. Apply the exact seeded search strategy within remaining limits.
4. Retain candidates only when their named cost evidence improves according to
   the objective and noise rule.
5. Reduce the best candidate while preserving the threshold, using new permits.
6. Re-run a fixed confirmation workload with the declared sample policy.
7. Persist a `CostReproducer`, `PerformanceObservation`, search report, and
   execution receipt.

Timing-based searches record all confirmation samples and may end
`inconclusive` when noise prevents a decision. Budget exhaustion, interrupted
reduction, missing confirmation, or incomplete cleanup can never pass.

## Artifact semantics

A cost reproducer records exact input, contract, objective, seed, strategy,
environment, metric, threshold, and result identities. It means “this input
preserved this cost condition under this evidence,” not “the target is wrong.”

The accompanying performance observation is consumable by `check performance`,
`baseline performance`, and `diff performance` without new workload calls.
Those lifecycle commands do not resume or repeat search.

If an independent crash, invariant, or other correctness oracle also fires,
the run may link a normal fuzz failure. The cost condition itself remains a
performance result.

## Initial domain boundary

Initial adapters are callable code and PostgreSQL only:

- Code search consumes the accepted `CallableContract`, language-native value
  adapter, and sandboxed harness.
- PostgreSQL search consumes the accepted `PostgresQueryContract`, typed
  session, disposable database lifecycle, and result limits.

HTTP remains out of scope until a later proposal defines controlled service-side
cost evidence, reset semantics, and noise policy. Remote response time alone is
not enough to attribute target cost.

There is no universal value engine, universal shrinker, domain-neutral schema
parser, or second execution loop.

## Acceptance gates

- Target and reproducer search forms preview with zero calls.
- Exact changed evidence refuses execution.
- Only accepted domain contracts and generators construct inputs.
- Every candidate, reduction, retry, confirmation, and cleanup interaction
  consumes a finite pre-call permit.
- Search cannot consume the reserved cleanup allowance or exceed any kernel
  ceiling.
- Deterministic metrics and capable adapters reproduce exact candidates under
  the same plan; sampled metrics report variance and may be inconclusive.
- Reduction preserves the named cost threshold and never changes the objective.
- Cost results are never labeled correctness failures without a separate
  violated oracle.
- Performance check/baseline/diff consume the saved observation with zero calls.
- No performance-owned generator, fuzz-owned metric policy, hidden direct
  executor, HTTP timing guess, or checkout-generated state remains.

## Phase 1: Cost objective, search plan, and artifacts

Status: [ ] Waiting for accepted performance evidence and one domain adapter

LOC: +350-550 / -50-100

Verify: CLI preview, objective validation, typed artifact identities, remaining
budget, seeded search order, confirmation, inconclusive outcomes, replay, and
zero-call downstream lifecycle behavior pass on a controlled adapter.

```text
+ src/performance/search.rs
+ tests/cost_search_cli.rs
~ src/config/performance.rs
~ src/performance/mod.rs
~ src/performance/model.rs
~ src/performance/measure.rs
~ src/commands/performance.rs
~ src/cli/scan.rs
~ src/execution/model.rs
~ src/execution/artifact.rs
~ src/execution/budget.rs
~ src/execution/runner.rs
~ src/fuzz/model.rs
~ src/fuzz/reproducer.rs
~ codeatlas.json
```

## Phase 2: Callable and PostgreSQL search bridges

Status: [ ] Waiting for each corresponding accepted adapter

LOC: +100-170 / -20-60

Verify: Code and PostgreSQL consume their existing contracts, corpora,
shrinkers, and runners only after their explicit cost-search extensions pass;
deterministic and sampled objectives obey declared capabilities; neither domain
gains a second generator, metric-policy, or budget owner.

```text
~ src/performance/search.rs
~ src/fuzz/code/corpus.rs
~ src/fuzz/code/runner.rs
~ src/fuzz/code/report.rs
~ src/languages/ecmascript/fuzz.rs
~ src/languages/python/fuzz.rs
~ src/languages/rust/fuzz.rs
~ src/postgres/fuzz.rs
~ src/postgres/model.rs
~ src/postgres/target/client.rs
~ tests/code_fuzz_cli.rs
~ tests/postgres_cli.rs
~ tests/cost_search_cli.rs
```

## Phase 3: Dogfood, consolidation, and release hardening

Status: [ ] Not started

LOC: +50-80 / -30-90

Verify: Controlled CodeAtlas parser and PostgreSQL fixtures produce replayable
cost evidence; full checks pass; no duplicate owner, noisy pass, hidden call,
stale docs, compatibility alias, or source-adjacent artifact remains.

```text
~ README.md
~ docs/concepts/lexicon.md
~ codeatlas.json
~ proposals/codeatlas-cost-guided-search.md
~ proposals/codeatlas-fuzz-performance.md
~ tasks/check-self.js
~ tests/cost_search_cli.rs
```

This proposal is intentionally last. Its implementation must remain a thin
typed bridge over accepted execution, domain generation, and performance
evidence rather than becoming a fourth owner for any of them.

Total LOC: +500-800 / -100-250

## Layman's wins

- CodeAtlas can search for unusually expensive inputs without calling them
  correctness bugs.
- Search cannot exceed the same filesystem, network, call, and resource limits.
- Normal performance measurement can ship first and stand on its own.
- Code and database adapters are reused instead of rebuilt inside performance.
