declare void @callee()

define void @tokenized_calls(ptr %callback) {
entry: call void
  @callee()
commented: call void /* @fake() */ %callback()
/*
  call void @also_fake()
*/
same_line: call void %callback()
  ret void
}
