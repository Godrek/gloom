; One translation unit of a two-unit fixture. Its `@helper` has internal
; linkage, so it is private to this unit and unrelated to the identically
; named `@helper` of `local-shadow-second.ll`. `@shared_service` has external
; linkage and is defined here.
define void @first_entry() {
entry:
  call void @helper()
  call void @shared_service()
  ret void
}

define internal void @helper() {
entry:
  call void @first_only()
  ret void
}

define void @shared_service() {
entry:
  ret void
}

declare void @first_only()
