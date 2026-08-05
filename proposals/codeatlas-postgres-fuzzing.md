# PostgreSQL parameter fuzzing

Status: Accepted follow-on; static Phase 1 is ready, live phases wait for the
execution sandbox, and generated fuzzing also waits for the shared corpus

Decision scope: Typed PostgreSQL parameter contracts, deterministic value
generation, guarded execution, reduction, and `fuzz postgres`

Depends on:

- [`codeatlas-evidence-lifecycle-cli.md`](codeatlas-evidence-lifecycle-cli.md)
- [`codeatlas-execution-kernel-http-fuzz.md`](codeatlas-execution-kernel-http-fuzz.md)
  for the existing target/artifact contracts in static Phase 1 and a passing
  Phase 4 isolation backend before live Phases 2 through 4

Phase 3 additionally consumes only the shared `src/fuzz/corpus.rs` foundation
from [`codeatlas-code-fuzzing.md`](codeatlas-code-fuzzing.md) Phase 1; PostgreSQL
does not depend on callable contracts or language engines.

## Decision

CodeAtlas will add generative PostgreSQL query testing as `fuzz postgres`. It is
separate from `test postgres`:

- `test postgres` deterministically creates a disposable database, replays
  bootstrap/migrations, and validates known query contracts.
- `fuzz postgres` generates boundary and adaptive parameter values for supported
  static queries, executes them through the same disposable lifecycle, and
  reduces failures.

The first SQL capability remains PostgreSQL-specific. CodeAtlas does not add a
generic `fuzz sql` alias before another dialect and shared dialect contract
exist.

The execution kernel owns plans, authorization, call/resource budgets,
isolation, cancellation, artifacts, and receipts. PostgreSQL owns SQL discovery,
catalog evidence, parameter construction, query safety, database lifecycle,
result oracles, and database cleanup.

## Options considered

| Option | Fit | Tradeoff | Verdict |
|---|---|---|---|
| Treat current `PREPARE` validation as fuzzing | No implementation work | Generates no values and exercises no result behavior | Reject |
| Execute through one `psql` process per case | Reuses current runner | Loses prepared-session state and adds high process cost | Reject |
| Build a generic SQL value engine first | Future dialect symmetry | Erases PostgreSQL catalog/domain semantics before another dialect exists | Reject |
| Typed persistent PostgreSQL session inside current disposable lifecycle | Exact parameter/result evidence with bounded cost | Adds one focused client dependency and safety classifier | Adopt |

## Existing-first check

Reuse:

- `src/postgres/source/*` for migration and static query discovery.
- `src/postgres/model.rs` for inventory, test, baseline, diff, and finding
  contracts.
- `src/postgres/target/mod.rs` for disposable database creation, migration
  replay, metadata, and cleanup orchestration.
- `src/postgres/target/psql.rs` for bootstrap/migration and psql-specific
  meta-command semantics only.
- Existing PostgreSQL CLI/config/test fixtures.
- The execution kernel for all generic limits, isolation, artifacts, and
  receipts, including target classification, replay, redaction, leases, and
  resource sampling.

Current query inventory records ID, location, digest, parameter count, dynamic
status, and statement kind. It does not record catalog-backed parameter types,
constraints, table/column dependencies, or values. The new query contract and
typed session fill that gap rather than duplicating the existing source
inventory.

## Query eligibility contract

An automatically fuzzable query requires:

- A statically resolved query body and exact query ID.
- Stable placeholder order and count.
- A supported statement class.
- Parameter types resolved through PostgreSQL parse/describe/catalog evidence.
- Known nullability and applicable domain/enum/length/precision constraints.
- No blocked server-side function, privilege, filesystem, process, network, or
  transaction effect.
- A disposable target and restricted role.
- Finite case, statement, connection, time, row, result-byte, and resource
  limits.

Dynamic SQL, unresolved fragments, and runtime-built identifier lists remain
visible inventory findings but are not automatically fuzzed.

The query contract records:

