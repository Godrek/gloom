%Pair = type { i32, i1 }

declare %Pair @returns_pair(i32, ...)

define void @named_type_caller(ptr %callback) {
entry:
  %direct = call %Pair (i32, ...) @returns_pair(i32 1)
  %indirect = call %Pair (i32, ...) %callback(i32 2)
  call void %callback()
  ret void
}
