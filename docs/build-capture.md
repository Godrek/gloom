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
- Executable targets linked from objects captured compilations produced, whose
  link carries only arguments capture models: an output, object inputs, and
  flags that cannot name an input (`-g`, an optimization level, `-pipe`,
  `-pie`/`-no-pie`/`-fPIE`, `-m32`/`-m64`). A link carrying anything else —
  `-l`, `-L`, `-Wl,…`, `-Xlinker`, `-shared`, `-nostdlib`, a linker script —
  is refused, because its membership is not the membership its driver inputs
  spell out.

Anything else fails explicitly. Nothing outside this range is approximated:
no other platform, driver, language, or link shape is reconstructed, and
`docs/declared-build.md` remains the way to acquire a build Gloom cannot
capture.

## What capture observes

The wrapper Gloom writes into the capture directory records every invocation
the build makes: the resolved compiler, the verbatim argument vector, the
working directory, the environment, and the exit status. It then runs the real
compiler with the same arguments, so a captured build fails exactly where it
would have failed uncaptured, and a probe the build expected to fail stays a
failure rather than becoming a compilation.

A successful compilation is also fingerprinted while the build is still
running. The wrapper asks the compiler which files that compilation reads and
fingerprints each of them, and fingerprints the object it produced. This costs
one extra preprocessing pass per captured compilation, and it is what lets a
later replay tell the content the build compiled from content that changed
afterwards.

The compiler is resolved to one executable before anything else happens, and
that executable is what is asked for its version and target triple, baked into
the wrapper, run for every replay, and recorded. Identifying one Clang and
running another would put a toolchain in the observation context that compiled
nothing.

The wrapper is a small script on `PATH`, not a preloaded library or a system
call trace: it observes exactly what a build chose to run, works under any build
system that invokes the compiler by name, and adds nothing Gloom would have to
interpret. The cost is that a build reaching the compiler another way is not
observed — which is reported, not worked around.

- **Compilation.** An invocation carrying `-c`, with one C source and one
  object output. A compilation is identified by that object's resolved path, so
  two compilations that both write `unit.o` from different directories stay two
  compilations.
- **Membership.** The link that produced the requested target, named by its
  output file name. Its object inputs, in link order, are the target's
  contributing translation units. Each object is attributed to the last captured
  compilation that produced it *before* that link ran, and that object must
  still hold the content that compilation produced.
- **Generated inputs.** Files that did not exist before the build ran and that
  a contributing compilation then read, each recorded with the role it played —
  the translation unit's source, or a file included while compiling it — and
  with the content fingerprint taken while the build ran.
- **Environment.** The environment each compilation ran under. A replay runs
  under that environment and nothing else, so a build-local `CPATH` or
  `SOURCE_DATE_EPOCH` reaches the replay exactly as it reached the build. The
  record retains the variables that change what a compilation reads or produces
  (`gloom::capture::RETAINED_ENVIRONMENT_VARIABLES`).
- **Toolchain identity.** The version line the resolved compiler reports and
  the target triple it names. Together they qualify the observation context.
- **Build configuration.** The build command that was run. Each compilation's
  exact arguments are retained beside it.

Only the program snapshot identity is asserted rather than observed: Gloom
cannot know what program content a build is a build of, so the caller names it,
exactly as declared-artifact acquisition requires.

## How evidence is obtained

Each contributing compilation is replayed from its recorded working directory,
under its recorded environment, with its recorded arguments — the same
definitions, include paths, language and optimization settings the build used —
so that the IR is the IR of the compilation that really ran. Before the replay,
every input the wrapper fingerprinted must still hold that content; after it,
the replay must have read exactly those inputs. A source or header rewritten
between its compilation and the end of the build is refused, not published as
IR of content the build never compiled.

Only the output is redirected: `-c` and the object
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
- The target was linked more than once — by any link, including one capture
  does not model. The published image came from the last of them, so no earlier
  link's membership describes it.
- The target's link includes an object no captured compilation produced before
  that link, or names the same object twice.
- A linked object no longer holds the content the compilation that produced it
  wrote, because something rewrote it after that compilation.
- An input a contributing compilation read changed after it read it, or the
  replay read different inputs than the build did.
- A contributing compilation's environment or inputs could not be recorded, so
  its conditions cannot be reproduced.
- The link carries an argument capture does not model, so its membership is not
  the membership its driver inputs spell out: `-l`/`-L` library searches,
  `-Wl,…` or `-Xlinker` arguments forwarded to the linker (which can name
  objects, and can turn the link relocatable), an archive or shared object
  input, `-shared`, or anything else outside the modelled set.
- A compilation names no object output, compiles more than one source, compiles
  a language other than C, or passes a response file.
- A linked object was produced by a compiler other than the identified one.
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

Loading an export revalidates the capture without reopening any build file.
The published membership, target image, and compilations are recomputed from
the recorded argument vectors themselves: the link must still be one capture
models, its output must still be the published image and target name, its
object inputs must still be the published membership in order, and each
compilation's arguments must still name the source and object it publishes,
run by the identified compiler. The observation context and the acquired inputs
are then checked against that record, the historical extraction version is
preserved, and a document carrying both a `declared_build` and a
`captured_build` record is rejected: a snapshot makes one account of how its
evidence was acquired.

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
