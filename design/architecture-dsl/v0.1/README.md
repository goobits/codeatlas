# Atlas Architecture DSL v0.1

Status: proposed design package

Acceptance authority: Code Atlas owner

Production compiler: not included

Runtime integration: not included

This directory contains the proposed normative design and executable
specification evidence for Atlas Architecture DSL v0.1.

The design separates four concerns:

1. Restricted YAML declarations describe approved architectural intent.
2. A deterministic normalized graph represents those declarations.
3. Generated observations describe what source code contains.
4. Generated conformance results compare intent with observed evidence.

Only accepted declarations enter the governing graph. Proposed and unresolved
declarations may enter a non-governing review graph. Policies and temporary
exceptions remain outside both architecture graphs.

## Scope

Phases 1 through 6 may create:

- proposed specifications;
- draft JSON Schema 2020-12 schemas expressed as YAML;
- a closed draft vocabulary;
- examples and fixtures;
- a mechanically isolated private Rust reference validator;
- deterministic generated examples;
- validation and review evidence.

They may not create:

- a production Code Atlas compiler;
- a public Code Atlas API or CLI command;
- Goobits or Workshop integration;
- runtime permission or coordination behavior;
- network import resolution;
- automatic architecture mutation;
- another editable architecture format.

## Authority

Restricted YAML is the sole editable authority for declared architecture.
Existing `codeatlas.json` files remain Code Atlas tool configuration. Generated
JSON, lockfiles, observations, reports, indexes, diagrams, and prose are
projections or evidence, not another editable architecture authority.

The design package remains proposed until an independent Phase 7 review records
one of these outcomes:

- accept;
- revise;
- reject.

Files in this directory must not be treated as accepted product contracts before
that review.

## Package map

| Path | Purpose |
| --- | --- |
| `ARCHITECTURE_IMPACT_CHECK.md` | Existing ownership and scoped change classification |
| `DECISIONS.md` | Proposed, accepted-direction, open, and rejected choices |
| `spec/` | Proposed normative semantics |
| `schemas/` | Static document-shape schemas |
| `vocabularies/` | Closed typed architecture vocabulary |
| `examples/` | Human-readable positive examples |
| `fixtures/` | Mechanically checked valid and invalid cases |
| `reference-validator/` | Private executable specification evidence |
| `VALIDATION.md` | Reproducible checks and known design-only obligations |
| `RELEASE_NOTES.md` | Draft notes for the proposed design package |
| `MANIFEST.sha256` | Generated stable file manifest, excluding itself |

## Review rule

Generated examples and manifests must identify their generator, inputs, and
generation command. Do not edit those outputs by hand.

The private validator may prove parsing, canonicalization, import resolution,
graph construction, policy evaluation, and fixture behavior. Its existence does
not establish a public compiler contract.
