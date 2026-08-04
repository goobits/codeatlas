# Evidence lifecycle CLI

Status: Accepted; implementation pending

Decision scope: Public command grammar and artifact flow plus explicit test-gate
and PostgreSQL no-hidden-execution behavior; no new static analyzer

Depends on: None

Unblocks: Execution/HTTP, code fuzzing, PostgreSQL fuzzing, and performance
evidence proposals

## Decision

CodeAtlas will use one verb-subject grammar. The four evidence-lifecycle verbs
have one meaning everywhere they apply:

- `scan`: gather current evidence.
- `check`: apply rules and gates to current evidence.
- `baseline`: save a canonical comparison artifact.
- `diff`: compare current or supplied evidence with a canonical artifact.

The `tests` noun-group dissolves into this grammar. The architecture-only
top-level verbs `compile` and `observe` also disappear. Their internal engine
operations and artifact names remain valid; only the public command shape
changes.

The migration is a hard cut. CodeAtlas is pre-v1, so old and new commands never
ship together and no aliases, forwarding wrappers, deprecation messages, or
compatibility schemas remain.

## Deliberate behavior changes

This is not routing-only in two places:

- `check tests` turns the existing witness evidence into an explicit policy
  command. Unwitnessed public API gates return exit 1; complete non-gating
  evidence still renders, and `--gates-only` changes rendering only. Acceptance
  tests pin both report contents and exit codes for witnessed, unwitnessed,
  declared-only, and unknown cases.
- PostgreSQL baseline/diff stop performing a fresh live replay and instead load
  a versioned `PostgresObservation` produced by `test postgres`. Parser and
  zero-live-call behavior can land here, but the observation payload and
  execution integration are owned and completed by the PostgreSQL proposal
  after the kernel artifact contract exists.

## Options considered

| Option | Fit | Tradeoff | Verdict |
|---|---|---|---|
| Preserve the current mixed grammar | Zero migration | Retains `test`/`tests`, architecture-only verbs, and inconsistent flags | Reject |
| Add new commands beside old commands | Staged consumer migration | Creates legacy surface and two public vocabularies | Reject |
| Hard-cut to verb-subject grammar | Clean pre-v1 contract | Requires coordinated caller, test, doc, and artifact updates | Adopt |
| Force a complete rectangular verb/subject matrix | Maximum visual symmetry | Adds commands before their evidence models exist | Reject |

## Stable subject matrix

The accepted destination is intentionally sparse. A missing cell means the
operation has no proven product meaning yet.

| Verb | Subjects |
|---|---|
| `scan` | `code`, `http`, `postgres`, `architecture`, `tests`, `performance` |
| `check` | `code`, `http`, `postgres`, `architecture`, `tests`, `performance` |
| `baseline` | `code`, `http`, `postgres`, `architecture`, `performance` |
| `diff` | `code`, `http`, `postgres`, `architecture`, `performance` |
| `usage` | `code`, `tests`; future `http` and `postgres` only after consumer evidence exists |
| `inspect` | `code`, `architecture`; future `http` and `postgres` only after bounded graph evidence exists |
| `lexicon` | `code`; future domain subjects and `repository` cross-domain analysis |
| `docs` | `code`; future `http` and `postgres` after description contracts exist |
| `init` | `postgres`; expand only where discovery can propose useful config without inventing owner intent |
| `fuzz` | `code`, `http`, `postgres` |
| `test` | `postgres` |

`performance` subjects are introduced by the performance proposal, and
`fuzz code|postgres` by their domain proposals. This proposal reserves their
grammar but does not advertise them before their implementations ship.

## Tests become a subject

The current implementation remains in `src/commands/testing.rs`; only routing
changes. There is no second inventory, impact, or witness implementation.

| Current | Stable | Meaning |
|---|---|---|
| `tests inventory` | `scan tests` | Current test contexts, scripts, runners, and declarations |
| `tests impact --changed ...` | `usage tests --changed ...` | Tests known to consume changed paths |
| `tests witnesses` | `check tests` | Public API witness policy and gateable unwitnessed APIs |

`check tests` gains explicit gate semantics. Its complete report still includes
witnessed, declared-only, unwitnessed, and unknown evidence. `--gates-only`
filters rendering; it does not change the underlying decision.

## Architecture becomes a subject

