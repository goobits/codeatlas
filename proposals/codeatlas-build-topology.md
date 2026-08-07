# Measured build topology and TypeMill-assisted crate extraction

Status: [x] Complete; measured extraction rejected and fully removed

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

CodeAtlas trialed replacing the single 92,000-line Rust compilation unit with a
small explicit workspace only where a cohesive boundary and measured edit-loop
benefit agreed. The experimental targets were:

- `codeatlas-domain`: language-neutral evidence and resolved analysis inputs.
- `codeatlas-source`: path normalization, source selection, package evidence,
  and the narrow fact-provider contract shared by the application cache and
  language adapters.
- `codeatlas-languages`: Rust, Python, JavaScript/TypeScript, and Svelte syntax,
  callable, and source-graph adapters.
- `codeatlas-external-tool`: exact executable and fingerprint primitives shared
  by HTTP planning and the execution kernel.
- `codeatlas-execution`: plans, budgets, artifacts, sandboxing, proxying,
  workloads, cleanup, and receipts.

The Phase 4 gate rejected that topology before the execution boundary moved.
The extracted language boundary made a controlled application edit no faster,
made a parser edit 28.0 percent slower, and made the root test-build lane 29.9
percent slower. A faster cold build and independently runnable language tests
did not satisfy the accepted warm-loop requirement. The trial crates are
therefore removed rather than retained as architectural cruft, and Phases 5
and 6 are canceled. The independently locked isolation-conformance crate stays
unchanged.

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

The trial removed two module-level cycles rather than hiding them behind
facades: HTTP/PostgreSQL documentation imported the evidence-document model
from `outputs`, while `outputs` imported HTTP/PostgreSQL report models. Only
the evidence-document model and validation moved to domain; rendering and
code-reference presentation helpers stayed in `outputs`. The measurement gate
still required the entire trial boundary to be removed.

The clean Phase 3 dependency audit corrected one earlier assumption: language
adapters also consume the existing path, source-discovery, source-policy,
package-evidence, fuzz-directive, source-index, and effect-propagation owners.
Those dependencies could not point back into the application crate. One
dependency-light `codeatlas-source` member therefore moved the first four
owners without duplicating them. The root source index implemented one generic
fact-provider contract from that crate; root analysis wrapped language graph
construction and retained effect propagation. The pure adjacent-directive
parser moved beside its existing domain evidence instead of creating a second
grammar. No catch-all host interface, callback bag, or second source walker was
introduced.

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

Status: [x] Trial completed; removed by the Phase 4 gate

LOC: +1,950-2,250 / -1,750-2,050

Trial scaffold checkpoint: the root workspace named `crates/domain` explicitly,
continued to exclude the independently locked isolation probe, and packaged the
new crate. A focused topology test rejected a wildcard member and loss of the
probe's nested workspace or lock before any source move was attempted.

TypeMill checkpoint: stable Mill 0.8.18 advertised Rust file moves and was
probed from clean commit `c9ac41b`. Two destination-safe previews blocked
without source mutation; the final preview reported `target_not_supported`
because its workspace resource reconciliation reached an unrelated Python
import root. No plan was applied. The accepted ordinary-edit fallback owns the
move rather than retaining a partial TypeMill result or compatibility module.

Trial acceptance evidence: `codeatlas-domain` owned the former domain models plus
resolved analysis inputs and the evidence-document model/validation. Raw JSON
types remained config-private and one conversion test proved that resolution
preserved serialized evidence. HTTP and PostgreSQL no longer imported outputs;
outputs retained only rendering and code-reference presentation. Six domain
tests, 422 root tests, 26 focused CLI/docs/repository tests, all-target Clippy,
32 Node tests, 476 full-suite Rust tests, package assembly, schema/spec checks,
and seven zero-gate self-dogfood lanes passed. The old module, imports, config
re-exports, and compatibility paths were absent, while the standalone probe
manifest and lock retained their exact digests.

Fresh-cache comparison against the Phase 1 binary proves exact output identity:
scan `252122dbfcff8efe1c60558e34e6662a3badf9e63f00668125dd49119dc738b1`,
check `8de7fe56d8403b88fcd9754e0545f0bd773f0d07e5e2471019fdf01bc232abeb`,
and inspect `c7ffc4dcc0016862d3d75642ded81b0ee88e4960fa26c2feff6da348c0b78266`
match byte for byte. Complete artifacts are external under
`/tmp/codeatlas-gha-phase9.GAkQVR/artifacts/topology-domain-fresh.uDEuXd`.

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

