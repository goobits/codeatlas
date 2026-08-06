# HQA source-impact projection

Status: Accepted CodeAtlas design; implementation waits for the canonical
Phase 16A continuation gates

Decision scope: One bounded projection from CodeAtlas's existing source graph,
callable effects, and framework-owned surface bindings into the externally
owned AgentSpeak hints and source-impact contracts

Depends on:

- [`codeatlas-published-schemas.md`](codeatlas-published-schemas.md)
- [`codeatlas-structured-callable-evidence.md`](codeatlas-structured-callable-evidence.md)
- [`codeatlas-semantic-role-siblings.md`](codeatlas-semantic-role-siblings.md)
- the live Phase 9 OCI isolation proof as a canonical program-order gate, not
  as a source-impact algorithm or runtime dependency
- the accepted external `agentspeak.hqa-codeatlas-hints/v1` and
  `agentspeak.source-target/v1` schemas; and
- an accepted external `agentspeak.source-impact/v1` schema before the
  renderer phase begins.

`agentspeak.graph-alignment/v1` is a separate HQA continuation gate, not a
CodeAtlas dependency. This producer neither consumes nor drift-tests alignment
bytes merely to claim family-level convergence.

Does not authorize: an AgentSpeak-contract edit, an HQA/HIF/TypeMill edit, a
second source or effect analyzer, cross-version target matching, or any live
target execution

## Decision

CodeAtlas will project current, per-build source-impact manifests from the one
existing `SourceGraph` and each entry symbol's existing `CallableContract`.
React and Svelte adapters contribute one typed source fact:

```text
(surface hint kind, surface hint key, HIF action) -> entry symbol
```

The projection follows the already-resolved source graph from those entry
symbols, carries the callable effects and exact unresolved boundaries already
owned by CodeAtlas, and emits consumer-native AgentSpeak JSON. It never reparses
a display signature, walks effects a second time, constructs an HQA graph edge,
or emits an HQA `GraphTargetId`.

Two `scan code` formats share that exact in-memory binding evidence:

```bash
codeatlas scan code --scope source --all \
  --format hqa-hints --out hqa-hints.json

codeatlas scan code --scope source --all \
  --format hqa-source-impact \
  --application-build-namespace ci.example/release \
  --application-build 018f3f6c \
  --surface web \
  --out source-impact.json
```

`hqa-hints` and `hqa-source-impact` are separate renderers because they satisfy
separate external contracts: hints prioritize exploration, while source impact
describes which source evidence may justify retesting one target-action pair.
They are not separate analyzers. Both consume one graph build, one binding
model, and one deterministic hint projection. The source-impact document binds
the canonical digest of the exact hints document it accompanies so HQA can
reject a mismatched pair.

Each command invocation loads or builds one immutable source snapshot. The
source-impact renderer materializes the exact canonical hints document in
memory before recording its digest; it does not rely on mutable state left by a
prior hints command. If source changes between separately written artifacts,
their digest binding fails closed in HQA instead of pairing unlike snapshots.

The source-impact document is a current-build manifest, not a CodeAtlas-owned
cross-build diff. HQA may compare accepted manifests and project a change into
its existing invalidation owner. CodeAtlas does not align graph targets, reuse
target IDs, or decide that two runtime targets are the same.

## Options considered

| Option | Fit | Tradeoff | Verdict |
|---|---|---|---|
| Project the existing source graph and callable effects | Exact source and effect owners already exist; only the missing surface binding is new | Requires a source-graph/cache version bump and bounded framework evidence | Adopt |
| Extend the HTTP HQA-inventory renderer | Reuses an existing HQA-facing file | HTTP routes do not represent UI target-actions, callable effects, or framework handlers; it would mix unrelated contracts | Reject |
| Build a source-impact-specific parser and effect walker | Locally self-contained | Creates parallel syntax, resolution, effect, cache, and boundary truth | Reject |
| Use configuration-only target-to-symbol mappings | Useful for a future explicit unsupported-framework escape hatch | Stales easily and would make the first implementation ignore source evidence CodeAtlas can prove | Defer unless a real unsupported framework supplies a conformance need |

The adopted option is the smallest long-term system: one new fact enters the
existing evidence graph, and two external documents project from it.

## Permanent boundaries

- A surface hint is scheduling evidence, never runtime target identity.
  `data-hif-key` and `data-hqa-target` values may become hint keys; they never
  seed, replace, or authorize reuse of an HQA `target_...` ID.
- CodeAtlas source `NodeId` values remain source-graph annotations. They are not
  HQA graph IDs and cannot appear in an HQA target-ID field.
