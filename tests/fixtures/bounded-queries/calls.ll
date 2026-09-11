define void @a(ptr %unknown) {
 call void @b()
 call void %unknown()
 ret void
}
define void @b() {
 call void @c()
 ret void
}
define void @c() {
 call void @a(ptr null)
 ret void
}
define void @self() {
 call void @self()
 ret void
}
define internal void @helper() {
 ret void
}
