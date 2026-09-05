# Scope a callable identity by the linkage that decides sameness

An evidence contributor declares the scope in which its contributor callable
identity means one callable: the acquired input it was read from, or the
linkage namespace that joins acquired inputs. The opaque contributor
identity and its scope are one validated value, so they cannot disagree
while passing through acquisition and publication. The core never parses the
identity; it keys on the declared scope. An acquired-input identity is keyed
by the acquired input the core assigned as well, so each input publishes its
own program entity and no two can be joined — including two byte-identical
translation units, which a contributor cannot tell apart yet which compile
and link as two units. A linkage-namespace identity is keyed by the identity
alone within one observation context, and so is aggregated into one program
entity carrying a manifestation for every input that declares or defines it.
Validation holds the same line on a hand-edited export: a program entity
carrying an acquired-input identity may not span acquired inputs.

The namespace such an identity is joined in is the observation context's,
and nothing wider. A context names one build target, so its acquired inputs
are the ones contributing to that artifact; aggregation is keyed by context
and never crosses one, and across contexts the manifestations stay separate
entities related by a correspondence claim. That the declared inputs really
do contribute to the declared target is acquisition evidence Gloom does not
yet collect, so it is the observation context's declaration, on the same
footing as every other claim that context bounds. A publication that
declares one build target for inputs from two unrelated artifacts therefore
describes a link that was never performed — a misdeclared context, not a
name-based merge.

This aggregation is consistent with ADR 0002 because it does not infer
sameness from the program entity's display name. The contributor explicitly
asserts the same opaque identity and linkage-namespace scope at every
declaration, backed by contributor-identity evidence in each acquired input.
For the LLVM contributor, this assertion is the contributor's account of the
link-visible symbol the module declares; the readable label remains
separate. Correspondence across observation contexts or revisions still
requires a correspondence claim. Two identically named translation-unit-
local callables therefore remain different by construction rather than by
the accident of separate entity numbering.

Treating equal *names* across acquired inputs as one program entity was
rejected: that is the false merge ADR 0002 forbids, and it would join local
symbols too. What is aggregated is the contributor's explicitly scoped
identity, which for a local callable is never shared and for an exported one
is asserted at each declaration. The cost is a contract change for
contributors, who must now provide a scoped identity rather than emitting a
bare string and separate scope, and a correctness obligation the core cannot
yet discharge: aggregation is only as sound as the observation context's
declared build target.
