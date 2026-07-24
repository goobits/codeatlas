# Security and Resource Limits

Status: accepted normative specification

Architecture declarations, imports, generated observations, and repository
paths are untrusted input. Validation fails closed before semantic use.

## 1. Restricted YAML profile

V0.1 accepts a YAML 1.2-compatible subset with:

- UTF-8 text;
- mappings with string keys;
- sequences;
- strings;
- signed 64-bit integers;
- booleans;
- null;
- comments with no normative meaning.

It rejects:

- duplicate mapping keys;
- anchors;
- aliases;
- merge keys;
- custom tags;
- executable templates;
- environment interpolation;
- implicit timestamp coercion;
- binary scalars;
- floating-point values;
- NaN and infinity;
- multiple YAML documents in one file;
- unknown top-level or nested schema fields.

Quoted strings such as `"${TOKEN}"` are ordinary literal text only when the
schema permits that string. No interpolation occurs.

## 2. Default limits

The production architecture parser, schemas, and document loader enforce these
v0.1 defaults:

| Resource | Limit |
| --- | ---: |
| One source document | 1 MiB |
| Total source bytes per invocation | 64 MiB |
| YAML nesting depth | 64 |
| Import depth | 32 |
| Documents in one closure | 4,096 |
| Active and historical declaration IDs | 250,000 |
| Relations | 500,000 |
| Constraints | 50,000 |
| Policy exceptions | 10,000 |
| Source locations per observed fact | 1,024 |
| Identifier length | 200 bytes |
| String scalar length | 1 MiB |
| Diagnostics retained | 10,000 |

An invocation may choose lower limits. Raising production limits requires
measured evidence and separate approval.

## 3. Parse complexity

The parser must:

- count input bytes before allocation proportional to declared content;
- bound nesting and collection sizes;
- reject duplicate keys during parsing, not after last-write-wins behavior;
- reject alias expansion entirely;
- stop after the diagnostic limit;
- avoid recursive algorithms whose stack depth follows untrusted graph depth;
- use bounded graph traversal for cycle and path rules.

## 4. Path confinement

All materialized imports and source evidence paths are resolved within explicit
allowed roots.

The architecture document loader:

1. rejects a network scheme;
2. rejects NUL bytes;
3. normalizes separators;
4. rejects traversal beyond the configured root;
5. resolves symlinks component by component;
6. rejects a final canonical path outside every allowed root;
7. records repository-relative paths in generated output;
8. never emits local home or temporary-directory prefixes.

Absolute paths are accepted only as invocation inputs identifying allowed roots.
They cannot appear as portable declaration identity.

## 5. Import integrity

Imports require:

- exact module ID;
- exact architecture version;
- exact canonical module digest;
- a locally materialized source;
- an acyclic dependency;
- an allowed-root path.

The architecture compiler rejects digest substitution, identity substitution,
source swapping, floating versions, implicit latest, and network fallback.

Lockfiles record exact resolution but never authorize a changed import.

## 6. Generated observation trust

Generated status does not imply trust.

Observation consumers verify:

- schema;
- generator identity and version;
- repository identity;
- source commit format;
- content and envelope digests;
- structured source path confinement;
- extractor coverage declaration;
- deterministic versus inferred mode.

An observation from an unknown extractor may be displayed as evidence but
cannot independently produce a hard conformance gate.

## 7. Authority trust

The DSL records typed authority references but does not authenticate principals
or issue runtime grants.

The architecture validator checks:

- authority kind exists;
- artifact ID and version are structured;
- required digest is present under the selected change-control policy;
- locator remains within an allowed repository root;
- governing authority is permitted for the decision and scope.

Signature verification and organization identity are future production
concerns. V0.1 must not imply cryptographic authorization it does not perform.

## 8. Secrets and sensitive data

Declarations and generated examples must not contain:

- credentials;
- access tokens;
- private keys;
- cookies;
- personal journal content;
- runtime user records;
- unredacted environment variables;
- local absolute paths;
- process environment dumps.

Keys named `secret`, `token`, `password`, `privateKey`, or equivalent are
rejected in accepted architecture declaration data unless the vocabulary
explicitly defines a non-secret architectural concept and review approves it.

Observation generation must support source-location disclosure policies. A
consumer may receive stable IDs and redacted spans instead of source excerpts.

## 9. Constraint safety

The constraint language has:

- closed rule names;
- typed bounded arguments;
- no general expressions;
- no regex by default;
- no scripts or templates;
- no file reads;
- no network calls;
- no process execution.

`no_path` and `acyclic` use bounded iterative traversal. A resource-limit
failure is a validation error, not a successful rule result.

## 10. Denial-of-service behavior

On a limit violation, the architecture operation:

- stops the affected operation;
- emits one stable reason code with the measured and allowed values;
- does not continue with a partial governing graph;
- cleans temporary in-memory state;
- leaves source files unchanged.

It must not retry with relaxed limits automatically.

## 11. Generated files

Generated output writers:

- write only to an explicit caller-selected output path;
- through a temporary sibling file;
- after complete validation;
- with atomic replacement where supported;
- with stable LF line endings;
- without following an output symlink.

The specification manifest excludes itself and includes only files under the
accepted specification root.

## 12. Implementation boundary

Architecture implementation lives under the production
`src/architecture/` domain and is exposed only through the versioned CLI and
JSON contracts. It performs no network import resolution, accepted
architecture mutation, Goobits runtime integration, or runtime authority
operation.

The private Phase 7 validator was retired after its semantics received
production coverage. Its exact reviewed source remains recoverable from commit
`8f5a2df`; it is not a second dependency or execution path.

## 13. Failure posture

Ambiguous authority, unknown vocabulary, incomplete imports, digest mismatch,
and malformed policy fail closed.

Incomplete observation coverage fails honest:

- it does not become absence;
- it does not invent a match;
- it produces `unobserved`, `partial`, or `ambiguous` evidence.
