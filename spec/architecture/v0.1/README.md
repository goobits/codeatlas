# Atlas Architecture DSL v0.1

Status: accepted normative specification

Acceptance authority: Code Atlas owner

The accepted semantics live in this directory. Restricted YAML is the sole
editable authority for declared architecture. Generated graphs, observations,
conformance reports, lockfiles, indexes, diagrams, and prose remain derived
evidence or projections.

## Package map

| Path | Purpose |
| --- | --- |
| `ACCEPTANCE.md` | Acceptance scope, evidence, and deferred implementation work |
| `DECISIONS.md` | Accepted semantic decisions and separate implementation status |
| `spec/` | Normative language, compiler, conformance, security, and migration semantics |
| `schemas/` | Draft 2020-12 document-shape schemas expressed as restricted YAML |
| `vocabularies/` | Closed typed v0.1 architecture vocabulary |
| `examples/` | Normative examples and generated evidence |
| `MANIFEST.sha256` | Generated deterministic manifest of this package |

## Evidence

The private reference validator, fixtures, Phase 7 review evidence, and
historical manifest remain under
`design/architecture-dsl/v0.1/`. They test this specification but are not a
second architecture authority or a supported product API.

`ArchitectureChange` documents never enter an architecture graph and are never
replayed to recover current state. Accepted changes are materialized in current
modules, vocabularies, or policies. Change documents remain audit history.

Production compiler and provider behavior is implemented through separately
reviewed Code Atlas product contracts. Acceptance of this specification does
not move runtime authority into Code Atlas.
