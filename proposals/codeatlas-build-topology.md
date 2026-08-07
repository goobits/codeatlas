# Measured build topology and TypeMill-assisted crate extraction

Status: Accepted; implementation in progress

Decision scope: Reduce CodeAtlas edit/build/test cost while making dependency
direction explicit, without changing CLI behavior, evidence bytes, artifact
identity, sandbox guarantees, or the standalone isolation probe

Depends on:

- [`codeatlas-execution-kernel-http-fuzz.md`](codeatlas-execution-kernel-http-fuzz.md)
  Phase 6, so the execution boundary is green before it moves
- The installed stable `mill` advertising an exact move/rename capability for
  any TypeMill-assisted batch; unsupported work uses ordinary reviewed edits

Unblocks: Faster iteration through callable fuzzing, PostgreSQL fuzzing,
source-impact, and performance evidence

## Decision

CodeAtlas will replace the single 92,000-line Rust compilation unit with a
small explicit workspace only where a cohesive boundary and measured edit-loop
benefit agree. The accepted targets are:

- `codeatlas-domain`: language-neutral evidence and resolved analysis inputs.
- `codeatlas-languages`: Rust, Python, JavaScript/TypeScript, and Svelte syntax,
  callable, and source-graph adapters.
- `codeatlas-external-tool`: exact executable and fingerprint primitives shared
  by HTTP planning and the execution kernel.
- `codeatlas-execution`: plans, budgets, artifacts, sandboxing, proxying,
  workloads, cleanup, and receipts.

The root `codeatlas` package remains the application crate and owns config,
commands, CLI, HTTP, PostgreSQL, outputs, schema publication, and product-level
integration tests. HTTP and PostgreSQL are not extracted: both currently import
many application owners and would turn package boundaries into dependency
workarounds rather than simplify them. Lexicon remains in the application
crate because it has no heavy dependency or second runtime consumer; a crate
would add public surface without a measured build win.

## Options considered

| Option | Fit | Tradeoff | Verdict |
| --- | --- | --- | --- |
| Keep one crate | Smallest structural diff | Every app edit recompiles and relinks all parsers and the kernel | Reject |
| Split every top-level module | Maximum package count | Public surfaces, dependency edges, and coordination cost exceed the build benefit | Reject |
| Extract domain, languages, and execution in one move | Fastest apparent migration | Hides cycles and makes TypeMill review too large to prove | Reject |
| Measured leaf-first extraction | Independently green batches and honest stop points | More commits and explicit boundary work | Adopt |

## Existing-first and boundary audit

The current tree has one 92,000-line binary crate. Measured candidate sizes are
1,699 lines in `src/domain`, 16,615 in `src/languages`, 9,547 in
`src/execution`, and 7,365 in `src/lexicon`. `src/domain` has no production
dependency on another CodeAtlas module. Languages depends on domain plus the
resolved analysis-project shape. Execution depends on four raw config types,
two external-state lookups, and the existing exact-tool owner.

Two cycles must be removed as part of the final boundary rather than hidden
behind facades: HTTP/PostgreSQL documentation imports the evidence-document
model from `outputs`, while `outputs` imports HTTP/PostgreSQL report models.
Only the evidence-document model and validation move to domain; rendering and
code-reference presentation helpers stay in `outputs`.

PostgreSQL's ECMAScript SQL collector legitimately uses SWC directly for its
domain-specific visitor. Phase 3 does not move that visitor into languages or
invent a generic AST facade. The root may therefore retain `swc_core` after
language extraction; later removal requires a separately proven parse-once
facts contract, not a compile-time shortcut.

The existing `codeatlas-isolation-conformance` crate remains intentionally
standalone and excluded from the root workspace. Its separate manifest, lock,
static target, OCI recipe, and digest identity are security evidence. New
workspace members are explicit; `members = ["crates/*"]` is forbidden because
it would silently absorb the probe and change its dependency resolution.

## Non-negotiable invariants

- No compatibility module, re-export facade, duplicate type, alias crate, or
  old `crate::domain`/`crate::languages`/`crate::execution` path remains.
- Moving code cannot change serialized reports, canonical IDs, schema bytes,
  graph/cache identity, CLI help, decisions, errors, receipts, or cleanup.
- Raw strict JSON types stay config-owned. Crates receive resolved typed values;
  the raw and resolved coordinates are never represented by the same stale
  `*Config` type.
- Crates are `publish = false`, expose the minimum cross-crate API, deny
  unreachable public items, and depend only toward lower-level contracts.
- Unit and conformance tests move with their owner. Product integration, CLI,
  schema drift, packaging, and dogfood tests remain in the root package.
- Cargo, TypeMill, source-index, benchmark, and package state stays outside
  `/workspace`. The standalone probe retains its own lock and build cache.
