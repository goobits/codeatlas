# Sandboxed callable code fuzzing

Status: Accepted follow-on; Phases 1A and 1B and the Python Phase 2 adapter are
complete; the Rust Phase 2 implementation passes local generation, syntax,
type-check, planning, and image-contract gates but awaits target-observed OCI
proof; the JavaScript/TypeScript Phase 2 adapter remains

Decision scope: Deterministic boundary corpora, language harnesses, native
engine adapters, and `fuzz code`

Depends on:

- [`codeatlas-evidence-lifecycle-cli.md`](codeatlas-evidence-lifecycle-cli.md)
- [`codeatlas-structured-callable-evidence.md`](codeatlas-structured-callable-evidence.md)
- [`codeatlas-execution-kernel-http-fuzz.md`](codeatlas-execution-kernel-http-fuzz.md);
  its shared models are available to the static checkpoint, while a passing
  isolation backend remains a hard prerequisite for harness execution

## Decision

This proposal inherits the umbrella's **Defensive user-safety purpose**. Its
only runtime scope is release qualification of CodeAtlas or an exact
owner-provided disposable fixture; unrelated, third-party, remote, and
production targets are outside the product contract and remain unexecutable.

CodeAtlas will fuzz supported callables in Rust, Python, JavaScript, and
TypeScript by consuming the accepted structured callable evidence and execution
kernel. Language adapters own native value materialization, harness generation,
and engine translation. They do not own callable parsing, effects, plans,
budgets, sandboxing, artifacts, or receipts.

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
- The accepted structured callable/effect contract as the only semantic input
  and eligibility evidence.
- `src/testing` public API witnesses to prioritize reachable, unwitnessed
  contracts.
- Inspect/context/source-graph identity for exact targets and report links.
- The execution kernel for plans, authorization, budgets, isolation, tools,
  artifacts, replay, redaction, leases, cancellation, and receipts.
- Pinned native generation/search engines behind exact tool fingerprints rather
  than inventing CodeAtlas-specific shrinkers for every runtime. Provisioning
  uses the generalized `src/external_tool.rs` owner, not language downloaders.

No existing owner provides generated external harnesses or native engine
translation, so those remain focused additions here.

## Consumed callable evidence

The structured-callable proposal owns the cross-language contract, supported
semantic type vocabulary, receiver/constructibility evidence, effect
propagation, lexicon v4 transition, and removal of display-signature parsing.
This proposal consumes that accepted model without restating or extending it.

Automatic selection requires a reachable exact target, a supported callable
kind, fully constructible parameter/result evidence, no unresolved receiver or
lifecycle, no unknown required effect, and a verified sandbox satisfying the
derived plan. Language capability reports remain exact per contract; they never
advertise blanket language support.

An owner may configure an exact safe fixture, receiver factory, invariant, or
differential oracle through the structured-evidence contract. That adds
evidence; it does not add a force path, relax isolation, or let this fuzzer
reparse source signatures.

Every discovered public callable is accounted for. Its structured contract
either supplies receiver/factory requirements, ordered inputs, semantic types,
constructibility, results, effects, and supported oracle evidence, or it carries
exact deterministic block reasons. Public APIs are never silently omitted from
fuzzability evidence merely because a harness cannot yet invoke them.

## Managed code targets

Execution authority and runtime provisioning use one checked-in target per
analysis-project/language boundary:

```json
{
  "fuzz": {
    "code": {
      "targets": [{
        "id": "parser-fixtures",
        "project": "codeatlas",
        "language": "rust",
        "image": "ghcr.io/goobits/codeatlas-rust-fuzz@sha256:<digest>",
        "preauthorized": true
      }]
    }
  }
}
```

The target does not duplicate public callable inventory. It binds an existing
analysis project, exactly one of `rust|python|javascript|typescript`, an
optional digest-pinned runtime image, and single-shot preauthorization.
`--symbol` selects one exact public callable from the target's existing
inventory; it is required when more than one eligible callable remains. The
callable's structured effects and block evidence still decide whether the
target is preauthorized, review-only, or blocked. Omitting an image keeps
planning available but blocks before execution.

One target may therefore cover every public callable in a project/language
without a second per-API allowlist. Exact config and source-adjacent denials
remain the only subtractive lists. Runtime images may extend the CodeAtlas
base with project dependencies, but the checkout remains read-only and the
plan binds the image digest, engine identity, generated harness digest, and
callable schema.

