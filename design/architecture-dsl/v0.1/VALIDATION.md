# Validation Evidence

Status: accepted Phase 7 validation evidence

Validation proved that the reviewed specification was internally coherent,
reproducible, and contained. Owner acceptance and later production
authorization are recorded separately.

This is a historical record. The commands below reproduce commit `8f5a2df`,
where the private validator still existed. It was retired after the production
architecture implementation covered its semantics. Current checks run through
`pnpm run spec:check` and the production Cargo test suite.

## Baseline

The repository audit began from Code Atlas commit `253b18a`, after the
source-first wrapper and CI baseline was independently verified. Review
fixtures remain under `design/architecture-dsl/v0.1/`. The reviewed private
validator remains in Git history. Accepted normative artifacts were later
promoted to `spec/architecture/v0.1/`.

## Reproduction

Run from this directory:

```sh
cargo fmt --manifest-path reference-validator/Cargo.toml -- --check
cargo test --locked --jobs 1 --manifest-path reference-validator/Cargo.toml
cargo clippy --locked --jobs 1 \
  --manifest-path reference-validator/Cargo.toml \
  --all-targets -- -D warnings
cargo run --locked --jobs 1 \
  --manifest-path reference-validator/Cargo.toml \
  --bin generate_artifacts -- --check
```

Run the production repository checks separately from the Code Atlas root:

```sh
pnpm check
```

The reference suite contains 51 tests:

- 34 focused unit tests;
- 4 positive example integration tests;
- 1 restricted-YAML fixture matrix test;
- 7 invalid semantic integration tests;
- 5 refinement integration tests.

The generated-artifact check recomputes the canonical observation example,
conformance example, and `spec/architecture/v0.1/MANIFEST.sha256` without
modifying the checkout.

## Coverage summary

| Area | Evidence | Result |
| --- | --- | --- |
| Restricted YAML profile | Parser tests and ten invalid source fixtures | Mechanically checked |
| Static schemas | All six Draft 2020-12 schemas compiled and examples validated | Mechanically checked |
| Closed vocabulary | Kind, attribute, predicate, adapter, rule, and authority checks | Mechanically checked |
| Stable identity | Qualified IDs, duplicate detection, and retired-ID reservation | Mechanically checked |
| Imports and visibility | Exact digest, cycle, export, traversal, network, and symlink checks | Mechanically checked |
| Graph modes | Governing and persisted review graph status filtering | Mechanically checked |
| Constraints | Closed rules, typed endpoints, graph paths, and cardinality | Mechanically checked |
| Policies | Scope, closure staleness, expiration, authority, and visibility | Mechanically checked |
| Digests | Typed domain separation and canonical key ordering | Mechanically checked |
| Observation | Provenance, coverage, source locations, and generated metadata | Mechanically checked |
| Conformance | Uncertainty, unsupported coverage, inferred evidence, and inputs | Mechanically checked |
| Product examples | Tabby/Shelly uncertainty and one-way Workshop/Code Atlas use | Mechanically checked |
| Generated outputs | Deterministic regeneration and stable manifest | Mechanically checked |
| Clean checkout | Archive-based reproduction from the final Phase 6 commit | Passed |

## Recorded results

- Private reference suite: 51 passed, 0 failed.
- Private validator formatting: passed.
- Private validator Clippy with warnings denied: passed.
- Generated observation, conformance, and manifest freshness: passed.
- Production `pnpm check`: 4 Node wrapper tests and 42 Rust tests passed,
  with production formatting and Clippy clean.
- Fresh-archive reference suite and generated-artifact check: passed.

## Invalid-fixture coverage

### Restricted YAML files

The committed invalid files cover:

- duplicate keys;
- anchors;
- aliases;
- merge keys;
- custom tags;
- environment interpolation;
- multiple documents;
- floating-point values;
- non-string mapping keys;
- excessive nesting.

Each fixture declares its expected stable diagnostic in
`fixtures/invalid/expectations.yaml`.

### Semantic cases

Focused integration and unit tests cover:

- unknown object kinds and attributes;
- runtime-only data in declarations;
- unknown predicates and invalid endpoint kinds;
- unknown binding adapters, versions, and selector fields;
- unknown constraint rules and open expression fields;
- accepted declarations without governing authority or granted approval;
- changes without required evidence or removal plans;
- invalid policy exceptions;
- observations without provenance, coverage, locations, or generated metadata;
- duplicate, imported, private, unresolved, and retired IDs;
- cyclic or mismatched imports;
- path traversal, network imports, and symlink escape;
- expired and stale exceptions;
- inferred observations used as hard evidence;
- unsupported coverage misreported as absence;
- change type confused with decision or approval status;
- an `ArchitectureChange` presented as an architecture-graph input;
- `codeatlas.json` presented as architecture authority.

## Recent refinement gates

The suite specifically proves:

- accepted declarations enter the governing graph;
- proposed and unresolved declarations do not govern;
- rejected, superseded, and retired declarations do not govern;
- retired IDs remain reserved;
- persisted review graphs receive reproducible identity;
- policies and exceptions never alter `governingGraphDigest`;
- recorded `asOf` controls exception expiration;
- an affected closure change makes an exception stale;
- unrelated architecture outside that closure does not;
- `owner-direction` supports but does not promote a proposal;
- change type, decision status, and approval status remain independent;
- `ArchitectureChange` remains proposal and audit history outside both
  architecture graph modes;
- existing `codeatlas.json` is valid tool configuration but is not editable
  declared architecture.

## Security and resource-limit evidence

The validator currently proves parser byte and depth limits, prohibited YAML
features, local import confinement, network-source rejection, canonical
symlink confinement, typed identifiers, closed vocabularies, and deterministic
diagnostics.

The proposed specification also defines production-scale limits for aggregate
source bytes, import depth, closure size, graph size, relations, constraints,
exceptions, source locations, scalar length, and retained diagnostics. The
private validator does not simulate every production-scale exhaustion case.
Those limits remain mandatory acceptance criteria for a later production
compiler proposal.

## Design-only obligations

The following work is intentionally not implemented in this package:

- production repository extraction for every binding adapter;
- a released compiler library, CLI, or provider contract;
- production-scale load and denial-of-service tests;
- cryptographic authentication of authority artifacts;
- network import resolution;
- live Access, provider-selection, work-coordination, Shell, Git, or Workshop
  behavior;
- automatic architecture acceptance or mutation;
- production migration or promotion of the proposed schemas.

These are scope boundaries, not hidden claims of completion. Any later
production proposal must establish ownership, compatibility, performance,
security, migration, and removal plans independently.

## Honesty statement

Passing validation means the proposed semantics, schemas, examples, fixtures,
and private reference implementation agree with one another. It does not mean
the Code Atlas owner has accepted them, that the examples describe current
runtime implementation, or that this private validator is suitable for
production.
