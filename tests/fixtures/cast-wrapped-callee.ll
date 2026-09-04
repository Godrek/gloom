declare void @target(...)

define void @cast_wrapped_caller(ptr %callback) {
entry:
  call void bitcast (void (...)* @target to void ()*)()
  call void bitcast (void ()* bitcast (void (...)* @target to void ()*) to void ()*)()
  call void bitcast (void ()* inttoptr (i64 4096 to void ()*) to void ()*)()
  call void (...) bitcast (void (...)* @target to void (...)*)(i32 1)
  call void %callback()
  ret void
}
