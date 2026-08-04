# Sandboxed callable code fuzzing

Status: Accepted follow-on; implementation waits for the execution sandbox gate

Decision scope: Structured callable contracts, effect evidence, deterministic
boundary corpora, language harnesses, and `fuzz code`

Depends on:

- [`codeatlas-evidence-lifecycle-cli.md`](codeatlas-evidence-lifecycle-cli.md)
- [`codeatlas-execution-kernel-http-fuzz.md`](codeatlas-execution-kernel-http-fuzz.md),
  including a passing isolation backend

## Decision

CodeAtlas will fuzz supported callables in Rust, Python, JavaScript, and
TypeScript through one structured callable contract and the accepted execution
kernel. Language adapters own syntax, type extraction, harness generation, and
native engine translation. They do not own plans, budgets, sandboxing,
artifacts, or receipts.

Automatic selection is conservative. A callable is eligible only when its
target identity, parameters, receiver construction, result handling, and
required effects are known. Unknown or unsupported evidence is a visible block,
not an invitation to guess.

Static effect analysis never certifies purity. Every invocation still runs in
the enforced sandbox with the checkout read-only, only external scratch
writable, network/process access denied unless planned, and finite call/time/
resource limits.

## Options considered

| Option | Fit | Tradeoff | Verdict |
|---|---|---|---|
| Parse signature strings inside the fuzzer | Quick prototype | Duplicates lexicon heuristics and loses semantic type evidence | Reject |
| Generate source-local fuzz projects | Conventional native-tool workflow | Mutates consumer repositories and creates cleanup/ownership problems | Reject |
| Build one universal value/runtime engine | Uniform implementation | Reimplements language-native construction, search, and shrinking | Reject |
| One callable IR plus language-native harness adapters | Shared evidence with honest language boundaries | Requires a defined supported parity set | Adopt |

## Existing-first check

Reuse and consolidate:

- Language parsers and reachability adapters as the only syntax owners.
- `src/testing` public API witnesses to prioritize reachable, unwitnessed
  contracts.
- Inspect/context/source-graph identity for exact targets and report links.
- Existing lexicon callable evidence as behavior to replace with the structured
  contract, then delete.
- The execution kernel for plans, authorization, budgets, isolation, tools,
  artifacts, replay, redaction, leases, cancellation, and receipts.
- Pinned native generation/search engines behind exact tool fingerprints rather
  than inventing CodeAtlas-specific shrinkers for every runtime. Provisioning
  uses the generalized `src/external_tool.rs` owner, not language downloaders.

No existing owner provides a constructible cross-language callable contract or
generated harness, so those are new focused owners.

## Structured callable contract

Each language adapter emits the same semantic contract where its language can
prove equivalent meaning:

```text
CallableContract
  target identity and visibility
  callable kind
  receiver requirement
  type parameters and unresolved constraints
  ordered parameters
    name
    semantic type
    required/default/variadic state
    declared constraints
  result and error shapes
  constructibility evidence
  effect evidence
  source and export identities
```

The contract is consumed by scan/inspect, lexicon, public API witnesses, and
fuzz planning. No consumer reparses a display signature.

### Initial parity set

Automatically constructible types are deliberately bounded:

- Boolean.
- Bounded signed and unsigned integers.
- Floating point, including special values only where the language contract
  permits them.
- String and bytes.
- Null/none and optional/nullable values.
- Enums and finite literal unions.
- Bounded lists, tuples, sets, and maps whose element contracts are supported.
- Records/data objects whose required fields are supported.
- Supported result/error shapes.

Explicit-only or blocked types include:

- Unresolved generics, trait/protocol objects, existential values, and opaque
  foreign handles.
- Closures, callbacks, and runtime executors without an explicit adapter.
- Methods without a deterministic receiver factory and teardown.
- Resources requiring credentials, services, clocks, randomness, ambient
  global state, or host handles.
- Recursive contracts without a declared depth/size bound.
- Types whose validation constraints cannot be represented.

