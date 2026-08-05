# Structured callable evidence

Status: Accepted program child; implementation in progress

Decision scope: One cross-language `CallableContract`, deterministic effect
evidence, consumer migration, and the corresponding lexicon schema transition

Depends on:

- [`codeatlas-evidence-lifecycle-cli.md`](codeatlas-evidence-lifecycle-cli.md)
- [`codeatlas-published-schemas.md`](codeatlas-published-schemas.md)

Unblocks:

- [`codeatlas-code-fuzzing.md`](codeatlas-code-fuzzing.md)
- [`codeatlas-semantic-role-siblings.md`](codeatlas-semantic-role-siblings.md)

Independent of: the execution sandbox and all live target calls

## Decision

CodeAtlas will replace display-signature reparsing with one structured callable
contract emitted by the Rust, Python, JavaScript, and TypeScript language
adapters. Scan/inspect, lexicon, public API witnesses, semantic sibling
analysis, and later fuzz planning all consume the same evidence.

The contract records only semantics the language adapter can prove. Unknown
receiver construction, generic constraints, parameter types, results, or
effects remain explicit unknowns. A consumer may narrow eligibility based on
those unknowns; it may not recover missing structure by parsing the display
signature again.

This proposal performs no fuzzing and runs no analyzed code. Extracting it from
the code-fuzzing proposal makes the evidence contract independently reviewable
and prevents its second consumer from depending on a sandboxed execution
feature it does not need.

## Existing problem

`src/lexicon/callable_contract.rs` currently normalizes callable shapes from
human-facing signature strings. That was a useful prototype but is not a stable
semantic owner:

- display syntax differs across languages and parser versions;
- names, defaults, nested types, receivers, and constraints are lossy;
- every new consumer would need another interpretation layer;
- effect and constructibility evidence cannot be represented honestly;
- cache and report identities cannot distinguish structured from inferred
  string evidence.

The file is deleted when all current callers consume `CallableContract`. It is
not retained as a fallback or compatibility path.

## Contract model

`src/domain/callable.rs` owns the language-neutral evidence shape:

```text
CallableContract
  signatures[]
    callable kind and body availability
    async state
    owner/receiver requirement and constructibility
    type parameters and resolved constraints
    ordered parameters
      semantic role/name
      semantic type
      required/default/variadic state
      constructibility
    result semantic type and error/result shape
  effects[]
  stable block reasons
```

Target identity, language, visibility, exports, source span, and source file
remain owned by the containing `Symbol` or source-graph `NodeId`; the nested
contract does not duplicate them. A signature vector represents overloads
without concatenating and reparsing display text. Display signatures may remain
for presentation, but no policy depends on parsing them. Language-specific
syntax nodes do not leak into the shared model.

The contract is immutable evidence on one source snapshot. Stable ordering is
target ID, parameter position, effect kind, and evidence location. Equivalent
source evidence produces byte-identical contracts.

## Semantic type evidence

The initial parity vocabulary is deliberately bounded:

- Boolean.
- Bounded signed and unsigned integers.
- Floating point, with support for special values recorded separately.
- String and bytes with known length/encoding constraints.
- Null/none and optional/nullable values.
- Enums and finite literal unions.
- Bounded lists, tuples, sets, and maps.
- Records/data objects with ordered fields.
- Result/error shapes.
- Named model/type identity where it resolves exactly.

The model can represent unsupported or unresolved evidence without pretending
it is constructible. Initial explicit-unknown cases include unresolved
generics, trait/protocol objects, existential/opaque foreign values, callbacks,
recursive shapes without bounds, and receiver lifecycles without a proven
factory.

Language adapters advertise exact construct support. “Python supported” or
“TypeScript supported” is never a substitute for per-contract evidence.

## Effect evidence

`src/analysis/effects.rs` owns deterministic propagation over the bounded
source graph. Subject adapters contribute known source and sink facts for:

- filesystem read and write;
- network calls;
- database access;
- process creation;
- environment access;
- time, randomness, and ambient global state;
- unknown or unsupported effects.

