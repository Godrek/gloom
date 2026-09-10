; Reduced semantic fixture, not verbatim compiler output.
define internal void @helper() {
  ret void
}
define void @worker() {
  call void @helper()
  ret void
}
