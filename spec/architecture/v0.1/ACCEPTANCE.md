# Acceptance Record

## Decision

Atlas Architecture DSL v0.1 is accepted as the normative semantic basis for
Code Atlas architecture declarations, observations, and conformance.

The owner accepted the Phase 7 review candidate on 2026-07-23 after independent
review requested one clarification. Commit `8f5a2df` contains the reviewed
design package with that clarification: `ArchitectureChange` is non-governing,
is never replayed to reconstruct current architecture, and remains immutable
audit history after disposition.

## Accepted artifacts

The accepted package consists of:

- the six document kinds and common envelope;
- the restricted-YAML authoring profile;
- schemas and the closed v0.1 vocabulary;
- stable IDs, imports, exports, retirement, and supersession;
- governing and review graph semantics;
- policies and controlled exceptions outside architecture graphs;
- canonicalization and typed digest semantics;
- generated observation provenance and coverage;
- declared-versus-observed conformance semantics;
- security, resource-limit, versioning, and migration rules;
- the examples under this directory.

The exact Phase 7 evidence remains reproducible through
`design/architecture-dsl/v0.1/PHASE7_MANIFEST.sha256` and the private reference
validator. The manifest records paths at their reviewed design-package
locations. This promotion record maps the accepted normative files into
`spec/architecture/v0.1/`.

## Authority boundary

Restricted YAML is the sole editable authority for declared architecture.
`codeatlas.json` remains product configuration. Generated files and internal
representations may use other deterministic formats but are not editable
architecture authorities.

Only accepted active declarations govern. Proposed and unresolved declarations
may appear in a non-governing review graph. Policies and exceptions do not
change the governing graph.

An `ArchitectureChange` never enters the governing or review graph and is never
replayed to reconstruct current architecture. Approval requires updating the
authoritative module, vocabulary, or policy. The change record then remains
audit history.

## Implementation authorization

The owner separately authorized production implementation after accepting the
design. That implementation must preserve this specification, use narrow
versioned CLI and JSON contracts, and pass independent conformance and
compatibility review before release.

This authorization does not grant Code Atlas runtime Access authority, provider
approval authority, work coordination, Git effects, Shell lifecycle, or
automatic architecture mutation.
