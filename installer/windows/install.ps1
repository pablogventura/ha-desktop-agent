#Requires -RunAsAdministrator
<#
.SYNOPSIS
  Install ha-desktop-agent the same way as the NSIS setup (service + logon task).

.DESCRIPTION
  Prefer this over ha-desktop-agent-setup.exe /S when installing over OpenSSH or
  another non-interactive remote shell. setup.exe requests elevation (UAC); without
  an interactive desktop the consent prompt never appears and the process hangs.

  Run from an already-elevated PowerShell (Administrator), or schedule this script
  once as SYSTEM, for example:

    schtasks /Create /TN ha-desktop-agent-install /RU SYSTEM /RL HIGHEST /SC ONCE /ST 00:00 /TR "powershell.exe -NoProfile -ExecutionPolicy Bypass -File C:\Temp\install.ps1 -SourceDir C:\Temp" /F
    schtasks /Run /TN ha-desktop-agent-install

.PARAMETER SourceDir
  Directory that contains ha-desktop-agent.exe and optionally config.example.yaml.
  Defaults to the folder of this script.

.PARAMETER SkipSessionRun
  Create the logon task but do not start it immediately.
#>
[CmdletBinding()]
param(
    [string]$SourceDir = $PSScriptRoot,
    [switch]$SkipSessionRun
)

$ErrorActionPreference = "Stop"

$installDir = Join-Path ${env:ProgramFiles} "ha-desktop-agent"
$programData = if ($env:PROGRAMDATA) { $env:PROGRAMDATA } else { "C:\ProgramData" }
$configDir = Join-Path $programData "ha-desktop-agent"
$exeSrc = Join-Path $SourceDir "ha-desktop-agent.exe"
$exampleSrc = Join-Path $SourceDir "config.example.yaml"

if (-not (Test-Path -LiteralPath $exeSrc)) {
    throw "Missing $exeSrc"
}

New-Item -ItemType Directory -Force -Path $installDir | Out-Null
New-Item -ItemType Directory -Force -Path $configDir | Out-Null

Copy-Item -LiteralPath $exeSrc -Destination (Join-Path $installDir "ha-desktop-agent.exe") -Force
if (Test-Path -LiteralPath $exampleSrc) {
    Copy-Item -LiteralPath $exampleSrc -Destination (Join-Path $installDir "config.example.yaml") -Force
    $configPath = Join-Path $configDir "config.yaml"
    if (-not (Test-Path -LiteralPath $configPath)) {
        Copy-Item -LiteralPath $exampleSrc -Destination $configPath
    }
}

$exe = Join-Path $installDir "ha-desktop-agent.exe"
$binPath = "`"$exe`" service"

& sc.exe stop ha-desktop-agent 2>$null | Out-Null
Start-Sleep -Seconds 1
& sc.exe delete ha-desktop-agent 2>$null | Out-Null
Start-Sleep -Seconds 1
& sc.exe create ha-desktop-agent binPath= $binPath start= auto DisplayName= "Home Assistant desktop agent"
if ($LASTEXITCODE -ne 0) {
    throw "sc create failed with exit $LASTEXITCODE"
}
& sc.exe description ha-desktop-agent "MQTT desktop agent for Home Assistant" | Out-Null
& sc.exe start ha-desktop-agent
if ($LASTEXITCODE -ne 0) {
    throw "sc start failed with exit $LASTEXITCODE"
}

$tr = "`"$exe`" session"
& schtasks.exe /Create /TN ha-desktop-agent-session /TR $tr /SC ONLOGON /RL LIMITED /F
if ($LASTEXITCODE -ne 0) {
    throw "schtasks create failed with exit $LASTEXITCODE"
}
if (-not $SkipSessionRun) {
    & schtasks.exe /Run /TN ha-desktop-agent-session
}

Write-Host "Installed to $installDir"
Write-Host "Config: $(Join-Path $configDir 'config.yaml')"
Write-Host "Validate: & `"$exe`" validate"
