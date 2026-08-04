# Atlas Architecture DSL v0.1

Status: accepted normative specification

This specification defines a small, typed architecture graph authored in a
restricted YAML 1.2 profile. Its stable CLI and JSON exposure remains a
separate product contract.

## 1. Purpose

Atlas Architecture DSL represents owner-approved architectural intent without
becoming application runtime state.

Its conceptual model is:

- objects are nouns;
- relations are verbs;
- constraints are deterministic rules;
- typed authority references explain why declarations govern.

Code Atlas compiles accepted declarations, observes source code separately,
compares the two, and generates conformance reports and projections.

## 2. Source-of-truth hierarchy

1. Accepted restricted-YAML declarations are the editable architectural
   authority.
2. The normalized governing graph is their canonical compiled representation.
3. Generated observations are evidence about implementation.
4. Generated conformance is a comparison of accepted architecture and observed
   evidence.
5. Diagrams, prose, indexes, and context slices are projections.

`codeatlas.json` remains tool configuration. It is not an architecture
declaration and cannot be imported as one.

## 3. Runtime boundary

The DSL may declare that runtime concepts and their owning systems exist. It
must not store live instances of:

- principals or participants;
- Root Owner assignments;
- Access grants;
- executable capabilities;
- work claims or task assignments;
- tabs, terminals, process IDs, or sessions;
- Git tickets;
- user data or secrets.

The DSL may record an owner-approved provider classification as architectural
intent. It does not perform the approval action, grant authority, select a
runtime provider, coordinate work, execute workflows, or mutate accepted
architecture.

## 4. Document kinds

V0.1 defines exactly six document kinds.

Hand-authored:

1. `ArchitectureModule`
2. `ArchitecturePolicy`
3. `ArchitectureVocabulary`
4. `ArchitectureChange`

Generated:

5. `ArchitectureObservation`
6. `ArchitectureConformance`

Generated documents set `metadata.generated: true` and include generator and
source metadata. Hand-authored documents must not set `metadata.generated` to
true.

## 5. Common envelope

Every document contains:

```yaml
apiVersion: atlas.codeatlas.dev/v0.1
kind: ArchitectureModule
metadata:
  id: goobits.package.space
  name: Space package architecture
  architectureVersion: 4
  productRelease: "2.3.0"
  description: Declared architecture for the reusable Space package.
  maintainers:
    - goobits.team.platform
```

`apiVersion` identifies the DSL schema and semantic version.

`metadata.id` is stable document identity. A file path is never identity.

`metadata.architectureVersion` is a positive integer that increases when the
document's semantic content changes.

`metadata.productRelease` is optional product metadata. It does not replace
`architectureVersion`.

Generated documents also include:

```yaml
metadata:
  generated: true
  generator:
    id: codeatlas.tool.architecture-observer
    version: "0.1"
  generatedAt: "2026-07-23T00:00:00Z"
  sourceInputs:
    - architecture/root.atlas.yaml
  generationCommand: codeatlas scan architecture
```

## 6. Identifiers

Document, object, relation, constraint, binding, rule, adapter, artifact, and
retired IDs use fully qualified semantic identifiers.

The v0.1 grammar is:

```text
identifier = segment "." segment ("." segment)*
segment    = lowercase letter or digit followed by lowercase letters,
             digits, or hyphens
```

Each identifier:

- contains at least three segments;
- is at most 200 UTF-8 bytes;
- is ASCII lowercase;
- survives display-name changes;
- is never reused after retirement.

Examples:

- `goobits.app.tabby`
- `goobits.capability.shell-create`
- `codeatlas.capability.context-slice`
- `goobits.relation.shelly-consumes-tab-host`

Hand-authored semantic IDs must not be random UUIDs.

## 7. Vocabulary reference

Every architecture document except the bootstrapped core vocabulary names one
exact vocabulary:

```yaml
vocabulary:
  id: codeatlas.architecture.core
  version: 2
  digest: sha256:0123456789abcdef...
```

The core vocabulary does not reference or digest itself. It is bootstrapped by:

- the static `ArchitectureVocabulary` schema;
- the fixed v0.1 meta-kinds for object kinds, predicates, rules, adapters,
  authority kinds, decision states, approval states, severity, coverage, and
  conformance states.

Vocabulary documents may import an exact core vocabulary and add namespaced
extensions. An extension may not redefine a core term or another module's term.

Unknown kinds, properties, predicates, rules, adapters, and authority kinds
fail validation.

## 8. Decision, approval, and change control

Decision status is closed:

- `unresolved`
- `proposed`
- `accepted`
- `rejected`
- `superseded`

Approval status is independent:

- `not_required`
- `required`
- `granted`
- `denied`

Change type is independent:

- `EXTEND`
- `COMPOSE`
- `REPLACE`
- `INTRODUCE`

Change control uses a closed policy vocabulary:

- `open_review`
- `owner_approval_required`
- `accepted_adr_required`
- `versioned_successor_only`
- `release_owner_only`

An accepted active declaration may enter the governing graph only when its
authority and approval satisfy its change-control policy.