## One-way exclusions

The existing strict `fuzz` config owner gains one subject-shaped exclusion
block rather than engine-specific skip switches:

```json
{
  "fuzz": {
    "exclude": {
      "code": ["src/path.rs#symbol"],
      "http": ["POST /admin/export"],
      "postgres": ["query_<digest>"]
    }
  }
}
```

Each value is an exact canonical target for its subject; wildcards and a shared
string mini-language are rejected. An exclusion removes a whole target or
generated case before planning and remains visible as `blocked_by_policy` in
inventory, plan, and report evidence. It never grants capability, suppresses a
finding, or changes the analyzed public-API inventory.

`deny` means CodeAtlas must never generate an invocation for that target, even
when a disposable target and verified sandbox exist. It is reserved for
maintainer knowledge the effect model cannot safely infer, such as a hardwired
real provider, production-only credential dependency, or unacceptable external
cost. It is not an effect annotation. Ordinary effects and mutation are
represented by typed effects and handled by target classification; `deny` is
only for targets that must not be fuzzed even against a disposable target under
a verified sandbox. A second `requires_disposable` directive would duplicate
that existing effect/target classifier and is therefore rejected.

Source-adjacent API documentation may contribute the same one-way denial with
one cross-language directive grammar:

```text
@codeatlas-fuzz deny: <maintainer reason>
```

The payload is byte-for-byte the same across language adapters. These are
normative attachment examples; the reasons deliberately describe targets that
must remain denied even under isolation:

```rust
/// @codeatlas-fuzz deny: invokes the real compiler toolchain
pub fn compile_release(input: &str) -> Output { /* ... */ }
```

```javascript
/** @codeatlas-fuzz deny: calls the real payment provider */
export async function capturePayment(request) { /* ... */ }
```

```typescript
export interface ArtifactPublisher {
  /** @codeatlas-fuzz deny: publishes to the real artifact registry */
  publish(bundle: Bundle): Promise<Publication>;
}
```

```python
def load_production_credentials(name: str) -> Credentials:
    """@codeatlas-fuzz deny: requires production credentials"""
    # ...
```

Rust doc comments, JavaScript/TypeScript JSDoc, and Python docstrings are
associated with the exact declaration by their existing language
adapter; there is no repository-wide comment scraper. The reason is required,
bounded, and preserved as evidence. Malformed, duplicate-conflicting, or
unsupported directives produce a `check code` finding and block fuzz planning
for that callable.

Only `deny` exists. The directive vocabulary is subtractive-only: a comment may
contract what runs, but can never expand it. There is deliberately no `allow`
because a stale source comment must never re-enable a callable after its
implementation or propagated effects become unsafe. Source or interface
metadata cannot declare itself pure, override detected or unknown effects,
bypass a reviewed plan, or compensate for missing isolation. CodeAtlas never
rewrites a callable to skip effectful lines or branches for fuzzing. A
dependency may be substituted only by an explicit checked-in adapter whose
target, behavior, effect boundary, and oracle are independently verifiable;
otherwise the callable remains blocked.

A fuzz-aware API uses ordinary explicit dependency/context injection. Every
sandboxed code harness supplies the single planned `CODEATLAS_FUZZ=1` marker for
audit logging or selection of an explicit injected fake; protocol adapters may
carry an equivalent exact planned marker where environment is not the
transport. The marker is plan evidence, never an isolation or authorization
mechanism. If the target branches on it or skips an effect, the report
classifies the run as `alternate_behavior` and does not claim production-path
coverage. The unchanged effectful path remains blocked until separately
isolated and exercised.

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

Adopted native engines include:

- Rust typed generation and shrinking behind pinned proptest 1.11.0. One
  checked-in engine manifest and lock own provisioning; planning adds only the
  exact Cargo package/path binding for the read-only target. The runtime image
  may extend that base with an owner's exact dependency cache, while an
  unavailable dependency remains a visible execution failure rather than a
  network fallback.
- Python property generation and shrinking behind pinned Hypothesis.
- JavaScript/TypeScript property generation and shrinking will use pinned
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

`--target` chooses the checked-in runtime/authority boundary and `--symbol`
chooses the exact callable within it; they are not aliases for the same
coordinate.

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
- Every discovered public callable emits a complete structured fuzzability
  contract or exact block evidence; the parity fixture has no silent omissions.
