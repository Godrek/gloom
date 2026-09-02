declare { i32, i32 } @pair()
declare { i32, i1 } @llvm.sadd.with.overflow.i32(i32, i32)

define void @aggregate_return_caller(ptr %callback) {
entry:
  %value = call { i32, i32 } @pair()
  %sum = call { i32, i1 } @llvm.sadd.with.overflow.i32(i32 1, i32 2)
  %indirect = call { i32, i32 } %callback()
  ret void
}
