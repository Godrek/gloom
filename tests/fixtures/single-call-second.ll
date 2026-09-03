define void @second_caller() {
entry:
  call void @second_callee()
  ret void
}

declare void @second_callee()
