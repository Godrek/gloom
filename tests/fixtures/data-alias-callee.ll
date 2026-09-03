@data = global i8 0
@data_alias = alias i8, ptr @data
@chained_data_alias = alias i8, ptr @data_alias

define void @data_alias_caller() {
entry:
  call void @data_alias()
  call void @chained_data_alias()
  ret void
}
