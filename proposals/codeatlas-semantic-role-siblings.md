# Semantic-role sibling evidence

Status: Accepted advisory child; Phase 2 complete, Phase 3 pending

Decision scope: Deterministic evidence for conceptually duplicated
implementations across explicitly configured sibling packages or modules

Depends on:

- [`codeatlas-evidence-lifecycle-cli.md`](codeatlas-evidence-lifecycle-cli.md)
- [`codeatlas-published-schemas.md`](codeatlas-published-schemas.md)
- [`codeatlas-structured-callable-evidence.md`](codeatlas-structured-callable-evidence.md)

Independent of: execution kernel, sandbox, fuzzing, and live target calls

## Decision

CodeAtlas will extend `lexicon code` with deterministic semantic-role sibling
evaluations. The analyzer asks whether implementations in owner-declared sibling
areas appear to perform the same conceptual job and therefore deserve human
consolidation review.

It never scans arbitrary package pairs. Strict `codeatlas.json` comparison sets
name the sibling members whose parallelism is intentional enough to compare.
The analyzer reports discrete corroborations and counterevidence with exact
targets. It never emits a similarity percentage, never claims two bodies are
equivalent, and never gates a build.

The feature is advisory-only forever. A finding can justify inspection or a
TypeMill refactor chosen by an owner; it cannot prove deletion, replacement, or
behavioral equivalence.

## Why this belongs with lexicon evidence

Current lexicon analysis already finds exact names, structural shapes,
callable-shape candidates, and sourced conceptual relationships. Semantic-role
siblings are the next bounded question: different names or packages may express
the same role even when their raw signatures are not identical.

This does not become another top-level command or a second code-check report.
`lexicon code` remains the one owner for conceptual overlap. Structured callable
contracts supply semantic input; a focused sibling analyzer supplies one new
report section. `check code` remains free of an advisory that can never gate.

## Configuration contract

Only explicitly declared sets are analyzed:

```json
{
  "lexicon": {
    "semantic_siblings": {
      "comparison_sets": [
        {
          "id": "language_adapters",
          "purpose": "compare language adapter responsibilities",
          "members": [
            { "id": "ecmascript", "paths": ["src/languages/ecmascript"] },
            { "id": "python", "paths": ["src/languages/python"] },
            { "id": "rust", "paths": ["src/languages/rust"] },
            { "id": "typescript", "paths": ["src/languages/typescript"] }
          ],
          "maximum_nominations": 200
        }
      ]
    }
  }
}
```

Rules:

- Set and member IDs are unique, nonblank canonical identifiers.
- A set has at least two members; each member has at least one exact
  repository-relative file or directory path.
- Paths are normalized, must exist inside the analyzed repository root, and do
  not use globs, traversal, backslashes, absolute paths, or symlink escape.
- Members in one set may not overlap. Sets may overlap only when their IDs and
  purposes differ explicitly; each report retains the set ID.
- `maximum_nominations` is finite and may only tighten a built-in ceiling.
- Unknown fields, duplicate roots, unowned symbols, and a comparison set that
  resolves fewer than two nonempty members fail deterministically.

The config declares where comparison is meaningful. It does not declare that
members are duplicates and cannot suppress counterevidence.

## Tier 1 evidence model

Tier 1 uses no callable-body parser. It reuses structured facts already owned by
CodeAtlas:

- callable kind, receiver/owner role, ordered parameter roles, result/error
  roles, and constructibility;
- implemented trait/interface/declared-contract identity;
- direct and propagated effect evidence;
- exact named model types and record field roles;
- source-graph producers, consumers, and call/dependency neighborhoods;
- export/public reachability and package ownership;
- canonical identifier action/object tokens and configured lexicon concepts.

The analyzer first creates bounded nominations, then evaluates every nomination
through fixed corroboration and counterevidence checks. It never computes all
cross-member symbol pairs.

## Nomination is not corroboration

A pair may be nominated by one of these exact relationships:

- implementations of the same declared trait/interface/contract;
- the same canonical action-object role in different members;
- the same named model role with parallel producer/consumer placement;
- a configured canonical concept attached to both targets.

The nomination reason cannot also be counted as evidence that the
implementations are duplicated. Most importantly, a trait or interface's own
contract is never corroboration: two implementations naturally share the
parameters and result that the trait requires.

For same-contract nominations, all facts inherited solely from that declaration
are excluded. Only member-local implementation evidence can corroborate the
pair.

## Discrete corroboration

Corroboration kinds are named, counted once each, and carry exact supporting
targets:

- `implementation_role_match`: independently declared parameter/result roles
  beyond a shared contract obligation.
