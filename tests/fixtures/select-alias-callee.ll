@data = global i8 0

declare void @function()

@selected = alias void (), ptr select (i1 false, ptr @function, ptr @data)
@offset = alias void (), ptr getelementptr (i8, ptr @function, i64 0)

define void @select_alias_caller() {
entry:
  call void @selected()
  call void @offset()
  ret void
}
