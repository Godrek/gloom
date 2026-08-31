# Claims and evidence are canonical

Gloom stores program entities, context-specific manifestations, immutable
evidence records, and claims derived from that evidence as its canonical
knowledge. Graphs are purpose-specific projections and query indexes rather
than the source of truth. This costs more than a single mutable property graph,
but it preserves conflicting observations, supports explanation and
recomputation, and prevents a convenient visualization from silently defining
program truth.