- Source dependencies are hypotheses about what to retest. They never become
  HQA Interaction Graph states or transitions.
- Source knowledge plans; only HQA's observed runtime surface proves behavior.
- `agentspeak.graph-alignment/v1` remains a separate advisor-owned artifact.
  CodeAtlas neither produces nor consumes it in this program.
- The existing `src/analysis/effects.rs` worklist remains the sole effect
  propagation owner. Source impact reads its output and adds no effect sink,
  propagation queue, purity inference, or call-graph substitute.
- The current source index remains the sole parse/cache owner. Framework facts
  are extracted during the existing parse and survive through the existing
  graph cache; the HQA renderers do not rescan files.
- Surface-binding capability and completeness travel with the same source-graph
  facts and existing `AnalysisCompleteness` / `AnalysisBoundary` vocabulary.
  There is no second capability registry, and an absent impact can never imply
  that an adapter proved absence when it actually lacked coverage.
- `scan code` remains the public subject. No `impact`, `hqa`, or
  `source-impact` command family is added.
- The external AgentSpeak schemas are referenced and drift-tested from their
  owner. CodeAtlas checks in no reconstructed or vendored copy.

## Producer requirements for the contract advisor

These are CodeAtlas producer requirements, not draft schema bytes. The
AgentSpeak advisor retains ownership of every field name, `$id`, bound, and
wire-version decision.

`agentspeak.source-impact/v1` must provide one strict, bounded root that:

- scopes the document to one concrete application build and one concrete HIF
  `Surface`; `any` is not a concrete artifact scope;
- uses a typed build coordinate whose namespace and value compare exactly
  without a repository, environment variable, timestamp, or other ambient
  lookup;
- identifies each affected runtime coordinate as exactly one HQA hint
  `(kind, key)` plus one canonical HIF action, with no wildcard action in v1;
- allows one coordinate to cite one or more entry symbols through full
  `agentspeak.source-target/v1` blocks;
- binds the RFC 8785 canonical digest of the exact
  `agentspeak.hqa-codeatlas-hints/v1` document projected from the same source
  evidence;
- persists the structured syntax, structure, and dependency manifests and the
  digest of each manifest's RFC 8785 canonical bytes;
- carries typed callable effects, effect provenance, analysis completeness,
  omitted-count evidence, and unresolved boundaries without an unregistered
  free-form object;
- carries typed surface-binding capability evidence naming the concrete
  language/framework adapter, surface, supported binding forms, completeness,
  and unsupported or unresolved counts, so no impacts is distinguishable from
  no proven coverage;
- permits named state reads and writes to be absent. Absence means CodeAtlas
  did not prove named state identity; it never means no state was read or
  written;
- labels every row as a source hypothesis and a retest hint. A later consumer
  may derive an invalidation hint from a verified manifest change, but the
  producer never claims a runtime behavior change;
- contains no HQA graph ID, target correspondence, alignment proposal,
  acceptance decision, runtime edge, or target-identity fingerprint;
- defines canonical ordering and semantic uniqueness for hint coordinates,
  target-actions, entry targets, manifests, effects, and boundaries; and
- makes duplicate/conflicting coordinates, stale source-target digests,
  mismatched hint digests, invalid manifest digests, and every bound exhaustion
  fail closed before consumer state changes.

The schema should reuse the complete external source-target schema by `$ref`.
If JSON Schema cannot express a semantic uniqueness or digest rule, the schema
description must name the CodeAtlas producer and HQA loader as fail-closed
enforcers rather than implying that `uniqueItems` proves more than it does.

`agentspeak.hqa-codeatlas-hints/v1` remains unchanged by this proposal.
CodeAtlas emits only its accepted shape, validates every emitted
`evidence.target` through the full source-target contract during convergence
tests, and rejects a duplicate `(kind, key)` before serialization. HQA retains
its independent obligation to reject the whole input file on that duplicate.

## Artifact semantics

One source-impact document represents one current CodeAtlas source snapshot and
one application-build/surface scope. It contains:

```text
source-impact document
  exact application build coordinate
  concrete HIF surface
  exact hints-document digest
  producer, source-snapshot, and surface-binding capability evidence
  manifests[]
    entry source targets[]
    syntax features + canonical digest
    structure features + canonical digest
    dependency features + canonical digest
    callable effects[]
    unresolved boundaries[]
    completeness and omitted counts
  impacts[]
    surface hint (kind, key)
    canonical HIF action
    persisted manifest reference
    source_hypothesis + retest_hint labels
```