```text
PostgresQueryContract
  query identity, source, digest, and statement kind
  parameter positions and PostgreSQL type OIDs/names
  nullability, enum/domain/check evidence
  length and numeric precision/scale
  referenced objects where statically/catalog resolvable
  result columns and types
  function/effect evidence
  eligibility or exact block reasons
```

## Safety classes

Initial stable automatic support is conservative.

Allowed by default:

- Static `SELECT` queries using supported built-in expressions/functions.
- Read-only common table expressions whose contained statements are allowed.

Allowed only by exact checked-in target policy:

- `INSERT`, `UPDATE`, and `DELETE` inside per-case rollback on the disposable
  database.
- Known safe fixture functions with catalog and owner evidence.

Blocked initially:

- Dynamic SQL and unresolved string construction.
- DDL and transaction-control statements.
- `COPY ... PROGRAM` and server filesystem paths.
- Large-object host paths, untrusted extensions, external database links, and
  administrative operations.
- Privileged catalog modification.
- Unknown or externally effectful functions.
- Production or non-disposable targets.

Rollback is not presented as universal isolation. Sequences and other
nontransactional database effects are discarded when the entire disposable
database is dropped. External effects remain blocked even if SQL transaction
rollback would succeed.

DML is always classified as mutating/effectful by the kernel target classifier.
It therefore always requires an explicitly reviewed plan ID and never qualifies
for preauthorized single-shot execution, even against a local disposable
database. Checked-in policy controls DML eligibility; it does not weaken the
authorization mode.

## Disposable lifecycle

Every fuzz plan names the exact target, contract set, server capability,
restricted role, database naming pattern, and cleanup owner.

Execution:

1. Verify the target is eligible and non-production.
2. Create a fresh database from `template0` under the restricted role.
3. Replay bounded bootstrap and migrations through the existing psql owner.
4. Open one guarded typed client session for parameterized queries.
5. Parse/describe supported queries and verify plan contract digests.
6. Execute deterministic cases, then optional adaptive cases inside the
   remaining budget.
7. Reduce failures using new statement permits.
8. Close sessions, drop the database, and verify cleanup.
9. Persist the execution receipt, `PostgresObservation`, PostgreSQL report, and
   optional shared reproducer envelope.

`psql` never becomes a second generated-query executor. The typed client never
becomes a second migration owner.

The psql process, typed client, temporary database, and any managed server or
scratch root are kernel `ExecutionLease`s. A disposable database boundary does
not substitute for the sandbox: psql/client execution still requires a verified
backend, read-only checkout/runtime mounts, external-only scratch, exact network
destinations, shared redaction, resource ceilings, and cleanup evidence.

## Parameter corpus

PostgreSQL maps OIDs, domains, nullability, enums, length, precision, and scale
into the shared `src/fuzz/corpus.rs` boundary descriptors and pairwise selector.
This module materializes PostgreSQL protocol values and owns catalog semantics;
it does not duplicate the bounded integer/string/collection lattice or import a
language runtime value engine.

Applicable deterministic values include:

- Null where allowed and a non-null representative.
- Boolean alternatives.
- Numeric zero, one, negative one, type/domain min/max, adjacent values,
  precision/scale boundaries, and overflow candidates.
- Empty, one-unit, declared-limit, Unicode, combining, and oversized strings.
- Empty/singleton/declared-limit arrays and supported composite/domain values.
- Every enum value plus invalid textual input at the protocol boundary where
  meaningful.
- Date/time epoch, extrema, leap/daylight boundaries, and timezone forms where
  the exact PostgreSQL type supports them.
- UUID, JSON/JSONB, bytea, inet, and other types only after their canonical
  construction and limits are specified.

Unsupported types produce exact block evidence. Nested and multi-parameter
products are bounded and use deterministic pairwise/coverage selection rather
than an unbounded Cartesian product.

Adaptive generation and reduction use a concrete persisted seed and the
remaining case/call/time/resource budget. One SQL execution equals one kernel
call. Bootstrap statements, catalog queries, parse/describe operations,
generated statements, reductions, validation, and cleanup are separately
accounted in the receipt and included in finite whole-run ceilings.

## Oracles

Automatic oracles include:

