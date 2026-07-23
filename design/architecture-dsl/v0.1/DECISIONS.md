# Decision Register

This register distinguishes architecture direction, design-package decisions,
implementation state, and authority source.

## Status axes

### Decision status

- `unresolved`
- `proposed`
- `accepted`
- `rejected`
- `superseded`

### Implementation status

- `implemented`
- `partial`
- `absent`
- `conflicting`
- `not_applicable`

### Approval status

- `not_required`
- `required`
- `granted`
- `denied`

### Change type

- `EXTEND`
- `COMPOSE`
- `REPLACE`
- `INTRODUCE`

`REJECT` is a decision outcome, not a change type. `NEEDS OWNER APPROVAL` is an
approval gate, not a change type.

## Accepted product direction

These items constrain the proposal but do not make its internal DSL design an
accepted Code Atlas contract.

| ID | Direction | Decision | Implementation | Authority source |
| --- | --- | --- | --- | --- |
| DIR-001 | Code Atlas remains independently versioned and consumed through pinned provider contracts | accepted | implemented | owner direction and released package boundary |
| DIR-002 | Declared architecture and observed implementation remain distinct | accepted | absent | owner direction |
| DIR-003 | Code Atlas observes and reports but does not grant runtime authority | accepted | implemented | owner direction and existing product boundary |
| DIR-004 | Generated prose and diagrams do not override governing contracts | accepted | partial | owner direction and current generated-output notices |
| DIR-005 | Restricted YAML is the sole editable authority for declared architecture | accepted | absent | owner direction |
| DIR-006 | `codeatlas.json` remains tool configuration, not declared architecture | accepted | implemented | released configuration contract |
| DIR-007 | Production compiler and runtime integration require a later approval | accepted | not_applicable | scoped owner approval |

## Proposed v0.1 design decisions

Every item below remains proposed until the Phase 7 review accepts it.

| ID | Proposal | Decision | Implementation | Approval |
| --- | --- | --- | --- | --- |
| DSL-001 | Use six document kinds: module, policy, vocabulary, change, observation, conformance | proposed | absent | granted for design proof |
| DSL-002 | Use one recursive module grammar at system, product, package, and subsystem levels | proposed | absent | granted for design proof |
| DSL-003 | Give every declaration one fully qualified stable semantic ID and one declaring module | proposed | absent | granted for design proof |
| DSL-004 | Keep capabilities, providers, packages, and consumers distinct | proposed | absent | granted for design proof |
| DSL-005 | Use a closed, versioned, typed vocabulary | proposed | absent | granted for design proof |
| DSL-006 | Permit local, exact, digest-pinned imports only | proposed | absent | granted for design proof |
| DSL-007 | Keep declarations private unless explicitly exported | proposed | absent | granted for design proof |
| DSL-008 | Use a closed constraint language without general expressions | proposed | absent | granted for design proof |
| DSL-009 | Keep policy exceptions outside architecture graphs | proposed | absent | granted for design proof |
| DSL-010 | Compile accepted declarations into a governing graph and optional proposed or unresolved declarations into a review graph | proposed | absent | granted for design proof |
| DSL-011 | Reserve retired IDs permanently and record supersession explicitly | proposed | absent | granted for design proof |
| DSL-012 | Separate change type, decision status, and approval status | proposed | absent | granted for design proof |
| DSL-013 | Separate stable authority artifact identity from repository locator | proposed | absent | granted for design proof |
| DSL-014 | Use JSON Schema Draft 2020-12 for static document shape | proposed | absent | granted for design proof |
| DSL-015 | Use a private Rust reference validator as executable specification evidence | proposed | absent | granted for design proof |

## Typed digest registry

The draft must define and mechanically test every digest below. Adding,
removing, or changing a digest's semantic payload requires an explicit design
decision.

| Digest | Semantic payload |
| --- | --- |
| `sourceDocumentDigest` | Exact source bytes |
| `canonicalModuleDigest` | Canonical semantic content of one module-like document |
| `importClosureDigest` | One document's exact transitive materialized import closure |
| `architectureClosureDigest` | Exact governing architecture roots and their resolved module closures |
| `governingGraphDigest` | Canonical active accepted architecture graph only |
| `policyClosureDigest` | Exact conformance policies and exception declarations |
| `reviewGraphDigest` | Canonical persisted non-governing review graph, when persisted |
| `observationContentDigest` | Semantic observed facts excluding volatile capture metadata |
| `observationEnvelopeDigest` | Observation content plus provenance and capture metadata |
| `conformanceResultDigest` | Canonical conformance inputs and result |

A vocabulary document uses its `canonicalModuleDigest`; it does not need an
extra digest family.

## Governing versus review decisions

- Accepted active declarations enter the governing graph.
- Proposed and unresolved declarations do not enter the governing graph.
- Rejected and superseded declarations do not govern.
- Retired declarations do not govern, but their IDs remain reserved.
- A persisted review graph receives a reproducible `reviewGraphDigest`.
- A transient review projection does not require a persisted digest.
- Exceptions never change `governingGraphDigest`.
- Exception applicability is evaluated against `policyClosureDigest`, its
  exact affected closure, and a recorded `asOf`.

## Authority decisions

- Authority references use stable artifact identity plus an optional locator.
- Authority kinds form a closed vocabulary.
- `owner-direction` is a valid typed authority kind.
- Exploratory owner direction normally supports a proposal.
- A formally accepted owner decision may govern within its declared scope.
- Supporting authority never promotes a proposal by itself.

## Tabby and Shelly example status

Accepted direction:

- Tabby expresses generic tab-host behavior.
- Shelly is Shell-focused.
- Shelly's plus action creates a Shell immediately.
- Tabby and Shelly must not own competing tab lifecycles.

Unresolved:

- the exact composition mechanism between the products;
- whether the current repository accepts one root Space per tab;
- whether Shelly may explicitly host selected non-Shell shared surfaces.

The governing example therefore uses a generic tab-host capability. A separate
proposed example may preserve the owner-directed one-root-Space-per-tab idea as
supporting authority, with an ADR and migration still required.

## Validator isolation

The reference validator:

- sets `publish = false`;
- is not a member of the production Code Atlas crate or release package;
- has no public Code Atlas command or stable API;
- has no network resolution;
- performs no architecture mutation;
- performs no Goobits or Workshop integration;
- changes no production dependency or lockfile;
- uses an independent lockfile only as an isolated crate;
- is invoked deterministically from this design directory.

## Open owner decisions

The Phase 7 review must explicitly settle or defer:

1. Whether to accept the six document kinds.
2. Whether to accept the core object and relation vocabulary.
3. Whether to accept the canonicalization and digest rules.
4. Whether to accept the import, export, and retirement model.
5. Whether to accept the policy exception model.
6. Whether the reference validator should inform a later production compiler.
7. Which future Code Atlas capability surface, if any, should expose the DSL.
8. Whether the proposed Tabby root-Space direction receives a governing ADR.

## Rejected for v0.1

- network imports;
- overlays and inheritance;
- executable templates or expressions;
- automatic accepted-architecture mutation;
- live Access grants or runtime participants;
- work claims, process IDs, tabs, tasks, secrets, or user data;
- automatic exception rebasing;
- a second editable JSON architecture format;
- production Code Atlas, Goobits, Workshop, Shell, Access, or Git integration.