Each effect records its kind, direct or propagated status, source target/path,
and evidence confidence/completeness. Known sinks propagate through resolved
call edges in a stable worklist. Unresolved dynamic boundaries remain visible
counterevidence and yield unknown effect where policy requires it.

Absence of a detected sink is not proof of purity. Effect evidence can nominate
required capabilities or block automatic selection, but only the execution
kernel can classify an executable target and only runtime isolation enforces
effects.

## Consumer contract

- Scan and inspect expose structured callable evidence without changing target
  identity.
- Public API witnesses use the same callable target and no private signature
  parser.
- Lexicon derives callable shape and semantic-role evidence from the structured
  model.
- Semantic sibling analysis consumes contract roles, effects, named model
  types, and graph producers/consumers.
- Code fuzzing later maps supported semantic types into corpus descriptors and
  constructibility into deterministic eligibility.

Consumers may project or summarize the contract. They may not create a second
callable schema, reparse display text, or reinterpret unknown evidence as a
default.

## Lexicon and cache versioning

Moving callable candidates to structured evidence changes both meaning and, if
the serialized fields change as expected, public shape:

- bump scan from v2 to v3, publish and drift-test its generated schema, and
  remove the v2 schema from the shipped package in the same phase that adds the
  serialized callable field;
- bump context-slice from v3 to v4 because inspect serializes the same
  source-graph symbol nodes; publish and drift-test that schema in the same
  phase rather than hiding the nested public change;
- bump the source-graph/analysis algorithm identity so old cache entries cannot
  masquerade as structured evidence;
- bump the lexicon report from v3 to v4 when its actual serialized diff adds or
  changes callable evidence;
- publish and drift-test the v4 schema in the same phase;
- regenerate exact fixtures and renderer tests;
- delete the old signature heuristic and any stale candidate spelling.

If implementation proves the public lexicon bytes do not change, the proposal
does not force a ceremonial schema bump. The phase must show the exact diff and
version decision either way.

## Configuration

Strict `codeatlas.json` may declare exact receiver factories, invariant IDs, or
known effect evidence only where an owner can bind them to an exact target and
source digest. Configuration is additive evidence, never a force flag.

Unknown fields fail. Exact target miss, duplicate declarations, stale digests,
unresolvable types, or a claimed factory without a supported adapter produce a
deterministic diagnostic. This proposal does not add commands or a second
configuration file.

## Existing-first check

Reuse and consolidate:

- Current language parsers as the only syntax owners.
- Existing symbol, span, export, source-graph, package, and target identities.
- Context-slice/inspect graph projections.
- Testing public API witness identities.
- Lexicon symbols, renderers, and conceptual evidence taxonomy.
- Source-index snapshot and cache telemetry.

New code is limited to the shared contract, focused effect analysis, adapter
emission, and consumer migrations. It does not add a universal AST, universal
schema parser, runtime type checker, harness, executor, or fuzz value engine.

## Acceptance gates

- Rust, Python, JavaScript, and TypeScript pass one cross-language conformance
  table for equivalent callable/type/effect semantics.
- Unsupported constructs and incomplete evidence have stable, exact block
  reasons.
- Parameter order, named types, result/error shapes, receiver requirements,
  and effect ordering are deterministic.
- Effect propagation terminates under explicit node/edge bounds and exposes
  unresolved boundaries.
- Scan/inspect, lexicon, witnesses, semantic siblings, and fuzz planning have
  one contract owner.
- Repository search finds no display-signature callable parser or compatibility
  projection used as policy evidence.
- Cache identity, lexicon schema/version, published schema, fixtures, and docs
  match the actual serialized change.
- The phase makes zero target calls and creates no generated state under the
  checkout.

## Phase 1: Contract and language evidence

Status: [x] Complete (2026-08-04)

Execution checklist:

- [x] Map the existing symbol, parser, graph, lexicon, witness, cache, and
  inspection owners; record exact reusable evidence and retireable heuristics.
- [x] Pin the smallest language-neutral callable/type/receiver/completeness
  model without exposing parser syntax or creating a second symbol identity.
- [x] Emit equivalent deterministic contracts from Rust, Python, JavaScript,
  and TypeScript through one cross-language conformance table.
