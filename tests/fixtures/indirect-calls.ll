define void @dispatch(ptr %first, ptr %second) {
entry:
  call void %first()
  call void %second()
  ret void
}

define void @run_callback(ptr %callback, ptr %"asm") {
entry:
  call void %callback()
  call void %"asm"()
  call void asm sideeffect "", ""()
  call void asm unwind "call ignored", ""()
  ; call void @comment_only()
  %call = add i32 1, 2
  %invoke = add i32 3, 4
  ret void
}
