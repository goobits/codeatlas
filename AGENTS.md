# CodeAtlas engineering rules

## Start and context restart

At the start of every agent session, and again after a context compaction,
summary handoff, crash, or resumed long-running task:

1. Re-read this file and the active child proposal in full before changing
   source. The canonical ordering tracker is the `Canonical v1 completion
   tracker` in
   [`proposals/codeatlas-fuzz-performance.md`](proposals/codeatlas-fuzz-performance.md).
   Read its introduction, current verdict, order rules, and first incomplete
   phase. Completed foundations and old verification entries are audit history;
   do not reload them merely to reconstruct context.
2. Inspect Git status plus staged and unstaged diffs. Do not trust remembered
   cleanliness or a handoff narrative.
3. Verify live processes, exact binaries, external state roots, capabilities,
   and artifact IDs from actual state.
4. Resume the first incomplete checklist item after the latest verified
   checkpoint. Do not repeat completed work merely to rebuild context.
5. Keep that checklist current at phase transitions and before compaction.
   Put durable operating rules here, vocabulary in the lexicon, public
   behavior in README/specifications, and temporary evidence in the active
   proposal.

`AGENTS.md` is the auto-discovered operational entrypoint. Do not create a
generic wisdom or scratch document that an agent must remember to discover.
Repository memory is useful only when it is concise, actionable, and attached
to one authoritative owner.

## Product contract

CodeAtlas gathers deterministic evidence about code, HTTP, PostgreSQL, tests,
and declared architecture; applies explicit policy; and emits versioned,
inspectable reports and artifacts. It is read-only with respect to analyzed
source. An explicit output, baseline, generated configuration, or private
execution artifact is not source mutation, but its path and ownership must be
visible in the command contract.

CodeAtlas may exercise a caller-authorized disposable target through the
shared execution kernel. Planning remains zero-call. Execution is finite,
isolated, capability-checked, rate-limited, and receipted. CodeAtlas does not
infer permission to write a checkout, contact arbitrary services, use ambient
credentials, or run unplanned commands.

TypeMill is the separate semantic-mutation product. CodeAtlas may provide a
versioned finding, digest, and exact target; it neither edits consumer source
nor absorbs TypeMill's planning, transaction, recovery, or receipt semantics.
Keep the seam artifact-first and dependency-free unless identical pure logic
and ownership have first been demonstrated.

There are no pre-v1 compatibility users to preserve. When a command, field, or
term is replaced, remove it and update all callers, schemas, tests, and docs in
the same accepted phase. Do not add aliases, forwarding wrappers, deprecated
spellings, `--force`, unlimited sentinels, or dual artifact formats.

Every new public JSON artifact registers its owning model in
`src/published_schemas.rs` before it can ship. Use one
`codeatlas.<lower-kebab-kind>/v<positive-integer>` schema-version string as both
payload identity and schema ID; do not add a parallel API version. Register a
`codeatlas.*` annotation key and its value shape in the canonical lexicon
before emission. External schemas remain external: bind to an accepted
published contract, never vendor or reconstruct a draft locally.

## Architecture and ownership

- `src/cli` owns command parsing and presentation-neutral argument types. It
  does not own analysis, policy, execution, persistence, or rendering.
- `src/commands` owns application orchestration and exit semantics. Commands
  call domain owners rather than reimplementing their rules.
- `src/config` owns strict `codeatlas.json` parsing, defaults, resolution, and
  validation. Unknown fields fail; proposals do not create parallel config.
- `src/domain` owns the language-neutral code-evidence primitives. Language
  syntax and resolution stay in `src/languages`.
- `src/source_discovery` and `src/source_index` own bounded source discovery,
  immutable indexed facts, cache identity, and index telemetry.
- `src/analysis`, `src/dead_code`, `src/context_slice`, `src/testing`, and
  `src/lexicon` own their distinct static questions and report models.
- `src/http`, `src/postgres`, and `src/architecture` own subject-specific
  contracts, evidence, policy, and adapters. A shared outcome vocabulary does
  not make their schemas or oracles interchangeable.
