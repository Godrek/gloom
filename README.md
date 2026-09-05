# Gloom

Gloom is an early program-understanding prototype. It turns C and textual LLVM
IR into a call-graph projection that people can explore and tools can query.

The long-term product is broader than a call-graph viewer: Gloom will preserve
evidence about program structure, keep uncertainty visible, and derive
purpose-specific projections for human investigation. The current executable
validates only a small part of that direction.

## Current capabilities

- Compile one or more C files to textual LLVM IR with Clang.
- Ingest existing `.ll` files.
- Extract direct calls and retain unresolved indirect calls explicitly. A call
  counts as direct only when its callee operand, after constant casts are
  stripped, names a callable the module declares: a function, an ifunc, or an
  alias that resolves to one. A call through a global variable, or through an
  alias to data, stays unresolved.
- Preserve one linkage-namespace callable across definitions and declarations
  in several acquired inputs of one observation context, while keeping
  same-named translation-unit-local callables distinct — including two
  byte-identical translation units, which link as two units and stay two
  callables.
- Detect recursive strongly connected components.
- Query zero-incoming functions, reachability, and shortest call paths.
- Export schema 1.0 JSON.
- Generate a self-contained HTML viewer with search, pan, zoom, drag, cycle
  highlighting, and caller/callee inspection.
- Publish evidence-backed snapshots with first-class direct and indirect call
  sites, per-site complete/partial/absent resolution, and possible targets.
- Run callable search, immediate caller and callee queries, and bounded shortest
  call-path queries through one named-query seam without dropping unresolved
  sites. Search results include the acquired input, scoped contributor identity,
  and declaration behind each match so same-named local callables can be
  selected by program-entity identity.
- Expand compact call-site explanation handles into evidence, target
  derivations, and cross-context correspondence claims.

## Prototype limitations

The legacy schema 1.0 model gives callables stable opaque IDs derived from the
LLVM contributor's scoped identity evidence; readable symbol spellings remain
labels in exports and query results. It still merges all unresolved indirect
calls into one placeholder and treats all stored relationships alike during
traversal. The evidence-backed snapshot path preserves indirect call sites
independently. Its current named caller/callee queries are one-hop and its
shortest directed call path requires an explicit relationship bound (maximum
1,000). They use the published call-graph projection's target claims across its
contexts; context filters, resolution policies, and broader bounded-query
policies remain deferred to the full named-query work. The legacy `call_count`
is a count of merged static occurrences, not runtime invocations. Its
zero-incoming function query is not a semantic entry-point analysis.

Schema 1.0, the CLI, and the Rust library API are pre-stable prototype
interfaces. They may change as the evidence and identity model is implemented;
no compatibility window is promised yet.

## Install

Gloom requires Rust 1.85 or newer. C input also requires Clang.

```bash
cargo build --release
cargo install --path .
```

## Use

```bash
gloom build examples/demo.c -o graph.json --html graph.html
gloom analyze graph.json --cycles
gloom analyze graph.json --reachable main
gloom analyze graph.json --path main cleanup
```

Or run without installing:

```bash
cargo run -- build examples/demo.c -o graph.json --html graph.html
```

Existing textual LLVM IR can be used directly:

```bash
clang -S -emit-llvm -g -O0 source.c -o source.ll
gloom build source.ll -o graph.json
gloom view graph.json -o graph.html
```

The evidence-model migration is available alongside those prototype commands.
Publish a fully qualified static observation context and an optional
self-contained evidence viewer:

```bash
gloom publish tests/fixtures/direct-call.ll \
  --snapshot-id direct-call-example-v1 \
  --target direct-call-example \
  --build-configuration debug \
  --toolchain "textual LLVM IR" \
  --analysis-stage "llvm-ir extraction" \
  -o snapshot.json \
  --html snapshot.html
```

Query the stored call-graph projection and expand the returned explanation
handle without rerunning extraction or reconstructing the relationship:

```bash
gloom query-snapshot snapshot.json --search-callables caller
gloom query-snapshot snapshot.json --callees caller
gloom query-snapshot snapshot.json --callees caller \
  --caller-entity-id entity:direct-call-example-v1:input:0:callable:0
gloom query-snapshot snapshot.json --callers callee
gloom query-snapshot snapshot.json --call-path caller callee \
  --max-relationships 8
gloom query-snapshot snapshot.json --explain \
  explanation:entity:direct-call-example-v1:input:0:call-site:0
gloom view-snapshot snapshot.json -o snapshot.html
```

Name-only caller, callee, and path queries reject ambiguous callable labels,
reporting each candidate's declaration. Use `--search-callables` or the entity
ID reported in the snapshot to select the intended callable explicitly.
Legacy `analyze --reachable` and `analyze --path` do the same; their results pair
each opaque identity with its readable display label.

The published snapshot format is currently `2.0-pre.1`. The existing `build`,
`analyze`, and `view` commands continue to use the legacy schema 1.0 path during
the migration.

Open `graph.html` directly or serve the directory with
`python3 -m http.server 8000`, then visit
`http://localhost:8000/graph.html`.

## Documentation

- [Product vision](docs/PRODUCT_VISION.md): durable purpose, principles,
  boundaries, and success definition.
- [Domain language](CONTEXT.md): canonical project terminology.
- [Architecture decisions](docs/adr/): accepted, costly-to-reverse decisions.
- GitHub Issues: milestone specifications, acceptance criteria, research, and
  implementation work.

## Development

The viewer integration tests require Node.js 22 or newer to execute the generated
standalone HTML and compare its expanded evidence with the explanation query.

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

The LLVM fixture in `tests/fixtures/simple.ll` permits extraction tests without
invoking Clang. `examples/demo.c` exercises the complete C-to-viewer path.
Continuous integration runs formatting, linting, and tests on every push and
pull request.

## License

MIT