- Unsupported constructs and effect evidence from the accepted callable
  contract have deterministic block reasons.
- Exact config/interface exclusions are one-way, visible as
  `blocked_by_policy`, and cannot override effects or missing isolation.
- Rust doc comments, JavaScript/TypeScript JSDoc, and Python docstrings pass one
  directive conformance table through their existing
  language adapters; malformed directives create findings and block planning.
- Exclusions skip whole targets/cases; no harness silently bypasses an internal
  branch, and every dependency substitution has a verified adapter contract.
- Fuzz-aware context is explicit and planned; any changed target behavior is
  reported as `alternate_behavior`, never production-path coverage.
- Boundary corpus ordering and seeded replay are exact for capable engines.
- The target never observes a call without a kernel permit.
- Controlled negative isolation fixtures prove that a harness cannot write the
  checkout or home, traverse or follow symlinks outside scratch, use ambient
  `/tmp`, reach an unplanned network destination, or start an unplanned
  subprocess.
- Sandbox absence blocks before target invocation.
- Every automatic failure identifies its oracle and minimized reproducer.
- CodeAtlas dogfood creates no source-adjacent state or fake public library.

## Phase 1A: Static corpus, exclusions, and fuzzability evidence

Status: [x] Complete; dependency-independent and makes zero target calls

Measured LOC: +2,714 / -259 authored; +8,843 / -6,820 generated schema

Verify: Boundary descriptor ordering and finite pairwise selection, exhaustive
public-contract accounting, one-way exact exclusions, and the Rust, Python,
JavaScript, TypeScript, and SQL directive conformance table pass without
advertising or executing `fuzz code`.

```text
+ src/fuzz/corpus.rs
+ src/fuzz/code/mod.rs
+ src/fuzz/code/corpus.rs
+ src/fuzz/code/report.rs
+ src/fuzz/directive.rs
+ tests/fixtures/code_fuzz/
~ README.md
~ docs/concepts/lexicon.md
~ proposals/codeatlas-code-fuzzing.md
~ proposals/codeatlas-fuzz-performance.md
~ src/analysis/{effects,reachability}.rs
~ src/architecture/source_conformance.rs
~ src/commands/{dead_code,diff,fuzz}.rs
~ src/config/fuzz.rs
~ src/context_slice/{mod,model,slice,targets}.rs
~ src/domain/model.rs
~ src/domain/source_graph.rs
~ src/http/{mod,model,planning}.rs
~ src/languages/ecmascript/collection.rs
~ src/languages/python/{parser,reachability}.rs
~ src/languages/rust/parser.rs
~ src/languages/rust/reachability.rs
~ src/languages/typescript/{parser,parser/visitor}.rs
~ src/lexicon/{analyze,callables,grammar_candidates}.rs
~ src/outputs/text_tree.rs
~ src/postgres/{diff,model}.rs
~ src/postgres/source/{mod,query}.rs
~ src/postgres/target/{query,query/classification,query/tests}.rs
~ src/dead_code/model.rs
~ src/dead_code/analyze.rs
~ src/dead_code/mod.rs
~ src/published_schemas.rs
~ src/source_index/mod.rs
~ src/testing/{impact,mod,witnesses}.rs
~ src/tests/
~ schemas/
~ tasks/check-package.js
~ tests/{cli_contract,fuzz_plan,postgres_cli}.rs
```

Phase 1A owns only deterministic evidence. It may define the report and corpus
models later consumed by planning, but it cannot add a target-running shortcut,
native engine, harness executor, or public command that implies the isolation
gate passed. SQL remains config-first; its leading comment is a convenience for
one handwritten static query, not a claim of ORM or dynamic-query parity.

Verification checkpoint (2026-08-05): the five-adapter attachment table, exact
public-callable inventory, corpus ordering/bounds, subject exclusions,
PostgreSQL policy, HTTP zero-call planning, 30-schema registry, 399-unit-test
suite, CLI suites, warning-denying Clippy, package audit, and complete static
CodeAtlas dogfood pass. The dogfood scan found 2,581 callable contracts and
zero gates. One warm engineering sample measured `usage code` at 1.197 seconds
and `check code` at 1.226 seconds; RSS was unavailable, and this observation is
not a performance-product claim.

## Phase 1B: Harness, plan, and reproducer foundation

