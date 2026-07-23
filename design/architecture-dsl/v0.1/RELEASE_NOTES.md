# Atlas Architecture DSL v0.1 Design Package

Status: accepted Phase 7 review candidate

This records the review release of the architecture blueprint format and its
executable specification evidence. The owner later promoted its normative
artifacts to `spec/architecture/v0.1/`. It was not a production Code Atlas
release.

## Included

- six proposed document kinds;
- restricted-YAML authoring rules;
- stable declaration identity, imports, exports, and retirement;
- a closed typed vocabulary;
- governing and non-governing review graphs;
- policy and controlled exception semantics;
- typed digest and canonicalization rules;
- observation provenance and coverage;
- declared-versus-observed conformance semantics;
- Draft 2020-12 schemas expressed as YAML;
- Tabby/Shelly and Workshop/Code Atlas examples;
- valid and invalid fixtures;
- a private unpublished Rust reference validator;
- deterministic generated observation and conformance examples;
- a generated stable file manifest;
- requirements traceability and reproducible validation evidence.

## Deliberately not included

- a production compiler;
- a public Code Atlas API or CLI command;
- provider or runtime integration;
- changes to Goobits, Workshop, Access, Shell, Git, or coordination;
- network imports;
- automatic architecture acceptance or mutation;
- a second editable architecture format.

## Compatibility

No released Code Atlas contract changes. Existing `codeatlas.json`
configuration remains unchanged. The private reference validator is not a
supported API and creates no compatibility promise.

The earlier v0alpha1 conversational proof remains historical evidence only. It
is not an alternate editable authority. Production promotion, if approved,
requires a separate migration and release proposal.

## Review decision

The reviewer returned `accept` with one required clarification:
`ArchitectureChange` must never enter either architecture graph or be replayed
to reconstruct current architecture. Commit `8f5a2df` contains that
clarification and the final reproducible review evidence.

Production implementation received a later, separate owner authorization.