- Every TypeMill batch starts from a clean committed HEAD, persists and reviews
  an exact plan, applies only that plan ID, inspects the receipt, and accepts no
  manual post-apply import/module/visibility repair as successful TypeMill
  output. If the stable capability cannot prove the final form, do not apply it;
  use an ordinary reviewed edit instead.

## Phase 1: Pin the build and dependency baseline

Status: [x] Complete

LOC: +140-220 / -0-20

Verify: With one external Cargo target and fixed job count, record no-op check,
app-layer edit build, parser-layer edit build, root test-build, wall time, peak
RSS, compiled packages, and artifact sizes. Repeating the unchanged lane must
stay within 10 percent before its result can be used as a comparison.

```text
~ proposals/codeatlas-build-topology.md
~ proposals/codeatlas-fuzz-performance.md
```

The benchmark uses mtime-only touches restored by the next Cargo invocation;
it does not change source bytes or create a benchmark-only code path. Cold
dependency download time is recorded separately and is not an extraction win.

Baseline evidence (commit `8147807`, Rust/Cargo 1.97.1, Linux AArch64, 22
logical CPUs, 50.4 GB RAM, two Cargo jobs, external targets):

| Lane | Wall | Peak RSS | Compiled packages |
| --- | ---: | ---: | ---: |
| unchanged `cargo check` (median of three) | 0.135 s | 99,932 KiB | 0 |
| app edit then `cargo build` | 17.17 s | 1,665,200 KiB | 1 |
| parser edit then `cargo build` | 14.58 s | 1,666,904 KiB | 1 |
| root `cargo test --no-run` after the edit lanes | 23.80 s | 1,745,584 KiB | 1 |
| empty-target offline `cargo build` | 105.56 s | 3,079,308 KiB | 295 |

The unchanged checks measured 130.861, 134.607, and 136.304 milliseconds, a
4.0-percent full range around the median. An initial 11.58-second root refresh
was classified as warm-up rather than no-op evidence. The two touched source
files retained identical SHA-256 digests. The warm target ended at
9,934,517,413 bytes; the clean target was 2,885,988,130 bytes and its root
binary was 444,380,016 bytes. The separately measured registry fetch took
0.53 seconds, after which the clean build ran offline. Complete logs and timing
receipts live outside the checkout under
`/tmp/codeatlas-gha-phase9.GAkQVR/benchmarks/topology-phase1`.

## Phase 2: Extract the dependency-light domain crate and remove report cycles

Status: [~] In progress

LOC: +1,950-2,250 / -1,750-2,050

Scaffold checkpoint: the root workspace names `crates/domain` explicitly,
continues to exclude the independently locked isolation probe, and packages the
new crate. A focused topology test rejects a wildcard member and loss of the
probe's nested workspace or lock before any source move is attempted.

Verify: `cargo test -p codeatlas-domain`, root scan/check/inspect schema bytes,
and HTTP/PostgreSQL docs fixtures are unchanged. Import search finds no root
domain module or compatibility re-export and no HTTP/PostgreSQL dependency on
`outputs`.

```text
~ Cargo.toml
~ Cargo.lock
~ package.json
+ crates/domain/Cargo.toml
+ crates/domain/src/lib.rs
+ crates/domain/src/reference.rs
> crates/domain/src/callable.rs
> crates/domain/src/evidence.rs
> crates/domain/src/model.rs
> crates/domain/src/source_graph.rs
> crates/domain/src/traits.rs
~ src/config/analysis.rs
~ src/commands/docs.rs
~ src/http/docs.rs
~ src/postgres/docs.rs
~ src/outputs/reference.rs
~ src/outputs/html.rs
~ src/outputs/markdown.rs
~ src/main.rs
~ codeatlas.json
~ tasks/check-package.js
+ tests/build-topology.test.js
- src/domain/mod.rs
- src/domain/callable.rs
- src/domain/evidence.rs
- src/domain/model.rs
- src/domain/source_graph.rs
- src/domain/traits.rs
```

`>` denotes an exact move, not an added second copy. Config retains raw
`AnalysisProjectConfig`, `AnalysisContextConfig`, `RustAnalysisConfig`, and
`TestSubjectConfig`; it converts once into domain-owned resolved project,
context, Rust-option, and test-subject values. Existing resolved consumers move
to those values in the same phase, with no alias or re-export of retired names.

## Phase 3: Extract all language adapters as one parity crate

Status: [ ] Not started

LOC: +17,000-17,600 / -16,650-17,150

Verify: `cargo test -p codeatlas-languages`; the Rust/Python/JavaScript/
TypeScript conformance tables; scan, source graph, callable, effect, fuzzability,
and resolution fixtures; schema drift; and the neutral resolution-conformance
gate pass. Root dependency search removes parser crates not used outside the
new member and documents the retained PostgreSQL SWC consumer.