Capability reports list supported constructs per language. CodeAtlas advertises
exact evidence, never blanket language fuzz support.

## Lexicon and artifact versioning

`src/lexicon/callable_contract.rs` currently derives callable candidates from
signature strings. Once the structured contract ships, all lexicon callers move
to it and that heuristic file is deleted in the same phase.

The source-graph/analysis identity is bumped so old cached evidence cannot be
read as structured evidence. The lexicon JSON report moves from schema version
3 to version 4 if, as expected, structured callable fields or candidate kinds
change its serialized shape. The phase must decide this from the actual diff;
it may not change public JSON silently or bump only an internal constant while
fixtures remain stale.

## Effects and eligibility

Static analysis classifies known evidence for:

- Filesystem reads and writes.
- Network calls.
- Database access.
- Process creation.
- Environment access.
- Time/random/global state.
- Unknown or unsupported effects.

Known sinks propagate through the bounded source graph. The report includes the
path and confidence supporting a classification. Absence of a detected sink is
not proof that no effect exists, so effect evidence controls automatic
selection and planned capabilities but never disables sandbox enforcement.

Automatic public selection requires:

- Reachable public identity.
- Supported callable kind.
- Fully constructible parameter/result contract.
- No unresolved receiver or lifecycle.
- No unknown required effect.
- A verified sandbox backend satisfying the plan.

An owner may configure an exact internal or effectful fixture, factory,
invariant, or differential oracle. That adds evidence; it does not add a force
path or permit host filesystem access.

## Deterministic corpus

`src/fuzz/corpus.rs` owns domain-neutral boundary descriptors, canonical
ordering, bounded product construction, and deterministic pairwise selection.
It describes concepts such as bounded integer edges, floating special points,
length boundaries, Unicode/encoding classes, optional presence, and collection
shapes; it does not construct a Rust, Python, JavaScript, or PostgreSQL value.

Language adapters map `CallableContract` types into applicable descriptors and
materialize native values. PostgreSQL later maps catalog/domain evidence into
the same descriptors. Applicability, runtime construction, serialization, and
domain semantics stay with the adapter, so this shared lattice is not a
universal value engine.

The ordered deterministic corpus includes applicable values at and immediately
around meaningful boundaries:

- Zero, one, negative one, type minimum/maximum, and adjacent numeric values.
- Signed zero, finite extremes, infinities, and NaN where valid.
- Empty, one-unit, declared-limit, Unicode, combining, and encoding boundaries.
- Empty, singleton, declared-limit, duplicate, sorted, unsorted, and nested
  collection forms.
- Every enum/literal arm.
- Missing/present optional fields and bounded combinations.
- Fixture-declared application limits and representative values.

Nested products use deterministic pairwise or coverage-guided selection rather
than an unbounded Cartesian product. Case, depth, element, result-byte, and call
ceilings are materialized in the plan.

Adaptive search begins only after the deterministic corpus and consumes the
remaining plan budget. Planning persists a concrete seed, exact engine/version,
scheduling settings, and hard call/case/shrink limits. One worker is the
deterministic default. If a native engine cannot guarantee replay or reduction,
CodeAtlas records that limitation rather than promising it.

## Harness and native engine contract

Harnesses, manifests, compiler targets, downloaded engines, package stores,
Python bytecode, Node caches, and fuzz corpora live under external scratch/cache
roots. The consumer checkout is read-only and never gains a `fuzz/` directory,
dependency, fixture, or generated test.

Candidate native engines include:

- Rust structure-aware fuzzing behind a pinned cargo-fuzz/libFuzzer toolchain.
- Python property generation and shrinking behind pinned Hypothesis.
- JavaScript/TypeScript property generation and shrinking behind pinned
  fast-check or an equivalently capable exact adapter.

The adapter owns translation only. CodeAtlas owns target identity, input schema,
deterministic boundary prefix, plan, sandbox, permits, normalized failures,
reproducer envelope, and receipt.

Every generated invocation requests a kernel permit. A host watchdog enforces
time, CPU, RSS, PID, descriptor, output, and cleanup limits even if the harness
or native engine crashes or stops cooperating.

