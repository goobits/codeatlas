# HQA application-inventory seeding

Status: Accepted CodeAtlas-side interop child; implementation complete

Decision scope: One deterministic renderer from `codeatlas.http/v2` evidence to
the published `agentspeak.hqa-application-inventory/v1` contract

Depends on:

- [`codeatlas-evidence-lifecycle-cli.md`](codeatlas-evidence-lifecycle-cli.md)
- the externally owned, published HQA application-inventory v1 schema

Does not authorize: any HQA, HIF, TypeMill, or neutral-contract repository edit

## Decision

`scan http` will gain a second truthful renderer:

```bash
codeatlas scan http --format json
codeatlas scan http --format hqa-inventory --out hqa-routes.json
```

`json` remains the default and preserves the existing CodeAtlas report exactly.
`hqa-inventory` renders the same in-memory inventory into HQA's native
snake_case v1 envelope. It does not rescan source, call a target, infer roles,
or change which CodeAtlas operations exist.

The output is preparation evidence for HQA, not a coverage claim. CodeAtlas
will keep partial source completeness visible, preserve exact provenance in
tags, and omit every consumer field for which CodeAtlas has no evidence.

## Verified external contract

The proposal was checked read-only against HQA's current public schema and Rust
model. The v1 envelope requires:

```json
{
  "schema_version": "agentspeak.hqa-application-inventory/v1",
  "routes": []
}
```

Each route requires only `id` and `entry`. HQA accepts optional
`location_match`, `is_probe_only`, `tags`, and `expectation.status`; it defaults
or omits roles, readiness targets, exclusions, and expected transitions.

CodeAtlas does not import HQA as a runtime dependency. HQA owns the schema and
its semantics. CodeAtlas owns only this renderer and its exact v1 mapping.

## Input projection

One HTTP contract contains two complementary inventories:

- `operations`: OpenAPI operations, response contracts, and parameter/schema
  evidence.
- `source.operations`: statically discovered endpoints and pages with detector,
  confidence, source location, and explicit completeness.

The renderer takes their deterministic union by `(contract.id,
operation.key)`. This matters because OpenAPI-only operations are useful HQA
probes and source-only pages have no OpenAPI counterpart. Multiple source
detections for one key become one route with sorted, deduplicated provenance
tags; they do not create duplicate HQA route IDs.

Contract IDs are unique only inside the CodeAtlas report, while the same
operation key can legitimately exist in two contracts. Therefore HQA IDs are
not the bare operation key. The reversible identity is:

```text
codeatlas:http/<percent-encoded-contract-id>/<percent-encoded-operation-key>
```

Percent encoding uses UTF-8 bytes, RFC 3986 unreserved characters, and uppercase
hex. For example:

```text
codeatlas:http/public/GET%20%2Fhealth
```

The human-readable operation key remains in a tag. This prevents a later
second contract from changing or colliding with an existing route identity.

## Exact field mapping

| CodeAtlas evidence | HQA v1 field | Rule |
|---|---|---|
| Contract ID + operation key | `id` | Canonical composite identity above |
| Operation path | `entry` | `{ "kind": "url", "value": <path-or-prefix> }` |
| Source kind `page` | `is_probe_only` | Omit/false; the route is explorable |
| Endpoint or OpenAPI-only operation | `is_probe_only` | `true`; probe but do not treat as an explorable page |
| Static path | `location_match` | Omit; HQA's exact default is correct |
| Path containing `{...}` | Entry value + `location_match` | Use the static prefix before the first parameter (or `/`) and `prefix` |
| `pathPattern` | `tags[]` | Emit `codeatlas:path_pattern=<value>`; never assume its syntax is HQA's Rust regex dialect |
| Detector | `tags[]` | `codeatlas:detector=<value>` for every contributing detector |
| Confidence | `tags[]` | One `codeatlas:confidence=high|medium` tag for every distinct contributing confidence |
| Source evidence | `tags[]` | `codeatlas:evidence=<repository-path>:<one-based-line>` |
| Source completeness | `tags[]` | `codeatlas:source_completeness=complete|partial` on every route |
| Contract and operation | `tags[]` | Exact contract and human-readable operation tags |
| One unambiguous numeric 2xx response | `expectation.status` | Emit only when exactly one concrete 200-299 status is declared |
| Roles, readiness targets, transitions, exclusions | Nothing | Never emit; CodeAtlas has no application-profile authority |

All tags sort and deduplicate bytewise. A partial inventory never becomes
silent merely because HQA v1 has no top-level completeness field.

## Pattern honesty

CodeAtlas `pathPattern` is provenance from several detector dialects, including
SvelteKit/Medusa bracket paths, prefix wildcards, Express-like forms, and
JavaScript regex literals. HQA v1's `regex` mode interprets `entry.value` as a
Rust regular expression and also uses that same entry as a navigation target.
A direct copy would therefore be both syntactically and operationally wrong.

The v1 renderer uses only:

- exact matching for concrete paths; and
- conservative prefix matching for normalized `{parameter}` paths.

