declare void @"}"()

define void @quoted_brace_caller(ptr %callback) {
entry:
  call void @"}"()
  call void %callback()
  ret void
}