| Current | Stable | Meaning |
|---|---|---|
| `observe architecture` | `scan architecture` | Produce current implementation-binding evidence |
| `check architecture source` | `check architecture` | Validate declarations and source conformance |
| `compile architecture` | `baseline architecture` | Save the canonical compiled graph and lock |
| `check architecture observation` | `diff architecture` | Compare an observation with the exact saved graph |

The internal functions may still be named `compile` and `observe`; those words
describe real architecture operations. The public lifecycle is what becomes
uniform.

`scan architecture` retains `--observation-id` unless a separately versioned
artifact change defines a deterministic content-addressed replacement.
`diff architecture` likewise retains `--conformance-id` unless its artifact
contract formally derives it. Artifact identity is not silently discarded for
CLI neatness.

`baseline architecture --mode governing|review` writes the chosen mode into
the artifact. A review baseline is canonical review evidence but is
non-governing. `diff architecture` rejects it for governing conformance unless
an explicit review operation is later specified.

Policies affect observation conformance. `--policy` is not added to
`check architecture` until source-conformance policy semantics exist.

## Performance uses the lifecycle

There is no new top-level `observe` verb:

```text
scan performance       -> create a planned PerformanceObservation
check performance      -> apply configured budgets to an observation
baseline performance   -> save a reviewed observation as the canonical baseline
diff performance       -> compare an observation with a baseline
```

An **observation** is subject-qualified evidence captured from a particular
current state or run. It is a shared meaning, not a cross-domain schema or
envelope. `ArchitectureObservation` and `PerformanceObservation` use that same
meaning with domain-owned artifacts. A **measurement** is one numeric
performance sample. This removes the former vocabulary collision without
renaming the established architecture artifact.

## Output and policy flags

Flags are normalized only when their semantics are actually shared:

- `--out` always names one file. `scan code` no longer treats it as a directory.
- No `--out-dir` is retained while a command produces one artifact.
- `--format` exists only when a subject has at least two real renderers.
- Canonical baselines use their fixed versioned artifact format and do not
  pretend to support arbitrary display formats.
- `--gates-only` belongs to `check`; it is removed from `usage`.
- `--exact` belongs to `diff` only after additions and changes have a defined
  exact-policy meaning for that subject.
- `--against` belongs to `diff`; `check http --against` is removed as duplicate
  comparison behavior.
- Invalid combinations use parser-level requirements/conflicts rather than
  runtime exit-2 branches where Clap can express the contract exactly.

Common execution flags are structural rather than copied prose.
`src/cli/execution.rs` owns flattened `ExecutionLimitArgs` and `FuzzLimitArgs`;
the kernel proposal owns their authoritative semantics and complete flag list.
Every executing subject reuses those structs with `#[command(flatten)]`, while
domain CLI modules add only selectors and genuine domain-specific ceilings.

`usage code --scope all|public` should support the common `--workspace`,
`--format`, and `--out` contract. `--consumer-root` is valid only for public
usage and is enforced structurally. The current implementation restriction is
not preserved as product policy.

## Runtime evidence is explicit

Commands that can cause live or measured execution do not hide that work in a
baseline or diff:

```text
test postgres --target ...                         # preview
test postgres --plan plan_ABC --execute \
  --out postgres-observation.json
baseline postgres --observation postgres-observation.json
diff postgres --against baseline.json --observation postgres-observation.json

scan performance --target ...                 # preview
scan performance --plan plan_ABC --execute    # observation
baseline performance --observation observation_ABC
diff performance --against baseline_ABC --observation observation_DEF
```

Code and HTTP baselines remain source/contract evidence and may gather their
read-only current state directly. Architecture baselines compile declarations
without running target code. Live PostgreSQL replay has one owner: `test
postgres`.

The PostgreSQL plan/execute form is the final program contract and lands with
the PostgreSQL execution-kernel integration. This grammar proposal can remove
hidden live work from baseline/diff first; it does not manufacture a temporary
PostgreSQL plan implementation before the shared kernel exists.

All plan, observation, baseline, and reproducer arguments use the kernel's one
typed `ArtifactRef` contract. Managed IDs and explicit exported files therefore
have identical schema/digest verification instead of domain-specific lookup
rules.

## Cross-domain additions are separate capabilities

The following accepted destinations are owned by the separate
[`codeatlas-subject-evidence-parity.md`](codeatlas-subject-evidence-parity.md)
proposal rather than this grammar-only migration:

- `usage http`: known repository callers and tests for routes, with completeness
  evidence and no claim that an externally consumed route is unused.
- `usage postgres`: static query touches for tables and columns, with dynamic
  SQL incompleteness visible.
- `inspect http|postgres`: bounded projections after operation/query dependency
  graphs exist.
- `docs http|postgres`: generated only from OpenAPI descriptions, database
  comments, and explicit config; never invented prose.
- `lexicon repository --subjects code,http,postgres`: cross-domain concepts
  such as `User`, `/users`, and `users`, without pretending `lexicon code` owns
  all subjects.
- Generalized `init`: only for subjects where discovery can produce a truthful,
  useful configuration proposal.

Each addition has its own evidence contract and acceptance tests there.
Symmetry is a design diagnostic, not authorization to add an empty command.

## Migration contract

The hard cut updates all of these together:

- CLI enums, help, parsing tests, and positive command examples.
- Negative tests for removed `tests`, `compile`, and `observe` commands.
- Integration tests and package/task callers.
- README and architecture specification examples.
- Embedded `generationCommand` values in architecture artifacts.
- Generated examples and any canonical artifact digests affected by command
  metadata.
- The fuzz/performance proposal suite.

If a serialized public artifact changes shape, bump its schema version. If only
the canonical generation command or algorithm changes, bump the owning
algorithm/tool identity and regenerate exact fixtures. Do not bump versions by
habit or leave stale identities.

## Existing-first check

Reuse these owners:

- `src/commands/testing.rs` and `src/testing/*` for all test evidence.
- `src/commands/architecture/*` and `src/architecture/*` for compile,
  observation, conformance, and provider logic.
- `src/commands/output.rs` for file/stdout behavior.
- Existing verb subject enums under `src/cli`.
- `tests/testing_cli.rs`, CLI parser tests, architecture unit tests, and
  generated architecture examples.

A new architecture CLI acceptance test is justified because the current suite
does not prove the complete scan -> baseline -> diff public lifecycle.
`src/cli/execution.rs` is the one justified new routing owner because no current
module owns reusable execution/fuzz limit arguments.

## Phase 1: Hard-cut the testing grammar

Status: [x] Complete

Execution checkpoint:

- [x] Add `tests` as a subject under scan/check/usage using the existing
  testing command owner.
- [x] Give `check tests` gate decisions and `--gates-only` rendering semantics.
- [x] Remove the top-level `tests` parser/module with no alias or forwarding
  branch.
- [x] Update CLI/integration tests and the self-audit task to the stable forms.
- [x] Run focused testing/CLI/self-dogfood verification, record evidence here,
  and commit the coherent hard cut.

Verified checkpoint (2026-08-04):

- `cargo test --locked --jobs 1 --test testing_cli`: 3 passed. One versioned
  report contains witnessed, declared-only, unwitnessed, and unknown cases;
  only unwitnessed evidence gates and survives `--gates-only` rendering.
- `cargo test --locked --jobs 1 --bin codeatlas testing::`: 7 passed, including
  the four-class exit-policy table and the consolidated diagnostic renderer.
- Binary CLI parser tests: 3 passed; `tests` is rejected. Full
  `tests/cli_contract.rs`: 15 passed.
- `node tasks/check-self.js` passed all seven private reports. Artifacts remain
  mode-restricted below the external Cargo target at
  `/tmp/codeatlas-xdo-cache.hTn1Nk/cargo-target/codeatlas-self-audit`; the
  checkout received no generated state.

LOC: +220-350 / -160-260

Verify: New commands preserve `codeatlas.testing/v1` report behavior; `tests`
is rejected; inventory, impact, witness, Git-change, workspace, and output
contracts pass; `check tests` exit codes and `--gates-only` rendering are pinned
for every witness class.

```text
~ src/cli/mod.rs
~ src/cli/scan.rs
~ src/cli/check.rs
~ src/cli/usage.rs
- src/cli/tests.rs
~ tests/testing_cli.rs
~ tests/cli_contract.rs
```

## Phase 2: Hard-cut the architecture lifecycle

Status: [x] Complete

Execution checkpoint:

- [x] Route architecture evidence through scan/check/baseline/diff and remove
  the top-level compile/observe parser branches.
- [x] Load and validate the exact saved compilation for diff; reject review
  baselines as governing evidence without recompiling source declarations.
