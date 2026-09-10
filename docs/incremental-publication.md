# Incremental snapshot publication

`Application::publication_session()` creates an in-process publication boundary
for a declared build. A writer calls `reindex_declared_build` with a fresh
manifest and selected target. Query readers call `current()` once and retain the
returned `Arc<PublishedSnapshot>` for their investigation, passing it to the
existing application named-query and explanation methods.

```rust
use gloom::app::{Application, NamedQuery};
use std::path::Path;

let session = Application.publication_session();
let first = session.reindex_declared_build(
    Path::new("build-v1.json"), "server", |status| eprintln!("{status:?}"),
)?;
// Another thread can query `first` or session.current() during this call.
let second = session.reindex_declared_build(
    Path::new("build-v2.json"), "server", |status| eprintln!("{status:?}"),
)?;
let result = Application.query_snapshot(&first, NamedQuery::CallableSearch {
    label: "worker".into(),
})?;
# Ok::<(), String>(())
```

Each successful publication must have a new `program_snapshot_id`, including
when revisiting older content. A session rejects reuse of any ID it previously
published. A failed attempt can be retried with the same ID. The initial session
has no current snapshot until its first successful publication.

## Reuse and coherence

The session reads the manifest and selected textual LLVM IR artifacts afresh.
It reuses the acquired artifact and parsed LLVM observations for a compilation
only when the IR bytes, resolved compilation record (including argv, paths and
generated-input declarations), target, build configuration, toolchain, analysis
stage, and extractor identity/version match the last successful generation.
The comparison uses exact text equality, never timestamps or fingerprint equality.
The parser consumes the same bytes used for this comparison, without reopening
an artifact. Changed and newly selected compilations are analyzed again;
unselected compilations leave the cache after successful publication.

Cached observations stay inside the LLVM adapter. Each generation receives new
snapshot-scoped identities, a fully qualified observation context, evidence,
claims, and rebuilt projection indexes. The complete result, including its
retained build acquisition record, is validated before the current snapshot
pointer switches under a short write lock. Readers clone that pointer under a
short read lock and run queries without holding a session lock. Existing handles
continue to refer to the complete old generation and can still expand their
explanation handles or be exported independently.

A failed acquisition, extraction, or validation keeps both the last published
snapshot and its successful cache. A concurrent or reentrant writer is rejected.
The `status()` method and callback report indexing counts, preparation for
publication, successful publication, or a failure independently of query results.
`Publishing` means the candidate is coherent but the pointer has not switched;
`Published` means it has. These diagnostics are not a transaction paired with
`current()`: readers should use the snapshot identity on their pinned handle.
Callbacks can query the session and must not panic; a callback panic poisons the
writer session, while previously published snapshots remain readable.

## Run the example

Prepare two [declared-build manifests](declared-build.md) with different snapshot
IDs and their completed artifacts, then run:

```bash
cargo run --example incremental -- server build-v1.json build-v2.json
```

The example prints progress to stderr and callable-search results for both
retained generations to stdout. The integration tests additionally pause a writer
immediately before publication and run named queries and explanations against
the old snapshot from another thread.

## Boundary and limits

This is a memory-resident application API, suitable for the later local query
service. It does not add a file watcher, persistent cache, cross-process pointer,
or atomic filesystem export. Existing one-shot CLI commands keep their behavior.
A new process starts with an empty cache. Retaining handles keeps old generations
in memory; dropping them releases those snapshots. The cache retains only the
last successful generation's selected artifacts and observations.

As with declared-build ingestion, the build producer must supply coherent,
finished artifacts and declare their association with source, generated inputs,
compiler arguments, and target membership. Gloom does not rebuild source or
watch headers; unchanged `.ll` evidence is reusable even when archived source
files are unavailable. It does not detect a producer rewriting multiple artifacts
mid-acquisition. Build capture remains #9. Publication and index reconstruction
still process the whole selected target; this change saves acquisition material
and LLVM parsing/analysis for unchanged compilations, not all indexing work.
