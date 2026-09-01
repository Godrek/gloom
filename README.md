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
- Extract direct calls and retain unresolved indirect calls explicitly.
- Merge definitions and declarations across inputs.
- Detect recursive strongly connected components.
- Query zero-incoming functions, reachability, and shortest call paths.
- Export schema 1.0 JSON.
- Generate a self-contained HTML viewer with search, pan, zoom, drag, cycle
  highlighting, and caller/callee inspection.
- Publish an evidence-backed snapshot for direct LLVM calls, query its named
  callees, and expand compact explanation handles into evidence and derivations.

## Prototype limitations

The current model uses LLVM symbol names as identities, merges all unresolved
indirect calls into one placeholder, and treats all stored edges alike during
traversal. Its `call_count` is a count of merged static occurrences, not runtime
invocations. Its zero-incoming function query is not a semantic entry-point
analysis.

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
gloom query-snapshot snapshot.json --callees caller
gloom query-snapshot snapshot.json --explain \
  explanation:claim:direct-call-example-v1:input:0:direct-target:0
gloom view-snapshot snapshot.json -o snapshot.html
```

The published snapshot format is currently `2.0-pre`. The existing `build`,
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