## Phase 3: Extract shared source primitives and all language adapters

Status: [x] Trial completed; removed by the Phase 4 gate

LOC: +20,300-21,300 / -19,700-20,700

Verify: `cargo test -p codeatlas-source` and
`cargo test -p codeatlas-languages`; the Rust/Python/JavaScript/TypeScript
conformance tables; scan, source graph, callable, effect, fuzzability, and
resolution fixtures; schema drift; and the neutral resolution-conformance gate
pass. Root dependency search removes parser crates not used outside the new
member and documents the retained PostgreSQL SWC consumer. Import searches
prove that source selection, package evidence, fact caching, fuzz directives,
and effect propagation each retain one owner.

```text
~ Cargo.toml
~ Cargo.lock
+ crates/source/Cargo.toml
+ crates/source/src/lib.rs
> crates/source/src/paths.rs
> crates/source/src/source_discovery.rs
> crates/source/src/source_policy.rs
> crates/source/src/package/**
+ crates/languages/Cargo.toml
+ crates/languages/src/lib.rs
> crates/languages/src/**
~ crates/domain/src/**
~ src/analysis/**
~ src/source_index/**
~ src/testing/**
~ src/http/source/**
~ src/postgres/source/ecmascript/resolver.rs
~ src/main.rs
~ codeatlas.json
- src/paths.rs
- src/source_discovery/**
- src/source_policy.rs
- src/package/**
- src/fuzz/directive.rs
- src/languages/**
```

All four code languages remain one shipped parity set. No language is deleted
or split into a second crate merely to improve one benchmark.

Stable Mill 0.8.18 was probed from clean commit `c646af3` before this batch. Its
zero-mutation preview returned `target_not_supported`: stable Rust
cross-directory moves require unchanged module filenames or language-server
file-rename support. It persisted no plan, and the complete source-tree digest
remained unchanged, so the accepted ordinary-edit fallback owns this hard cut.

Trial source checkpoint: `codeatlas-source` owned the exact moved path,
source-policy, source-discovery, and package-evidence implementations plus one
generic fact-provider trait implemented by the root cache. Thirty moved tests
and 392 remaining root tests passed, preserving the original 422-test total;
the workspace, topology, package, formatting, and warning-denying source Clippy
checks also passed. Searches found no retired root module or duplicate source
owner, schemas were unchanged, and the standalone probe manifest and lock kept
their exact digests.

Trial language checkpoint: all 56 adapter files lived only in
`codeatlas-languages`. The root source index implemented the sole generic
fact-provider contract; the language crate collected syntax/resolution graph
evidence, while root analysis alone added propagated effects, validated, and
cached the complete source graph. The pure fuzz directive parser had one
domain owner. Thirty-six language tests, 354 root tests with three intentional
ignores, eight domain tests, 30 source tests, the neutral resolution fixture
against AgentSpeak Contracts `2d370e1`, 32 Node tests, all 14 Rust test
binaries, all-target workspace Clippy, package assembly, and the specification
guard passed.

Fresh-cache scan, check, and inspect artifacts remain byte-identical to the
Phase 1 binary with SHA-256 digests `252122dbfcff8efe1c60558e34e6662a3badf9e63f00668125dd49119dc738b1`,
`8de7fe56d8403b88fcd9754e0545f0bd773f0d07e5e2471019fdf01bc232abeb`,
and `c7ffc4dcc0016862d3d75642ded81b0ee88e4960fa26c2feff6da348c0b78266`.
Self-dogfood initially found the stale semantic-sibling paths in
`codeatlas.json`; after the adjacent config fix, all seven lanes pass over 334
files and 3,527 symbols with no gates or sibling candidates. Complete new-side
comparison and dogfood artifacts are external under
`/tmp/codeatlas-gha-phase9.GAkQVR/artifacts/topology-languages-fresh.eyqHJs`
and `/tmp/codeatlas-gha-phase9.GAkQVR/artifacts/topology-languages-dogfood`.

## Phase 4: Re-measure before touching the execution boundary

Status: [x] Complete; warm-loop gate failed and selected rollback

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

Measurement evidence (commit `3ad85ed`, same host/toolchain, two Cargo jobs,
same external warm target, exact source bytes):

