!ifndef VERSION
  !define VERSION "0.1.0"
!endif
!ifndef EXE_PATH
  !define EXE_PATH "..\..\target\x86_64-pc-windows-gnu\release\ha-desktop-agent.exe"
!endif

Name "ha-desktop-agent"
OutFile "..\..\dist\ha-desktop-agent-setup.exe"
InstallDir "$PROGRAMFILES64\ha-desktop-agent"
RequestExecutionLevel admin
Unicode true
SetCompressor lzma

Page directory
Page instfiles
UninstPage uninstConfirm
UninstPage instfiles

Section "Install"
  SetOutPath "$INSTDIR"
  File "/oname=ha-desktop-agent.exe" "${EXE_PATH}"
  File "/oname=config.example.yaml" "..\..\config.example.yaml"
  WriteUninstaller "$INSTDIR\uninstall.exe"

  CreateDirectory "$PROGRAMDATA\ha-desktop-agent"
  IfFileExists "$PROGRAMDATA\ha-desktop-agent\config.yaml" skip_config
    CopyFiles /SILENT "$INSTDIR\config.example.yaml" "$PROGRAMDATA\ha-desktop-agent\config.yaml"
  skip_config:

  ExecWait 'sc.exe create ha-desktop-agent binPath= "$INSTDIR\ha-desktop-agent.exe service" start= auto DisplayName= "Home Assistant desktop agent"'
  ExecWait 'sc.exe description ha-desktop-agent "MQTT desktop agent for Home Assistant"'
  ExecWait 'sc.exe start ha-desktop-agent'
  ExecWait 'schtasks.exe /Create /TN ha-desktop-agent-session /TR "$INSTDIR\ha-desktop-agent.exe session" /SC ONLOGON /RL LIMITED /F'
SectionEnd

Section "Uninstall"
  ExecWait 'sc.exe stop ha-desktop-agent'
  ExecWait 'sc.exe delete ha-desktop-agent'
  ExecWait 'schtasks.exe /Delete /TN ha-desktop-agent-session /F'
  Delete "$INSTDIR\ha-desktop-agent.exe"
  Delete "$INSTDIR\config.example.yaml"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"
SectionEnd
