# Canonicalization and Hashing

Status: accepted normative specification

Canonicalization makes semantically identical inputs reproducible and gives
different kinds of evidence distinct identities.

## 1. Digest syntax

All v0.1 digests use SHA-256 and serialize as:

```text
sha256:<64 lowercase hexadecimal characters>
```

Hash inputs use a domain separator:

```text
atlas.codeatlas.dev/<digest-kind>/v0.1\n
```

The canonical payload bytes follow the newline. Domain separation prevents two
different digest families with identical payload bytes from being confused.

## 2. Source document digest

`sourceDocumentDigest` covers the exact source bytes after the file has passed
the UTF-8 and maximum-size checks.

It includes:

- comments;
- whitespace;
- line endings;
- key order;
- all authored metadata.

Its purpose is exact source identity, not semantic equivalence.

## 3. Canonical semantic representation

Parsed YAML is converted to a JSON-compatible semantic model:

- mappings become objects;
- sequences become arrays;
- strings remain strings;
- booleans remain booleans;
- null remains null;
- normative numeric values are signed 64-bit integers only.

Floating-point values, NaN, infinity, binary scalars, timestamps created by
implicit YAML coercion, and implementation-specific scalar tags are prohibited.

Canonical bytes use RFC 8785 JSON Canonicalization Scheme after the
field-specific normalization below.

## 4. Field-specific normalization

Map keys sort according to RFC 8785.

These arrays are sets and sort by their canonical element bytes:

- imports;
- exports;
- maintainers;
- governing authority references;
- supporting authority references;
- affected IDs;
- superseded IDs;
- expected adds, removes, and required effects;
- source inputs;
- source locations;
- applicable exception IDs.

Duplicate set entries fail validation rather than disappearing silently.

Arrays whose order is explicitly part of meaning retain order. V0.1 uses
ordered arrays only for:

- human-authored migration steps;
- human-authored verification steps;
- path predicate sequences in a `no_path` rule.

Identifiers are ASCII lowercase and require no Unicode normalization. Other
strings preserve their Unicode scalar values. Canonical timestamps use RFC 3339
UTC with `Z` and second precision unless the schema explicitly permits greater
precision.

Source locators are normalized to forward-slash repository-relative paths.
They remain part of provenance. Stable artifact identity remains separate.

## 5. Canonical module digest

`canonicalModuleDigest` covers the canonical semantic content of one complete
document after:

- restricted parsing;
- static schema validation;
- vocabulary-independent envelope normalization;
- set ordering.

It excludes no authored semantic fields. A locator or description change
therefore changes the module digest even though stable IDs remain unchanged.

Generated envelope fields are included for generated document module identity
unless a more specific digest family below excludes them.

## 6. Import closure digest

`importClosureDigest` identifies one document's exact transitive materialized
import closure.

The payload is a sorted array of:

```json
{
  "moduleId": "goobits.package.space",
  "architectureVersion": 4,
  "canonicalModuleDigest": "sha256:..."
}
```

The root document is included. Source paths are excluded because identity and
content digests already identify the modules.

## 7. Architecture closure digest

`architectureClosureDigest` identifies the exact root architecture compilation
bundle before decision filtering.

Its payload contains:

- sorted root module IDs;
- each root's `importClosureDigest`;
- exact vocabulary ID, version, and canonical digest;
- compiler semantic version.

It includes proposed, unresolved, rejected, superseded, and retired authored
records because those records can affect review and identity reservation. It
does not include policy documents or observations.

## 8. Governing graph digest

`governingGraphDigest` covers:

- active accepted object nodes;
- active accepted relation edges;
- active accepted binding declarations;
- active accepted constraint declarations;
- graph provenance needed to identify declaring modules and governing
  authority.

It excludes:

- policy rules and exceptions;
- proposed and unresolved declarations;
- rejected and superseded declarations;
- retired declarations;
- observations;
- conformance results;
- volatile timestamps.

Changing an exception or `asOf` cannot change this digest.

## 9. Policy closure digest

`policyClosureDigest` covers the exact canonical policy documents and their
materialized imports, including exception declarations.

It excludes the evaluation-time `asOf`. The same policy can therefore be
evaluated reproducibly at different recorded times without pretending the
policy source changed.

## 10. Review graph digest

`reviewGraphDigest` exists only when a review graph is persisted, compared,
cited, or used for an impact decision.

It covers:

- accepted active declarations;
- proposed active declarations;
- unresolved active declarations;
- their decision and approval annotations;
- provenance needed to identify declaring modules and supporting authority.

Rejected, superseded, and retired declarations remain outside the active graph.
Transient review projections need no persisted digest.

## 11. Observation digests

`observationContentDigest` covers semantic observed facts:

- observed object and relation facts;
- structured source evidence;
- extractor identity and version per fact;
- deterministic or inferred mode;
- confidence where applicable;
- coverage declarations.

It excludes volatile capture metadata:

- `observedAt`;
- generation command;
- process identity;
- local absolute paths;
- repository checkout location.

Two scans with identical semantic facts have the same
`observationContentDigest` even when capture times differ.

`observationEnvelopeDigest` covers:

- `observationContentDigest`;
- repository identity;
- source commit;
- `observedAt`;
- generator identity and version;
- generation command;
- normalized source input locators.

## 12. Conformance result digest

The `conformanceResultDigest` payload contains:

```yaml
conformanceInputs:
  governingGraphDigest: sha256:...
  architectureClosureDigest: sha256:...
  policyClosureDigest: sha256:...
  observationContentDigest: sha256:...
  vocabularyDigest: sha256:...
  validatorVersion: codeatlas.tool.architecture-conformance/0.1
  asOf: "2026-07-23T00:00:00Z"
result: {}
```

The result is the canonical structured conformance output, including applicable,
stale, expired, irrelevant, and rejected exception dispositions.

## 13. Lockfile

The generated import lockfile contains:

- root document identities;
- exact source locators;
- exact architecture versions;
- `sourceDocumentDigest`;
- `canonicalModuleDigest`;
- `importClosureDigest`;
- vocabulary identity and digest;
- validator identity and version.

The lockfile is generated, not editable authority. It sorts entries by module
ID and excludes local absolute paths.

## 14. Manifest

`MANIFEST.sha256`:

- is generated from committed specification-package files;
- excludes itself;
- sorts paths by UTF-8 byte order;
- uses forward-slash relative paths;
- hashes exact checked-in bytes;
- contains one lowercase SHA-256 and two spaces before each path;
- uses LF line endings.

The manifest proves package reproduction. It does not replace typed semantic
digests.

## 15. Reproducibility

Given identical source bytes, root selection, vocabulary, validator version,
resource-limit profile, and `asOf`, repeated runs must produce byte-identical:

- normalized graphs;
- lockfiles;
- observations generated from the same source facts;
- conformance results;
- diagnostics;
- generated examples;
- manifest.
