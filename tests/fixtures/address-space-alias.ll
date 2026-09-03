define void @plain_target() {
entry:
  ret void
}

define void @space_target() addrspace(1) {
entry:
  ret void
}

@opaque_space = alias void (), ptr addrspace(1) @space_target
@typed_space = alias void (), void () addrspace(1)* @space_target
@no_space = alias void (), ptr @plain_target

define void @address_space_caller() {
entry:
  call addrspace(1) void @opaque_space()
  call addrspace(1) void @typed_space()
  call void @no_space()
  call addrspace(1) void @space_target()
  ret void
}