The manifest table deduplicates identical entry-symbol sets and dependency
projections. A digest is only an index and equality shortcut: the complete
human-readable manifest remains in the document. A reader can therefore see
which source targets, structures, dependencies, effects, and boundaries differ
without reversing a hash.

The syntax section records exact source targets and content digests, not source
text. The structure section records named source symbols, symbol kinds, and
the existing structured callable evidence. The dependency section records the
bounded resolved source edges and external/unresolved endpoints reached from
the entry symbols. Each section has its own digest, and the complete manifest
has a digest over those persisted sections.

CodeAtlas emits `path`, exact file `content_digest`, one-based `line` when
known, and registered `codeatlas.node_id` / `codeatlas.symbol` annotations.
Current `Span` columns are not proven UTF-16 units, so CodeAtlas does not emit a
source-target `range` until a language adapter supplies measured UTF-16
positions. It never converts a display line/column by assumption.

## Surface-action binding evidence

`src/domain/surface_binding.rs` owns the language-neutral source fact and its
coverage evidence. It reuses `AnalysisCompleteness` and `AnalysisBoundary`
rather than creating a general-purpose capability registry. One edge contains:

- a hint kind and nonblank bounded hint key;
- one exact HIF action spelling emitted from an adapter-owned closed mapping;
- one callable entry `NodeId`; and
- sorted, deduplicated source evidence plus explicit incompleteness.

The graph validates that the entry exists, belongs to the same project, and is
a callable symbol. Semantic duplicates are rejected even when their evidence
lists differ. A shared handler remains visible as the destination of several
coordinates; several handlers for one coordinate remain several explicit
edges. Neither case is silently collapsed into runtime identity.

The source graph carries these edges as source facts so the existing index
caches them and `inspect code` can audit them. Adding the field bumps the
private source-graph/cache identities. Because context slices expose source
graph evidence and their graph digest, they hard-cut to the next schema version
and publish the corresponding generated CodeAtlas-owned schema in the same
phase. No dual reader or old-digest compatibility branch remains.

Each applicable adapter also records one bounded coverage row even when it
finds no edge. The row names its language/framework and concrete surface,
states which literal source forms it can prove, reuses the existing
completeness value, and links any unsupported form to the existing typed
boundaries. This makes zero bindings an auditable result rather than a silent
claim of full analysis.

## Initial framework capability

The first shipped producer is deliberately `web`-only. Other exact HIF
surfaces remain unsupported until a language/framework adapter can prove the
same binding contract.

| Source form | Initial result | HIF action hypothesis |
|---|---|---|
| React intrinsic element with one literal `data-hif-key` or `data-hqa-target` and a direct resolvable click handler | Binding | `activate`; checkbox/radio also `toggle` |
| React intrinsic textbox/textarea with a direct resolvable input/change handler | Binding | `enter_text`, `replace_text` |
| React intrinsic select with a direct resolvable change handler | Binding | `select` |
| React intrinsic range/number input with a direct resolvable input/change handler | Binding | `set_value` |
| React intrinsic element with a direct resolvable focus handler | Binding | `focus` |
| Svelte intrinsic element with the same literal keys and direct `on:event={handler}` or property-event handler | Same bounded mappings | Same action set |
| Custom components, spread props, dynamic/computed keys, dynamic event names, inline closures, unresolved aliases, generated markup, or unsupported syntax | No guessed binding | Exact unresolved boundary |
| Plain JavaScript/TypeScript without React JSX, Rust, Python, native, desktop, or mock surfaces | No automatic surface binding | Explicit unsupported capability, while existing source/effect evidence remains unchanged |

Only intrinsic elements are eligible because CodeAtlas can relate those source
attributes to HQA's observed web reference keys. Component props do not prove
which runtime element receives a key or handler. `data-testid`, DOM `id`,
accessibility names, screenshots, selector hashes, and call hierarchy do not
become automatic hint keys in v1.

React uses the existing SWC parse. Svelte gets a bounded, framework-owned
markup fact extractor with exact quote/brace/tag limits and no regex-only
claim of full Svelte parsing. If the extractor cannot prove a complete literal
attribute and direct handler expression, it records an unsupported-syntax
boundary. Both adapters resolve handler names through the existing ECMAScript
module/symbol resolution; neither creates a second import resolver.

## Named state reads and writes

The initial artifact omits named state reads and writes. Current
`CallableEffect::AmbientState` and other coarse effect kinds do not identify a
state cell, and converting one into a name would fabricate identity.

