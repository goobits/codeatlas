# Conformance Semantics

Status: proposed normative draft

Conformance explains how observed implementation evidence compares with active
accepted architecture. It does not change either input.

## 1. Inputs

Every conformance document identifies:

```yaml
conformanceInputs:
  governingGraphDigest: sha256:...
  architectureClosureDigest: sha256:...
  policyClosureDigest: sha256:...
  observationContentDigest: sha256:...
  vocabularyDigest: sha256:...
  validatorVersion: codeatlas-reference-validator/0.1
  asOf: "2026-07-23T00:00:00Z"
```

The validator rejects missing, malformed, or mismatched inputs. `asOf` is an
explicit input and never defaults to wall-clock time.

## 2. Observation facts

Each fact includes:

- a stable fact ID;
- an observed kind or predicate;
- observed subject and object identities where applicable;
- deterministic or inferred mode;
- extractor ID and version;
- repository and source commit in the envelope;
- one or more structured source paths and spans;
- coverage;
- confidence for inferred facts.

Structured source evidence follows the current Code Atlas model:

```yaml
sourceLocations:
  - path: packages/tabby/src/index.ts
    span:
      start:
        line: 1
        column: 1
      end:
        line: 42
        column: 2
```

Stringly typed `file.ts:1-42` locations are not valid.

## 3. Coverage

Coverage is explicit:

- `complete`: the extractor can prove presence and absence for the declared
  feature within the stated scope;
- `partial`: some relevant constructs were inspected, but absence cannot be
  proven for the full scope;
- `unsupported`: the extractor does not support the language, construct, or
  repository type;
- `unknown`: coverage cannot be established from the evidence.

Coverage also identifies:

- repository scope;
- language or source type;
- observed feature;
- included roots;
- excluded roots;
- deterministic limitations.

Unsupported or unknown coverage produces `unobserved`, not `absent`.

## 4. Result states

### matched

Deterministic evidence inside declared coverage satisfies the declaration or
constraint.

### partial

Evidence satisfies part of a declaration, or coverage is partial and some
required facts are present.

### absent

A required declared construct is not present, and a deterministic extractor has
complete coverage capable of proving that absence.

### conflicting

Deterministic in-coverage evidence directly contradicts a declared value,
relationship, cardinality, or owner.

### unexpected

Deterministic in-coverage evidence identifies a governed public construct or
relationship that policy requires to be declared but is not.

### unobserved

No supported complete observation can evaluate the declaration. Unsupported,
unknown, or insufficient coverage produces this state.

### ambiguous

Evidence has multiple plausible mappings, incompatible inferred facts, or
insufficient identity to select one declaration deterministically.

## 5. Hard-gate eligibility

A result may independently become an error only when:

- the source fact is deterministic;
- the extractor version is known;
- the relevant feature coverage is complete;
- the binding is accepted and deterministic;
- the reason follows a closed conformance rule;
- no applicable accepted exception changes its disposition.

Inferred evidence is advisory. It can support review but cannot independently
create a hard `absent`, `conflicting`, or `unexpected` failure.

## 6. Matching

The validator evaluates in this order:

1. Select governing declarations.
2. Resolve accepted deterministic bindings.
3. Select observation facts within the binding scope.
4. Establish relevant coverage.
5. Evaluate object, relation, and constraint expectations.
6. Assign a preliminary result state and reason code.
7. Evaluate exact policy exceptions using recorded `asOf`.
8. Assign final severity and exception disposition.
9. Sort results deterministically.

One observation fact may support multiple results when the vocabulary declares
that relationship. Every reuse is explicit in result evidence.

## 7. Result record

Each result contains:

```yaml
id: codeatlas.conformance.tabby-package
declarationId: goobits.app.tabby
state: matched
severity: advisory
reasonCode: binding.exact-match
evidence:
  factIds:
    - codeatlas.fact.npm-package-tabby
  coverageIds:
    - codeatlas.coverage.npm-packages
exceptions:
  applied: []
  stale: []
  expired: []
  irrelevant: []
  rejected: []
explanation: The accepted package binding matched one observed package.
```

The human explanation is generated from structured facts. It is not the
authoritative result.

## 8. Exception evaluation

An exception remains outside the governing graph and is evaluated only after a
preliminary finding exists.

An exception applies only when:

- its decision is accepted;
- its approval and authority are valid;
- its constraint ID and version match;
- its `baseClosureDigest` equals the exact affected closure;
- its affected IDs include the result;
- its scope matches;
- `asOf` is before its expiration or within its release boundary;
- its removal plan is present.

Changing an unrelated module does not stale the exception. Changing the
relevant affected closure does.

Exception dispositions:

- `applied`: valid and changes finding severity or gate behavior;
- `stale`: affected closure changed;
- `expired`: time or release boundary passed;
- `irrelevant`: valid but does not match this result;
- `rejected`: exception declaration is invalid or unauthorized.

No automatic rebasing occurs in v0.1.

## 9. Severity

The closed severity set is:

- `error`
- `warning`
- `advisory`

Policy may lower a preliminary error to warning or advisory through an
applicable exception. The underlying result state remains visible.

An exception cannot convert a conflict into `matched`.

## 10. Review graph

Conformance evaluates the governing graph only.

A persisted review graph may support impact analysis or design review. It can
show how proposed or unresolved declarations would compare with evidence, but
its results must be labeled non-governing and cannot satisfy a release gate.

## 11. Determinism

Results sort by:

1. severity order: error, warning, advisory;
2. declaration ID;
3. state;
4. reason code;
5. result ID.

Evidence IDs and exception lists sort lexically. Repeated evaluation with exact
inputs and `asOf` produces byte-identical output and
`conformanceResultDigest`.

## 12. Failure and uncertainty

The system must prefer an honest uncertainty state over a false claim:

- lack of extractor support becomes `unobserved`;
- incomplete scope becomes `partial` or `unobserved`;
- multiple candidate bindings become `ambiguous`;
- inferred evidence remains advisory;
- stale exceptions remain visible and do not apply;
- an invalid policy does not modify the governing result.