- Server process/connection loss.
- Timeout or resource/output/row/result-byte limit.
- SQLSTATE outside the query's declared acceptable result/error contract.
- Parameter/result shape mismatch.
- Constraint, transaction, session, or database cleanup failure.
- Forbidden effect attempt.

A database error is not automatically a product failure: invalid generated
values may be expected negative cases. The plan distinguishes acceptance and
rejection cases and names the expected oracle.

Query answer correctness requires a declared invariant, reference query,
differential target, or model. Result types and successful execution alone do
not prove semantic correctness.

## PostgreSQL observation artifact

`test postgres` produces a versioned `PostgresObservation` domain payload in
the kernel artifact envelope. It records:

- Contract, source, migration, bootstrap, target, server, catalog, role,
  Squawk, psql/client, config, policy, and execution-plan digests.
- Normalized migration/query replay evidence, catalog objects, resolved query
  contracts, lint/prepare/result findings, and gate counts.
- Isolation capability and environment fingerprint, receipt link, cleanup
  evidence, outcome, and completeness.
- Its canonical `observation_<digest>` ID and schema/API versions, with secrets
  and connection material excluded.

`baseline postgres` converts one complete, passing observation into a typed
baseline envelope. `diff postgres` compares another observation against it.
Both accept the kernel's `ArtifactRef`, rehash imported files, and make zero
database, psql, client, or target calls. Partial, blocked, cancelled, failed, or
incompletely cleaned observations cannot become governing baselines.

## CLI and artifact lifecycle

```bash
# Deterministic replay preview; makes zero database calls.
codeatlas test postgres --target local-db

# Reviewed execution produces the observation.
codeatlas test postgres --plan plan_ABC --execute \
  --out postgres-observation.json

# Baseline and diff consume evidence and make zero live database calls.
codeatlas baseline postgres --observation postgres-observation.json
codeatlas diff postgres \
  --against postgres-baseline.json \
  --observation postgres-observation.json

# Fuzz preview and reviewed execution.
codeatlas fuzz postgres --target local-db
codeatlas fuzz postgres --replay reproducer_ABC.json
codeatlas fuzz postgres --plan plan_ABC --execute
```

Target and reproducer forms without `--execute` are zero-call previews. A
reproducer is evidence for a new exact plan, not an implicit execution path.

A local disposable target may use the kernel's preauthorized single-shot form
only for statically read-only query workloads when runtime evidence proves the
exact managed database lifecycle, restricted role, contained network
destination, effect policy, sandbox, and cleanup capabilities. DML, remote,
unknown-effect, or policy-exception workloads require reviewed plan IDs. A
reviewed ID never substitutes for missing isolation; unavailable required
capabilities block before psql, client, or database calls.

PostgreSQL-specific selectors are:

```text
--query
--profile
--seed
```

All common execution and fuzz limits come from the kernel-owned flattened CLI
structs. PostgreSQL adds only genuine row/result/database ceilings and neither
relists nor reparses the shared flags.

The base adapter does not promise cost-guided control. That follow-on must add
and verify a typed `PostgresCostEvidence` capability for the exact supported
rows/buffers/planner/execution metrics plus threshold-preserving generation and
reduction. Ordinary query fuzzing is not assumed to provide it.

## Acceptance gates

- Static query identity and parameter order are deterministic.
- Catalog-backed types and supported constraints produce exact contract
  evidence.
- Dynamic, DDL, transaction, privileged, filesystem, program, external-link,
  and unknown-effect queries block before generated execution.
- DML is never single-shot; reviewed authorization still blocks when any
  required sandbox capability is absent.
- Psql, typed-client, and managed database processes run with read-only source,
  external scratch, exact network policy, shared redaction/resource limits, and
  kernel cleanup leases under the accepted isolation suite.
- The typed session is the only generated parameter execution path.
- The target never observes a statement without a kernel permit.
- `max_calls + 1`, burst, concurrency, timeout, row, result-byte, and output
  tests prove enforcement from the database side.
- DML cases cannot survive disposable database cleanup.
- Success, failure, cancellation, interruption, and reduction all drop the
  database and close sessions.
- Seeds and capable reductions replay against unchanged source/catalog/tool
  evidence.