It keeps the original pattern in a provenance tag. A future regex translator
would require a separately accepted, detector-specific conversion contract and
navigable example-value semantics. This proposal does not guess.

## Format and filtering contract

There is no `--include-medium-confidence` flag. A default-on one-way flag would
be ceremonial, and filtering inside a renderer would violate CodeAtlas's
`analyze -> render` boundary. The HQA renderer includes the same operations as
the JSON report and exposes confidence in tags.

Source inclusion/exclusion remains owned by strict HTTP contract configuration.
If a future product needs a confidence threshold, it belongs in analysis with
the same effect on every renderer, not in this adapter.

CodeAtlas's source report remains camelCase where it already is. The external
HQA envelope is emitted in consumer-native snake_case. No blended boundary
format is introduced.

## OpenAPI status rule

An expectation is emitted only when the merged OpenAPI operation declares
exactly one concrete status whose parsed value is from 200 through 299.
Ranges such as `2XX`, `default`, conflicting duplicate declarations, or
multiple distinct successful statuses remain visible only in CodeAtlas input
and produce no HQA expectation. Omission means unknown, not success.

## Bounds and failure behavior

- Rendering is linear in the already bounded HTTP report plus deterministic
  ordered maps/sets.
- Output uses the existing `--out` one-file contract and stdout behavior.
- Duplicate composite IDs, invalid paths, empty prefixes, invalid concrete
  statuses, or an output that cannot satisfy HQA v1 fail before bytes are
  written.
- Rendering makes zero target calls and writes only the explicit output file.
- The renderer emits no credentials, headers, bodies, absolute checkout paths,
  or HQA profile data.
- Partial source evidence is accepted and tagged; malformed evidence is not
  silently dropped.

## Existing-first check

Reuse:

- `HttpInventoryReport` as the only analysis result.
- HTTP operation normalization and existing contract ordering.
- `src/commands/output.rs` for stdout/one-file behavior.
- `src/outputs` as the renderer owner.
- The HQA v1 schema as the external authority.

No second route scanner, OpenAPI parser, HQA client, network call, application
profile, role model, or cross-repository package is added.

## Acceptance gates

- A golden CodeAtlas fixture containing source-only pages, source-only
  endpoints, OpenAPI-only operations, merged operations, duplicate source
  evidence, and two contracts renders byte-identically.
- Composite route IDs remain unique and stable across contract ordering.
- Dynamic routes use navigable prefixes and never copy `pathPattern` into HQA
  regex mode.
- Partial completeness, detector, confidence, source path/line, contract, and
  operation provenance survive in sorted tags.
- Exactly one concrete 2xx becomes an expectation; ambiguous responses do not.
- No roles, transitions, readiness targets, or readiness claims are emitted.
- Existing `scan http` JSON bytes remain unchanged and the default stays JSON.
- The rendered fixture validates against the externally published HQA v1
  schema during convergence verification.
- CodeAtlas tests and implementation touch no HQA path.

## Phase 1: CodeAtlas renderer and CLI

Status: [x] Complete (2026-08-04)

Implementation diff: +815 / -9 non-proposal lines

Verify: Default and explicit CodeAtlas JSON are byte-identical; the deterministic
seven-route golden covers both contracts and every accepted mapping class; the
committed external HQA v1 schema at HQA `8ac0eb7` and digest
`sha256:43ca6099a29dbb37ececd816c2e0f0852d4b6f3ed66e5db997ffb0d00fa806fc`
accepts it. Full Rust tests and clippy pass, all 15 Node tests pass, and the
architecture-spec and package checks pass. CodeAtlas dogfood scans 236 files
and 2,294 symbols, reports 131 advisory findings with zero gates, adds no
dynamic-boundary finding for this renderer, and projects its own HTTP evidence
into three HQA routes with zero target calls or source-adjacent state.

```text
+ src/outputs/hqa_inventory.rs
+ tests/fixtures/http/hqa-inventory-input.json
+ tests/fixtures/http/hqa-inventory.generated.json
~ src/cli/mod.rs
~ src/outputs/mod.rs
~ src/http/mod.rs
~ src/commands/http.rs
~ src/cli/scan.rs
~ tests/http_cli.rs
~ README.md
~ proposals/codeatlas-hqa-seeding.md
```

The accepted estimate undercounted the fail-closed boundary checks and the
explicit input/golden corpus. The added surface remains one renderer, one CLI
format selector, and focused tests; it adds no scanner, parser, client, runtime
dependency, or external-repository edit.

Total implementation LOC: +815 / -9

## External follow-up, not CodeAtlas scope

HQA may consume the emitted v1 file through its existing bounded application
inventory reference. Any later hints-reference or frontier change remains an
HQA-owned proposal after its own gates. CodeAtlas neither schedules nor edits
that work here.

## Layman's wins

- One CodeAtlas command can hand HQA a useful route seed file.
- OpenAPI and source-discovered routes both survive without pretending the scan
  was complete.
- Dynamic routes are handled conservatively instead of feeding one tool's path
  syntax to another as if it were a regex.
- The integration adds no calls, credentials, or dependency on HQA code.