- `src/execution` owns cross-domain plans, authorization, target
  classification, budgets, isolation, artifact identity, redaction, leases,
  resource sampling, and receipts.
- `src/fuzz` owns domain-neutral fuzz envelopes, boundary descriptors,
  deterministic selection, outcome kinds, and reproducer linkage. Domains own
  contract mapping, value materialization, generation strategy, and oracles.
- `src/performance` owns measured curves, optimization candidates, hotspots,
  regressions, and performance baselines. Static analysis may nominate a
  candidate but may not label it a hotspot without runtime evidence.
- `src/outputs` owns rendering only. Report meaning stays with the producer.
- `src/external_tool.rs` owns exact external-tool discovery, pinned
  provisioning, fingerprints, install locking, and capability probes.

These future owners become binding when their accepted proposal introduces
them. Until then, do not create a temporary owner under an adapter.

Keep adapters thin and dependency direction pointed toward contracts and
primitives. Language- or subject-specific code may depend on shared contracts;
the shared layer must not depend back on an adapter. A directory that already
exists does not automatically own a new concern.

Do not create godmodules. A large module carries the burden of proof: retain it
only when splitting would weaken cohesion, invariant enforcement, or execution
safety. Convenience, history, common imports, and broad topic similarity are
not sufficient. Before extending a mixed-responsibility file, extract the
independent concern behind one focused private owner.

## Naming grammar

Treat names as part of the product model. One concept has one preferred term
across commands, config, source, schemas, diagnostics, tests, and docs. The
canonical meanings live in
[`docs/concepts/lexicon.md`](docs/concepts/lexicon.md).

- Name functions `verb_object[_qualifier]`, such as `resolve_target`,
  `classify_finding`, `reserve_call_permit`, or `write_receipt`.
- Start predicates with `is_`, `has_`, `can_`, or `supports_`.
- Start constructors and conversions with `new` or `from_`.
- Reserve `on_` for actual event handlers.
- Put qualifiers after the concept: `graph_digest`, `source_snapshot`, and
  `cache_limit_bytes`.
- Name a type for its semantic role and coordinate system, not its container or
  implementation detail.
- Translate protocol-owned words at the adapter boundary. For example,
  Schemathesis `example` becomes a CodeAtlas `case`.
- Name tests for the observable contract or regression they prove.

When replacing a synonym, audit the repository and remove the retired spelling
instead of hiding conceptual duplication behind a wrapper. Retain an external
term only at the protocol boundary where it is required.

## Evidence pipeline and performance discipline

The static evidence pipeline is `discover -> index -> analyze -> render`, with
one owner per stage:

- Discover owns bounded path selection and ignore policy.
- Index owns parse-once immutable facts, graph identity, and reusable
  snapshots.
- Analyze owns a typed question over one snapshot and produces a versioned
  domain report.
- Render owns deterministic serialization or presentation without changing
  report meaning.

Pass typed values across stage boundaries. Avoid repeated repository scans,
file reads, parses, manifest loads, schema loads, and graph construction within
one command. Reuse one snapshot or one shared bounded traversal. Borrow by
default and clone only at an explicit ownership boundary. Concurrency, queues,
captured output, pagination, and cancellation must be bounded and deterministic
at the public boundary.

Do not add a cache without a named owner, key, invalidation rule, size ceiling,
and hit/miss/eviction evidence. A cache is not a substitute for removing
duplicate work. Instrument stable stages with consistent elapsed time, item
count, byte count, and RSS fields where supported.

Static complexity, fan-in, fan-out, allocation, blocking-I/O, query-shape, and
call-path evidence can nominate an `optimization candidate`. Only measured
runtime evidence can establish a `hotspot` or `regression`. An optimization
must not change canonical evidence, IDs, findings, exit decisions, or artifact
digests unless the accepted contract explicitly changes.

Hot-path changes need explicit cold- and warm-run budgets on representative
workspaces. Record the dataset, file/symbol/byte scale, command, environment,
and allowed regression beside the benchmark or test. Never make an
unreproducible timing anecdote a product claim.