- [x] Add bounded direct and propagated effect evidence with explicit unknown
  boundaries over the existing source graph and snapshot.
- [x] Prove zero-call behavior, deterministic ordering/block reasons, external
  generated state, focused checks, self-dogfood, and a clean Phase 1 commit.

Starting checkpoint, 2026-08-04:

- The execution fail-closed checkpoint is committed as `c4fac1b`; this static
  branch assumes no sandbox capability and makes no target calls.
- The worktree is clean, CodeAtlas self-dogfood passes over 261 files, and all
  build/cache/report state remains under `/tmp/codeatlas-xdo-cache.hTn1Nk`.
- Stable Mill may be considered only for an exact semantic move/delete after
  a clean committed checkpoint. Phase 1 begins with additive contract/parser
  work, so no mutation tool is assumed before the owner map is complete.

Owner map and model decisions, 2026-08-04:

- Rust, Python, and ECMAScript parsers already emit the same recursive `Symbol`
  values consumed by scan, public-API projection, lexicon, witnesses, and the
  source-graph collectors. `Symbol.callable` is therefore the single parsed
  owner; source-graph nodes carry that evidence forward rather than parsing it
  again.
- Existing source-graph lexical-reference edges run caller to referenced
  target. `src/analysis/effects.rs` can propagate adapter-owned direct sink
  facts over those edges with no second graph or resolver.
- `src/lexicon/callable_contract.rs` is the retireable display-signature
  heuristic. It remains only until Phase 2 migrates both current consumers, and
  it will not become a fallback.
- Source-index facts are the parse-once cache. The scan schema, source-graph
  schema, source-index algorithm identity, and Rust/Python/ECMAScript parser
  namespaces change with the structured evidence so stale cache entries cannot
  deserialize as current facts.
- A callable contract contains ordered overload signatures and effects, but no
  target ID, language, visibility, export path, file, or span already owned by
  its containing symbol/node. Contract completeness is derived from one sorted,
  deduplicated block-reason set rather than stored as a second fallible truth.
- The initial four-language conformance gate passes for typed and explicitly
  untyped functions, instance receivers, and overload merging. Scan and inspect
  share the serialized symbol evidence, so their v3/v4 schema transitions are
  part of this phase rather than deferred consumer work.
- One shared qualified-action matcher owns namespace-boundary semantics while
  each adapter retains its own language/domain sink vocabulary. Focused
  `callable_effects` modules keep effect walking separate from semantic type
  mapping rather than growing three mixed-responsibility parser files.
- Effect propagation uses a stable delta worklist: each unique origin fact is
  propagated once per reachable callable edge, bounded independently by node,
  edge, fact, and work-item ceilings. Limit failures leave the public graph
  unchanged.

Implementation diff: +3,526 / -54 authored lines; +2,812 / -1,072 generated
schema lines. The accepted estimate understated the language-specific semantic
type mappers and four-language conformance surface; no universal parser was
introduced to force the implementation back under that estimate.

Completion evidence, 2026-08-04:

- The cross-language table proves callable shapes, receivers, overloads, exact
  block reasons, and all nine known direct effect kinds across Rust, Python,
  JavaScript, and TypeScript. Bounded propagation, cycles, exact unknown
  boundaries, graph immutability on limit failure, and namespace boundaries
  have focused tests.
- `cargo test --locked --jobs 1` passes 392 tests with four intentionally
  ignored live cases; clippy passes for all targets with warnings denied; 15
  Node tests, architecture-spec validation, and package validation pass.
- Scan v3 and context-slice v4 are generated from the registered Rust models;
  the schema updater and read-only registry drift test pass, and the retired
  v2/v3 schema files are absent.
- Final repeated scan bytes are identical at
  `sha256:92d382150efe4e6599c121acf3b36fce84971f410d57ddfa0c1cb4a0490d9750`;
  repeated propagated-effect inspection bytes are identical at
  `sha256:739cd6d4ef5dab6016860c4b3cb0a67825b2b39f12251b0e1e73a932b997dac0`.
  The exact `write_text_or_print` target carries a propagated filesystem-write
  fact whose original source target is `write_file`.
