# Ingest declared build evidence

A build producer can supply a local JSON manifest linking compilation records
and explicit target membership to existing textual LLVM IR (`.ll`) artifacts.
Gloom lists the declared targets and publishes one selected target through the
same application seam used by named queries:

```bash
cargo run -- build-targets tests/fixtures/declared-build/manifest.json
cargo run -- ingest-build tests/fixtures/declared-build/manifest.json \
  --target server -o /tmp/server-snapshot.json
cargo run -- query-snapshot /tmp/server-snapshot.json --search-callables worker
cargo run -- query-snapshot /tmp/server-snapshot.json --callees server
cargo run -- query-snapshot /tmp/server-snapshot.json \
  --call-path server helper --max-relationships 2
```

The Rust entry points are `Application::declared_build_targets` and
`Application::publish_declared_build`. The returned `PublishedSnapshot` contains
only the selected target's acquired inputs. Existing named search, caller,
callee, and path queries therefore operate within that target's observation
context. Select another target by publishing another snapshot from the manifest.
External callable declarations in a selected compilation remain queryable even
when their definitions are outside the acquired evidence; membership does not
assert a closed world or complete call resolution.

## Manifest version 1

All fields below are required, including empty `generated_inputs` lists. Unknown
fields and schema versions are rejected. The complete two-target example lives
in [the integration fixture](../tests/fixtures/declared-build/manifest.json).

```json
{
  "schema_version": "1",
  "program_snapshot_id": "my-build-content-v1",
  "build_configuration": "debug; FEATURE=1",
  "toolchain": "Clang 18.1.8 / LLVM 18",
  "analysis_stage": "per-translation-unit LLVM IR before linking",
  "compilations": [
    {
      "id": "main-compilation",
      "working_directory": "build",
      "source_input": "../main.c",
      "compiler_arguments": ["clang", "-S", "-emit-llvm", "-DFEATURE=1", "-Igenerated", "../main.c", "-o", "main.ll"],
      "evidence_artifact": "main.ll",
      "generated_inputs": [
        {"path": "generated/config.h", "kind": "configured"}
      ]
    }
  ],
  "targets": [
    {"name": "server", "compilation_ids": ["main-compilation"]}
  ]
}
```

`working_directory` is relative to the manifest's directory. `source_input`,
`evidence_artifact`, and generated/configured input paths are relative to that
working directory. Absolute paths are accepted. Ingestion resolves these paths
without changing the process working directory. Compiler argv is preserved
verbatim, including the executable and argument ordering. It is never executed.
The only files ingestion needs to read are the manifest and the selected `.ll`
artifacts; archived source files, generated headers, and compilers need not be
installed. No network operation is part of ingestion, export, or querying.

Compilation IDs identify individual compilation occurrences, so the same source
can have separate records for different invocations. Target names and compilation
IDs must be unique, every membership reference must exist, and a target must
have at least one nonrepeated member. Missing fields, blank configuration,
unknown targets, and missing or unsupported selected evidence artifacts fail
with diagnostics. Compilation databases alone do not supply this contract:
Gloom does not infer membership from source directories or replay their commands.
All declaration records must be structurally valid, but artifacts belonging only
to unselected targets need not be available.

## Evidence and limits

The build producer asserts the snapshot identity, build configuration, toolchain,
compilation-to-artifact association, generated inputs, and membership. Gloom
validates declaration structure and reference consistency; it does not establish
that a link command ran or independently prove that supplied IR was produced by
the declared compiler argv. Supplying unrelated artifacts under one target would
be a false declaration. The producer must supply coherent, finished artifacts
and keep them unchanged throughout acquisition; Gloom does not detect a producer
rewriting multiple artifacts mid-acquisition. It does not rebuild source or
watch headers. Capture of a real build is separate work tracked by #9.
This ingestion format currently supports textual LLVM IR; native object files,
bitcode, archive extraction, and link-time transformations are not supported.

The exported snapshot's optional `declared_build` record retains the manifest
location and selected declaration. Its `declaration.compilations` and
`acquired_input_ids` arrays correspond one-to-one in target membership order.
Declaration paths are resolved to absolute paths; argv remains verbatim.
Each ID is the same
acquired-input ID cited by searchable callable declarations and expanded
explanation evidence. Follow it to recover the original argv, working directory,
source input, and generated/configured inputs; the acquired input retains the
IR content fingerprint and evidence-artifact location. Generated-input paths are
build declarations, not reconstructed source locations for individual LLVM calls.

Loading an export revalidates the declaration against its observation context,
acquired inputs, and target membership without reopening build files. It preserves
the historical extraction version while checking observation-context integrity;
the reader's version need not match the extractor's version. Snapshots
published through the earlier `publish` command have no `declared_build` record
and make only that command's caller-supplied context declaration.

The checked-in `.ll` files are deliberately reduced semantic fixtures with
illustrative build records. They test acquisition and query behavior offline;
they are not benchmark measurements or verbatim compiler output. A pinned
real-build benchmark profile remains tracked by #11.
