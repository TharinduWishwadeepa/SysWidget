using System;
using System.Collections.Generic;
using System.Linq;
using System.Runtime.InteropServices;
using LibreHardwareMonitor.Hardware;

namespace SysWidget;

public sealed record Snapshot(
    string? CpuName, float? CpuLoad, float? CpuTemp,
    string? GpuName, float? GpuLoad, float? GpuTemp, float? VramUsedMb, float? VramTotalMb,
    float RamLoad, float RamUsedGb, float RamTotalGb);

public sealed class HardwareMonitor : IDisposable
{
    private readonly Computer _computer;

    public HardwareMonitor()
    {
        _computer = new Computer { IsCpuEnabled = true, IsGpuEnabled = true };
        _computer.Open(); // slow (1-3 s) - call off the UI thread
    }

    public Snapshot Read()
    {
        foreach (var hw in _computer.Hardware)
        {
            hw.Update();
            foreach (var sub in hw.SubHardware) sub.Update();
        }

        var cpu = _computer.Hardware.FirstOrDefault(h => h.HardwareType == HardwareType.Cpu);
        var gpu = PickGpu();
        var (ramLoad, ramUsed, ramTotal) = ReadRam();

        return new Snapshot(
            CpuName: cpu?.Name,
            CpuLoad: Find(cpu, SensorType.Load, "CPU Total"),
            // Intel: "CPU Package"; AMD: "Core (Tctl/Tdie)"
            CpuTemp: Temp(cpu, "CPU Package", "Core (Tctl/Tdie)", "Core (Tctl)", "Core (Tdie)", "Core Max", "Core Average"),
            GpuName: gpu?.Name,
            // NVIDIA/AMD: "GPU Core"; Intel iGPU: "D3D 3D"
            GpuLoad: Find(gpu, SensorType.Load, "GPU Core", "D3D 3D") ?? Max(gpu, SensorType.Load),
            GpuTemp: Temp(gpu, "GPU Core", "GPU Hot Spot"),
            VramUsedMb: Find(gpu, SensorType.SmallData, "GPU Memory Used", "D3D Dedicated Memory Used"),
            VramTotalMb: Find(gpu, SensorType.SmallData, "GPU Memory Total"),
            RamLoad: ramLoad, RamUsedGb: ramUsed, RamTotalGb: ramTotal);
    }

    public void Dispose() => _computer.Close();

    // Prefer a discrete GPU over the integrated one.
    private IHardware? PickGpu()
    {
        var gpus = _computer.Hardware
            .Where(h => h.HardwareType is HardwareType.GpuNvidia or HardwareType.GpuAmd or HardwareType.GpuIntel)
            .ToList();
        return gpus.FirstOrDefault(g => g.HardwareType == HardwareType.GpuNvidia)
            ?? gpus.FirstOrDefault(g => g.HardwareType == HardwareType.GpuAmd)
            ?? gpus.FirstOrDefault();
    }

    private static IEnumerable<ISensor> AllSensors(IHardware hw) =>
        hw.Sensors.Concat(hw.SubHardware.SelectMany(s => s.Sensors));

    private static float? Find(IHardware? hw, SensorType type, params string[] names)
    {
        if (hw is null) return null;
        var sensors = AllSensors(hw).Where(s => s.SensorType == type && s.Value.HasValue).ToList();
        foreach (var name in names)
        {
            var match = sensors.FirstOrDefault(s => s.Name == name);
            if (match != null) return match.Value;
        }
        return null;
    }

    private static float? Max(IHardware? hw, SensorType type) =>
        hw is null ? null : AllSensors(hw).Where(s => s.SensorType == type && s.Value > 0).Max(s => s.Value);

    // A temperature of 0 means "not readable" (e.g. driver missing), so treat it as absent.
    private static float? Temp(IHardware? hw, params string[] names)
    {
        var t = Find(hw, SensorType.Temperature, names);
        return t > 0 ? t : Max(hw, SensorType.Temperature);
    }

    // RAM comes straight from Windows - simpler and always available.
    private static (float load, float usedGb, float totalGb) ReadRam()
    {
        var status = new MEMORYSTATUSEX { dwLength = (uint)Marshal.SizeOf<MEMORYSTATUSEX>() };
        if (!GlobalMemoryStatusEx(ref status) || status.ullTotalPhys == 0) return (0, 0, 0);
        const double GB = 1024.0 * 1024 * 1024;
        double used = status.ullTotalPhys - status.ullAvailPhys;
        return ((float)(used * 100 / status.ullTotalPhys), (float)(used / GB), (float)(status.ullTotalPhys / GB));
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct MEMORYSTATUSEX
    {
        public uint dwLength;
        public uint dwMemoryLoad;
        public ulong ullTotalPhys;
        public ulong ullAvailPhys;
        public ulong ullTotalPageFile;
        public ulong ullAvailPageFile;
        public ulong ullTotalVirtual;
        public ulong ullAvailVirtual;
        public ulong ullAvailExtendedVirtual;
    }

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool GlobalMemoryStatusEx(ref MEMORYSTATUSEX lpBuffer);
}
