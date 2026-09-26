using System;
using System.Drawing;
using System.Drawing.Drawing2D;
using System.Threading;
using System.Windows;
using Forms = System.Windows.Forms;

namespace SysWidget;

public partial class App : System.Windows.Application
{
    private Mutex? _singleInstance;
    private Forms.NotifyIcon? _tray;
    private MainWindow? _window;

    protected override void OnStartup(StartupEventArgs e)
    {
        base.OnStartup(e);

        _singleInstance = new Mutex(true, @"Local\SysWidget.SingleInstance", out bool isFirst);
        if (!isFirst) { Shutdown(); return; }

        var settings = Settings.Load();
        _window = new MainWindow(settings);
        _window.Show();
        _tray = BuildTray(settings);
    }

    protected override void OnExit(ExitEventArgs e)
    {
        if (_tray != null) { _tray.Visible = false; _tray.Dispose(); }
        _singleInstance?.Dispose();
        base.OnExit(e);
    }

    private Forms.NotifyIcon BuildTray(Settings settings)
    {
        var showHide = new Forms.ToolStripMenuItem("Show / hide widget", null, (_, _) => ToggleWindow());

        var onTop = new Forms.ToolStripMenuItem("Always on top") { Checked = settings.Topmost, CheckOnClick = true };
        onTop.CheckedChanged += (_, _) =>
        {
            _window!.Topmost = onTop.Checked;
            settings.Topmost = onTop.Checked;
            settings.Save();
        };

        var startup = new Forms.ToolStripMenuItem("Start with Windows") { Checked = Autostart.IsEnabled() };
        startup.Click += (_, _) =>
        {
            try
            {
                if (startup.Checked) Autostart.Disable(); else Autostart.Enable();
                startup.Checked = Autostart.IsEnabled();
            }
            catch (Exception ex)
            {
                System.Windows.MessageBox.Show(ex.Message, "SysWidget", MessageBoxButton.OK, MessageBoxImage.Warning);
            }
        };

        var reset = new Forms.ToolStripMenuItem("Reset position", null, (_, _) => _window!.ResetPosition());
        var exit = new Forms.ToolStripMenuItem("Exit", null, (_, _) => Shutdown());

        var menu = new Forms.ContextMenuStrip();
        menu.Items.AddRange(new Forms.ToolStripItem[] { showHide, onTop, startup, reset, new Forms.ToolStripSeparator(), exit });

        var tray = new Forms.NotifyIcon
        {
            Icon = MakeTrayIcon(),
            Text = "SysWidget",
            ContextMenuStrip = menu,
            Visible = true,
        };
        tray.DoubleClick += (_, _) => ToggleWindow();
        return tray;
    }

    private void ToggleWindow()
    {
        if (_window!.IsVisible) _window.Hide();
        else { _window.Show(); _window.Activate(); }
    }

    // Small bar-chart icon drawn at runtime so no .ico file is needed.
    private static Icon MakeTrayIcon()
    {
        using var bmp = new Bitmap(32, 32);
        using (var g = Graphics.FromImage(bmp))
        {
            g.SmoothingMode = SmoothingMode.AntiAlias;
            g.Clear(Color.Transparent);
            using var bg = new SolidBrush(Color.FromArgb(24, 26, 31));
            g.FillRectangle(bg, 1, 1, 30, 30);
            using var b1 = new SolidBrush(Color.FromArgb(76, 194, 255));
            using var b2 = new SolidBrush(Color.FromArgb(126, 231, 135));
            using var b3 = new SolidBrush(Color.FromArgb(210, 168, 255));
            g.FillRectangle(b1, 5, 14, 6, 13);
            g.FillRectangle(b2, 13, 6, 6, 21);
            g.FillRectangle(b3, 21, 10, 6, 17);
        }
        return Icon.FromHandle(bmp.GetHicon());
    }
}