| Lane | Monolith | Extracted trial | Change | Result |
| --- | ---: | ---: | ---: | --- |
| unchanged `cargo check` median | 0.135 s | 0.120 s | 10.9% faster | informational |
| app edit then `cargo build` | 17.17 s / 1,665,200 KiB | 17.17 s / 1,731,456 KiB | 0.0% wall / 4.0% RSS worse | fail |
| parser edit then `cargo build` | 14.58 s / 1,666,904 KiB | 18.66 s / 1,730,748 KiB | 28.0% wall / 3.8% RSS worse | fail |
| root `cargo test --no-run` | 23.80 s / 1,745,584 KiB | 30.91 s / 1,785,768 KiB | 29.9% wall / 2.3% RSS worse | fail |
| empty-target offline build | 105.56 s / 3,079,308 KiB | 96.36 s / 2,622,060 KiB | 8.7% wall / 14.8% RSS better | pass |

The controlled application lane compiled only the root package in both
topologies, yet retained the same wall time and a larger binary. The split
avoided rebuilding parser source but could not avoid relinking the same parser
and runtime dependency graph; the extra package boundary then added work to
parser and test lanes. `cargo test -p codeatlas-languages` independently passed
36 tests in 14.67 seconds at 739,944 KiB without compiling the application,
but that scoped benefit is not permission to waive the primary gate. The cold
target was 3,110,495,604 bytes and the binary 453,947,752 bytes, respectively
7.8 and 2.2 percent larger than baseline. Complete receipts remain external at
`/tmp/codeatlas-gha-phase9.GAkQVR/benchmarks/topology-phase4`.

## Phase 5: Make execution inputs crate-safe without duplicating config

Status: [x] Canceled by the Phase 4 measurement gate; not implemented

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

Status: [x] Canceled by the Phase 4 measurement gate; not implemented

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

Status: [x] Complete

LOC: +150-300 / -250-600

Verify: Restore the exact monolithic source topology, then run root focused
tests, `pnpm check`, package assembly, published-schema drift, neutral interop,
complete CodeAtlas self-dogfood, and generated-state audits. Scan, check, and
inspect output digests must match the Phase 1 baseline. No trial workspace
member, facade, duplicate type, unused dependency, wildcard workspace member,
or checkout-local build state may remain. The standalone isolation probe's
manifest and lock digests remain exact.

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
> src/domain/**
> src/languages/**
> src/package/**
> src/paths.rs
> src/source_discovery/**
> src/source_policy.rs
> src/fuzz/directive.rs
- crates/domain/**
- crates/languages/**
- crates/source/**
- tests/build-topology.test.js
```

The mechanical rollback returns product source exactly to baseline commit
`18f0cec` before verification. Future crate extraction requires a new measured
proposal that demonstrates the warm build benefit before moving product code;
the rejected split is not retained behind a feature, facade, or alias.

Rollback evidence: the product tree matches Phase 1 commit `18f0cec` exactly;
only this proposal and the canonical tracker differ. `pnpm check` passes in
119.36 seconds with 31 Node tests, 426 root unit tests plus every integration
and probe test, warning-denying Clippy, architecture/spec/schema drift checks,
eight zero-gate CodeAtlas self-audit lanes, and a 416-file package. Neutral
resolution conformance passes against the sibling AgentSpeak contracts. Fresh
fixture scan, check, and inspect outputs retain exact SHA-256 digests
`252122dbfcff8efe1c60558e34e6662a3badf9e63f00668125dd49119dc738b1`,
`8de7fe56d8403b88fcd9754e0545f0bd773f0d07e5e2471019fdf01bc232abeb`,
and `c7ffc4dcc0016862d3d75642ded81b0ee88e4960fa26c2feff6da348c0b78266`.
The standalone probe manifest and lock remain
`57da423323b81fa25bf6d94cce674ee04d6050ec258d4279df607a7edb123150`
and `e4c00d348e93c460b9cd404b2aaec163fbd32a63aa46393e8546b47cbfb09b6a`.
Generated-state and retired-owner searches are clean. Complete verification
and byte artifacts remain outside the checkout under
`/tmp/codeatlas-gha-phase9.GAkQVR`.

Total retained LOC: documentation-only measurement evidence; the trial's
product and workspace changes net to zero after rollback

## Layman's wins

- We measured the proposed split instead of assuming more crates meant faster
  development.
- The common edit and test loops got slower, so none of that extra structure is
  kept.
- The security probe remains independently locked, and product behavior stays
  byte-for-byte unchanged.
- TypeMill was tried only from clean commits and failed closed; no partial
  refactor or cleanup residue was accepted.