Status: [x] Complete; target-observed hosted isolation passed

Measured together with the first Python adapter checkpoint below: +5,555 / -490
authored and +1,602 / -0 generated schema. The shared kernel, workload
protocol, typed replay contract, and hosted acceptance fixture cross the phase
boundary, so this proposal records the exact combined slice instead of
inventing a per-file allocation.

Verify: Zero-call plans and replay derivation, exact evidence digests,
pre-call permits, watchdog limits, external-only harness state, unchanged-seed
replay, and the full target-observed isolation suite pass using controlled
language-neutral fixtures.

```text
+ schemas/codeatlas-code-fuzz-{workload,report}-v1.schema.json
+ src/commands/fuzz/code.rs
+ src/execution/{call_permit,permit_protocol,unix_socket}.rs
+ src/fuzz/code/harness.rs
+ src/fuzz/code/planning.rs
+ src/fuzz/code/runner.rs
+ tests/code_fuzz_cli.rs
~ .github/workflows/live-oci-isolation.yml
~ README.md
~ docs/concepts/lexicon.md
~ package.json
~ src/config/{fuzz,mod}.rs
~ src/domain/callable.rs
~ src/execution/{artifact,budget,mod,model,proxy,workload}.rs
~ src/execution/sandbox/container/{workload.rs,workload_harness.py}
~ src/execution/sandbox/container.rs
~ src/http/{planning.rs,schemathesis/adapter.rs,schemathesis/toolchain.rs}
~ src/fuzz/code/mod.rs
~ src/fuzz/code/report.rs
~ src/fuzz/model.rs
~ src/fuzz/reproducer.rs
~ src/commands/fuzz.rs
~ src/cli/fuzz.rs
~ src/external_tool.rs
~ src/published_schemas.rs
~ tasks/check-isolation-live.js
~ tests/container-image-build.test.js
~ tests/execution_isolation.rs
~ tests/fuzz_plan.rs
~ tests/isolation-live.test.js
+ tests/support/artifact.rs
```

Local implementation checkpoint (2026-08-07): zero-call target and replay
plans, the shared permit bridge, external runtime files, host-owned result and
oracle digests, strict workload/report schemas, typed recursively validated
replay inputs, required target/project/language coordinates, the Python
Hypothesis adapter, and the shared Python workload-image build transaction pass
their focused local tests. Python import-time target code consumes a readiness
permit before any generated case. The existing hosted OCI matrix now builds
the focused hash-locked Python image and contains one target-observed marker,
readiness, generated-case, reduction, retry, replay, read-only-source, and
cleanup proof. The phase remains open until that exact committed matrix passes;
no receipt-only assertion closes the gate.

Hosted checkpoint (2026-08-07): GitHub Actions run `31162205411` executed exact
commit `c7e183f`. Both managed HTTP profiles reached their accepted assertions,
then code planning correctly blocked before the Python target observed a call:
the live fixture configured a shared `max_cases` ceiling of 12 while requesting
32 cases for the callable adapter. The narrow correction raises the checked-in
fixture ceiling to 32 while HTTP continues to tighten it to 12. The run restored
5,187,296,341 cache bytes from the compatible generation ending in `1b840fcc`,
saved a useful 5,640,060,261-byte generation under the exact `c7e183f` key, and
uploaded artifact `8987634484` with SHA-256
`022f1636ca55dba31aa3c1231680200e8e87b29e7ccdb0cb72c82092f0a1a56d`.
The target-observed gate remains open pending one duplicate-checked run of the
corrected revision.

