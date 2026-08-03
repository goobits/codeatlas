# CodeAtlas engineering guidance

## Naming contract

Treat naming as part of the product model. One concept has one preferred word
across source, commands, report schemas, diagnostics, tests, and documentation.
When a better term replaces an older synonym, remove the old spelling instead
of keeping an alias or forwarding wrapper. CodeAtlas has no legacy surface to
preserve before v1.

- Name functions `verb_object[_qualifier]`, such as `resolve_target`,
  `classify_finding`, or `write_report`.
- Start predicates with `is_`, `has_`, `can_`, or `supports_`.
- Start constructors and conversions with `new` or `from_`.
- Reserve `on_` for actual event handlers.
- Put qualifiers after the concept, such as `graph_digest`, `source_snapshot`,
  or `cache_limit_bytes`.
- Name types for the semantic role and coordinate system of their data. Prefer
  names such as `ResolvedTarget`, `SourceEvidence`, and `ContextPage` over
  generic containers or implementation details.
- Keep shared analysis operations language-neutral. JavaScript, TypeScript,
  Svelte, Python, and Rust syntax belongs behind the adapter that owns it, not
  in the common contract name.
- Preserve terminology required by an external protocol only at that boundary,
  then translate it immediately into the CodeAtlas vocabulary.
- Name tests for the observable contract or regression they prove, not for the
  internal step they happen to execute.

Consistent names should make related implementations easy to find and make
duplicate concepts or misplaced ownership conspicuous to both maintainers and
CodeAtlas itself. Apply these rules to new and touched code. Do not launch a
repository-wide rename solely to conform old code.
