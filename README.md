# SysWidget (Rust)

Tiny desktop widget for Windows 11: CPU load + temperature, NVIDIA GPU load +
temperature + VRAM, and RAM usage. Single ~350 KB exe, no runtime needed.

Built for an Intel CPU + NVIDIA GPU laptop (ASUS TUF F16).

## Build (GitHub Actions)

1. Push this folder's contents to a GitHub repo (Cargo.toml at the repo root).
2. Actions -> Build SysWidget runs on every push (or use "Run workflow").
3. Open the finished run and download the **SysWidget** artifact (a zip with `SysWidget.exe`).

## Install

1. Exit and uninstall the old .NET SysWidget first (Settings -> Apps).
2. Put `SysWidget.exe` somewhere permanent, e.g. `C:\Program Files\SysWidget\`.
3. Run it (it asks for admin - needed for the CPU temperature driver).
4. Right-click the widget or tray icon -> **Start with Windows**.
   If you move the exe later, turn this off and on again.

Windows SmartScreen may warn because the exe isn't signed: More info -> Run anyway.

## Behaviour

- Drag anywhere; position is remembered (`HKCU\Software\SysWidget`).
- Right-click the widget or the tray icon: show/hide, always on top, start with
  Windows, reset position, exit. Double-click the tray icon to show/hide.
- **On battery** GPU readings pause and NVIDIA's library is shut down, so the
  RTX GPU can power off.
- **Eco mode** (Armoury Crate) turns the GPU off; the widget shows
  "Not available" and checks again every 5 s.
- When the GPU is idle and powered down it shows "Sleeping (idle)".
- GPU load, temperature and VRAM are read separately; any value the driver
  won't report shows as "--" while the others still update.
- Nothing is read while the widget is hidden.

## Requirements on the PC

- PawnIO driver (https://pawnio.eu) for CPU temperature.
- NVIDIA driver (provides `nvml.dll`).

## Files

- `src/main.rs` - window, drawing, tray and menu
- `src/sensors.rs` - CPU load, RAM, battery, CPU name (Windows APIs)
- `src/pawnio.rs` - Intel CPU package temperature via PawnIO
- `src/nvml.rs` - NVIDIA GPU via NVML
- `src/settings.rs` - saved settings and the start-with-Windows task
- `syswidget.rc`, `assets/` - app icon, manifest (run as admin, DPI, heap) and version info
- `pawnio/IntelMSR.bin` - signed PawnIO module (LGPL-2.1, see `pawnio/COPYING`)