Hosted checkpoint (2026-08-07): retry run `31162790047` executed exact commit
`e706d9f` and passed the corrected zero-call planning boundary. The Python
target then observed one readiness permit and 32 generated-case permits, every
cleanup lease released and verified, and no reduction or retry before the
sparse equality fixture exhausted its bounded search. Collection also found
that the private harness result depended on a late-created adapter report
directory. The correction makes the fixture predicate monotone so native
shrinking deterministically minimizes to 2 and moves the private result into
the kernel-precreated `control/` directory. The Rust harness protocol owns that
exact result path, the Python conformance test prevents adapter drift, and the
adapter refuses a missing or replaced control directory; this does not add a
report owner or a new scratch-path contract. The run restored 5,640,060,261
cache bytes from the exact `c7e183f` generation, saved a useful
5,123,919,194-byte `e706d9f`
generation, and uploaded artifact `8987839822` with SHA-256
`a3f8204ec490165835e66c617a1a73e3821b0a557dddcc64a9bf8c9b204e19d0`.
The target-observed gate remains open pending one duplicate-checked corrected
revision. Hosted run `31244354451` on exact revision `a1361d8` proved one
readiness, 7 generated, 10 reduction, and 1 retry permits with verified cleanup.
It then failed final collection because the shared container supervisor
reserved `CODEATLAS_SCRATCH` but did not forward it to the workload child; the
Python adapter therefore had no authoritative result root. Artifact
`9018050517` has SHA-256
`777acbb1c34ab1329b376caeaaf1cc6790a4688fbcf4a2e12aa7574d768a0d62`.
The narrow correction adds that existing kernel-owned variable to the shared
base environment and proves it in the supervisor test before one
duplicate-checked retry. Hosted run `31244812846` then passed on exact revision
`b6766c6`. It proved target-observed generated, reduction, retry, and replay
permits; minimized the failing integer to `2`; retained one plan-bound report
and reproducer; kept source bytes unchanged; and verified every cleanup lease.
The accepted receipt is
`receipt_835c98048b98b77f9c458d2eb7a3b7d905346deb17fa8d09774ca42a9f1ee71c`
with digest
`sha256:d74cadd2f814d8e7a35b1f605d899073e11c61ab31c5d348a708affe40be9fb8`.
Hosted artifact `9018189257` is 121,513,997 bytes with GitHub-verified digest
`sha256:12d96a9018ef409febc53e53ba5d5741c10f5ec590390c926b812b91ae84058a`.
The full local `pnpm check` surface also passes: 436 root unit tests, every
integration suite, the independently locked isolation probe, warning-denying
Clippy, schema/spec drift, eight-lane self-dogfood with zero gates, and a
429-file package.

## Phase 2: Rust, Python, JavaScript, and TypeScript adapters

Status: [ ] In progress; Python is complete with target-observed hosted proof,
Rust passes every locally knowable gate and awaits the exact hosted proof, and
the JavaScript/TypeScript boundary remains pending

Current Python checkpoint is included in the exact combined measurement above.
Remaining Rust plus JavaScript/TypeScript adapter work is forecast at
+1,800-2,800 / -100-300 authored; the estimate will be replaced by measured
LOC at the cross-language gate.

Verify: Each language passes the shared parity suite, deterministic prefix,
native-engine budget/replay contract, automatic oracles, reduction, and sandbox
escape suite; capability evidence names unsupported engine behavior.

```text
+ containers/code-fuzz-python/{Containerfile,Containerfile.dockerignore}
+ containers/code-fuzz-rust/{Containerfile,Containerfile.dockerignore,engine.rs}
+ src/languages/ecmascript/fuzz.rs
+ src/languages/python/fuzz.rs
+ src/languages/python/{fuzz_harness.py,fuzz_requirements.txt}
+ src/languages/rust/fuzz.rs
+ src/languages/rust/{fuzz_cargo.lock,fuzz_cargo.toml,fuzz_driver.py,fuzz_harness.rs.tpl}
 ~ .github/workflows/live-oci-isolation.yml
 ~ Cargo.toml
 ~ Cargo.lock
~ codeatlas.json
 ~ package.json
~ src/languages/code_fuzz.rs
~ src/languages/ecmascript/mod.rs
~ src/languages/python/mod.rs
~ src/languages/python/parser/callable.rs
~ src/languages/rust/mod.rs
~ src/fuzz/code/harness.rs
~ src/fuzz/code/runner.rs
~ src/external_tool.rs
~ src/tests/callable_contract.rs
+ tasks/build-workload.js
- tasks/build-http-workload.js
~ tasks/check-isolation-live.js
~ tests/container-image-build.test.js
~ tests/execution_isolation.rs
~ tests/code_fuzz_cli.rs
~ tests/fixtures/code_fuzz/
```