A later addition is permitted only if the existing callable-evidence owner can
add one typed state-access fact with deterministic source targets, propagation,
and conformance across every applicable Rust, Python, JavaScript, TypeScript,
and Svelte adapter. That work must change `CallableContract` once and feed all
of its consumers. Source impact may never add a private state walker or infer a
state name from an effect kind.

## Bounds and failure behavior

Built-in ceilings are part of the producer contract; configuration or CLI may
only tighten them:

- 4,096 unique emitted hints;
- 16,384 surface-action binding edges;
- 64 entry symbols for one hint-action coordinate;
- dependency depth 16;
- 4,096 dependency nodes and 32,768 dependency edges per manifest;
- 4,096 effects and 4,096 unresolved boundaries per manifest;
- 8 MiB per source file read for source-target evidence; and
- 64 MiB of RFC 8785 canonical output bytes for either artifact.

Zero, an unlimited sentinel, a command-line increase, semantic duplication,
or resource-bound exhaustion fails before either output file is replaced.
Ordinary language incompleteness remains typed evidence in a valid manifest;
resource truncation never masquerades as completeness.

The producer reads each impacted file at most once when creating source-target
digests, keeps no source text in the artifact, makes zero target calls, uses no
network or ambient credentials, emits no absolute checkout path, and writes
only the explicit `--out` file through the existing atomic replacement owner.

## Existing-first check

Reuse:

- `SourceGraph`, `NodeId`, source edges, boundaries, and graph validation as the
  only source dependency evidence.
- `CallableContract` and `src/analysis/effects.rs` as the only callable/effect
  model and propagation owner.
- the source index and per-file parser facts as the only parse/cache path.
- `src/inspection/projection.rs` for deterministic bounded traversal, with a
  fail-closed complete-projection wrapper rather than another graph queue.
- TypeScript/SWC, Svelte script parsing, and ECMAScript resolution in their
  existing language owners.
- `src/commands/output.rs` for stdout and one-file atomic output.
- the existing `scan code` subject and the HQA-inventory precedent for an
  explicit consumer-native format.
- the external AgentSpeak schemas and CodeAtlas annotation registry.
- semantic-role-sibling analysis for consolidation dogfood.

New code is justified for the missing language-neutral binding fact,
framework-owned extraction, and source-impact projection. There is no existing
owner for those semantics. The proposal adds no new command, analyzer, graph,
effect walker, cache, schema copy, cross-version matcher, or runtime executor.

## Acceptance gates

- Reordered source files, framework attributes, hints, bindings, and graph
  insertion order produce byte-identical hints and source-impact artifacts.
- Cold parse, warm fact-cache, and warm graph-cache lanes emit identical graph,
  hints, manifest, and artifact digests.
- One conformance table covers every supported React and Svelte source form and
  every named unsupported form above.
- A project with no bindings distinguishes complete supported analysis,
  unsupported framework/language, and incomplete parsing through persisted
  coverage and boundary evidence; none of those states is inferred from an
  empty impact array.
- Shared handlers and multi-handler coordinates remain explicit; duplicate
  `(kind, key)` hints and duplicate/conflicting target-actions fail closed.
- Each emitted action validates as the exact external HIF action spelling and
  each emitted surface is the exact concrete HIF `Surface` spelling.
- Every source target validates through the complete external source-target
  schema, matches the exact file digest, and contains no fabricated UTF-16
  range.
- Every section digest and complete manifest digest re-derives from persisted
  RFC 8785 bytes. Mutating any persisted feature fails digest verification.
- Effects and unresolved boundaries equal the existing source graph and
  callable contract; a mutation that introduces a second effect traversal or
  hides an unknown boundary fails the focused test.
- No artifact field or producer path contains an HQA graph target ID, graph
  alignment, runtime state, runtime transition, or behavior-proof claim.
- Existing tree, Mermaid, and JSON `scan code` bytes remain unchanged apart
  from the separately versioned context-slice/source-graph evidence hard cut.
- The two HQA formats use one-file `--out`; the pre-v1 directory-shaped
  `scan code --out` residue is removed rather than extended.
- AgentSpeak convergence tests run against a pinned sibling checkout in CI,
  validate both artifacts and every cross-file `$ref`, and cannot silently skip
  in release validation.
- Projects with no supported framework binding stay within +10% cold time,
  +5% warm time, and +10% peak RSS on the recorded representative workspace;
  the measured dataset, file/symbol/byte scale, cache state, and raw results are
  retained with the benchmark evidence.
