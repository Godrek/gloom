# Scope a callable identity by the linkage that decides sameness

An evidence contributor declares the scope in which its contributor callable
identity means one callable: the acquired input it was read from, or the
namespace the link joins acquired inputs in. The core never parses such an
identity, so the declared scope is what lets it reject an input-scoped identity
that manifests in a second acquired input. Two identically named
translation-unit-local callables are therefore different callables by
construction rather than by the accident of separate entity numbering, while an
exported symbol keeps one identity across the inputs that name it — the
evidence a link-time correspondence claim would rest on, which ADR 0002 still
requires to be evidence-backed rather than inferred from matching names.

Treating equal names across acquired inputs as one program entity was rejected:
it is exactly the false merge ADR 0002 forbids for local symbols, and for
exported ones it would assert a link the acquired evidence does not describe.
The cost is a contract change for contributors, who must now say what a name
means rather than emitting it bare, and a publication that acquires one
translation unit twice is refused rather than merged, because its contributor
cannot tell the two acquisitions apart and so asserts one input-scoped identity
for both.