- [x] Update generation-command metadata and canonical architecture docs and
  examples to the stable lifecycle vocabulary.
- [x] Add end-to-end lifecycle coverage for deterministic IDs, governing versus
  review mode, exact artifact use, and removed-command rejection.
- [x] Run focused architecture/CLI verification and external dogfooding, record
  the checkpoint here, and commit the phase.

Verified checkpoint (2026-08-04):

- `cargo test --locked --jobs 1 --bin codeatlas architecture::`: 38 passed,
  including strict saved-compilation loading and digest-drift rejection.
- `cargo test --locked --jobs 1 --test architecture_cli`: 3 passed. The public
  lifecycle is deterministic, review baselines cannot govern, removed commands
  and noun groups exit 2, IDs remain exact, and diff succeeds after the current
  declaration source is deleted because it consumes the saved graph.
- Binary parser tests: 3 passed. Full `tests/cli_contract.rs`: 15 passed.
- `node tasks/check-self.js` passed all seven private reports from the external
  target. `cargo fmt --check` and `git diff --check` passed; no generated state
  appeared under the checkout.

LOC: +350-600 / -200-350

Verify: Scan observations and baseline graphs remain canonical; governing and
review modes cannot be confused; artifact IDs remain exact; new commands pass;
old `compile` and `observe` commands are rejected.

```text
~ src/cli/mod.rs
~ src/cli/scan.rs
~ src/cli/check.rs
~ src/cli/baseline.rs
~ src/cli/diff.rs
~ src/cli/inspect.rs
~ src/cli/architecture.rs
~ src/commands/architecture/compile.rs
~ src/commands/architecture/observe.rs
~ src/commands/architecture/conform.rs
~ src/commands/architecture/source_check.rs
+ src/architecture/baseline.rs
~ src/architecture/compiler.rs
~ src/architecture/graph.rs
~ src/architecture/observation/mod.rs
~ src/architecture/conformance.rs
+ tests/architecture_cli.rs
~ README.md
~ spec/architecture/v0.1/spec/atlas-architecture-dsl-v0.1.md
~ spec/architecture/v0.1/examples/conformance/architecture-conformance.generated.json
```

## Phase 3: Normalize outputs, gates, diffs, and live artifacts

Status: [~] In progress; PostgreSQL artifact completion waits for the kernel
and PostgreSQL observation contract

LOC: +300-500 / -150-250

Verify: `--out` is always one file; invalid combinations fail during parsing;
checks own gate filtering; every shipped `--exact` has tested policy semantics;
PostgreSQL baseline/diff make zero live calls.

```text
+ src/cli/execution.rs
~ src/cli/scan.rs
~ src/cli/check.rs
~ src/cli/baseline.rs
~ src/cli/diff.rs
~ src/cli/usage.rs
~ src/cli/postgres.rs
~ src/cli/test.rs
~ src/commands/mod.rs
~ src/commands/output.rs
~ src/commands/dead_code.rs
~ src/commands/http.rs
~ src/commands/postgres.rs
~ tests/cli_contract.rs
~ tests/http_cli.rs
~ tests/postgres_cli.rs
```

## Phase 4: Remove migration residue and sync canonical references

Status: [ ] Not started

LOC: +100-200 / -100-200

Verify: Repository search finds no old public command, alias, stale help,
generation command, compatibility branch, duplicate output option, or outdated
proposal reference; full required checks pass with external generated state.

```text
~ src/cli/mod.rs
~ README.md
~ proposals/codeatlas-fuzz-performance.md
~ proposals/codeatlas-execution-kernel-http-fuzz.md
~ proposals/codeatlas-code-fuzzing.md
~ proposals/codeatlas-postgres-fuzzing.md
~ proposals/codeatlas-performance-evidence.md
~ proposals/codeatlas-cost-guided-search.md
~ tests/cli_contract.rs
```

The final CLI migration is net smaller or approximately flat in routing code.
Any net growth comes from missing acceptance coverage and explicit artifact
loading, not from retaining two grammars.

Total LOC: +970-1,650 / -610-1,060

## Layman's wins

- Commands become predictable: gather, check, save, or compare any evidence.
- The confusing `test` versus `tests` and architecture-only verbs disappear.
- Expensive live work cannot hide inside baseline or diff commands.
- No old command surface remains to maintain.
