!define COFLOW_CLI_UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\{BCC224F8-C097-430C-9071-891235F401F0}_is1"
!define COFLOW_LEGACY_TOOLS_UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\{EC88990D-CC9E-4C34-8CA5-04AA3517E5A7}_is1"

!macro RunCoflowSkills Verb
  nsExec::ExecToStack '"$INSTDIR\coflow.exe" skill ${Verb} -g'
  Pop $0
  Pop $1
  ${If} $0 != 0
    DetailPrint "Coflow skill ${Verb} failed with exit code $0: $1"
  ${EndIf}
!macroend

!macro UpdateCoflowPath Action
  nsExec::ExecToStack 'powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$INSTDIR\installer\coflow-path.ps1" -Action ${Action} -Path "$INSTDIR"'
  Pop $0
  Pop $1
  ${If} $0 != 0
    DetailPrint "Coflow PATH ${Action} failed with exit code $0: $1"
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREINSTALL
  ReadRegStr $0 HKCU "${COFLOW_LEGACY_TOOLS_UNINSTALL_KEY}" "UninstallString"
  ${If} $0 != ""
    DetailPrint "Migrating the legacy Coflow Tools installation"
    ExecWait '$0 /VERYSILENT /SUPPRESSMSGBOXES /NORESTART' $1
    ${If} $1 != 0
      MessageBox MB_ICONSTOP "The legacy Coflow Tools installation could not be removed (exit code $1)."
      Abort
    ${EndIf}
  ${EndIf}

  ReadRegStr $0 HKCU "${COFLOW_CLI_UNINSTALL_KEY}" "UninstallString"
  ${If} $0 != ""
    DetailPrint "Migrating the existing Coflow CLI-only installation"
    ExecWait '$0 /VERYSILENT /SUPPRESSMSGBOXES /NORESTART' $1
    ${If} $1 != 0
      MessageBox MB_ICONSTOP "The existing Coflow CLI installation could not be removed (exit code $1)."
      Abort
    ${EndIf}
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTINSTALL
  !insertmacro UpdateCoflowPath "Add"
  !insertmacro RunCoflowSkills "install"
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ${If} $UpdateMode <> 1
    !insertmacro RunCoflowSkills "uninstall"
    !insertmacro UpdateCoflowPath "Remove"
  ${EndIf}
!macroend
