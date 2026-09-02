declare void @callee()

define void @tokenized_calls(ptr %callback) {
entry: call void
  @callee()
  br label %commented
commented: call void ; @fake()
  %callback()
  br label %same_line
; call void @also_fake()
same_line: call void %callback()
  ret void
}
