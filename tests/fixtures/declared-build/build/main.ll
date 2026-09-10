; Reduced semantic fixture, not verbatim compiler output.
declare void @worker()
define void @server() {
  call void @worker()
  ret void
}
