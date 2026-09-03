# Complete resolution requires an explicit completeness basis

A call site's complete resolution asserts a closed target set, so at least one
of its call-site-resolution evidence records must carry a completeness basis:
the boundary the contributor observed and the guarantee that no other target
exists within it. A basis on evidence for a partial or absent resolution is a
contradiction and is rejected, and every call-site-resolution evidence record
belongs to exactly one resolution, so a basis cannot ride along unreferenced.
This preserves the independence of resolution from evidence type decided in
issue #4 — runtime-scoped evidence with a stated basis may support
completeness, and static-scoped evidence without one may not — and it keeps a
closed-world projection's guarantee limited to the scope its evidence
explicitly justifies, as the parent user story requires. Treating static scope
as a proxy for completeness was rejected: a single-translation-unit or
heuristic static scan is static and still open, so the proxy would have both
licensed unjustified claims and forbidden justified ones. The cost is a
contract change for contributors, who must now say what they closed over
instead of declaring completeness bare.
