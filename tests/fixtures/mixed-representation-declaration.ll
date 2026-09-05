; One half of a two-unit fixture in which the link-visible `@foo` is written
; differently in each unit. Here it is an external declaration the caller
; invokes; `mixed-representation-alias.ll` defines it as an alias. Both units
; assemble and name the same symbol, so representation belongs to each
; manifestation rather than deciding whether the two are one callable.
declare void @foo()

define void @mixed_caller() {
entry:
  call void @foo()
  ret void
}
