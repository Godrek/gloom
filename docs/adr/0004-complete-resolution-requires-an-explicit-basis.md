# Complete resolution requires an explicit completeness basis

A call site's complete resolution asserts a closed target set, so at least one
of its call-site-resolution evidence records must carry a completeness basis:
the boundary the contributor observed and the guarantee that no other target
exists within it. A basis on evidence for a partial or absent resolution is a
contradiction and is rejected. This preserves the independence of resolution
from evidence type decided in issue #4 — runtime-scoped evidence with a stated
basis may support completeness, and static-scoped evidence without one may not
— and it keeps closed-world conclusions limited to an explicitly justified
scope, as the parent user story requires. Treating static scope as a proxy for
completeness was rejected: a single-translation-unit or heuristic static scan
is static and still open, so the proxy would have both licensed unjustified
claims and forbidden justified ones. The cost is a contract change for
contributors, who must now say what they closed over instead of asserting
completeness by assertion alone.