This proposal guarantees failure/coverage-oriented engine control only. The
cost-guided follow-on must separately extend an accepted adapter with a typed
`CostSearchCapability` proving external metric feedback, threshold-preserving
shrink, and replay semantics. Those abilities are not inferred from ordinary
cargo-fuzz, Hypothesis, or fast-check support.

## Oracles

Automatic oracles are limited to evidence CodeAtlas can prove:

- Panic, crash, uncaught exception, sanitizer finding, or child process loss.
- Per-case or whole-run timeout.
- Resource, output, result-size, or forbidden-effect limit.
- Result/error shape violation.
- Serialization failure where serialization belongs to the contract.
- Cleanup failure.

Wrong answers require an explicit invariant, roundtrip, model, reference
implementation, or differential implementation. Types alone are never a
semantic correctness oracle.

Reduction produces the smallest input known to preserve the exact oracle under
the same plan evidence and remaining budget. A reproducer records source,
contract, engine, environment, seed, input, oracle, and result digests with
secrets redacted.

## CLI

Reviewed execution:

```bash
codeatlas fuzz code --target public-api
codeatlas fuzz code --replay reproducer_ABC.json
codeatlas fuzz code --plan plan_ABC --execute
```

Both target and reproducer forms are zero-call previews. A replay executes only
through the kernel's persisted-plan derivation and shared reproducer envelope,
so loading a reproducer is never an implicit call.

Eligible single-shot execution:

```bash
codeatlas fuzz code --target parser-fixture --execute
```

Single-shot remains available only when the exact checked-in target is
preauthorized and the runtime verifies complete local/disposable isolation.
Unknown/effectful targets always require reviewed plans and may still block.

Domain selectors:

```text
--symbol
--profile
--seed
```

All common execution and fuzz limits come from the kernel-owned flattened CLI
structs. This subject neither relists nor reparses them. CLI limits only tighten
checked-in ceilings.

## CodeAtlas dogfooding

CodeAtlas remains a binary crate. It is not converted into a public library to
manufacture self-fuzz targets.

Initial dogfooding uses its real public boundaries:

- CLI argument and configuration parsing.
- Stable report/reproducer deserialization.
- Explicit safe parsing/normalization fixtures.
- Public command processes inside the sandbox with read-only checkout and
  external state.

Exact internal targets are optional only when a supported harness can access
them from external scratch without changing their visibility or injecting
source files into the checkout.

## Acceptance gates

- Rust, Python, JavaScript, and TypeScript pass one semantic type conformance
  table for the supported parity set.
- Unsupported constructs and effect evidence have deterministic block reasons.
- No display-signature parser remains as a parallel contract source.
- Boundary corpus ordering and seeded replay are exact for capable engines.
- The target never observes a call without a kernel permit.
- Checkout, home, absolute, traversal, symlink, `/tmp`, network, and subprocess
  escape attempts fail under the accepted isolation suite.
- Sandbox absence blocks before target invocation.
- Every automatic failure identifies its oracle and minimized reproducer.
- Public lexicon schema and algorithm identities match the actual serialized
  changes.
- CodeAtlas dogfood creates no source-adjacent state or fake public library.

## Phase 1: Structured callable, effect, and lexicon evidence

Status: [ ] Not started

LOC: +900-1,300 / -200-350

Verify: Cross-language contracts and block reasons match the conformance table;
effect propagation is deterministic; inspect/lexicon/witness consumers use one
contract; lexicon v4 and cache identities are exact where public shape changes.

