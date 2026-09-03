@handler = external global ptr

define void @global_variable_caller() {
entry:
  call void @handler()
  call void @declared_target()
  ret void
}

declare void @declared_target()
