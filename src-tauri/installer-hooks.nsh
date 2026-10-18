!macro NSIS_HOOK_POSTUNINSTALL
  ; Remove the optional per-user auto-clean task. Ignore "task not found".
  nsExec::Exec 'schtasks.exe /Delete /TN "CC DiskSage Auto Clean" /F'
  Pop $0
!macroend
