; Stop the running service first so its exe can be replaced on upgrade.
!macro NSIS_HOOK_PREINSTALL
  nsExec::ExecToLog 'sc.exe stop OpenSenses'
  Sleep 1500
!macroend

!macro NSIS_HOOK_POSTINSTALL
  nsExec::ExecToLog '"$INSTDIR\opensenses-service.exe" install'
!macroend

; On a real uninstall (not an update), put the laptop's settings back to
; how they were before OpenSenses changed them, then remove the service.
!macro NSIS_HOOK_PREUNINSTALL
  ${If} $UpdateMode <> 1
    nsExec::ExecToLog '"$INSTDIR\opensenses-service.exe" restore'
  ${EndIf}
  nsExec::ExecToLog '"$INSTDIR\opensenses-service.exe" uninstall'
  Sleep 1500
!macroend