## 9. Authority references

Authority references separate stable identity from location:

```yaml
decision:
  status: accepted
  authority:
    governing:
      - kind: accepted-adr
        artifact:
          id: goobits.adr.tabby-lifecycle
          version: 1
        locator:
          path: docs/adr/ADR-0091-tabby-lifecycle.md
          section: decision
        digest: sha256:0123456789abcdef...
    supporting:
      - kind: glossary
        artifact:
          id: goobits.glossary.core
          version: 6
        locator:
          path: docs/GLOSSARY.md
          section: Tabby
```

Initial authority kinds are:

- `normative-spec`
- `accepted-adr`
- `owner-decision`
- `owner-direction`
- `glossary`
- `package-contract`
- `schema`
- `policy`
- `released-api`
- `architecture-module`

An exploratory `owner-direction` may support a proposal. A formal accepted
owner decision may govern within its declared scope. Supporting authority never
promotes a proposal.

## 10. ArchitectureModule

An `ArchitectureModule` declares one owned architecture boundary.

```yaml
apiVersion: atlas.codeatlas.dev/v0.1
kind: ArchitectureModule
metadata: {}
vocabulary: {}
decision: {}
approval: {}
changeControl: {}
imports: []
exports:
  objects: []
  relations: []
  bindings: []
  constraints: []
objects: {}
relations: {}
bindings: {}
constraints: {}
retired: {}
```

The same grammar applies at system, product, package, and bounded subsystem
levels.

### 10.1 Imports

Imports are local, exact, and digest-pinned:

```yaml
imports:
  - module: goobits.package.space
    architectureVersion: 4
    digest: sha256:0123456789abcdef...
    source: ../../packages/space/architecture.atlas.yaml
```

`source` is a materialized locator, not identity. Network URLs, floating
versions, version ranges, and implicit latest are prohibited.

Import cycles, digest mismatches, identity mismatches, version mismatches, path
escapes, and duplicate IDs in the closure fail compilation.

### 10.2 Exports

Declarations are private unless explicitly exported. Imports do not imply
re-export.

Removing an export is a compatibility change. A module cannot reference another
module's private ID.

### 10.3 Objects

Objects are keyed by fully qualified stable ID:

```yaml
objects:
  goobits.capability.shell-create:
    kind: capability
    name: Create Shell
    summary: Create a Shell surface in an eligible Space.
    attributes:
      visibility: public
      contract: goobits.contract.shell-create-v1
    decision:
      status: accepted
      authority:
        governing:
          - kind: package-contract
            artifact:
              id: goobits.contract.shell-create
              version: 1
        supporting: []
    approval:
      status: granted
    changeControl:
      policy: versioned_successor_only
```

The vocabulary defines each object kind's required and optional attributes.
Unknown attributes fail semantic validation. There are no unrestricted
top-level property bags.

#### Provider classifications

A `provider` object describes an implementation. A `provider_approval` object
records an owner-controlled classification in one scope. The two objects remain
separate because implementation ownership, organization approval, runtime
eligibility, and Access authorization are different facts.

A provider classification:

- links to exactly one provider through `approves`;
- links to one or more capabilities through `covers`;
- records lifecycle, scope, origin, owner, compatibility range, risk, and
  source;
- never replaces the provider's `provides` and `implements` relations;
- never grants authority to invoke the provider.

The declaration-level `approval.status` field controls whether a declaration
may enter an architecture graph. It is not provider approval and must not be
used as a provider-selection signal.

Code Atlas may expose a deterministic projection of approved classifications
from the governing graph. That projection reports eligibility and
authorization as unevaluated unless separate owning systems provide those
facts.

### 10.4 Relations

Relations are keyed by fully qualified stable ID:

```yaml
relations:
  goobits.relation.shelly-consumes-tab-host:
    predicate: consumes
    subject: goobits.app.shelly
    object: goobits.capability.tab-host
    decision:
      status: accepted
      authority:
        governing:
          - kind: owner-decision
            artifact:
              id: goobits.decision.tab-host
              version: 1
        supporting: []
    approval:
      status: granted
    changeControl:
      policy: owner_approval_required
```

The vocabulary declares predicate subject kinds, object kinds, cardinality,
cycle behavior, and any inverse. `uses` is not a catch-all predicate.

Runtime ownership and architectural governance use distinct terms. The
architecture predicate `governs` never creates a runtime Access ownership
relationship.

### 10.5 Bindings

Bindings map declared IDs to observable source constructs:

```yaml
bindings:
  goobits.binding.tabby-package:
    target: goobits.app.tabby
    adapter:
      kind: npm.package
      version: 1
    selector:
      name: "@goobits/tabby"
    cardinality: exactly_one
    decision:
      status: accepted
      authority:
        governing:
          - kind: package-contract
            artifact:
              id: goobits.contract.tabby-package
              version: 1
        supporting: []
    approval:
      status: granted
    changeControl:
      policy: versioned_successor_only
```

Each adapter defines a closed selector schema, deterministic matching,
cardinality, evidence, coverage, failure behavior, and versioning.

