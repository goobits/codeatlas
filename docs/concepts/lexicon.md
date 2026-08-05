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
| **effect** | An evidenced way a workload may change or contact state: filesystem, process, network, database, or other target-visible mutation. | Domain evidence; `execution::target` corroboration | HTTP method and language type are not effect oracles. Unknown effect is effectful for policy. |
| **target class** | The kernel decision over locality, disposability, environment, effects, destinations, cleanup, and required capabilities. | `execution::target` | Canonical classes include local disposable, remote disposable/staging, production, and unknown. Domains do not rederive eligibility. |
| **capability** | A runtime control or evidence source positively proven by a probe, including its limits and environment fingerprint. | Capability provider and `execution` | Tool presence or operating-system name alone is not a capability. Missing required capability blocks. |
| **isolation** | Enforced confinement of writable filesystems, network, processes, environment, resources, secrets, and cleanup for one execution. | `execution::sandbox` | Environment-variable redirection, declarations, or reviewed authorization are not isolation. |
| **permit** | A finite pre-call reservation from the atomic execution ledger. | `execution::budget` | Consumed and rejected calls are not refunded. Do not use post-run counters as enforcement. |
| **lease** | Registered ownership of a managed resource with a bounded cleanup action and verification probe. | `execution::lease` | Dropping or killing a resource is not verified release. |
| **logical scratch root** | A plan-stable named requirement for disposable writable state whose physical external path is assigned only after authorization. | `execution` plan and sandbox | A logical root is not a checkout path, mount point, or permission to write arbitrary temporary state. |
| **secret reference** | A name and exact injection scope that authorize runtime lookup of an ambient secret without resolving or persisting its value during planning. | `execution::redaction`; domain configuration supplies references | A reference is not the secret value. Literal environment and header values are non-secret semantic evidence. |
| **redaction** | Fail-closed removal or non-capture of secret values and bounded sensitive payloads while preserving auditable references and scope. | `execution::redaction`; domain patterns | Masking after unrestricted persistence is not sufficient redaction. |
| **report** | A versioned domain result intended for inspection or policy, rendered without changing its semantic content. | Producing domain; format in `outputs` | A report is not automatically an observation, baseline, receipt, or reproducer. |

## Registered CodeAtlas annotation keys

| Key | JSON value | Meaning | Status |
|---|---|---|---|
| `codeatlas.node_id` | string | Opaque exact CodeAtlas graph node ID; consumers must not parse it as a foreign symbol or range. | Reserved; emission waits for the accepted external source-target schema. |

`codeatlas.symbol` is not registered. It remains conditional on the external
contract's final core fields. CodeAtlas publishes neither key in a local copy
of that external schema.

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
