declare void @aliasee()
declare void @variadic_aliasee(...)

@aliased = alias void (), ptr @aliasee
@resolved = ifunc void (), ptr @resolver
@split = alias void (),
    ptr @aliasee
@cast_aliased = alias void (), ptr bitcast (void (...)* @variadic_aliasee to void ()*)
@partitioned = alias void (), ptr @aliasee, partition "review"
@before_module_asm = alias void (), ptr @aliasee

module asm ""

define ptr @resolver() {
entry:
  ret ptr @aliasee
}

define void @alias_caller() #0 {
entry:
  call void @aliased()
  call void @resolved()
  call void @split()
  call void @cast_aliased()
  call void @partitioned()
  call void @before_module_asm()
  call void @before_attributes()
  ret void
}

@before_attributes = alias void (), ptr @aliasee

attributes #0 = { nounwind }
