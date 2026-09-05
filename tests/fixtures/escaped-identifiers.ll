; Spellings that decide identity, each checked against the assembler.
;
; `@"\66oo"` is `@foo`: LLVM decodes a quoted name's `\XX` hex escapes before
; the symbol is emitted, so the call below reaches the internal `@foo` and the
; object file holds one local `t foo`.
;
; `@0` and `@"0"` are two different globals: the unquoted all-digit form is an
; unnamed value numbered by its slot, which the assembler emits as
; `__unnamed_1`, while the quoted form is a named global whose name is `0`.
;
; `@"a\22b"` shows that a backslash never ends a quoted identifier: `\22` is a
; quote character inside the name, and the emitted symbol is `a"b`.
define internal void @foo() {
entry:
  ret void
}

define void @0() {
entry:
  ret void
}

define void @"0"() {
entry:
  ret void
}

define void @"a\22b"() {
entry:
  ret void
}

define void @escaped_caller() {
entry:
  call void @"\66oo"()
  ret void
}