- `effect_set_match`: the same evidenced effect kinds and capability needs.
- `model_role_match`: the same named input/output model roles or field-role
  projection.
- `producer_position_match`: parallel upstream producer relationships.
- `consumer_position_match`: parallel downstream consumer relationships.
- `dependency_role_match`: the same independently selected dependency role.
- `lifecycle_role_match`: the same evidenced create/read/update/cleanup stage.

An evaluation needs at least two independent corroboration kinds to become a
review candidate. The report contains an integer corroboration count and the
ordered evidence list. It contains no score, probability, distance, or
percentage.

## Mandatory counterevidence

Every nominated pair runs the same counterevidence checklist and records each
result as `present`, `absent`, or `unknown`:

- conflicting or unknown effects;
- different authority/security boundaries;
- different lifecycle or cleanup ownership;
- incompatible result/error semantics;
- disjoint producer or consumer roles;
- different externally owned protocol obligations;
- distinct concepts declared in the lexicon policy;
- incomplete graph/type evidence that prevents comparison.

A decisive present counterevidence item produces `separate_by_evidence`, not a
duplication candidate. Required unknown evidence produces `inconclusive`.
Otherwise two or more independent corroborations produce `review_candidate`.
No evidence state is collapsed into “low similarity.”

## Report contract

Lexicon output gains a bounded, deterministically ordered section:

```text
semantic_sibling_analysis
  comparison_sets[]
    id
    members[]
    nominations_considered
    evaluations[]
      targets[2]
      nomination
      corroborations[]
      counterevidence_checks[]
      corroboration_count
      disposition
```

Evaluations sort by comparison-set ID and exact target IDs. Evidence sorts by
kind then source target. The report includes all bounded nominated evaluations,
not just flattering candidates, so reviewers can see why a tempting pair was
kept separate or remained unknown.

Adding this section requires the next lexicon schema version after structured
callable evidence (expected v5), its generated published schema, and exact text
and JSON renderer tests. No gate count or exit-code field is added.

## Complexity and determinism

- Path membership is resolved once against the shared source snapshot.
- Nomination indexes use declared contract ID, canonical role, named model, and
  graph-role keys; they do not compare every symbol body to every other body.
- Per-key cross-member expansion and total nominations obey explicit ceilings.
- Oversized keys produce an honest bounded diagnostic and aggregate omitted
  count; they are not sampled nondeterministically.
- Stable maps/sets and target IDs determine all output ordering.
- Identical config and source evidence produce byte-identical results.
- No cache is added unless the source-index owner can name its key,
  invalidation, byte ceiling, and telemetry. Tier 1 should initially reuse the
  current snapshot without a new cache.

## Tier 2 body skeleton is deferred

Body-skeleton comparison is not part of this acceptance unit. It would add
language-specific control/data-flow normalization, much higher false-positive
risk, and another cache/complexity surface.

If Tier 1 cannot distinguish a named, checked-in conformance fixture that
requires implementation-body facts, the shortfall is recorded with exact
counterexample targets. A separate proposal may then authorize the smallest
language-specific skeleton evidence needed. “More findings would be nice” is
not a continuation gate, and no Tier 2 placeholder ships now.

## Dogfooding

CodeAtlas declares two real comparison sets:

- language adapters under `src/languages/*`;
- HTTP source detectors under `src/http/source/*`.

The dogfood review classifies every emitted evaluation as actionable review,
parallel-by-design evidence, or analyzer defect. A finding is not suppressed
merely because the current duplication is intentional; the counterevidence or
lexicon distinct-concept policy should explain that decision.

Any chosen refactor remains outside this analyzer. CodeAtlas stays read-only,
and stable Mill is used only if it advertises the exact refactor capability.

## Existing-first check

Reuse:

- Structured callable/effect evidence and source graph.
- Existing lexicon identifier grammar, concepts, `distinct_from` policy,
  symbols, renderers, and schema/version owner.
- Current repository path normalization and project/package ownership.
- Existing inspection targets for follow-up evidence.

New code is limited to strict comparison-set config, nomination indexes,
evaluation rules, and report models. There is no generic clone detector,
embedding model, fuzzy score, universal AST, source-body normalizer, new command,
or gate.

## Acceptance gates

- Only configured nonoverlapping sibling members are compared.
- Fixture ordering changes do not change output bytes.
- Same-trait implementations receive zero corroboration from the trait's own
  required contract.
- Every evaluation includes the full mandatory counterevidence checklist.
- Two independent corroborations are required; counts never become similarity
  percentages.
- Decisive divergence yields `separate_by_evidence`; incomplete evidence yields
  `inconclusive`.
