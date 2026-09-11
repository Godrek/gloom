# Bounded named queries

`Application::investigate_snapshot` and `gloom investigate` execute the same
`BoundedQuery` request against one immutable published snapshot. Every request
selects a build target, explicit observation-context IDs, a resolution policy,
a world policy, and result/traversal limits. Only callable search and declared
call relationships are supported; arbitrary relationship kinds or a generic
`traverse` operation are rejected.

The earlier `Application::query_snapshot` and `query-snapshot` commands remain
compatibility interfaces for the standalone viewer. New local adapters should
use the bounded interface. It does not execute legacy schema 1.0 traversal.

## Example

Publish a declared build, read its observation-context ID from the exported
snapshot, then create `query.json`:

```json
{
  "scope": {
    "build_target": "server",
    "observation_context_ids": ["COPY-EXACT-CONTEXT-ID-FROM-SNAPSHOT"]
  },
  "resolution_policy": "include-possible",
  "world": {"kind": "open"},
  "bounds": {"max_depth": 3, "max_results": 100, "max_steps": 100000},
  "query": {"name": "callees", "caller": {"label": "server"}}
}
```

```bash
cargo run -- ingest-build tests/fixtures/declared-build/manifest.json \
  --target server -o /tmp/server.json
cargo run -- investigate /tmp/server.json --request query.json
```

The request deserializes as `gloom::queries::BoundedQuery`; pass it with a pinned
`PublishedSnapshot` to `Application::investigate_snapshot` in Rust. Unknown JSON
fields, omitted qualification/policy/bounds, nonexistent contexts, and contexts
belonging to another target are errors. Select 1–100 distinct contexts. A request
with no matching relationship returns an empty result, with the declared world
policy and any truncation reasons still visible.

## Named operations

| `query.name` | Remaining query fields | Meaning |
| --- | --- | --- |
| `callable-search` | `label` | Substring search; one item per matching manifestation in the selected contexts. |
| `callers` | `callee` selector | Breadth-first incoming expansion, up to `max_depth` relationships. |
| `callees` | `caller` selector | Breadth-first outgoing expansion, up to `max_depth` relationships. |
| `call-path` | `start`, `end` selectors | Shortest directed path through declared target claims, up to `max_depth` relationships. |
| `recursive-cycles` | `start` selector | Simple directed cycles through that starting point, up to `max_depth` relationships. |

A selector is `{"label":"worker"}`, `{"entity_id":"EXACT-ENTITY-ID"}`, or
both. A label must select exactly one callable within the chosen contexts;
ambiguous labels require search and identity selection. Search items carry
entity identity, observation context, acquired input, contributor identity, and
any declaration evidence, so identically named local callables remain distinct.
Search may return several manifestations of the same entity. Specifying both
selector fields requires both to match.

Directions and relationship kinds are fixed by the operation and returned in
result metadata. Context filtering applies to both call-site resolution and
target evidence. Selecting multiple contexts includes qualifying target claims;
it never inserts traversal relationships from correspondence claims or names.
Call-site summaries retain the original resolution and report
`targets_omitted_by_scope` when some target contexts were excluded.

## Resolution, uncertainty, and explanations

`include-possible` retains complete and partial target claims, plus individual
unresolved sites. `complete-only` includes only sites and relationships whose
published resolution is complete. It does not turn the rest of the program into
a closed-world projection. Outgoing expansion includes unresolved call sites
belonging to reached callers. Incoming expansion additionally reports incomplete
sites anywhere in the selected scope as `unattributed: true` when they were not
already included by a known target claim: unknown targets cannot be asserted to
call the selected callee. These are uncertainty records, not invented incoming
relationships. Limits can truncate these records just like other results.

Relationship and call-site items carry compact explanation handles. Use
`Application::explain_snapshot` on the same pinned snapshot, or
`query-snapshot --explain HANDLE`, to expand supporting evidence, completeness
bases, target claims, derivations, and correspondence. Explanations describe the
original evidence and may include contexts filtered out of the bounded view.
Search declarations carry evidence IDs and source locations instead of call-site
explanation handles. Path and cycle items contain the supporting relationships
and their handles. A zero-length path from a callable to itself is valid.

`returned_static_call_site_cardinality` counts distinct returned call-site
entities supported by static resolution evidence. Repeated target claims or
cycle appearances do not multiply that count. It covers the returned records,
not all sites in the target; search returns zero. Runtime-only sites are excluded.
`runtime_invocation_measure` is always null: none of these queries measures
execution frequency, even when runtime-scoped evidence contributes targets.

## Open and explicitly closed scopes

`{"kind":"open"}` means missing claims establish no impossibility. A failed
path search is not proof that a call cannot happen. This remains true after
filtering to complete-only evidence.

An explicit scope can instead be:

```json
{"kind":"closed-call-sites","call_site_ids":["EXACT-CALL-SITE-ID"]}
```

Every listed site must exist, have complete resolution, and have its resolution
and all target contexts selected. The published snapshot already validates the
required completeness bases. The list must contain 1–`max_results` distinct
sites. The result exposes `closed_scope_explanation_handles` for inspecting
those bases. Traversal then uses only this enumerated set of recorded call sites;
closure says nothing about omitted sites, unobserved code, or target membership.
It cannot be used to justify closed-world callable search, which is rejected.
Any truncation still prevents an exhaustive negative conclusion.

A definite recursive cycle has complete resolution at every constituent site,
with every target context selected. Its `closed_call_site_scope` explicitly
names the sites whose resolution evidence justifies that cycle's local closure.
A cycle requiring partial claims or a site with excluded target contexts is a
potential recursive cycle and carries no such closure. Neither classification
asserts that recursion executes in a particular run. Cycles elsewhere in the
reachable neighborhood are found by choosing a starting point on those cycles.

## Bounds and result shape

- `max_depth`: 1–100 relationships per expansion depth, path, or cycle. Search
  accepts the shared bounds object but has no traversal depth.
- `max_results`: 1–10,000 top-level items. Call-site summaries and relationships
  each count as an item. A path or cycle is one item with at most `max_depth`
  relationships; a search item has one manifestation. Closure handles are
  separately bounded by the explicit closure list.
- `max_steps`: 1–1,000,000 scanned entity/manifestation or projection records
  during the investigation. Repeated scans count again. This limits traversal
  and dense cycle enumeration, not snapshot loading, scope/selector validation,
  evidence lookup, string sizes, or wall-clock time.

Results return `steps`, bounds, and distinct `truncation` reasons (`max-depth`,
`max-results`, `max-steps`). Hitting a bound keeps collected records but does not
claim an exhaustive answer. The depth marker conservatively reports matching
relationships at the boundary, including relationships back to visited entities.
The result cap is reported only when another item would be emitted. Contexts
are fully qualified in the response. Query errors go to CLI stderr with a
nonzero exit code; successful structured results go to stdout.

The initial implementation scans the validated in-memory projection and reuses
its identities and explanation handles. Breadth-first path search stores
predecessors, and cycle enumeration stores one active depth-first path. It does
not copy the whole snapshot into the result. Dedicated lookup indexes and the
local viewer service can build on this shared query contract.
