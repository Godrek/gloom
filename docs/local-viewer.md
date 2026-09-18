# Local viewer

`gloom serve` runs a local query service over one published snapshot and serves
a focused-neighborhood viewer to a browser on the same machine. The viewer runs
the [bounded named queries](bounded-queries.md) the CLI runs, through the same
`Application::investigate_snapshot` seam, so a caller, callee, path, resolution
policy, or context filter means the same thing in both.

```bash
gloom ingest-build tests/fixtures/declared-build/manifest.json \
  --target server -o snapshot.json
gloom serve snapshot.json --port 7878
```

The command prints the address it listens on and serves until interrupted. The
snapshot is pinned when the service starts: every answer describes one immutable
published snapshot, and serving a different one means starting the service again.

## What a developer can do

1. **Select a scope.** The viewer asks for the snapshot's build targets and the
   observation contexts recorded for each, and explores nothing until a target
   and at least one context are selected. Selecting several contexts includes
   their qualifying target claims, exactly as a CLI request does.
2. **Search for a starting point.** A bounded callable search returns one item
   per matching manifestation, with the acquired input, scoped contributor
   identity, and declaration evidence that tell same-named callables apart.
3. **Expand a focused neighborhood.** Callers and callees return the
   relationships, unresolved call sites, and unattributed sites the query layer
   reports, within the declared `max_depth`, `max_results`, and `max_steps`.
4. **Focus a path or a cycle.** Mark a start and an end for a call path, or
   enumerate recursive cycles through the starting point. Definite and potential
   recursive cycles are labelled as the core classifies them.
5. **Inspect evidence.** Every relationship and call-site record carries an
   explanation handle. Expanding one shows the call-site resolution, supporting
   evidence records with their provenance and any completeness basis, target
   claims, derivations, and correspondence claims.

Truncation reasons, executed steps, the declared world policy, and the returned
static call-site cardinality are shown with every result, so a bounded answer is
never mistaken for an exhaustive one. A query a bound stopped before it reported
anything is shown as the bound it hit, never as a no-match or an absence in the
selected scope: it reached nothing, so it establishes nothing about what exists.

## The local service

Four routes, and deliberately no fifth:

| Route | Request | Answer |
| --- | --- | --- |
| `GET /` | — | The viewer page, embedded in the executable. |
| `GET /investigation-scope` | — | Build targets, their observation contexts, and the bounds limits the core enforces. |
| `POST /investigate` | One `BoundedQuery` document | One `BoundedQueryResult`. |
| `POST /explain` | `{"explanation_handle": "…"}` | The expanded `Explanation`. |

There is no route that returns the program snapshot, its call-graph projection,
or any unbounded listing of entities, claims, or evidence. The browser therefore
cannot obtain the snapshot and reinterpret it: it composes a scope and a named
query, and the Rust core decides what that question means. A refused request
carries the core's own message and a `4xx` status; the viewer displays it rather
than paraphrasing it.

Presentation stays in the client. Which neighborhood is on screen, where a
callable is drawn, what is expanded, and which path is highlighted are the
page's own state, recomputed from results it already has. It derives no
relationship, resolution, or correspondence of its own.

## No required network access

The service and the page are auditable on this point:

- The listener binds `127.0.0.1` only. The bind address is not configurable,
  because serving a snapshot to a wider network is a different decision.
- A request whose `Host` header names anything but a loopback host is refused,
  so a page elsewhere cannot reach the service through a name that resolves to
  the loopback address.
- The page is a single asset compiled into the executable. It loads no script,
  stylesheet, font, or data from anywhere; every response declares a content
  security policy of `default-src 'none'` with `connect-src 'self'`.
- The transport is a small hand-written HTTP/1.1 reader and writer over
  `std::net::TcpListener`. No HTTP or client-side dependency was added, so the
  claim above can be checked by reading `src/service.rs` and the page itself.
- Nothing is reported, measured, or transmitted anywhere.

The service is a local development tool. It has no authentication beyond the
loopback binding and the `Host` check, answers one request per connection on a
single thread, reads request heads up to 8 KiB and bodies up to 256 KiB, and is
not intended to be exposed to other machines or to serve many clients.

Because the accept loop is single-threaded, one request may not hold it open.
The time budget is spent across a whole request rather than restarted by each
read, so a client that trickles bytes just inside every individual read's
timeout is refused with `408` once its deadline passes, and the next client is
served. `BoundLocalQueryService::with_request_deadline` changes that budget from
its fifteen-second default.

## The self-contained export

`gloom view-snapshot snapshot.json -o snapshot.html` still writes the small
self-contained evidence viewer, and remains the right tool for a prototype-sized
snapshot that has to travel as one file — an attachment, an artifact of a build,
a page opened with no gloom executable at hand. It embeds the projected call
sites and their evidence in the page, so it grows with the snapshot and
interprets the embedded records in the browser.

The local viewer is the right tool for everything larger, and for exploring a
snapshot with the query semantics the CLI uses. The two are complementary: the
export ships a whole small snapshot, the service answers bounded questions about
any snapshot.