- `test postgres` persists a complete typed observation; baseline and diff load
  its `ArtifactRef` and make zero live calls.
- No `fuzz sql` alias or parallel PostgreSQL budget/artifact owner exists.

## Phase 1: Typed query contract and safety classification

Status: [ ] Not started

LOC: +500-800 / -80-150

Verify: Static query IDs, parameter order, statement class, catalog types,
constraints, effect blocks, DML reviewed-only classification, and eligibility
reasons are deterministic and covered by focused inventory/contract tests.

```text
+ src/postgres/target/query.rs
~ src/postgres/mod.rs
~ src/postgres/model.rs
~ src/postgres/source/parameters.rs
~ src/postgres/source/mod.rs
~ src/postgres/target/mod.rs
~ src/config/postgres.rs
~ src/config/mod.rs
~ tests/postgres_cli.rs
~ tests/fixtures/postgres/codeatlas.json
```

## Phase 2: Guarded typed session and disposable execution

Status: [ ] Not started

LOC: +800-1,200 / -100-200

Verify: Test preview makes zero calls; persistent parse/describe/execute
behavior, restricted role, per-call permits, transaction policy, result limits,
verified sandbox, shared leases/redaction, typed observation identity, session
closure, database drop, and interruption cleanup pass against the live ignored
fixture.

```text
+ src/postgres/target/client.rs
~ Cargo.toml
~ Cargo.lock
~ src/postgres/target/mod.rs
~ src/postgres/target/psql.rs
~ src/postgres/model.rs
~ src/commands/postgres.rs
~ src/cli/postgres.rs
~ src/cli/test.rs
~ src/cli/baseline.rs
~ src/cli/diff.rs
~ src/execution/model.rs
~ src/execution/artifact.rs
~ src/execution/budget.rs
~ src/execution/lease.rs
~ src/execution/redaction.rs
~ src/execution/runner.rs
~ src/execution/sandbox/mod.rs
~ tests/postgres_cli.rs
~ package.json
```

## Phase 3: Deterministic and adaptive PostgreSQL fuzzing

Status: [ ] Not started

LOC: +650-1,000 / -80-180

Verify: Boundary corpus, bounded combinations, positive/negative oracle
classification, exact seed replay, reduction, statement/rate/concurrency
ceilings, shared boundary descriptors, blocked query classes, and reproducer
artifacts pass.

```text
+ src/postgres/fuzz.rs
~ src/postgres/mod.rs
~ src/postgres/model.rs
~ src/commands/fuzz.rs
~ src/cli/fuzz.rs
~ src/config/postgres.rs
~ src/fuzz/model.rs
~ src/fuzz/corpus.rs
~ src/fuzz/report.rs
~ src/fuzz/reproducer.rs
~ src/execution/model.rs
~ tests/postgres_cli.rs
~ tests/fixtures/postgres/codeatlas.json
```

## Phase 4: Self-dogfood, consolidation, and release hardening

Status: [ ] Not started

LOC: +200-350 / -150-300

Verify: Full static and live gates pass; no duplicate query executor, unsafe
SQL class, hidden live baseline/diff, database residue, SQL alias, stale report,
source-adjacent artifact, or compatibility path remains.

```text
~ README.md
~ docs/concepts/lexicon.md
~ codeatlas.json
~ proposals/codeatlas-postgres-fuzzing.md
~ proposals/codeatlas-fuzz-performance.md
~ package.json
~ tasks/check-self.js
~ src/postgres/fuzz.rs
~ src/postgres/target/client.rs
~ tests/postgres_cli.rs
```

The implementation is intentionally net higher because it adds typed catalog
contracts, one persistent guarded client, generated values, and live cleanup
proof. It must not retain psql as a parallel query executor or create a generic
SQL abstraction before another dialect exists.

Total LOC: +2,150-3,350 / -410-830

## Layman's wins

- CodeAtlas can try difficult database values instead of only checking that a
  query parses.
- Every generated statement runs in a disposable database under strict limits.
- Live replay, baselines, comparisons, and fuzzing have distinct honest jobs.
- Failures replay without risking a production database.
