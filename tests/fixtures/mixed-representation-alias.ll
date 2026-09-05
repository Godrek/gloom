; The other half: here the link-visible `@foo` is an alias for a local
; definition, so this unit writes as an alias the symbol the sibling unit
; merely declares.
define internal void @mixed_body() {
entry:
  ret void
}

@foo = alias void (), ptr @mixed_body