AI-inferred bindings are observation evidence. They cannot silently become
accepted declarations.

### 10.6 Constraints

Constraints use a closed rule vocabulary:

```yaml
constraints:
  goobits.constraint.one-tab-governor:
    rule: exactly_one_incoming
    severity: error
    arguments:
      target: goobits.lifecycle.tab-root-space
      predicate: governs
    decision:
      status: accepted
      authority:
        governing:
          - kind: accepted-adr
            artifact:
              id: goobits.adr.tab-lifecycle-owner
              version: 1
        supporting: []
    approval:
      status: granted
    changeControl:
      policy: accepted_adr_required
```

Arguments are validated against the selected rule's closed schema. General
expressions, scripts, templates, SQL, and arbitrary predicates are prohibited.

### 10.7 Retired IDs

Retired IDs remain reserved:

```yaml
retired:
  goobits.lifecycle.shelly-tab:
    retiredInArchitectureVersion: 4
    supersededBy:
      - goobits.lifecycle.tab-root-space
    authority:
      governing: []
      supporting: []
```

A retired ID cannot reappear in any active declaration.

## 11. ArchitecturePolicy

An `ArchitecturePolicy` contains cross-module rules and controlled exceptions:

```yaml
apiVersion: atlas.codeatlas.dev/v0.1
kind: ArchitecturePolicy
metadata: {}
vocabulary: {}
decision: {}
approval: {}
changeControl: {}
imports: []
rules: {}
exceptions: {}
```

An exception identifies:

- exact constraint ID and version;
- exact affected module or policy import-closure digest;
- exact affected IDs;
- narrow scope;
- rationale;
- expiration or release boundary;
- removal plan;
- governing authority.

Exceptions have no wildcard scope and cannot use `ignore: true`. A relevant
closure change makes an exception stale. Unrelated module changes do not.
V0.1 performs no automatic exception rebasing.

Policies and exceptions are not part of the governing graph. They form the
policy closure used by conformance at a recorded `asOf`.

## 12. ArchitectureVocabulary

An `ArchitectureVocabulary` declares versioned:

- object kinds and typed attributes;
- predicates and kind compatibility;
- constraint rules and argument schemas;
- binding adapters and selector schemas;
- authority kinds;
- decision, approval, severity, coverage, observation, and conformance enums.

The vocabulary is closed. Core changes require an Architecture Impact Check and
owner approval. Extensions use namespaces and cannot redefine imported terms.

## 13. ArchitectureChange

An `ArchitectureChange` is a proposal and later immutable audit record. Current
architecture is never reconstructed by replaying change documents.

Required concepts include:

```yaml
baseGraphDigest: sha256:0123456789abcdef...
change:
  type: REPLACE
decision:
  status: proposed
approval:
  status: required
affectedIds: []
currentOwner: goobits.package.shelly
intendedOwner: goobits.package.tab-host
expectedEffects:
  adds: []
  removes: []
  requires: []
migrationPlan: {}
removalPlan: {}
verificationPlan: {}
targetGraphDigest: null
```

A replacement requires migration and removal plans. After authoritative modules
change and conformance verifies the effects, a generated target graph digest may
be recorded and the accepted change becomes immutable history.

## 14. ArchitectureObservation

An `ArchitectureObservation` is generated evidence. Each fact records:

- repository identity;
- source commit;
- extractor ID and version;
- structured source paths and spans;
- deterministic or inferred mode;
- confidence for inferred evidence;
- coverage;
- content digest;
- capture time in the generated envelope.

Coverage states are:

- `complete`
- `partial`
- `unsupported`
- `unknown`

Lack of coverage is not evidence of absence.

## 15. ArchitectureConformance

An `ArchitectureConformance` compares one exact governing architecture with one
exact observation content set and one exact policy context.

Result states are:

- `matched`
- `partial`
- `absent`
- `conflicting`
- `unexpected`
- `unobserved`
- `ambiguous`

Only deterministic evidence inside declared complete coverage may independently
create a hard error for absence, conflict, or unexpected implementation.
Inferred observations remain advisory unless separately accepted.

## 16. Generated artifacts

Committed generated observations, conformance examples, lockfiles, and manifests
must include:

- generated status;
- generator identity and version;
- source inputs;
- relevant typed digests;
- generation command;
- prohibition on manual editing.

`MANIFEST.sha256` uses stable path order, hashes exact checked-in bytes,
excludes itself, and is regenerated with `pnpm run spec:write` rather than
hand-edited.

## 17. Non-goals

V0.1 does not include:

- network imports;
- overlays or module inheritance;
- arbitrary expressions;
- workflow or task execution;
- runtime provider selection;
- automatic accepted-architecture mutation;
- runtime health or coordination state;
- general graph queries;
- diagram layout;
- runtime integration outside Code Atlas.

## 18. Acceptance

The Code Atlas owner accepted this specification on 2026-07-23. The acceptance
scope and reviewed evidence are recorded in `../ACCEPTANCE.md`.

Future semantic changes require the versioning and migration process. Passing
tests alone does not alter accepted architecture.
