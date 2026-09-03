# Gloom

Gloom is a program-understanding system that helps people explore software through trustworthy, evidence-backed structural views. Its language describes what Gloom knows about a program and how that knowledge is presented.

## Language

**Program-understanding system**:
A system that helps people comprehend software by collecting, relating, and querying evidence about program structure. Gloom is one; its scope is broader than any single graph projection or analysis technique.
_Avoid_: Call-graph visualizer, static-analysis platform

**Evidence source**:
An origin from which Gloom learns facts about software, such as a source-oriented extractor, linked artifact, optimized artifact, or runtime trace. LLVM is an evidence source rather than Gloom's domain model.
_Avoid_: Frontend, source of truth

**Claim**:
A statement about a program relationship or entity, supported by evidence and qualified by the context in which it holds.
_Avoid_: Fact, universal edge

**Observation context**:
An immutable, fully qualified setting that bounds where a claim holds. It identifies the program snapshot, target, build configuration, toolchain, extraction method, analysis stage, and any applicable runtime workload.
_Avoid_: Global truth, profile

**Projection**:
A selected, purpose-specific view derived from claims and their observation contexts. A projection is a view of Gloom's knowledge, not the canonical knowledge itself.
_Avoid_: The graph, universal graph

**Call-graph projection**:
A projection of callable entities and possible calling relationships. It is Gloom's first high-value projection, not its permanent domain boundary.
_Avoid_: Callgraph, program graph

**Program entity**:
A logical element of software that Gloom can identify and relate, such as a source function or call site. An entity remains distinct from the representations produced for particular observation contexts.
_Avoid_: Node, symbol

**Manifestation**:
A context-specific representation of a program entity, such as an emitted LLVM function, linked symbol, specialized clone, or inlined body. Correspondence between manifestations is explicit and evidence-backed.
_Avoid_: Version, duplicate

**Call site**:
A program entity representing one syntactic or lowered invocation location. Target claims attach to a call site; aggregate caller-to-callee relationships are derived projections.
_Avoid_: Call edge, call count

**Target claim**:
A claim that a call site may invoke a particular callable entity or manifestation. A target claim records its evidence without implying that observed targets are the only possible targets.
_Avoid_: Callee edge, resolved call

**Resolution**:
The degree to which a call site's possible targets are known: complete, partial, or absent. Resolution is independent of the kinds of evidence supporting individual target claims; completeness is not inferred from how evidence was obtained but requires an explicit completeness basis.
_Avoid_: Confidence, certainty

**Completeness basis**:
An evidence contributor's explicit declaration that it observed a closed target set at one call site, naming the boundary it observed and the guarantee that no other target exists within that boundary. Only a call site's resolution evidence carries one, and only complete resolution may rest on it.
_Avoid_: Whole-program flag, completeness score

**Evidence scope**:
Whether an evidence record describes static program structure or an observed runtime execution. Runtime-scoped evidence holds only in an observation context that names its workload.
_Avoid_: Evidence kind, confidence level

**Evidence support**:
What an evidence record supports: a call site's target-set resolution or one target claim. Support keeps resolution evidence and target evidence from being substituted for one another.
_Avoid_: Evidence role, edge weight

**Contributor callable identity**:
The identity an evidence contributor asserts for a callable within one observation context. It is the contributor's claim of sameness across its own contexts and is distinct from a display name.
_Avoid_: Symbol name, function name

**Build-scoped source projection**:
A source-oriented projection for one real compilation, with source correspondence wherever evidence preserves it. It does not reconstruct language-level meaning that its evidence sources have erased.
_Avoid_: Source-faithful graph, source graph

**Correspondence claim**:
A claim that two program entities or manifestations represent related program meaning across observation contexts or revisions. Names or similar bodies alone do not establish correspondence.
_Avoid_: Identity match, same symbol

**Program snapshot**:
An immutable identity for the program content being observed. Program-entity identity is stable within a snapshot; continuity across snapshots is expressed through correspondence claims.
_Avoid_: Revision, current program

**Evidence record**:
An immutable account of support produced by an evidence source in an observation context. Derived claims refer to their input evidence records so their conclusions remain explainable and recomputable.
_Avoid_: Result, annotation

**Open-world projection**:
A projection in which an absent claim means only that the claim is not present, not that the relationship is impossible.
_Avoid_: Incomplete graph

**Closed-world projection**:
A projection whose declared scope has complete enough evidence for absence to support a negative conclusion. The completeness guarantee applies only within that explicit scope.
_Avoid_: Complete graph, whole program

**Build target**:
A build-produced artifact or runnable unit whose contributing program entities are tracked as part of its observation context. Target membership does not by itself imply whole-program call resolution.
_Avoid_: Root, binary graph

**Relationship claim**:
A claim that relates program entities or manifestations within an observation context. Queryable edges are projection-specific representations of relationship claims rather than canonical knowledge.
_Avoid_: Edge, link

**Build root**:
A callable designated by build or artifact evidence as a starting point for execution in a build target.
_Avoid_: Entry point

**Exported callable**:
A callable made available beyond its defining linkage unit or artifact boundary. Export status does not imply that the callable begins execution.
_Avoid_: Entry point, public function

**Runtime root**:
A callable observed or configured as the beginning of a runtime execution flow.
_Avoid_: Entry point

**Starting point**:
A program entity selected by a user or query as the origin of an exploration. It carries no claim about build or runtime semantics.
_Avoid_: Entry point, root

**Definite recursive cycle**:
A cycle supported entirely by target claims whose resolution is complete for the selected closed-world scope.
_Avoid_: Cycle

**Potential recursive cycle**:
A cycle that depends on conservative or partially resolved target claims and therefore may not occur in a concrete program.
_Avoid_: Cycle

**Explanation handle**:
A compact reference from a query result to the evidence records and derivations that support it.
_Avoid_: Provenance blob, metadata link

**Build acquisition**:
The explicit process by which Gloom obtains build evidence, either by ingesting declared build artifacts or by capturing a real build. Approximate reconstruction is not build acquisition unless labeled as such.
_Avoid_: Build discovery, automatic build

**Published snapshot**:
An immutable program snapshot made available for queries after its required evidence and indexes are coherent. In-progress indexing does not mutate a published snapshot.
_Avoid_: Current graph, live graph

**Named query**:
A reusable query definition that fixes projection, relationship, context, resolution, and direction semantics. User interfaces and adapters share named queries rather than reimplementing traversal rules.
_Avoid_: Graph algorithm, generic traversal

**Evidence contributor**:
An extractor or analysis extension that supplies validated, versioned evidence records with declared capabilities. Contributors may extend vocabulary without redefining core semantics.
_Avoid_: Plugin, frontend

**Benchmark profile**:
A reproducible corpus definition that fixes program revision, build target, build configuration, toolchain range, acquisition method, and expected measurements or correctness checks.
_Avoid_: Benchmark codebase, latest Valkey
