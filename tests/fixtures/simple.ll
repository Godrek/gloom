declare i32 @puts(ptr)

define i32 @main() {
entry:
  %result = call i32 @step(i32 2)
  ret i32 0
}

define i32 @step(i32 %n) {
entry:
  %done = icmp eq i32 %n, 0
  br i1 %done, label %exit, label %again
again:
  %next = sub i32 %n, 1
  %value = call i32 @step(i32 %next)
  br label %exit
exit:
  %message = call i32 @puts(ptr null)
  ret i32 0
}
