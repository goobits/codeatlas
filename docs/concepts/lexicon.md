# CodeAtlas lexicon

This is the canonical vocabulary for commands, configuration, source, report
schemas, diagnostics, tests, and proposals. One concept has one preferred term.
An external protocol may retain its own word only inside the adapter that maps
it to this table.

Subject-qualify a term whenever its meaning or identity could otherwise be
ambiguous: `code target`, `HTTP contract`, `PostgreSQL observation`,
`architecture baseline`, or `performance regression`.

## Canonical terms

| Term | Exact meaning | Canonical owner | Qualification and retired wording |
|---|---|---|---|
| **subject** | The evidence domain to which a verb applies, such as code, HTTP, PostgreSQL, architecture, or tests. | CLI grammar and every report envelope | Do not use *group* or *noun bucket*. |
| **target** | The exact entity selected for inspection, planning, comparison, or execution, together with the evidence needed to resolve it again. | Subject resolver; execution classification in `execution::target` | Always qualify when crossing subjects. A URL or path alone is not necessarily a resolved target. |
| **target block** | A versioned cross-tool source address whose external owner defines interoperable core coordinates and a producer-annotation map. | External source-target contract; producer adapters | Carry only coordinates the producer knows. CodeAtlas fabricates neither UTF-16 ranges nor foreign symbol handles. |
| **annotation key** | A registered producer-namespaced key for tool-specific target-block evidence that cannot shadow or redefine a core field. | Publishing producer; CodeAtlas keys in the `codeatlas.` namespace | CodeAtlas keys use `codeatlas.<lower_snake_case_name>`. Do not emit unregistered keys or move shared coordinates into annotations. |
| **contract** | A declared or inferred set of valid inputs, outputs, effects, and invariants owned by one subject adapter. | `languages`, `http`, `postgres`, or `architecture` | There is no universal contract schema. Do not use *type* as a synonym: a type is only evidence within a contract. |
| **callable contract** | Language-neutral structured evidence for one code callable's overload signatures, receiver requirements, ordered parameters, result types, conservative effects, and exact block reasons. | `domain::CallableContract`; language adapters produce it | Target identity remains on the containing symbol or graph node. Display signatures are presentation evidence and are never reparsed to recover policy facts. |
| **PostgreSQL query contract** | Typed evidence for one exact static application query: content-addressed identity, placeholder order, statement class, parameter/result shapes, catalog bindings, referenced objects, effects, eligibility, and exact block reasons. | `postgres::model::PostgresQueryContract`; PostgreSQL source and catalog adapters produce it | It is not an OpenAPI or callable contract and never treats successful parsing or types as a semantic oracle. Dynamic SQL remains inventoried with block evidence. |
| **statement class** | The root PostgreSQL operation class resolved from one bounded token stream, including the effective root after a `WITH` clause. | PostgreSQL query classifier | Use `select`, `insert`, `update`, `delete`, `data_definition`, `transaction_control`, `privilege`, `administrative`, or `unknown`; do not reuse the retired coarse query *kind*. |
| **callable shape** | A deterministic lexicon comparison and display projection over structured callable signatures, parameter roles, receiver requirements, constructibility, and semantic types. | `lexicon::callable_shape`; source evidence remains in `CallableContract` | A callable shape is not another contract schema, does not parse a display signature, excludes implementation bodies, and cannot prove behavioral equivalence. |
| **repository lexicon** | One bounded report relating normalized naming evidence from explicitly selected code, HTTP, and PostgreSQL subjects while retaining each term's owner and provenance. | `lexicon::repository`; subject adapters only extract raw terms | It is not three subprocess reports, a universal domain graph, or an alias for `lexicon code`. Missing or partial subject evidence remains visible. |
| **term evidence** | One normalized repository-lexicon term plus its observed spelling, subject, semantic role, owner, exact target, source, confidence, and completeness. | `lexicon::subject_terms`; domain adapters supply raw observations | Normalization never erases the observed text or upgrades incomplete subject coverage. |
| **term relationship** | A deterministic cross-subject grouping based on an exact normalized term, a declared project concept, or an unsuppressed pinned domain relation. | `lexicon::repository` over the canonical concept policy | The only claim is `related_evidence`. It is never a semantic-equivalence or automatic consolidation claim. |
| **semantic sibling** | One target pair from an explicitly configured sibling set whose independently sourced roles merit a bounded conceptual-overlap evaluation. | `lexicon::semantic_siblings` | A semantic sibling is not a clone, duplicate, or equivalence claim. The analysis is advisory and never gates. |
| **nomination** | The exact deterministic relationship that admits one semantic-sibling pair to evaluation under a finite ceiling. | `lexicon::semantic_siblings::nominate` | Nomination evidence is not corroboration and is counted once even when several indexes nominate the same pair. |
| **corroboration** | One discrete kind of member-local evidence, independent of the nomination, that supports consolidation review for a semantic-sibling pair. | `lexicon::semantic_siblings::evaluate` | Count kinds, not repeated facts. Do not emit a similarity score, probability, percentage, or body-equivalence claim. |
| **counterevidence** | One mandatory named check that records `present`, `absent`, or `unknown` evidence against consolidating a semantic-sibling pair. | `lexicon::semantic_siblings::evaluate` | Every evaluation runs the complete checklist. Decisive evidence keeps targets separate; required unknown evidence stays inconclusive. |
| **disposition** | The deterministic advisory result of one semantic-sibling evaluation: `review_candidate`, `separate_by_evidence`, or `inconclusive`. | `lexicon::semantic_siblings` report model | A disposition is not a gate, suppression, automatic refactor decision, or proof of behavioral equivalence. |
| **constructibility** | Evidence describing whether CodeAtlas can materialize one callable receiver or parameter directly, only through a declared factory, not at all, or not yet known. | `CallableContract`; later language fuzz adapter consumes it | Result handling uses the result semantic type separately. A type annotation alone does not prove constructibility, and unknown never defaults to direct. |
| **fuzzability inventory** | Static, zero-call accounting for every discovered public callable, including its exact target, callable contract, deterministic corpus descriptors, supported oracle evidence, and block reasons. | `fuzz::code` over one source graph and reachability result | It is planning evidence, not a fuzz run or claim that a blocked callable can be invoked. Silent omission is a conformance failure. |
| **fuzz directive** | The one source-adjacent `@codeatlas-fuzz deny: <reason>` instruction attached by a language syntax owner to an exact callable or handwritten static SQL query. | `fuzz::directive`; Rust, Python, JavaScript/TypeScript, and PostgreSQL adapters own attachment | Only `deny` exists. A directive may contract what runs but can never grant capability, declare purity, waive isolation, or skip an internal branch. SQL uses config first; a leading comment is only a convenience for one static query. |
| **fuzz denial** | Maintainer evidence that an exact target must never be fuzzed, including under verified isolation against a disposable target. | Source fuzz directive or exact `fuzz.exclude` configuration; consuming subject reports `blocked_by_policy` | Ordinary effects and mutation remain typed effect and target-classification evidence. Do not add `allow`, `requires_disposable`, `skip`, `off`, or `ignore` aliases. |
| **boundary point** | One domain-neutral descriptor for a meaningful scalar, text, presence, variant, or collection edge used in deterministic corpus construction. | `fuzz::corpus` | A boundary point is not a language or PostgreSQL value. Subject adapters decide applicability and materialize native values. |
| **test witness** | Observed or declared evidence connecting a test context to one exact public code symbol, plus that symbol's shared callable evidence when known. | `testing` witness analysis and `codeatlas.testing-witness/*` | A witness is not proof that assertions cover every behavior. Use *unwitnessed* only when graph completeness supports it. |
| **schema version** | The immutable identity of one published artifact shape. New artifacts use one `codeatlas.<lower-kebab-kind>/v<positive-integer>` string as both payload version and schema ID. | Published-schema registry and the artifact's domain owner | Existing integer-plus-API report versions remain legacy facts. A new artifact never adds a parallel API version. |
| **evidence** | A deterministic, attributable fact used to build or evaluate a contract, finding, decision, or artifact. | Producing domain | Evidence is not proof of runtime behavior unless it was observed at runtime. |
| **finding** | A versioned analysis result with exact evidence, classification, target linkage, and gate eligibility. | Producing analysis domain | Avoid generic *issue* or *problem* in schemas. A finding may be informational. |
| **inventory** | A versioned, bounded list of currently known subject entities with explicit provenance and completeness. | Producing subject scan | An inventory is preparation evidence, not proof of exhaustive coverage or runtime reachability. |
| **hint** | Bounded advisory evidence offered to another decision owner without changing that owner's policy or claiming authority over its result. | Producing adapter; consuming product validates it | A hint is not a command, gate, target mutation, or inferred permission. |
| **case** | One generated input or input sequence submitted to a fuzz workload. | `fuzz` envelope; domain materialization | Schemathesis/Hypothesis *example* maps to `case` at the adapter boundary. Public `max_examples` is retired in favor of `max_cases`. |
| **call** | One budgeted interaction that can reach a target or perform target-scoped setup, validation, retry, reduction, or cleanup. | `execution::budget` | A case can consume multiple calls. Do not equate request, query, or invocation counts with the whole-run call budget. |
| **call category** | One kernel-owned reason for consuming a call permit, used identically by planned expectations and receipt usage. | `execution` model and `execution::budget` | The canonical categories are setup, readiness, authentication, generated case, stateful step, reduction, retry, validation, and cleanup. Domains do not add parallel counters. |
| **workload** | A typed, finite description of the work a domain asks the execution kernel to plan and run. | Domain adapter payload inside `ExecutionPlan` | Qualify by subject, such as HTTP fuzz workload or performance workload. |
| **plan** | An immutable, content-addressed, zero-call authorization candidate containing exact evidence digests, workload, effects, capabilities, destinations, and ceilings. | `execution` | A preview persists a plan. A command line or dry-run printout is not itself a plan. |
| **authorization** | Permission to execute one exact valid plan under current policy; it does not waive budgets, evidence checks, or missing capabilities. | `execution::policy` | Use `reviewed` or `preauthorized_isolated` modes. Do not use *force* or *override*. |
| **receipt** | An immutable content-addressed record of what one authorized execution actually did, consumed, cleaned, and produced. | `execution` | A receipt is not a report summary and cannot claim what enforcement did not observe. |
| **artifact reference** | A strict typed ID or explicit file path that resolves to one schema-checked, rehashed canonical artifact. | `execution::artifact` | Prefer `ArtifactRef` in source. Do not invent per-domain ID/path lookup rules. |
| **observation** | A versioned subject-qualified capture of current evidence at a stated time and environment, suitable for later baseline or diff operations. | Subject payload; identity/addressing in `execution::artifact` when execution-produced | `architecture observation`, `PostgreSQL observation`, and `performance observation` share the lifecycle meaning, not a payload schema. |
| **measurement** | One runtime quantity and its unit, sample context, and support/confidence evidence inside a performance observation or receipt. | `performance` or `execution::resource` | Do not call a static estimate a measurement. |
| **baseline** | A saved canonical artifact selected as the comparison reference for one subject and contract version. | Subject baseline payload; shared artifact identity where applicable | A baseline is intentional, not merely the oldest observation. |
| **diff** | A deterministic comparison of current evidence or an observation against an exact baseline/reference. | Subject diff owner | Use *diff* for the operation and report; avoid parallel *compare* commands. |
| **optimization candidate** | A source location or behavior nominated by static or measured evidence as worth investigating. | Static analysis or `performance` | `candidate` must be qualified in user-facing prose. It is not yet a hotspot. |
| **hotspot** | A target whose runtime contribution is established by reproducible measured evidence and a stated ranking rule. | `performance` | Static complexity, fan-in, allocation, or query shape alone cannot establish a hotspot. |
| **curve** | Ordered measurements across one controlled input-size or load axis, with all other planned variables held or recorded. | `performance` | Do not use *benchmark* for an unrepeatable series or mix axes in one curve. |
| **regression** | A statistically and operationally qualified degradation against an exact performance baseline and budget. | `performance` | A single slower sample is not a regression. Subject-qualify outside performance context. |
| **failure** | A reproducible contract or execution-oracle violation, distinct from an infrastructure block, partial run, cancellation, or unsupported capability. | Shared outcome taxonomy; domain oracle decides violation | Do not label budget exhaustion or missing isolation a target failure. |
| **reproducer** | A minimized, versioned domain input plus evidence and plan linkage sufficient to derive a new replay plan when preconditions still match. | `fuzz::reproducer`; replay in `execution` | *Crash file* and *seed file* are too narrow. A reproducer never bypasses normal planning or authorization. |
| **replay** | Zero-call derivation of a new plan from a valid reproducer, followed only by ordinary authorization and execution. | `execution` | Replay is not an execution shortcut and does not expand saved ceilings. |
| **effect** | An evidenced way a workload may change or contact state: filesystem, process, network, database, or other target-visible mutation. | Domain evidence; `execution::target` corroboration | HTTP method and language type are not effect oracles. `contained` means the verified disposable sandbox owns the effect; `uncontained` and `unknown` require review or block. PostgreSQL DML remains reviewed even when contained. |
| **target class** | The kernel decision over locality, disposability, environment, effects, destinations, cleanup, and required capabilities. | `execution::target` | Canonical classes include local disposable, remote disposable/staging, production, and unknown. Domains do not rederive eligibility. |
| **capability** | A runtime control or evidence source positively proven by a probe, including its limits and environment fingerprint. | Capability provider and `execution` | Tool presence or operating-system name alone is not a capability. Missing required capability blocks. |
| **isolation** | Enforced confinement of writable filesystems, network, processes, environment, resources, secrets, and cleanup for one execution. | `execution::sandbox` | Environment-variable redirection, declarations, or reviewed authorization are not isolation. |
| **isolation probe** | A private target-side executable that attempts named confinement and exhaustion cases and emits the strict evidence consumed by one sandbox capability provider. | `codeatlas-isolation-conformance` report model and probe; `execution::sandbox` evaluates it | Probe source, a built image, runtime metadata, or a fake fixture is not capability evidence. Only a nonce-bound result observed through the inspected live backend can grant capability. |
| **permit** | A finite pre-call reservation from the atomic execution ledger. | `execution::budget` | Consumed and rejected calls are not refunded. Do not use post-run counters as enforcement. |
| **lease** | Registered ownership of a managed resource with a bounded cleanup action and verification probe. | `execution::lease` | Dropping or killing a resource is not verified release. |
| **logical scratch root** | A plan-stable named requirement for disposable writable state whose physical external path is assigned only after authorization. | `execution` plan and sandbox | A logical root is not a checkout path, mount point, or permission to write arbitrary temporary state. |
| **secret reference** | A name and exact injection scope that authorize runtime lookup of an ambient secret without resolving or persisting its value during planning. | `execution::redaction`; domain configuration supplies references | A reference is not the secret value. Literal environment and header values are non-secret semantic evidence. |
| **redaction** | Fail-closed removal or non-capture of secret values and bounded sensitive payloads while preserving auditable references and scope. | `execution::redaction`; domain patterns | Masking after unrestricted persistence is not sufficient redaction. |
| **report** | A versioned domain result intended for inspection or policy, rendered without changing its semantic content. | Producing domain; format in `outputs` | A report is not automatically an observation, baseline, receipt, or reproducer. |

