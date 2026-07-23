# Versioning and Migration

Status: accepted normative specification

V0.1 separates format, architecture, product, vocabulary, adapter, rule, and
provider versions. One number cannot stand in for every compatibility boundary.

## 1. Version axes

### apiVersion

Identifies document schema and semantic rules:

```yaml
apiVersion: atlas.codeatlas.dev/v0.1
```

Changing parsing, required shape, decision eligibility, or canonicalization
requires an `apiVersion` compatibility decision.

### architectureVersion

A positive integer local to one document identity. It increments for any
semantic content change.

Moving a file without changing semantic content does not require a new stable
document ID. If the locator is authored inside the document and changes, the
document content and digest change as usual.

### productRelease

Optional released product metadata, such as `"2.3.0"`. It does not control
architecture import resolution.

### vocabulary version

An integer and canonical digest identify exact vocabulary semantics. Modules
pin both.

### rule and adapter versions

Constraint rules and binding adapters carry independent positive integer
versions. A selector or argument semantic change requires a new version.

### provider release

A provider keeps stable identity while release and implementation versions are
metadata:

```yaml
objects:
  codeatlas.provider.codeatlas:
    kind: provider
    attributes:
      releaseVersion: "0.6.1"
      implementationVersion: "0.6.1+rust"
```

A release number is not encoded into permanent provider identity.

## 2. Compatibility

Compatible changes may include:

- adding an optional field with a deterministic default;
- adding a namespaced vocabulary extension not used by existing modules;
- adding a new generated projection;
- adding an advisory diagnostic reason that does not alter acceptance.

Potentially breaking changes include:

- removing or renaming a document field;
- changing a default;
- changing canonicalization or digest payloads;
- changing decision eligibility;
- removing an export;
- changing predicate domain or range;
- changing rule or adapter semantics;
- changing exception applicability;
- reusing or changing a stable ID's meaning.

Breaking changes require a versioned successor and migration plan.

## 3. Stable identity

Stable IDs survive:

- file moves;
- display-name changes;
- package-directory changes;
- implementation-language changes;
- provider release changes.

Stable IDs do not survive semantic replacement. Replacement introduces a new
ID, records supersession, retires the old ID, migrates consumers, and removes
the old authority.

Retired IDs remain reserved forever within the architecture lineage.

## 4. ArchitectureChange lifecycle

An `ArchitectureChange` records:

1. exact `baseGraphDigest`;
2. change type;
3. decision and approval status;
4. affected IDs;
5. current and intended owners;
6. expected effects;
7. migration plan;
8. removal or retirement plan;
9. verification plan.

Lifecycle:

1. Draft against an exact governing graph.
2. Review for overlap and authority.
3. Accept, reject, or leave proposed.
4. Change authoritative modules only after approval.
5. Compile a new governing graph.
6. Verify expected effects through conformance.
7. Record generated `targetGraphDigest`.
8. Preserve the immutable change record as history.

The current architecture comes from active accepted modules, not replayed
change documents.

## 5. Change type rules

### EXTEND

Add behavior or declarations to an existing coherent owner without changing its
meaning or authority.

### COMPOSE

Connect existing owners through their contracts without moving authority.

### REPLACE

Move responsibility from an old authority to a new one. Requires migration,
consumer conversion, verification, retirement, and deletion of old mutation
paths.

### INTRODUCE

Create a genuinely new responsibility with a named owner, demonstrated need,
contract, and verification.

Rejection is recorded in `decision.status`. Approval gating is recorded in
`approval.status`.

## 6. Import upgrades

An import upgrade is explicit:

1. materialize the new module locally;
2. verify its ID and architecture version;
3. update the pinned digest;
4. recompute the import closure;
5. inspect changed exports and declared effects;
6. validate the importing module;
7. update affected policies and exceptions through explicit review;
8. regenerate lockfiles and conformance.

No floating or automatic latest upgrade exists.

## 7. Exception evolution

Exceptions bind to the exact affected `baseClosureDigest`.

- An unrelated module change leaves the exception valid.
- A relevant closure change makes it stale.
- Expiration is evaluated from recorded `asOf`.
- V0.1 never rebases an exception automatically.
- Reapproval produces a new exception architecture version and authority record.

Historical exception records remain audit evidence. They never become
architecture graph nodes.

## 8. Observation evolution

Extractor upgrades retain:

- stable extractor identity;
- explicit extractor version;
- declared feature coverage;
- old observation envelopes for reproducibility.

Changing extractor semantics requires a new extractor version. A conformance
result always identifies the exact observation content and validator version.

## 9. Design-package promotion

The owner accepted the Phase 7 design package and promoted its normative
artifacts to:

```text
spec/architecture/v0.1/
```

`../ACCEPTANCE.md` records the source commit, evidence, accepted scope, and
deferred implementation work. Historical fixtures and the private reference
validator remain under `design/architecture-dsl/v0.1/` as non-authoritative
evidence.

## 10. Earlier v0alpha1 proof

No authority-bearing v0alpha1 DSL artifact was found in the Code Atlas baseline
used for this draft.

If later recovered, it may be:

- historical evidence;
- a migration example;
- a negative fixture showing runtime data mixed with architecture.

It may not coexist as a second editable authority. Any accepted semantic content
must be migrated into v0.1 with stable IDs, explicit authority, and a recorded
change.

## 11. Future production compiler

The private reference validator is not promoted automatically.

A later production proposal must independently define:

- public capability and API ownership;
- compatibility and support policy;
- production dependencies;
- CLI or service surface;
- performance budgets;
- migration from private evidence code;
- security review;
- Code Atlas release and Goobits pinning sequence.

Production code may reuse proven algorithms only after that review. It must not
inherit private test APIs as accidental public contracts.

## 12. Removal rule

A migration is incomplete while two editable architecture authorities or two
mutation paths remain live.

Preserve history, identity, credit, and audit evidence. Remove superseded
editable declarations, wrappers, aliases, and mutation paths after verified
consumer migration.