- Semantic-role-sibling dogfood compares the existing HQA inventory projection
  with every new source-impact module and either consolidates shared pure
  mechanisms or records decisive counterevidence.
- Full tests, warnings-denied Clippy, published-schema drift, package checks,
  self-dogfood, external-state audit, and `/workspace` generated-state hygiene
  pass before each phase commit.

## Phase 1: Typed binding evidence and framework adapters

Status: Blocked on the canonical Phase 16A continuation gates

LOC: +1,100-1,600 / -100-250

Verify: source-graph validation and cache tests, context-slice schema drift,
React/Svelte conformance, unsupported-boundary fixtures, cold/warm digest
parity, and focused `inspect code` evidence

```text
+ src/domain/surface_binding.rs
~ src/domain/mod.rs
~ src/domain/source_graph.rs
+ src/languages/typescript/parser/surface_actions.rs
~ src/languages/typescript/parser.rs
~ src/languages/typescript/parser/module_info.rs
+ src/languages/svelte/surface_actions.rs
~ src/languages/svelte/reachability.rs
+ src/languages/ecmascript/surface_actions.rs
~ src/languages/ecmascript/mod.rs
~ src/languages/ecmascript/collection.rs
~ src/languages/ecmascript/connections.rs
~ src/source_index/mod.rs
~ src/source_index/snapshot.rs
~ src/context_slice/model.rs
~ src/context_slice/slice.rs
~ src/published_schemas.rs
~ schemas/codeatlas-context-slice-v*.schema.json
+ tests/fixtures/source-impact/frameworks/*
```

This phase is independently useful: CodeAtlas can inspect exact source-level
surface bindings and their incompleteness before any external renderer ships.
It does not add an empty public field; the version hard cut lands with real
React and Svelte fixture evidence in the same commit.

## Phase 2: External manifests and `scan code` renderers

Status: Blocked on the accepted AgentSpeak source-impact schema; the current
hints and source-target contracts are already valid inputs

LOC: +1,200-1,800 / -150-300

Verify: manifest/digest unit tests, external-schema convergence, deterministic
goldens, CLI parser/output acceptance, source-target freshness, bound
exhaustion, and unchanged existing scan formats

```text
+ src/source_impact/mod.rs
+ src/source_impact/model.rs
+ src/source_impact/project.rs
+ src/source_impact/source_target.rs
+ src/source_impact/limits.rs
~ src/inspection/projection.rs
+ src/commands/source_impact.rs
~ src/commands/mod.rs
~ src/commands/output.rs
~ src/cli/scan.rs
~ src/main.rs
~ src/tests/interop.rs
~ tests/cli_contract.rs
+ tests/source_impact_cli.rs
+ tests/fixtures/source-impact/hqa-hints.generated.json
+ tests/fixtures/source-impact/source-impact.generated.json
~ README.md
~ docs/concepts/lexicon.md
~ spec/architecture/v0.1/README.md
```

The AgentSpeak files are not added under `schemas/` and are not registered as
CodeAtlas-owned public schemas. The convergence test resolves them from the
pinned sibling contract checkout and asserts their exact accepted identities.

## Phase 3: Dogfood, performance, and final consolidation

Status: Blocked on Phases 1 and 2

LOC: +250-450 / -100-250

Verify: bounded CodeAtlas self-dogfood, semantic-sibling review, cold/warm
performance budgets, complete external HQA fixture handoff, package audit, and
repository/generated-state hygiene

```text
~ codeatlas.json
~ tasks/check-self.js
~ README.md
~ docs/concepts/lexicon.md
~ proposals/codeatlas-source-impact.md
~ proposals/codeatlas-fuzz-performance.md
~ src/source_impact/*
~ tests/fixtures/source-impact/*
```

The hardening pass removes any temporary parser branch, duplicated mapping
table, private digest helper, compatibility output behavior, or stale schema
fixture found during implementation. Final architecture is intentionally
higher-LOC because it adds two externally consumed artifacts and two real
framework adapters; it does not retain an old and new implementation in
parallel.

Total LOC: +2,550-3,850 / -350-800

## Layman's wins

- HQA can learn which interactions are worth retesting after source changes
  without pretending source code proves runtime behavior.
- React and Svelte are mapped once through CodeAtlas's real source graph and
  effect model, not through a second pile of parsers and call graphs.
- Every impact explains which files, symbols, dependencies, effects, and blind
  spots produced it; a mystery hash is never the only evidence.
- Runtime target IDs stay fresh and opaque, while hints, source impact, and
  cross-version alignment remain three visibly separate concerns.
