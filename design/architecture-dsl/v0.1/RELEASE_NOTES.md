# Atlas Architecture DSL v0.1 Design Package

Status: proposed, awaiting independent Phase 7 review

This is a review release of the architecture blueprint format and its
executable specification evidence. It is not a production Code Atlas release.

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

The independent reviewer should record one outcome:

- `accept`: approve the proposed v0.1 semantics for a separately planned
  production implementation;
- `revise`: return exact semantic or evidence gaps to this design package;
- `reject`: retain the package as historical design evidence without adopting
  it.

No outcome authorizes production implementation unless that work receives its
own scope, Architecture Impact Check, compatibility plan, and owner approval.
