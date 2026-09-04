; Linkage keywords in every position the LangRef allows before a name, so a
; misread prefix shows up as a misclassified identity. Verified with
; clang -x ir -mllvm -opaque-pointers -c; the object's symbol table is
; t/i for every local global here and T/W for every exported one, and
; @struct_local is private so it is emitted with no symbol at all.
define internal dso_local fastcc noalias ptr @tricky_local(i32 zeroext %n) unnamed_addr {
entry:
  ret ptr null
}

define private { i32, i32 } @struct_local() {
entry:
  ret { i32, i32 } zeroinitializer
}

$exported_odr = comdat any
define linkonce_odr dso_local protected void @exported_odr() comdat {
entry:
  ret void
}

define internal void @"quoted internal"() {
entry:
  ret void
}

@alias_local = internal unnamed_addr alias void (), ptr @exported_odr
@alias_public = weak_odr alias void (), ptr @exported_odr
@ifunc_local = internal ifunc void (), ptr @resolver
define internal ptr @resolver() {
entry:
  ret ptr null
}

define void @user() {
entry:
  %p = call fastcc ptr @tricky_local(i32 zeroext 1)
  call void @alias_local()
  call void @alias_public()
  call void @ifunc_local()
  call void @"quoted internal"()
  %s = call { i32, i32 } @struct_local()
  ret void
}
