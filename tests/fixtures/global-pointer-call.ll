@function_pointer = external global ptr

define void @calls_through_global() {
entry:
  call void @function_pointer()
  ret void
}