```text
+ src/domain/callable.rs
+ src/analysis/effects.rs
+ src/config/code.rs
+ tests/callable_contract.rs
~ src/domain/mod.rs
~ src/domain/model.rs
~ src/domain/source_graph.rs
~ src/config/mod.rs
~ src/languages/definition.rs
~ src/languages/ecmascript/collection.rs
~ src/languages/typescript/parser/visitor.rs
~ src/languages/python/parser.rs
~ src/languages/python/reachability.rs
~ src/languages/rust/parser.rs
~ src/languages/rust/parser/signatures.rs
~ src/languages/rust/reachability.rs
~ src/context_slice/model.rs
~ src/outputs/context_slice.rs
~ src/lexicon/mod.rs
~ src/lexicon/model.rs
~ src/commands/lexicon.rs
~ src/outputs/lexicon.rs
~ src/source_index/mod.rs
~ src/tests/public_api.rs
~ tests/cli_contract.rs
- src/lexicon/callable_contract.rs
```

## Phase 2: Corpus, harness, and reproducer foundation

Status: [ ] Not started

LOC: +900-1,400 / -80-180

Verify: Boundary corpus ordering, size/depth limits, plan/reproducer artifacts,
pre-call permits, watchdog limits, external-only harness state, and seed replay
pass using controlled language-neutral fixtures.

```text
+ src/fuzz/corpus.rs
+ src/fuzz/code/mod.rs
+ src/fuzz/code/corpus.rs
+ src/fuzz/code/harness.rs
+ src/fuzz/code/runner.rs
+ src/fuzz/code/report.rs
+ tests/code_fuzz_cli.rs
+ tests/fixtures/code_fuzz/
~ src/fuzz/model.rs
~ src/fuzz/report.rs
~ src/fuzz/reproducer.rs
~ src/commands/fuzz.rs
~ src/cli/fuzz.rs
~ src/execution/model.rs
~ src/execution/lease.rs
~ src/execution/redaction.rs
~ src/execution/runner.rs
~ src/execution/sandbox/mod.rs
~ src/external_tool.rs
~ src/environment.rs
~ codeatlas.json
```

## Phase 3: Rust, Python, JavaScript, and TypeScript adapters

Status: [ ] Not started

LOC: +1,200-1,900 / -100-250

Verify: Each language passes the shared parity suite, deterministic prefix,
native-engine budget/replay contract, automatic oracles, reduction, and sandbox
escape suite; capability evidence names unsupported engine behavior.

```text
+ src/languages/ecmascript/fuzz.rs
+ src/languages/python/fuzz.rs
+ src/languages/rust/fuzz.rs
~ Cargo.toml
~ Cargo.lock
~ src/main.rs
~ src/languages/definition.rs
~ src/languages/registry.rs
~ src/languages/ecmascript/mod.rs
~ src/languages/python/mod.rs
~ src/languages/rust/mod.rs
~ src/fuzz/code/harness.rs
~ src/fuzz/code/runner.rs
~ src/external_tool.rs
~ tests/code_fuzz_cli.rs
~ tests/fixtures/code_fuzz/
~ package.json
```

## Phase 4: Self-dogfood, consolidation, and release hardening

Status: [ ] Not started

LOC: +300-500 / -250-450

Verify: Safe CodeAtlas public boundaries fuzz successfully; full checks pass;
no source-local harness/cache, duplicate callable parser, language-specific
budget, unsafe fallback, stale schema fixture, compatibility alias, or fake
public export remains.

```text
~ codeatlas.json
~ README.md
~ docs/concepts/lexicon.md
~ proposals/codeatlas-code-fuzzing.md
~ proposals/codeatlas-fuzz-performance.md
~ tasks/check-self.js
~ package.json
~ src/fuzz/code/mod.rs
~ src/fuzz/code/report.rs
~ tests/callable_contract.rs
~ tests/code_fuzz_cli.rs
```

The implementation is intentionally net higher because it adds a structured
cross-language contract and four real runtime adapters. It must not retain the
old signature heuristic, parallel effect models, source-local fuzz projects, or
language-private execution controls.

Total LOC: +3,300-5,100 / -630-1,230

## Layman's wins

- CodeAtlas can safely try difficult values against supported functions.
- It clearly explains which APIs it cannot construct or isolate instead of
  pretending everything is fuzzable.
- One type model powers inspection, vocabulary checks, witnesses, and fuzzing.
- Failures replay without adding fuzzing files or dependencies to the project.
