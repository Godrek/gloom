!0 = !{}
!declare = !{!0}

declare void @callee()

define void @metadata_only() {
entry:
  ret void, !call !0
}

define void @aggregate_prefix(ptr %callback) prefix { i32, i32 } { i32 1, i32 2 } {
entry:
  call void %callback()
  ret void
}
