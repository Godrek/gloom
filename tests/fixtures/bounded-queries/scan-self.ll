define void @self() {
  call void @self()
  ret void
}