Keep command evidence bounded. Write complete output to an external task log
and surface the command status, elapsed time, relevant counts or digests, and a
short failure tail. Concision must not hide whether a required check ran, what
it covered, or why it failed.

Treat paid remote runners and long-lived machine time as finite budgets. Finish
every locally knowable check before dispatch, refuse an equivalent active or
completed run, and rerun the narrowest externally required boundary before a
full matrix when that can establish the fix. A cache must preserve useful work,
not merely report a hit: prevent an early empty immutable entry from blocking a
later populated generation, and retain exact compatibility and byte evidence.
At every remote checkpoint record the service/run ID, exact revision, last
completed stage, cache identity/result, artifact digest/path, and concise
failure boundary in the active proposal. After compaction, query that state
once and resume it; do not redispatch work or reload full logs to reconstruct
history.

## Execution safety

All target calls and managed processes use the shared execution kernel. The
domain supplies a typed workload and evidence; it does not create a private
executor, budget, sandbox, artifact store, redactor, or cleanup registry.

The required lifecycle is:

`resolve -> classify -> plan -> authorize -> isolate -> execute -> clean -> receipt`

- Target or replay planning persists an immutable plan and makes zero target
  calls.
- Changed source, config, target, contract, tool, engine, or policy evidence
  invalidates the plan.
- Every call acquires a finite pre-call permit. Failures and rejected calls are
  not refunded; retries and shrinking consume new permits.
- Rate, concurrency, elapsed, CPU, RSS, process, descriptor, output, result,
  response, and artifact budgets are finite and enforced where required.
- Missing required isolation blocks before the first call. Reviewing a plan
  authorizes execution; it cannot manufacture a missing capability.
- The checkout and runtime roots are read-only. One external disposable
  scratch root is the only writable mount.
- Network, processes, environment variables, and secret references are denied
  by default and allowlisted exactly. Secret values never enter artifacts,
  arguments, captured output, or logs.
- Every process, proxy, container, temporary database, and scratch root has one
  cleanup lease. Cleanup is budgeted, runs on every outcome, is verified, and
  is recorded in the receipt.
- Budget exhaustion, incomplete cleanup, unavailable isolation, cancellation,
  and interruption can never produce `passed`.

Single-shot execution is merely plan persistence plus the same executor in one
process. It is eligible only when the kernel corroborates a checked-in target
as preauthorized, fully local, disposable, isolated, and non-effectful. Remote,
production, unknown, mutating, or policy-exception targets require a reviewed
plan and may still be blocked.

Do not use environment redirection as a sandbox, trust a generator as the sole
call counter, fall back to host execution, forward a container-control socket
into a child, or infer safety from HTTP methods or type declarations.

## Quality lenses

Review every phase through all applicable lenses, not only whether tests pass:

- **Conceptual integrity:** one vocabulary and one meaning per abstraction.
- **Consolidation:** one canonical owner; no parallel helpers or schemas.
- **Performance:** bounded I/O, parsing, allocation, concurrency, calls, and
  persisted state.
- **Correctness and determinism:** identical evidence yields identical output.
- **Reliability and cleanup:** interruption leaves no unowned process or state.
- **Boundaries:** dependency direction holds and no godmodule accumulates
  policy.
- **Testability and conformance:** observable contracts have acceptance
  coverage at the smallest owning layer.
- **Observability and auditability:** stable evidence, metrics, plans, errors,
  artifacts, and receipts explain every decision.
- **Security:** least authority, checkout confinement, explicit destinations,
  private artifacts, redaction, and no secret persistence.
- **Simplicity:** smallest deletable design, with no legacy compatibility
  surface or speculative abstraction.

## Test-value gate

Optimize for confidence per durable test line, not a coverage quota. Every test
must name a unique observable contract or regression risk and use the smallest
layer that proves it. Delete or consolidate a test when no plausible regression
can be named or another test already proves the same behavior.

Tests are deterministic and isolated: no timing sleeps, external network,
ambient ordering, leaked processes, or leaked temporary state. Put shared
semantics in one conformance table; add language- or domain-specific cases only
for syntax, protocol, or oracle differences. A regression test should fail
before its fix when practical.

