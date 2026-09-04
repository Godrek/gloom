declare void @real()

define void @wrapped_caller() {
entry:
  call void dso_local_equivalent @real()
  call void no_cfi @real()
  call void getelementptr (i8, ptr @real, i64 0)()
  br label %other
other:
  call void blockaddress(@wrapped_caller, %other)()
  ret void
}
