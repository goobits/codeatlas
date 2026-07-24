# Atlas Architecture DSL v0.1 Review Evidence

Status: accepted Phase 7 evidence

Canonical specification: `../../../spec/architecture/v0.1/`

This directory preserves the review package that proved Atlas Architecture DSL
v0.1 before product implementation began. It is evidence, not a second
editable architecture authority.

The Code Atlas owner accepted the normative semantics on 2026-07-23. The
accepted specification, schemas, vocabulary, examples, decision register, and
current deterministic manifest now live under `spec/architecture/v0.1/`.

## Evidence map

| Path | Purpose |
| --- | --- |
| `ARCHITECTURE_IMPACT_CHECK.md` | Original ownership and scope classification |
| `REQUIREMENTS.md` | Phase 7 requirements traceability |
| `fixtures/` | Valid and invalid review fixtures |
| `VALIDATION.md` | Reproducible Phase 7 checks |
| `RELEASE_NOTES.md` | Historical review-candidate notes |
| `PHASE7_MANIFEST.sha256` | Immutable manifest of the reviewed package at commit `8f5a2df` |

The historical manifest intentionally records the original design-package
paths, including the private validator that existed at review time. Do not
regenerate or edit it. The removed validator remains recoverable from commit
`8f5a2df`. The canonical package has its own generated `MANIFEST.sha256`.

## Boundary

The retired private validator:

- is unpublished and outside the production Code Atlas workspace;
- has no supported public API or CLI contract;
- performs no network import resolution;
- performs no architecture mutation;
- owns no Goobits, Workshop, Access, Shell, Git, or coordination behavior.

Production Code Atlas now owns the separately reviewed compiler, observation,
and conformance implementation. The original review fixtures remain here as
historical evidence, while current product tests exercise the accepted
semantics.
