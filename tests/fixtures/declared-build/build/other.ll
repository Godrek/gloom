; Reduced semantic fixture, not verbatim compiler output.
define internal void @helper() {
  ret void
}
define void @other_tool() {
  call void @helper()
  ret void
}
