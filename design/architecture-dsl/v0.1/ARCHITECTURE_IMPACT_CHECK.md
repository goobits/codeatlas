# Architecture Impact Check

## Request

Build the proposed Atlas Architecture DSL v0.1 design package, schemas,
examples, fixtures, and private executable specification evidence.

## Current scope

The approved scope is design and validation through Phase 6. Phase 7 is an
independent accept, revise, or reject gate.

The request does not authorize a production compiler, public CLI or API,
runtime integration, Goobits changes, Workshop changes, automatic mutation, or
network-resolved imports.

## Existing ownership

Current Code Atlas owns repository scanning, language adapters, source-symbol
observation, package and dependency analysis, generated documentation, output
rendering, diffing, and `codeatlas.json` tool configuration.

Relevant current owners include:

| Concern | Current owner |
| --- | --- |
| Tool configuration | `src/config.rs` and `codeatlas.json` |
| Observed scan model | `src/domain/model.rs`, including `ScanReport` and structured `Span` |
| Language observation | `src/languages/` |
| Package and dependency analysis | `src/package.rs` and `src/analysis/` |
| Generated outputs | `src/outputs/` |
| Public command behavior | `src/cli.rs` and `src/commands/` |
| Released package contract | root `Cargo.toml`, `package.json`, README, and release workflows |

No accepted editable architecture declaration format, declared graph,
architecture policy format, or declared-versus-observed conformance engine was
found in the clean baseline at commit `253b18a`.

The current JSON report schema is an observation and documentation contract. It
is not an editable architecture authority and must not be repurposed as one.

## Conceptual overlap

This proposal:

- extends Code Atlas's observation and repository-intelligence direction;
- composes with the existing structured source-location model;
- introduces a proposed declared-architecture design domain;
- does not replace current scanner, report, configuration, or output contracts;
- does not yet extend the released product surface.

The clean end state, if later accepted and implemented, has one editable
declared-architecture authority and one generated observation path. It must not
create a parallel source scanner or a second `codeatlas.json` meaning.

## Authority and state

The proposed single editable authority is restricted YAML in accepted
`ArchitectureModule`, `ArchitecturePolicy`, `ArchitectureVocabulary`, and
`ArchitectureChange` documents.

Generated normalized graphs, observations, conformance reports, lockfiles,
indexes, manifests, and diagrams remain derived evidence.

The governing graph contains active accepted declarations only. The optional
review graph may contain proposed and unresolved declarations but never governs
conformance. Exceptions remain in the policy closure and never alter the
governing graph digest.

## Public contract

No public contract is added by this phase.

A later owner-approved production proposal may expose versioned capabilities
for declaration compilation, conformance, targeted context, or impact analysis.
The private reference validator and its internal types are not that contract.

## Capability check

Current Code Atlas already provides:

- deterministic Rust data structures;
- SHA-256 support;
- JSON serialization;
- structured source paths and spans;
- source scanning and package observation;
- deterministic output tests.

The proposed reference validator should reuse standard parsing, hashing, and
schema-validation libraries inside its isolated crate. It must not add those
dependencies to the production Code Atlas crate.

## Data evolution

No production data migration occurs in this task.

The earlier `v0alpha1` conversation proof is treated as historical design
evidence only. No equivalent authority-bearing file was found in the Code Atlas
repository baseline. If one is found later, it must be either:

- migrated through an explicit owner-approved change;
- retained as historical evidence;
- or converted into a negative fixture.

It may not survive as a second editable authority.

## Performance

No measured production bottleneck justifies product optimization in this task.
The reference validator uses explicit resource bounds for safety and
deterministic tests for reproducibility. Those bounds are design constraints,
not production performance claims.

## Removal

This package does not remove current production behavior.

If a prior architecture proof is later discovered, the migration specification
must identify its disposition. Any future production cutover must remove
superseded editable declarations and compatibility paths after verified
migration.

## Verification

The design package must prove:

- restricted YAML rejection rules;
- static schema validity;
- closed vocabulary and typed graph semantics;
- exact local import resolution;
- stable IDs, exports, and retirement;
- deterministic canonicalization and typed digests;
- governing and review graph separation;
- policy exceptions outside the governing graph;
- observation provenance and coverage;
- conformance hard-gate rules;
- security and resource limits;
- deterministic generated examples and manifest;
- clean-checkout reproducibility.

## Decision

Change type: `INTRODUCE`

Decision status: `proposed`

Approval status: `granted` for Phases 1 through 6 only

Production approval status: `required`

The proposal introduces a new design domain without changing the released
Code Atlas product. Production adoption requires a separate decision after
independent review.