## Registered CodeAtlas annotation keys

| Key | JSON value | Meaning | Status |
|---|---|---|---|
| `codeatlas.node_id` | string | Opaque exact CodeAtlas graph node ID; consumers must not parse it as a foreign symbol or range. | Registered; emit only when the exact graph node is known. |
| `codeatlas.symbol` | string | Declared identifier of the addressed declaration, exactly as observed in source. | Registered; it is an annotation rather than a universal source-target core field. |

The external `agentspeak.source-target/v1` schema remains the sole target-block
owner. CodeAtlas validates against it from the neutral contracts repository and
publishes no local copy.

## Deliberately separate concepts

Keep the following boundaries even when fields look similar:

- `CallableContract`, `PostgresQueryContract`, and OpenAPI operations remain
  domain-owned. The shared fuzz corpus owns boundary descriptors and selection,
  not a universal value or contract engine.
- A shared outcome taxonomy names failure kinds; each domain oracle decides
  what constitutes a violation.
- Fuzz reports and performance reports remain distinct. Similar metadata is
  not sufficient reason to merge their payloads.
- A PostgreSQL administrative client and typed query session have different
  authority and remain separate.
- Static candidates and measured hotspots are related lifecycle states, not
  synonyms.

## Vocabulary changes

Change this table in the same phase that changes a public term. Audit source,
schemas, diagnostics, fixtures, proposals, and docs; remove the retired
spelling rather than preserving an alias. When an external protocol requires a
retired word, document the one adapter boundary that translates it.
