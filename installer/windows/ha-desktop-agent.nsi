!ifndef VERSION
  !error "VERSION must be passed: makensis -DVERSION=X.Y.Z (Makefile reads Cargo.toml)"
!endif
!ifndef EXE_PATH
  !error "EXE_PATH must be passed to the Windows release exe"
!endif

Name "ha-desktop-agent"
OutFile "..\..\dist\ha-desktop-agent-setup.exe"
InstallDir "$PROGRAMFILES64\ha-desktop-agent"
RequestExecutionLevel admin
Unicode true
SetCompressor lzma

; Interactive UI when launched from Explorer; /S skips pages.
Page directory
Page instfiles
UninstPage uninstConfirm
UninstPage instfiles

!macro StopAndRemoveService
  nsExec::ExecToLog 'sc.exe stop ha-desktop-agent'
  Pop $0
  nsExec::ExecToLog 'sc.exe delete ha-desktop-agent'
  Pop $0
  ; Give SCM a moment before recreate (silent installs over remote shells).
  Sleep 1000
!macroend

Section "Install"
  SetOutPath "$INSTDIR"
  File "/oname=ha-desktop-agent.exe" "${EXE_PATH}"
  File "/oname=config.example.yaml" "..\..\config.example.yaml"
  File "/oname=install.ps1" "install.ps1"
  WriteUninstaller "$INSTDIR\uninstall.exe"

  ReadEnvStr $0 PROGRAMDATA
  StrCmp $0 "" 0 +2
    StrCpy $0 "C:\ProgramData"
  CreateDirectory "$0\ha-desktop-agent"
  IfFileExists "$0\ha-desktop-agent\config.yaml" skip_config
    CopyFiles /SILENT "$INSTDIR\config.example.yaml" "$0\ha-desktop-agent\config.yaml"
  skip_config:

  !insertmacro StopAndRemoveService
  nsExec::ExecToLog 'sc.exe create ha-desktop-agent binPath= "$INSTDIR\ha-desktop-agent.exe service" start= auto DisplayName= "Home Assistant desktop agent"'
  Pop $0
  nsExec::ExecToLog 'sc.exe description ha-desktop-agent "MQTT desktop agent for Home Assistant"'
  Pop $0
  nsExec::ExecToLog 'sc.exe start ha-desktop-agent'
  Pop $0

  nsExec::ExecToLog 'schtasks.exe /Create /TN ha-desktop-agent-session /TR "\"$INSTDIR\ha-desktop-agent.exe\" session" /SC ONLOGON /RL LIMITED /F'
  Pop $0
  ; Start the session helper now (ONLOGON alone leaves session entities unavailable until next logon).
  nsExec::ExecToLog 'schtasks.exe /Run /TN ha-desktop-agent-session'
  Pop $0
SectionEnd

Section "Uninstall"
  nsExec::ExecToLog 'sc.exe stop ha-desktop-agent'
  Pop $0
  nsExec::ExecToLog 'sc.exe delete ha-desktop-agent'
  Pop $0
  nsExec::ExecToLog 'schtasks.exe /Delete /TN ha-desktop-agent-session /F'
  Pop $0
  Delete "$INSTDIR\ha-desktop-agent.exe"
  Delete "$INSTDIR\config.example.yaml"
  Delete "$INSTDIR\install.ps1"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"
SectionEnd
