# Complete resolution requires static resolution evidence

A call site's complete resolution asserts that its recorded targets are the
only targets it can invoke, so it must be supported by at least one
call-site-resolution evidence record whose scope is static. Runtime-scoped
resolution evidence may support absent or partial resolution but never
completeness, because an observed workload reports what happened rather than
what is possible. This costs contributors the ability to certify a closed world
from tracing alone, and because resolution evidence must share its observation
context's scope, complete resolution can only be asserted in a context with no
runtime workload. It keeps a convenient observation from silently narrowing the
target set a projection is allowed to treat as closed.