- Nomination and output ceilings fail or report omission deterministically.
- Lexicon schema, published schema, JSON/text renderers, and stats agree.
- Exit code is always non-gating for these evaluations.
- CodeAtlas dogfood covers both declared corpora and creates no generated state
  under the checkout.

## Phase 1: Config and evidence contract

Status: [x] Complete

Measured LOC: +1,247 / -26 authored; +1,205 / -848 generated schema

Verify: Strict comparison sets, path confinement, nomination/counterevidence
enums, report ordering, bounds, and same-contract evidence exclusion pass.

```text
+ src/lexicon/semantic_siblings/model.rs
+ src/lexicon/semantic_siblings/mod.rs
+ src/config/semantic_siblings.rs
~ src/config/lexicon.rs
~ src/config/mod.rs
~ src/lexicon/mod.rs
~ src/lexicon/model.rs
~ src/lexicon/analyze.rs
~ src/published_schemas.rs
~ src/http/target/tests.rs
~ tests/cli_contract.rs
~ schemas/codeatlas-lexicon-*.schema.json
```

Checkpoint evidence:

- Three config tests prove canonical ordering, finite ceilings, strict nested
  fields, overlap refusal, missing-path refusal, and target-observed symlink
  confinement.
- Three model tests prove shared-contract exclusion, the complete ordered
  counterevidence checklist, deterministic report order, and exact nomination
  accounting.
- Lexicon v5 is the sole registered and checked-in schema. Warning-denying
  Clippy, focused schema/CLI tests, and all seven static self-dogfood commands
  pass from external build and artifact roots.
- The Phase 1 report intentionally contains an empty sibling section. Phase 2
  consumes the already resolved sets and populates the same v5 contract without
  another schema transition.

## Phase 2: Tier 1 analyzer and renderers

Status: [x] Complete

Measured LOC: +1,765 / -50 authored code and fixtures

Verify: Bounded nominations, discrete corroboration, mandatory counterevidence,
all three dispositions, JSON/text output, lexicon v5 population, and
non-gating CLI behavior pass the conformance fixtures.

```text
+ src/lexicon/semantic_siblings/nominate.rs
+ src/lexicon/semantic_siblings/evaluate.rs
+ src/lexicon/semantic_siblings/tests.rs
+ tests/fixtures/semantic_siblings/
~ src/lexicon/analyze.rs
~ src/outputs/lexicon.rs
~ src/commands/lexicon.rs
~ schemas/codeatlas-lexicon-*.schema.json
~ tests/cli_contract.rs
```

Checkpoint evidence:

- Nomination indexes expand only configured cross-member keys, stop at the
  per-key and comparison-set ceilings before evaluation, and report exact
  omitted counts without sampling or body parsing.
- Every evaluated pair carries the ordered eight-check counterevidence set;
  shared-contract facts cannot corroborate, and the model derives all three
  dispositions without a score or gate.
- Graph boundaries are indexed once before fact collection rather than scanned
  once per callable. An ordinary unconfigured `lexicon code` run still avoids
  source-graph construction entirely.
- Nine focused config/model/analyzer tests pass. The real Rust CLI fixture
  emits 43 bounded evaluations (3 review candidates, 22 separated by evidence,
  18 inconclusive, 0 omitted), including exact action-role examples for every
  disposition.
- Reversing configured member order produces byte-identical JSON; every fixture
  evaluation contains eight checks; text and JSON renderers agree; the command
  remains exit-zero; lexicon v5 schema drift and warning-denying Clippy pass.

## Phase 3: CodeAtlas dogfood and consolidation

Status: [ ] Not started

LOC: +150-250 / -50-120

Verify: Both CodeAtlas comparison sets are reviewed; actionable shared helpers
are rehomed only through their real owner; intentional parallels carry visible
counterevidence; no duplicate analyzer/helper/schema or Tier 2 residue remains.

```text
~ codeatlas.json
~ docs/concepts/lexicon.md
~ proposals/codeatlas-semantic-role-siblings.md
~ proposals/codeatlas-fuzz-performance.md
~ tasks/check-self.js
~ src/lexicon/semantic_siblings/*
~ tests/fixtures/semantic_siblings/
```

Total LOC: +850-1,350 / -100-250

## Layman's wins

- CodeAtlas can point out when sibling packages appear to solve the same job in
  parallel, even if names and signatures differ.
- It shows concrete reasons for and against consolidation instead of an opaque
  similarity score.
- Intentional parallel adapters stay defensible because counterevidence is part
  of every result.
- The feature stays read-only, bounded, deterministic, and advisory.
