@data = global i8 0

declare void @function()

@selected = alias void (), ptr select (i1 false, ptr @function, ptr @data)

define void @select_alias_caller() {
entry:
  call void @selected()
  ret void
}