Prefer focused behavioral assertions over broad snapshots. Normalized golden
reports, plans, observations, baselines, reproducers, and receipts are
appropriate when canonical artifact shape is the contract. Helpers may remove
setup repetition but must not hide the operation, evidence, or assertion.
Record the owner, reason, and time budget for every intentionally slow or live
test.

Execution conformance tests must observe the target side when proving that a
blocked call, write, process, or destination never arrived. A receipt-only
assertion is insufficient.

## Dogfooding and TypeMill

Use CodeAtlas itself after each relevant phase. At minimum, gather source scan,
code check, usage, exact-target inspection, lexicon, test inventory, and test
witness evidence. Classify findings honestly; do not weaken a gate or suppress
a real finding merely to make self-analysis green. Performance claims require
the corresponding observation or baseline evidence.

Perform CodeAtlas source refactors through the installed stable `mill` when it
advertises the exact required capability:

1. Preview the mutation or command-list plan; source must remain unchanged.
2. Review the persisted plan, including targets, boundaries, and edits.
3. Apply that exact plan ID.
4. Inspect the receipt, run validators and focused tests, then recheck
   CodeAtlas evidence.

The installed stable Mill binary may intentionally differ from an in-progress
TypeMill checkout. Capability evidence, not version-string equality, decides
whether it can be used. If a required operation is unsupported, make an
ordinary reviewed edit; do not manufacture a CodeAtlas dependency on TypeMill.

## Keep generated state off `/workspace`

This host-mounted checkout is source storage only. Caches, compiler output,
downloaded tools, coverage, profiling data, fuzz corpora, minimized inputs,
databases, containers' writable layers, and temporary test state must resolve
outside `/workspace`, even when ignored by Git.

Before a build, test, package install, formatter that caches, fuzz run, or
benchmark, create and export an external task root:

```bash
codeatlas_task_root="$(mktemp -d /tmp/codeatlas-task-cache.XXXXXX)"
case "$(realpath "$codeatlas_task_root")" in /workspace|/workspace/*) exit 1 ;; esac
mkdir -p "$codeatlas_task_root"/{cargo-target,sccache,npm,pnpm-store,pip,uv,pycache,xdg,tmp,logs,artifacts}
export CARGO_TARGET_DIR="$codeatlas_task_root/cargo-target"
export SCCACHE_DIR="$codeatlas_task_root/sccache"
export npm_config_cache="$codeatlas_task_root/npm"
export npm_config_store_dir="$codeatlas_task_root/pnpm-store"
export PIP_CACHE_DIR="$codeatlas_task_root/pip"
export UV_CACHE_DIR="$codeatlas_task_root/uv"
export PYTHONPYCACHEPREFIX="$codeatlas_task_root/pycache"
export XDG_CACHE_HOME="$codeatlas_task_root/xdg"
export TMPDIR="$codeatlas_task_root/tmp"
```

Always set `CARGO_TARGET_DIR` for Cargo. Preserve external `CARGO_HOME` and
`RUSTUP_HOME`; redirect either if it resolves inside `/workspace`. Redirect
tool-specific state too. Do not reinstall `node_modules` unless explicitly
required. Never delete pre-existing generated state without approval; remove
only state created by the current task and inspect the checkout before handoff.

## Validation and Git safety

Run the narrowest relevant test first, then the acceptance surface affected by
the change. Shared contract changes require CLI behavior, artifact and error
shape, domain parity, cancellation/cleanup, and conformance coverage as
applicable. Run the full required suite for release hardening or when targeted
checks cannot cover a broad shared surface. Never report a check as passing
unless it actually ran.

Before every commit, inspect status plus staged and unstaged diffs. Preserve
other agents' and users' work; stage only owned paths. Do not amend, rewrite,
discard, clean, or delete shared state without explicit authorization. Do not
commit with failing required checks or unresolved generated residue.

Canonical documentation:

- [README and public CLI](README.md)
- [Canonical lexicon](docs/concepts/lexicon.md)
- [Architecture specification](spec/architecture/v0.1/README.md)
- [Umbrella program](proposals/codeatlas-fuzz-performance.md)
