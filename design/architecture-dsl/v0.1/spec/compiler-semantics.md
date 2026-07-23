# Compiler Semantics

Status: proposed normative draft

This document specifies deterministic behavior for the private reference
validator and a possible future compiler. It does not define a public API.

## 1. Compilation inputs

A compilation request supplies:

- one or more local root documents;
- an allowed source-root set;
- an exact core vocabulary;
- resource limits;
- a mode: `governing` or `review`;
- an explicit validator version;
- an explicit `asOf` only when policy or conformance is evaluated.

The compiler never searches the network and never selects an implicit latest
version.

## 2. Restricted parsing

For every source document:

1. Read bytes within the configured file limit.
2. Require valid UTF-8.
3. Reject prohibited YAML features before semantic construction.
4. Reject duplicate mapping keys.
5. Parse into the restricted scalar, sequence, and mapping model.
6. Validate the static document schema.
7. Preserve structured diagnostic locations.

Comments are allowed but carry no normative meaning.

## 3. Vocabulary bootstrap

The fixed v0.1 meta-schema validates `ArchitectureVocabulary` shape. The
compiler then:

1. verifies vocabulary identity, version, and digest;
2. loads imported vocabularies in exact closure order;
3. rejects duplicate or redefined core terms;
4. constructs registries for object kinds, predicates, rules, adapters,
   authority kinds, and closed enums;
5. validates all other documents against those registries.

No declaration can extend the vocabulary implicitly.

## 4. Import resolution

Imports are resolved relative to the importing document's directory after
normalizing paths against configured roots.

For every import:

1. Reject a URL or network-like source.
2. reject absolute paths unless explicitly within an allowed root;
3. canonicalize the parent directory without following an escaping symlink;
4. load the materialized document;
5. verify `metadata.id`;
6. verify `metadata.architectureVersion`;
7. verify `canonicalModuleDigest`;
8. reject cycles;
9. enforce import-depth, module-count, and byte limits.

Resolution produces a deterministic lock entry. The source locator is not
module identity.

## 5. Declaration collection

The compiler visits the exact import closure and collects:

- active objects;
- active relations;
- active bindings;
- active constraints;
- exports;
- retired IDs;
- decision, approval, authority, and change-control metadata.

Exactly one module declares each stable ID. Imports never merge or override a
declaration.

The compiler rejects:

- duplicate stable IDs;
- active reuse of a retired ID;
- unresolved references;
- cross-module references without an import;
- cross-module references to private IDs;
- an export of an unknown or imported-only ID.

## 6. Typed semantic validation

After collection:

1. Validate object attributes against the selected object-kind definition.
2. Validate relation subject and object existence.
3. Validate predicate subject and object kind domains.
4. Validate binding adapter version and selector schema.
5. Validate binding target visibility and cardinality declaration.
6. Validate constraint rule and argument schema.
7. Validate decision, approval, authority, and change-control consistency.
8. Validate retirement and supersession references.
9. Validate document-kind-specific invariants.

Unknown fields, terms, and rules fail closed.

## 7. Decision eligibility

The governing compiler includes a declaration only when:

- `decision.status` is `accepted`;
- it is not retired;
- its governing authority satisfies the vocabulary;
- its approval satisfies its change-control policy;
- every imported dependency required by the declaration is eligible.

`proposed`, `unresolved`, `rejected`, and `superseded` declarations do not enter
the governing graph.

The review compiler may include:

- accepted declarations;
- proposed declarations;
- unresolved declarations.

It annotates every node and edge with decision state. Rejected, superseded, and
retired records remain outside the active review graph but remain available as
history and reserved identity.

## 8. Graph construction

The normalized graph contains:

- eligible object nodes;
- eligible relation edges;
- eligible constraint declarations;
- eligible binding declarations;
- module and authority provenance needed to explain every entry.

It does not contain:

- policy exceptions;
- observation facts;
- conformance results;
- rejected, superseded, or retired declarations;
- source comments or formatting;
- runtime instances.

The governing graph and review graph are separately canonicalized.

## 9. Constraint evaluation

Constraint rules are selected by exact name and version from the vocabulary.
The evaluator operates on normalized typed graph data.

V0.1 supports only closed rules documented by the core vocabulary. Evaluation
must:

- be deterministic;
- produce stable reason codes;
- identify affected declaration IDs;
- reject unknown arguments;
- respect explicit rule cardinality and path semantics;
- terminate within resource limits.

No user-supplied code or expression is evaluated.

## 10. Policy evaluation

Policy compilation is separate from graph compilation.

For each exception, validate:

- exact constraint ID and version;
- exact `baseClosureDigest`;
- exact affected IDs;
- narrow scope;
- governing authority;
- removal plan;
- expiration or release boundary.

At conformance time, an exception is:

- `applicable` when the affected closure matches and `asOf` is within bounds;
- `stale` when the relevant closure digest changed;
- `expired` when `asOf` is beyond its bound;
- `irrelevant` when no result matches its constraint and affected IDs;
- `rejected` when structurally or semantically invalid.

An exception may change the disposition of a conformance finding. It never
changes architecture membership or `governingGraphDigest`.

## 11. Observation validation

Observation documents are generated and validated independently. Each fact
must identify:

- extractor and version;
- source repository and commit;
- structured source location;
- deterministic or inferred mode;
- coverage;
- semantic fact content.

Inferred facts include bounded confidence. Deterministic facts must not claim
coverage their extractor cannot prove.

## 12. Conformance evaluation

Conformance receives exact typed inputs:

- `governingGraphDigest`;
- `architectureClosureDigest`;
- `policyClosureDigest`;
- `observationContentDigest`;
- vocabulary canonical digest;
- validator version;
- recorded `asOf`.

It checks facts against declarations and constraints, then applies relevant
policy exceptions. Every result records evidence, coverage, reason code,
severity, and exception disposition.

## 13. Diagnostics

Diagnostics contain:

```text
code
severity
message
document identity
source path
structured span
related stable IDs
```

Diagnostics sort by:

1. source path;
2. start line;
3. start column;
4. code;
5. stable ID;
6. message.

The same invalid input and validator version must produce byte-identical
diagnostic output.

## 14. Failure behavior

Compilation fails closed for:

- parsing and schema errors;
- vocabulary mismatches;
- import or digest errors;
- duplicate or retired ID violations;
- visibility violations;
- typed relation violations;
- unknown rules or adapters;
- resource-limit violations;
- ambiguous governing authority.

Unsupported observation coverage does not fail compilation. It produces
`unobserved` conformance rather than false absence.

## 15. Side effects

Validation is read-only except for explicit generation into the design
package's generated-example or manifest paths.

The private validator must not:

- modify source declarations;
- access the network;
- update product configuration;
- write runtime state;
- invoke Git;
- create processes beyond its own deterministic test and generation command.
