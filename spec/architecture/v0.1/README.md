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
| `examples/` | Normative examples and accepted Phase 7 generated evidence |
| `MANIFEST.sha256` | Generated deterministic manifest of this package |

## Evidence

The Phase 7 fixtures, validation record, and historical manifest remain under
`design/architecture-dsl/v0.1/`. The private proof validator was retired after
its semantics moved into the separately reviewed product implementation. Its
reviewed source remains recoverable from commit `8f5a2df`.

Current production checks compile the bundled schemas and vocabulary, exercise
the valid and invalid semantics, and verify this package manifest:

```bash
pnpm run spec:check
cargo test --locked --jobs 1 architecture
```

The generated observation preserves the accepted Phase 7 inferred-evidence
fixture and its historical generator metadata. The conformance example is
regenerated from that observation by the current production command. They are
normative document-shape examples, not claims about the current repository.

`ArchitectureChange` documents never enter an architecture graph and are never
replayed to recover current state. Accepted changes are materialized in current
modules, vocabularies, or policies. Change documents remain audit history.

Production compiler and provider behavior is implemented through separately
reviewed Code Atlas product contracts. Acceptance of this specification does
not move runtime authority into Code Atlas.

The provider-approval example demonstrates separate provider implementation
and owner-controlled classification objects. It is architecture intent only:
eligibility, runtime selection, and Access authorization remain outside the
DSL.
