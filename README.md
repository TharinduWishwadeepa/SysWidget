# SysWidget

A small floating desktop widget for Windows 11 showing CPU load + temperature,
GPU load + temperature + VRAM, and RAM usage. Updates every 1.5 s.

## Build (once)

1. Install the .NET 8 SDK:            `winget install Microsoft.DotNet.SDK.8`
2. (Optional, for an installer) Inno Setup: `winget install JRSoftware.InnoSetup`
3. In this folder, in PowerShell:     `powershell -ExecutionPolicy Bypass -File .\build.ps1`

Output:
- `publish\SysWidget.exe` - portable, runs as-is (no .NET needed on the PC)
- `dist\SysWidget-Setup.exe` - installer (if Inno Setup is installed)

## Use

- Drag the widget anywhere; the position is remembered.
- Tray icon (right-click): Show/hide, Always on top, Start with Windows, Reset position, Exit.
- It asks for admin (UAC) because CPU temperature sensors need it. "Start with Windows"
  creates a Task Scheduler logon task, so there is no UAC prompt at login.

## CPU temperature shows "--"

Reading CPU temperature needs the PawnIO kernel driver (the signed, modern replacement
for WinRing0 that LibreHardwareMonitor uses). Install it from https://pawnio.eu, then
restart the widget. GPU temperature (NVIDIA/AMD) and RAM work without it.

## Files

- `HardwareMonitor.cs` - sensor reading (LibreHardwareMonitorLib + Windows RAM API)
- `MainWindow.xaml(.cs)` - the widget UI
- `App.xaml.cs` - tray icon and menu
- `Autostart.cs` - start-with-Windows scheduled task
- `Settings.cs` - saves position/options to `%APPDATA%\SysWidget\settings.json`
