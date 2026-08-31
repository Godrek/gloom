# Gloom Product Vision

## Purpose

Gloom is a program-understanding system that helps people investigate unfamiliar
or complex software through trustworthy structural views. Its first high-value
projection is a call graph; that projection is not its permanent domain
boundary.

Human code comprehension is the primary product outcome. Scripts, services, and
AI agents operate on the same underlying knowledge through first-class machine
interfaces. Those interfaces enforce precise, reusable semantics rather than
creating a separate automation product.

Gloom is static-first and evidence-plural. Static analysis must provide a useful
standalone workflow. Runtime observations may enrich its knowledge, but Gloom
does not aim to become a profiler or tracing platform.

## Product promise

A user can select a real build target, find a callable, inspect bounded callers
and callees, follow a path, and understand why every displayed relationship is
present. Missing or uncertain knowledge remains visible instead of being
replaced by guesses.

The same investigation is available through the CLI and local query interfaces
without scraping the visual interface or sending a whole codebase to an AI
prompt.

## Decision priorities

When goals conflict, Gloom uses this order:

1. Trustworthiness and explainability.
2. Local privacy.
3. Usefulness for the focused human workflow.
4. Predictable, bounded performance.
5. Contract stability after stabilization.
6. Extensibility.

An honest partial answer is preferable to a complete-looking guess. Correctness
does not, however, excuse an interface too slow or opaque to support an actual
investigation.

## Trust model

Gloom's canonical knowledge consists of program entities, their
context-specific manifestations, immutable evidence records, and claims derived
from that evidence. A graph is a selected projection of this knowledge for a
particular question.

Every claim is bounded by an observation context. Relevant context includes the
program snapshot, build target and configuration, toolchain, extraction method,
analysis stage, and runtime workload when applicable. Claims from different
contexts may coexist without being silently collapsed.

Absence is not impossibility by default. A projection may support negative
conclusions only when it declares a closed-world scope with a justified
completeness guarantee.

Every query result exposes a compact explanation handle leading back to its
supporting evidence and derivations.

## Identity

Names are labels, not identities. Translation-unit-local functions with the
same name remain distinct, as do source entities, emitted functions, linked
symbols, optimized clones, and inlined manifestations.

Identity is stable within an immutable program snapshot. Correspondence across
analysis contexts or revisions is an evidence-backed claim, not a heuristic
merge disguised as identity.

Call sites are first-class program entities. Possible targets attach to call
sites as claims; aggregate caller-to-callee relationships and static call-site
counts are derived projections. Static cardinality and runtime invocation
counts are always distinct measures.

## Uncertainty

Gloom represents target resolution separately from evidence type. A call site's
target set may be complete, partial, or absent. Individual target claims may be
supported by direct static resolution, conservative analysis, runtime
observation, or other declared evidence.

Runtime observation never implies that an observed target is the only possible
target. Conservative relationships may produce potential recursive cycles;
they are not reported as definite recursive cycles.

## Build fidelity

C analysis must use evidence from a real build: compiler flags, includes,
defines, generated headers, target, toolchain, and contributing inputs. Gloom
supports explicit acquisition by ingesting declared build artifacts and by
capturing builds when target or link membership is otherwise unavailable. It
does not silently invent missing configuration and label the result
build-faithful.

A build-scoped source projection reflects one concrete compilation and retains
source correspondence wherever the evidence preserves it. It does not promise
to reconstruct macros, language semantics, or other information already erased
by LLVM IR.

Minimal target membership belongs in the first production-shaped validation;
whole-program LLVM resolution does not. Source-oriented, linked, optimized, and
runtime evidence are observation contexts from which comparative projections
can be derived, not competing universal graphs.

## Product shape

The language-neutral Rust core owns domain semantics, evidence validation,
identity, indexing, projection and query semantics, orchestration, and local
interfaces. Language- and toolchain-specific contributors remain behind
versioned contracts.

