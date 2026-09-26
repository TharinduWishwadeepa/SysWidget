using System;
using System.Threading;
using System.Threading.Tasks;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;
using System.Windows.Media;

namespace SysWidget;

public partial class MainWindow : Window
{
    private static readonly TimeSpan Interval = TimeSpan.FromSeconds(1.5);

    private static readonly Brush Cool  = Frozen(0x7E, 0xE7, 0x87); // < 65 °C
    private static readonly Brush Warm  = Frozen(0xE3, 0xB3, 0x41); // 65-79 °C
    private static readonly Brush Hot   = Frozen(0xFF, 0x7B, 0x72); // >= 80 °C
    private static readonly Brush Muted = Frozen(0x6E, 0x76, 0x81);

    private readonly Settings _settings;
    private readonly CancellationTokenSource _cts = new();

    public MainWindow(Settings settings)
    {
        InitializeComponent();
        _settings = settings;
        Topmost = settings.Topmost;

        bool restored = settings.Left is double l && settings.Top is double t && IsOnScreen(l, t);
        if (restored) { Left = settings.Left!.Value; Top = settings.Top!.Value; }

        Loaded += (_, _) =>
        {
            if (!restored) ResetPosition();
            _ = PollAsync(_cts.Token);
        };
        Closed += (_, _) => _cts.Cancel();
    }

    public void ResetPosition()
    {
        var area = SystemParameters.WorkArea;
        Left = area.Right - ActualWidth - 12;
        Top = area.Top + 12;
        SavePosition();
    }

    private async Task PollAsync(CancellationToken ct)
    {
        try
        {
            using var monitor = await Task.Run(() => new HardwareMonitor(), ct);
            using var timer = new PeriodicTimer(Interval);
            do
            {
                var snapshot = await Task.Run(() => monitor.Read(), ct);
                Render(snapshot); // back on the UI thread after await
            }
            while (await timer.WaitForNextTickAsync(ct));
        }
        catch (OperationCanceledException) { }
        catch (Exception ex)
        {
            Hint.Text = "Sensor error: " + ex.Message;
            Hint.Visibility = Visibility.Visible;
        }
    }

    private void Render(Snapshot s)
    {
        CpuName.Text = s.CpuName ?? "";
        CpuLoad.Text = Percent(s.CpuLoad);
        SetTemp(CpuTemp, s.CpuTemp);
        SetBar(CpuBar, s.CpuLoad);

        GpuSection.Visibility = s.GpuName is null ? Visibility.Collapsed : Visibility.Visible;
        GpuName.Text = s.GpuName ?? "";
        GpuLoad.Text = Percent(s.GpuLoad);
        SetTemp(GpuTemp, s.GpuTemp);
        SetBar(GpuBar, s.GpuLoad);
        GpuDetail.Text = (s.VramUsedMb, s.VramTotalMb) switch
        {
            (float used, float total) when total > 0 => $"VRAM  {used / 1024:0.0} / {total / 1024:0.0} GB",
            (float used, _) => $"VRAM  {used / 1024:0.0} GB used",
            _ => "",
        };

        RamLoad.Text = Percent(s.RamLoad);
        RamDetail.Text = $"{s.RamUsedGb:0.0} / {s.RamTotalGb:0.0} GB";
        SetBar(RamBar, s.RamLoad);

        if (s.CpuTemp is null)
        {
            Hint.Text = "CPU temperature unavailable - install the PawnIO driver (see README).";
            Hint.Visibility = Visibility.Visible;
        }
        else
        {
            Hint.Visibility = Visibility.Collapsed;
        }
    }

    private static string Percent(float? v) => v is float f ? $"{f:0}%" : "--";

    private static void SetTemp(TextBlock target, float? celsius)
    {
        if (celsius is float c && c > 0)
        {
            target.Text = $"{c:0}°C";
            target.Foreground = c < 65 ? Cool : c < 80 ? Warm : Hot;
        }
        else
        {
            target.Text = "--°C";
            target.Foreground = Muted;
        }
    }

    private static void SetBar(Border bar, float? percent)
    {
        double track = ((FrameworkElement)bar.Parent).ActualWidth;
        bar.Width = track * Math.Clamp(percent ?? 0, 0, 100) / 100;
    }

    private void OnDrag(object sender, MouseButtonEventArgs e)
    {
        if (e.ButtonState != MouseButtonState.Pressed) return;
        DragMove(); // blocks until the mouse is released
        SavePosition();
    }

    private void SavePosition()
    {
        _settings.Left = Left;
        _settings.Top = Top;
        _settings.Save();
    }

    private static bool IsOnScreen(double left, double top) =>
        left >= SystemParameters.VirtualScreenLeft - 50 &&
        top >= SystemParameters.VirtualScreenTop - 50 &&
        left < SystemParameters.VirtualScreenLeft + SystemParameters.VirtualScreenWidth - 50 &&
        top < SystemParameters.VirtualScreenTop + SystemParameters.VirtualScreenHeight - 50;

    private static Brush Frozen(byte r, byte g, byte b)
    {
        var brush = new SolidColorBrush(Color.FromRgb(r, g, b));
        brush.Freeze();
        return brush;
    }
}
