; The second translation unit. Its `@helper` is a different internal function
; that happens to share a name with the first unit's, and it calls a callable
; the first unit never mentions. `@shared_service` is only declared here: the
; definition lives in the first unit and the linker joins them.
define void @second_entry() {
entry:
  call void @helper()
  call void @shared_service()
  ret void
}

define internal void @helper() {
entry:
  call void @second_only()
  ret void
}

declare void @shared_service()

declare void @second_only()
