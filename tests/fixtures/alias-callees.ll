declare void @aliasee()

@aliased = alias void (), ptr @aliasee
@resolved = ifunc void (), ptr @resolver

define ptr @resolver() {
entry:
  ret ptr @aliasee
}

define void @alias_caller() {
entry:
  call void @aliased()
  call void @resolved()
  ret void
}