- Final self-audit covers 270 files and 2,691 scan symbols with zero gates.
  The execution state root is empty, this phase made zero target calls, and all
  compiler/cache/report state remains under `/tmp/codeatlas-xdo-cache.hTn1Nk`.
  Mill was not used because this additive phase never reached a clean committed
  semantic-move checkpoint.

```text
+ src/domain/callable.rs
+ src/analysis/effects.rs
+ src/languages/effects.rs
+ src/tests/callable_contract.rs
~ src/analysis/mod.rs
~ src/analysis/reachability.rs
~ src/commands/diff.rs
~ src/context_slice/model.rs
~ src/context_slice/slice.rs
~ src/domain/mod.rs
~ src/domain/model.rs
~ src/domain/source_graph.rs
~ src/languages/ecmascript/collection.rs
~ src/languages/mod.rs
~ src/languages/reachability.rs
+ src/languages/typescript/parser.rs
+ src/languages/typescript/parser/callable.rs
+ src/languages/typescript/parser/callable_effects.rs
~ src/languages/typescript/parser/format.rs
~ src/languages/typescript/parser/visitor.rs
~ src/languages/python/parser.rs
+ src/languages/python/parser/callable.rs
+ src/languages/python/parser/callable_effects.rs
~ src/languages/python/reachability.rs
~ src/languages/rust/parser.rs
+ src/languages/rust/parser/callable.rs
+ src/languages/rust/parser/callable_effects.rs
~ src/languages/rust/reachability.rs
~ src/lexicon/analyze.rs
~ src/lexicon/callables.rs
~ src/lexicon/grammar_candidates.rs
~ src/outputs/text_tree.rs
~ src/source_index/mod.rs
~ src/published_schemas.rs
~ src/tests/dead_code/rust.rs
~ src/tests/dead_code/source_policy.rs
~ src/tests/docs/rendering.rs
~ src/tests/mod.rs
~ tasks/check-package.js
~ tests/cli_contract.rs
+ schemas/codeatlas-scan-v3.schema.json
- schemas/codeatlas-scan-v2.schema.json
+ schemas/codeatlas-context-slice-v4.schema.json
- schemas/codeatlas-context-slice-v3.schema.json
```

## Phase 2: Consumer migration and lexicon v4

Status: [ ] Not started

LOC: +250-350 / -130-220

Verify: Inspect, lexicon, witnesses, and published schema all consume the
structured contract; cache identities reject old evidence; the signature
heuristic is gone.

```text
~ src/context_slice/model.rs
~ src/outputs/context_slice.rs
~ src/lexicon/mod.rs
~ src/lexicon/model.rs
~ src/lexicon/analyze.rs
~ src/commands/lexicon.rs
~ src/outputs/lexicon.rs
~ src/testing/witnesses.rs
~ src/published_schemas.rs
~ schemas/codeatlas-lexicon-*.schema.json
~ tests/cli_contract.rs
- src/lexicon/callable_contract.rs
```

## Phase 3: Dogfood and hardening

Status: [ ] Not started

LOC: +100-150 / -30-50

Verify: CodeAtlas's own callable and effect evidence is classified; exact
targets remain stable; no second parser/model/cache identity or stale schema
survives; focused and full dogfood pass from external state.

```text
~ codeatlas.json
~ docs/concepts/lexicon.md
~ proposals/codeatlas-structured-callable-evidence.md
~ proposals/codeatlas-code-fuzzing.md
~ tasks/check-self.js
~ tests/callable_contract.rs
```

Implemented Phase 1: +3,526 / -54 authored lines.

Remaining Phase 2-3 estimate: +350-500 / -160-270 authored lines.

## Layman's wins

- CodeAtlas understands function inputs and effects once instead of repeatedly
  guessing from how a signature happens to print.
- The same facts power inspection, naming analysis, test evidence, duplication
  analysis, and later fuzz planning.
- Missing information stays visible instead of quietly becoming an unsafe
  default.
- This useful static evidence can ship without waiting for sandboxed execution.
