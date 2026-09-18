# Capture a real build

When a build producer supplies no manifest, Gloom can acquire the same
knowledge by capturing a build. The build runs once under a compiler wrapper;
the compilations it makes and the link that produces one executable image are
what Gloom publishes.

```bash
cargo run -- capture-build \
  --project-root tests/fixtures/build-capture \
  --target server \
  --snapshot-id my-build-content-v1 \
  --capture-dir /tmp/server-capture \
  -o /tmp/server-snapshot.json \
  -- sh build.sh
cargo run -- query-snapshot /tmp/server-snapshot.json --callees main
cargo run -- investigate /tmp/server-snapshot.json --request query.json
```

The Rust entry point is `Application::capture_build`, taking a
`gloom::capture::BuildCaptureRequest`. The returned `PublishedSnapshot` holds
only the selected target's translation units, so the
[bounded named queries](bounded-queries.md) operate within that target's
observation context. Capture another target by capturing the build again into
another capture directory.

## Supported range

Build capture is supported **only on Linux with Clang**, in this initial range:

- Linux, with a Clang reporting a Linux target triple.
- Clang 14 through 20 (`gloom::capture::SUPPORTED_CLANG_MAJOR_VERSIONS`).
- A build that invokes its C compiler as `clang`, found on `PATH`. A build
  that invokes `cc`, `gcc`, a cross-compiler driver, or Clang by absolute path
  is not observed, and its compilations are reported missing rather than
  guessed at.
- C translation units, one source per compilation, each naming its object
  output.
- Executable targets linked from objects captured compilations produced.

Anything else fails explicitly. Nothing outside this range is approximated:
no other platform, driver, language, or link shape is reconstructed, and
`docs/declared-build.md` remains the way to acquire a build Gloom cannot
capture.

## What capture observes

The wrapper Gloom writes into the capture directory records every invocation
the build makes: the resolved compiler, the verbatim argument vector, the
working directory, and the exit status. It then runs the real compiler with the
same arguments, so a captured build fails exactly where it would have failed
uncaptured, and a probe the build expected to fail stays a failure rather than
becoming a compilation.

The wrapper is a small script on `PATH`, not a preloaded library or a system
call trace: it observes exactly what a build chose to run, works under any build
system that invokes the compiler by name, and adds nothing Gloom would have to
interpret. The cost is that a build reaching the compiler another way is not
observed — which is reported, not worked around.

- **Compilation.** An invocation carrying `-c`, with one C source and one
  object output.
- **Membership.** The link that produced the requested target, named by its
  output file name. Its object inputs, in link order, are the target's
  contributing translation units.
- **Generated inputs.** Files that did not exist before the build ran and that
  a contributing compilation then read, each recorded with the role it played:
  the translation unit's source, or a file included while compiling it.
- **Toolchain identity.** The version line the captured compiler reports and
  the target triple it names. Together they qualify the observation context.
- **Build configuration.** The build command that was run. Each compilation's
  exact arguments are retained beside it.

Only the program snapshot identity is asserted rather than observed: Gloom
cannot know what program content a build is a build of, so the caller names it,
exactly as declared-artifact acquisition requires.

## How evidence is obtained

Each contributing compilation is replayed from its recorded working directory
with its recorded arguments — the same definitions, include paths, language
and optimization settings the build used — so that the IR is the IR of the
compilation that really ran. Only the output is redirected: `-c` and the object
output become `-S -emit-llvm` into a `.ll` file under the capture directory,
`-fno-discard-value-names` retains the names the source gave its values, and
`-MD -MF` records the inputs the compilation read. The replayed argument vector
is retained in the record as `extraction_arguments`, so the adjustment is
visible rather than implied.

Replay happens after the build has finished, so the sources and generated
inputs the build compiled must still be there. A build that deletes them as
part of its own run cannot be replayed, and capture reports the compiler's
failure rather than substituting anything for what is gone.

Each retained `.ll` is one acquired input, with acquisition method
`captured-build`. Evidence provenance therefore names a file a reader can open,
and a reader can tell evidence Gloom captured from evidence a producer
declared.

## Explicit failures

Capture publishes nothing when it cannot observe what it would need:

- The build command failed.
- No compilation was captured, because the build did not invoke `clang` on
  `PATH`.
- The requested target was never linked; the diagnostic names the targets the
  build did link.
- The target was linked more than once, so its membership is ambiguous.
- The target's link includes an object no captured compilation produced, or
  names the same object twice.
- More than one captured compilation produced the same object, so which
  compilation a link member came from is not observable.
- The link reaches membership Gloom does not observe: a `-l` library search, an
  archive or shared object input, or a link that produces something other than
  an executable image.
- A compilation names no object output, compiles more than one source, compiles
  a language other than C, or passes a response file.
- The compiler is not a Clang in the supported range, or does not target Linux.
- The capture directory already holds a capture. Each capture is a fresh
  directory; captures are never appended to one another.

None of these is repaired by reconstructing configuration. A build Gloom cannot
capture is one to declare instead.

## What the snapshot retains

The exported snapshot's optional `captured_build` record holds the observed
build: the build command, project root, capture directory, toolchain identity,
each contributing compilation, and the link that established membership. Its
`capture.compilations` and `acquired_input_ids` arrays correspond one-to-one in
link order, and each ID is the acquired-input ID cited by searchable callable
declarations and expanded explanation evidence. Follow it to recover the
argument vector, working directory, source input, object output, and generated
inputs behind any published callable.

Loading an export revalidates the capture against its observation context,
acquired inputs, and membership without reopening any build file. It preserves
the historical extraction version while checking context integrity. A snapshot
makes one account of how its evidence was acquired: a document carrying both a
`declared_build` and a `captured_build` record is rejected.

Capture establishes that the compilations really ran and that the link really
named them. It does not establish whole-program call resolution, and target
membership never implies a closed world. The IR is per-translation-unit IR
before linking, so link-time transformations, LTO, and anything the linker
changes are outside what this evidence describes.

## Relationship to declared-artifact acquisition

Captured and declared acquisition describe the same kind of build and answer
the [bounded named queries](bounded-queries.md) identically when they describe
the same build: the same observation context, the same acquired evidence, the
same callables, relationships, resolutions, and explanation handles. The
regression test in `tests/build_capture.rs` captures the fixture build, writes
the declaration a producer would have written for that same build, publishes it
through `ingest-build`, and compares every bounded query's result.

What differs is only the account of acquisition. A declared build is a
producer's assertion that the named artifacts belong to the named target;
Gloom validates its structure. A captured build is Gloom's own observation of
the compilations and the link, and the acquisition method on each acquired
input says which one a snapshot rests on.