Local Rust checkpoint (2026-08-08): the adapter reuses the accepted callable
contract, corpus, Cargo metadata owner, shared permit transport, execution
limits, artifact store, and workload runner. It supports the exact v1 Copy
primitive set and emits deterministic block evidence for every other Rust
shape. Zero-call planning is byte-stable, records one proptest engine and one
exact delegated Cargo command, and creates neither `Cargo.lock` nor `target/`
in the consumer checkout. Ten focused Rust unit tests, five code-fuzz CLI
integrations, 16 image/orchestration tests, Python syntax checks, generated
harness parsing, and an external offline Cargo type-check pass. The type-check
caught and fixed the missing `Read` trait import before remote dispatch. The
full local signoff then used CodeAtlas itself to identify the workload image's
Cargo entrypoint as an undeclared tooling root; `codeatlas.json` now declares
that exact root and the repeated self-audit reports 304 advisory findings with
zero gates. The warning-denying Clippy surface and 435-file package pass. The
first incomplete Rust gate is now target-observed generated, reduction, retry,
and replay execution in the digest-pinned workload image; no live capability
is inferred from the local checks.

Hosted checkpoint (2026-08-08): duplicate-checked GitHub Actions run
`31271703117` was dispatched once against exact revision `08b45f9`. It is the
first run carrying the Rust workload image and target-observed Rust adapter
fixture; the gate remains open until the run reaches and passes those exact
assertions.

Hosted checkpoint (2026-08-08): run `31271703117` built all four images and
successfully published the probe, HTTP, and Python workloads, then the local
registry exited during the Rust workload push before any target test ran. The
evidence proves a coherent resource-contract defect: the Rust OCI archive is
337,509,376 bytes and all four archives total 459,627,520 bytes, while the
registry's 512 MiB tmpfs was charged to a contradictory 256 MiB container
memory ceiling. The existing registry owner now has one 1 GiB storage budget,
256 MiB process headroom, a conservative pre-push archive-budget check, and
exact argument tests; no adapter or private retry path was added. The run
restored and retained a useful 5,116,156,017-byte Cargo cache generation.
Artifact `9025869252` is 457,135,570 bytes with GitHub-verified digest
`sha256:fae614fb2522559265fb32aec07e81e11481cd91906dea1bc34eb4e1047103f6`.
The target-observed Rust gate remains open pending one duplicate-checked run of
the corrected revision.
Corrected run `31272558257` was dispatched once against exact revision
`b84f98d`. It passed the corrected registry budget and published all four
workload images, then exposed a post-search Python harness defect before the
Rust assertions: the target consumed one readiness, seven generated-case, ten
reduction, and one retry permit, but the harness referenced the shared
`RESULT_SCHEMA` constant without importing it after that constant moved to the
shared runtime owner. Collection therefore found no result file. The narrow
correction imports that existing owner and adds a focused source-contract
regression; formatting, private Python syntax, and the exact Rust unit test
pass locally. Artifact `9026127213` is 457,141,040 bytes with GitHub-verified
digest
`sha256:97bb70f61478ea23acfe8c47d8146b88739ac379fcdf0f90346665a2e3c286d2`.
The target-observed Rust gate remains open pending one exact corrected run.

## Phase 3: Self-dogfood, consolidation, and release hardening

Status: [ ] Not started

LOC: +300-500 / -250-450

Verify: Safe CodeAtlas public boundaries fuzz successfully; full checks pass;
no source-local harness/cache, duplicate callable parser, language-specific
budget, unsafe fallback, stale schema fixture, compatibility alias, or fake
public export remains.

```text
~ codeatlas.json
~ README.md
~ proposals/codeatlas-code-fuzzing.md
~ proposals/codeatlas-fuzz-performance.md
~ tasks/check-self.js
~ package.json
~ src/fuzz/code/mod.rs
~ src/fuzz/code/report.rs
~ tests/code_fuzz_cli.rs
```

The implementation is intentionally net higher because it adds external
harness generation and four real runtime adapters. It must not grow a second
callable/effect model, source-local fuzz projects, or language-private execution
controls.

Revised projected authored LOC: +10,369-11,569 / -1,099-1,499, plus the
already measured generated-schema delta. The increase is the accepted shared
permit transport, strict typed replay surface, host-owned artifact validation,
and real native runtime packaging rather than four thin language wrappers.

## Layman's wins

- CodeAtlas can safely try difficult values against supported functions.
- It clearly explains which APIs it cannot construct or isolate instead of
  pretending everything is fuzzable.
- It reuses the same accepted callable evidence as inspection, vocabulary
  checks, witnesses, and sibling analysis.
- Failures replay without adding fuzzing files or dependencies to the project.
