declare void @aliasee()
declare void @variadic_aliasee(...)

@aliased = alias void (), ptr @aliasee
@resolved = ifunc void (), ptr @resolver
@split = alias void (),
    ptr @aliasee
@cast_aliased = alias void (), ptr bitcast (void (...)* @variadic_aliasee to void ()*)

define ptr @resolver() {
entry:
  ret ptr @aliasee
}

define void @alias_caller() {
entry:
  call void @aliased()
  call void @resolved()
  call void @split()
  call void @cast_aliased()
  ret void
}
