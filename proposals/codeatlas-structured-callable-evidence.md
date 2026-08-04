# Structured callable evidence

Status: Accepted program child; implementation pending

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
  target_id
  language
  visibility and export identities
  callable_kind
  owner/receiver
    requirement
    constructibility
    implemented/declared contract identity when proven
  type_parameters
    identity
    resolved constraints or explicit unknown
  parameters[]
    position
    semantic role/name
    semantic type
    required/default/variadic state
    declared constraints
    constructibility
  result
    semantic type
    error/result shape
  effects[]
  source evidence
  evidence completeness and block reasons
```

Target identity is the existing CodeAtlas symbol/node identity. Display
signatures may remain for presentation, but no policy depends on parsing them.
Language-specific syntax nodes do not leak into the shared model.

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

Status: [ ] Not started

LOC: +550-800 / -40-80

Verify: The four-language conformance table, stable target/type/receiver model,
unknown evidence, and deterministic effect propagation pass from one source
snapshot.

```text
+ src/domain/callable.rs
+ src/analysis/effects.rs
+ src/config/code.rs
+ tests/callable_contract.rs
~ src/domain/mod.rs
~ src/domain/model.rs
~ src/domain/source_graph.rs
~ src/languages/definition.rs
~ src/languages/ecmascript/collection.rs
~ src/languages/typescript/parser/visitor.rs
~ src/languages/python/parser.rs
~ src/languages/python/reachability.rs
~ src/languages/rust/parser.rs
~ src/languages/rust/parser/signatures.rs
~ src/languages/rust/reachability.rs
~ src/source_index/mod.rs
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

Total LOC: +900-1,300 / -200-350

## Layman's wins

- CodeAtlas understands function inputs and effects once instead of repeatedly
  guessing from how a signature happens to print.
- The same facts power inspection, naming analysis, test evidence, duplication
  analysis, and later fuzz planning.
- Missing information stays visible instead of quietly becoming an unsafe
  default.
- This useful static evidence can ship without waiting for sandboxed execution.
