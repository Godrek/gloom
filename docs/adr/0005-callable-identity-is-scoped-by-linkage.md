# Scope a callable identity by the linkage that decides sameness

An evidence contributor declares the scope in which its contributor callable
identity means one callable: the acquired input it was read from, or the
linkage namespace that joins acquired inputs. The opaque contributor identity
and its scope are one validated value, so they cannot disagree while passing
through acquisition and publication. The core never parses the identity. It
rejects an acquired-input-scoped identity that manifests in a second acquired
input, while one linkage-namespace identity within one observation context is
aggregated into one program entity with a manifestation for every input that
declares or defines it.

This aggregation is consistent with ADR 0002 because it does not infer
sameness from the program entity's display name. The contributor explicitly
asserts the same opaque identity and linkage-namespace scope at every
declaration, backed by contributor-identity evidence in each acquired input.
For the LLVM contributor, this assertion is the contributor's account of the
link-visible symbol the module declares; the readable label remains separate.
Correspondence across observation contexts or revisions still requires a
correspondence claim. Two identically named translation-unit-local callables
therefore remain different by construction rather than by the accident of
separate entity numbering.

Treating equal names across acquired inputs as one program entity was rejected:
it is exactly the false merge ADR 0002 forbids for local symbols, and for
exported ones it would assert a link the acquired evidence does not describe.
The cost is a contract change for contributors, who must now provide a scoped
identity rather than emitting a bare string and separate scope, and a
publication that acquires one translation unit twice is refused rather than
merged, because its contributor cannot tell the two acquisitions apart and so
asserts one input-scoped identity for both.
