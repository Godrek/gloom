; Escapes name arbitrary bytes; whitespace is part of a quoted identifier.
declare void @"\FF"()
declare void @"\FE"()
declare void @"\EF\BF\BD"()
declare void @"trailing "()
declare void @"%FF"()

define void @caller() {
  call void @"\FF"()
  call void @"\FE"()
  call void @"\EF\BF\BD"()
  call void @"trailing "()
  call void @"%FF"()
  ret void
}