The core offers named queries rather than generic graph traversal. Each query
fixes its projection, allowed relationship kinds, contexts, resolution policy,
and direction. CLI output, a local service, snapshot export, and viewer requests
are adapters over the same application/query layer.

The viewer is a projection client. It owns presentation state and layout; it
does not independently reinterpret evidence or traversal semantics. Small
self-contained HTML snapshots remain a useful export, not the scalable product
architecture.

Evidence contributors declare their capabilities and emit versioned records
that the core validates. They may add namespaced semantics without redefining
core vocabulary. LLVM is one contributor family, not the Gloom domain model.

The kernel remains language-neutral without collapsing all languages to their
lowest common denominator. Language-specific contributors may retain concepts
such as Rust traits, C macros, or C++ templates through typed extensions.
Concepts move into the kernel only when shared workflows require common
behavior. Rust is a likely future validation case, not a scheduled commitment.

## Local privacy

Indexing, storage, analysis, querying, and visualization require no network
access. Source, evidence, identifiers, and telemetry do not leave the machine
without an explicit, visibly scoped integration. Remote clients and services
may be supported as opt-in adapters without weakening the local workflow.

## First production-shaped validation

The first vertical slice validates build-faithful call-path investigation for
one pinned `valkey-server` benchmark profile on Linux and a documented
Clang/LLVM range. It includes:

- explicit build acquisition and target membership;
- correct translation-unit-local identity;
- first-class call sites and visible unresolved indirect calls;
- target selection and callable search;
- bounded caller, callee, and path queries;
- observation-context filters and evidence inspection;
- a responsive focused-neighborhood viewer;
- immutable snapshots published atomically after incremental work; and
- reproducible correctness and performance measurements.

Small semantic fixtures establish identity and relationship rules. Integration
fixtures cover build acquisition and cross-translation-unit behavior. The
pinned Valkey profile supplies scale measurements and reviewed expectations for
known paths, unresolved sites, and target membership.

Measurement protocols cover clean indexing, a one-file warm reindex, peak
memory, bounded-query latency distributions, and time to first useful view.
Budgets are set only after a reproducible baseline exists.

Detailed acceptance criteria and implementation work belong in GitHub Issues,
not in this vision.

## Public contracts

The current JSON schema 1.0 and CLI are pre-stable prototype interfaces. A
future durable contract requires versioned schemas, additive minor evolution,
explicit major migrations, unknown-field tolerance, and a documented reader
support window.

Bounded queries are the primary live interface. Snapshot export remains a
first-class interchange format. Storage engines, wire encodings, and UI
technology remain implementation choices until evidence justifies commitment.

## Non-goals

Gloom does not aim to:

- resolve every indirect call or hide unresolved targets;
- render every program entity simultaneously;
- replace compilers, LLVM analyses, profilers, or tracing systems;
- treat one build configuration as universal program truth;
- infer source-language meaning that available evidence has erased;
- become an IDE or a general developer-workflow platform;
- require proprietary source to leave the developer's machine; or
- commit to a delivery schedule for every LLVM-based language.

## Success

Gloom succeeds when a developer can reproduce a real target's analysis, move
from a subsystem or callable to a precise source relationship, follow a bounded
path, and inspect the evidence behind every useful result or uncertainty. The
same named query produces consistent semantics for the viewer, CLI, scripts,
and agents.

At scale, clean and incremental indexing remain measurable, bounded queries are
predictable, published snapshots are coherent, and changes between program
snapshots can be compared without pretending heuristic correspondence is
identity.

## Document authority

- The repository README describes current, executable reality.
- This vision owns durable product purpose, principles, boundaries, and success.
- `CONTEXT.md` owns canonical domain vocabulary and contains no implementation
  decisions.
- `docs/adr/` records accepted, costly-to-reverse decisions and their rationale.
- GitHub Issues own milestone specifications, acceptance criteria, research,
  and implementation work.
- Versioned contract documents own public schemas and protocols once those
  contracts stabilize.
