define i32 @caller() {
entry:
  %value = call i32 @callee(i32 42)
  ret i32 %value
}

define i32 @callee(i32 %value) {
entry:
  ret i32 %value
}
