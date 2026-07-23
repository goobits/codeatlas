# Requirements Traceability

Status: accepted Phase 7 traceability evidence

This matrix traces the Phase 0 through Phase 6 handoff into the specification
accepted under `../../../spec/architecture/v0.1/` and its executable checks.
The acceptance and later production authorization are recorded separately.

## Scope and authority

| Requirement | Specification owner | Mechanical evidence |
| --- | --- | --- |
| Design package only, no production compiler in Phase 7 | `README.md`, `ARCHITECTURE_IMPACT_CHECK.md` | Private crate isolation and parent package checks |
| Code Atlas remains independently managed | `ARCHITECTURE_IMPACT_CHECK.md` | No production dependency or public command added |
| Restricted YAML is the sole editable declared-architecture authority | `README.md`, `spec/atlas-architecture-dsl-v0.1.md` | `refinements::codeatlas_json_is_configuration_not_architecture` |
| Generated observations, reports, and manifests are non-editable evidence | `spec/compiler-semantics.md` | `examples::committed_generated_examples_are_current` |
| Journal OS and runtime coordination remain out of scope | `README.md`, `DECISIONS.md` | Design-boundary diff and full parent checks |

## Documents, decisions, and identity

| Requirement | Specification owner | Mechanical evidence |
| --- | --- | --- |
| Exactly six v0.1 document kinds | `spec/atlas-architecture-dsl-v0.1.md` | Six Draft 2020-12 schemas and example validation |
| Decision, approval, and change type are independent axes | `DECISIONS.md`, `spec/versioning-and-migration.md` | `refinements::change_decision_and_approval_axes_are_independent` |
| Governing and supporting authority are typed | `spec/atlas-architecture-dsl-v0.1.md` | semantic authority validation and policy tests |
| Stable artifact identity is separate from repository location | `spec/atlas-architecture-dsl-v0.1.md` | schema and semantic validation |
| Stable semantic IDs are fully qualified and never reused | `spec/versioning-and-migration.md` | identifier and retired-ID tests |
| Every stable ID has one declaring module | `spec/compiler-semantics.md` | duplicate and imported-ID graph tests |

## Module and vocabulary model

| Requirement | Specification owner | Mechanical evidence |
| --- | --- | --- |
| One recursive module grammar | `spec/atlas-architecture-dsl-v0.1.md` | module schema and examples |
| Imports are local, exact, digest-pinned, and acyclic | `spec/compiler-semantics.md` | graph import tests and import-path tests |
| Source locators cannot escape allowed roots | `spec/security-and-resource-limits.md` | traversal, network, and symlink tests |
| Declarations are private unless exported | `spec/compiler-semantics.md` | cross-module visibility tests |
| Core vocabulary is closed, typed, and digest-pinned | `vocabularies/core.v0.1.atlas.yaml` | vocabulary self-validation and semantic invalid cases |
| Unknown kinds, attributes, predicates, adapters, and rules fail | vocabulary and schemas | invalid semantic integration tests |
| Object kinds and predicates enforce typed contracts | vocabulary and compiler semantics | subject, object, attribute, and selector tests |
| Capabilities, providers, packages, and consumers remain distinct | core vocabulary and examples | Workshop and Code Atlas example validation |

## Graph, policy, and change semantics

| Requirement | Specification owner | Mechanical evidence |
| --- | --- | --- |
| Only active accepted declarations govern | `spec/compiler-semantics.md` | governing and review graph tests |
| Proposed and unresolved declarations may enter only a review graph | `spec/compiler-semantics.md` | Tabby example and review digest tests |
| Rejected, superseded, and retired declarations do not govern | `spec/versioning-and-migration.md` | graph status and retirement tests |
| Persisted review graphs have reproducible identity | `spec/canonicalization-and-hashing.md` | `persisted_review_graph_digest_is_reproducible` |
| Policies and exceptions do not change architecture identity | `spec/conformance-semantics.md` | policy and governing digest tests |
| Exceptions are exact, scoped, approved, visible, and time-bound | policy schema and conformance semantics | policy unit tests and invalid semantic cases |
| Relevant closure changes make exceptions stale | `spec/conformance-semantics.md` | `relevant_closure_changes_make_exceptions_stale` |
| `asOf` controls expiration deterministically | `spec/conformance-semantics.md` | `recorded_as_of_controls_expiration` |
| Owner direction may support but cannot promote a proposal | `DECISIONS.md` | `owner_direction_does_not_promote_a_proposal` |
| Architecture changes use an exact base and expected effects | `spec/versioning-and-migration.md` | change schema and example validation |
| Architecture changes remain proposal and audit records and never enter or reconstruct an architecture graph | main and versioning specifications | `architecture_changes_never_compile_as_architecture_graphs` |
| Replacements require retirement and removal plans | `spec/versioning-and-migration.md` | invalid change semantic cases |

## Parsing, canonicalization, and digests

| Requirement | Specification owner | Mechanical evidence |
| --- | --- | --- |
| Restricted YAML excludes aliases, tags, interpolation, floats, and duplicate keys | `spec/security-and-resource-limits.md` | parser unit tests and ten invalid fixtures |
| Parsing and graph evaluation use explicit resource limits | security specification | byte and depth limit tests |
| Canonicalization is deterministic and key-order independent | `spec/canonicalization-and-hashing.md` | canonicalization unit tests |
| The complete typed digest registry uses domain separation | `DECISIONS.md`, hashing specification | distinct-domain and typed-digest tests |
| Observation content identity excludes volatile capture time | hashing and conformance specifications | generated artifact construction and conformance tests |
| The manifest is stable, sorted, generated, and excludes itself | hashing and security specifications | generator `--check` and clean-checkout reproduction |

## Observation and conformance

| Requirement | Specification owner | Mechanical evidence |
| --- | --- | --- |
| Observations contain source commit, generator, locations, coverage, mode, and digests | observation schema and conformance semantics | generated observation and invalid semantic tests |
| Coverage distinguishes complete, partial, unsupported, and unknown | `spec/conformance-semantics.md` | generated example and conformance unit tests |
| Unsupported coverage cannot prove absence | conformance semantics | `unsupported_coverage_cannot_prove_absence` |
| Inferred evidence cannot independently create hard results | conformance semantics | `inferred_observations_cannot_create_hard_matches` |
| Conformance records all semantic input identities and `asOf` | conformance schema and hashing specification | generated conformance validation |
| Applied, stale, expired, irrelevant, and rejected exceptions remain visible | conformance semantics | generated conformance and policy tests |

## Product examples

| Requirement | Specification owner | Mechanical evidence |
| --- | --- | --- |
| Tabby and Shelly share a generic tab-host capability without inventing composition | `DECISIONS.md`, Tabby example | `tabby_example_separates_governing_and_review_graphs` |
| The one-root-Space-per-tab direction remains a proposal | Tabby example | review graph assertions |
| Workshop consumes Code Atlas without a reverse dependency | Workshop example | `workshop_example_has_no_codeatlas_to_workshop_dependency` |
| Workshop does not absorb Access or commit execution | Workshop example constraints | example graph compilation |

## Review outcome

Phase 7 accepted the semantic package after clarifying that
`ArchitectureChange` is always non-governing audit history. Passing this matrix
proves internal consistency and reproducibility. Product implementation and
provider contracts still require their own tests and release review.
