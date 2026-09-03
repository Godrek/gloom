define void @first_caller() {
entry:
  call void @first_callee()
  ret void
}

declare void @first_callee()