```text
~ Cargo.toml
~ Cargo.lock
+ crates/languages/Cargo.toml
+ crates/languages/src/lib.rs
> crates/languages/src/**
~ src/analysis/**
~ src/source_index/**
~ src/testing/**
~ src/http/source/**
~ src/postgres/source/ecmascript/resolver.rs
~ src/main.rs
~ codeatlas.json
- src/languages/**
```

All four code languages remain one shipped parity set. No language is deleted
or split into a second crate merely to improve one benchmark.

## Phase 4: Re-measure before touching the execution boundary

Status: [ ] Not started

LOC: +80-140 / -0

Verify: Repeat Phase 1 on the same host, toolchain, external cache identity,
job count, and source bytes. The app-layer edit lane must improve wall time or
peak RSS by at least 20 percent, scoped language tests must avoid rebuilding
the application crate, and cold build regression must remain below 10 percent.

```text
~ proposals/codeatlas-build-topology.md
~ proposals/codeatlas-fuzz-performance.md
```

Failure to meet those budgets blocks further extraction until the measurement
explains why. It does not authorize a speculative parser abstraction or hiding
the result.

## Phase 5: Make execution inputs crate-safe without duplicating config

Status: [ ] Not started

LOC: +450-750 / -250-500

Verify: Raw JSON still rejects unknown fields; resolved isolation/runtime
inputs have one conversion owner; artifact and scratch roots are explicit
inputs; exact tool fingerprints remain byte-identical; all plan, policy,
sandbox, and zero-call tests pass.

```text
~ Cargo.toml
~ Cargo.lock
+ crates/external-tool/Cargo.toml
> crates/external-tool/src/lib.rs
~ src/config/execution.rs
~ src/commands/fuzz.rs
~ src/http/planning.rs
~ src/http/schemathesis/toolchain.rs
~ src/execution/artifact/store.rs
~ src/execution/isolation.rs
~ src/execution/runner.rs
~ src/main.rs
- src/external_tool.rs
```

The external-tool leaf stays separate because HTTP planning and execution are
independent consumers. It is not folded into execution and no root re-export
preserves the old path.

## Phase 6: Extract the execution kernel as one invariant crate

Status: [ ] Not started

LOC: +9,900-10,600 / -9,550-10,050

Verify: `cargo test -p codeatlas-execution`; all target-observed proxy,
budget, artifact, sandbox, cancellation, cleanup, fake-runtime, and non-live
isolation cases; root HTTP plan/execute integration; canonical plan/report/
receipt vectors; schema drift; and the accepted live artifact identity remain
unchanged. No second kernel module or config dependency remains.

```text
~ Cargo.toml
~ Cargo.lock
+ crates/execution/Cargo.toml
+ crates/execution/src/lib.rs
> crates/execution/src/**
~ src/cli/execution.rs
~ src/commands/fuzz.rs
~ src/fuzz/**
~ src/http/planning.rs
~ src/http/schemathesis/**
~ src/main.rs
~ codeatlas.json
- src/execution/**
```

The standalone isolation-conformance crate remains excluded and independently
locked while becoming a path dependency of `codeatlas-execution`.

## Phase 7: Consolidate, benchmark, dogfood, and hard-cut the old topology

Status: [ ] Not started

LOC: +150-300 / -250-600

Verify: Root and per-crate focused tests, `pnpm check`, package assembly,
published-schema drift, neutral interop, complete CodeAtlas self-dogfood, and
generated-state audits pass. Warm app-layer build/test lanes improve by at
least 30 percent or peak RSS by at least 25 percent over Phase 1; cold build
regression remains below 10 percent. No old module path, facade, duplicate
type, unused dependency, wildcard workspace member, or checkout-local build
state remains.

```text
~ Cargo.toml
~ Cargo.lock
~ README.md
~ AGENTS.md
~ codeatlas.json
~ tasks/check-self.js
~ tasks/check-package.js
~ proposals/codeatlas-build-topology.md
~ proposals/codeatlas-fuzz-performance.md
```

If the final performance budget fails, the phase removes or revises the
specific extraction that caused it rather than waiving the budget or retaining
a compatibility layer.

Total LOC: +29,670-31,860 / -28,450-30,370 physical move/edit lines; expected
net authored surface +700-1,300 lines for manifests, explicit boundary inputs,
and conformance coverage

## Layman's wins

- Editing a command no longer recompiles every parser and sandbox module.
- Language and execution tests can run independently with lower memory use.
- Package boundaries match real ownership instead of hiding cycles or aliases.
- TypeMill performs reviewable moves where it can prove the final form; it is
  never trusted as an incomplete cleanup shortcut.
