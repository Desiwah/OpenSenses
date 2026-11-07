; Stop the running service first so its exe can be replaced on upgrade.
!macro NSIS_HOOK_PREINSTALL
  nsExec::ExecToLog 'sc.exe stop OpenSenses'
  Sleep 1500
!macroend

!macro NSIS_HOOK_POSTINSTALL
  nsExec::ExecToLog '"$INSTDIR\opensenses-service.exe" install'
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  nsExec::ExecToLog '"$INSTDIR\opensenses-service.exe" uninstall'
  Sleep 1500
!macroend
