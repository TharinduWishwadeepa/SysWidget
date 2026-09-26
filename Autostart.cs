using System;
using System.Diagnostics;
using System.IO;
using System.Security;
using System.Security.Principal;
using System.Text;

namespace SysWidget;

/// <summary>
/// The app needs admin rights, and Windows silently skips elevated apps in the Run key,
/// so "start with Windows" is a Task Scheduler logon task with highest privileges.
/// </summary>
public static class Autostart
{
    private const string TaskName = "SysWidget";

    public static bool IsEnabled() => Schtasks($"/Query /TN \"{TaskName}\"") == 0;

    public static void Disable() => Schtasks($"/Delete /F /TN \"{TaskName}\"");

    public static void Enable()
    {
        string exe = SecurityElement.Escape(Environment.ProcessPath!);
        string user = SecurityElement.Escape(WindowsIdentity.GetCurrent().Name);

        // Battery conditions are turned off so it also starts on unplugged laptops.
        string xml = $"""
            <?xml version="1.0" encoding="UTF-16"?>
            <Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
              <Triggers>
                <LogonTrigger>
                  <Enabled>true</Enabled>
                  <UserId>{user}</UserId>
                  <Delay>PT10S</Delay>
                </LogonTrigger>
              </Triggers>
              <Principals>
                <Principal id="Author">
                  <UserId>{user}</UserId>
                  <LogonType>InteractiveToken</LogonType>
                  <RunLevel>HighestAvailable</RunLevel>
                </Principal>
              </Principals>
              <Settings>
                <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
                <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
                <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
                <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
                <Priority>7</Priority>
              </Settings>
              <Actions Context="Author">
                <Exec>
                  <Command>{exe}</Command>
                </Exec>
              </Actions>
            </Task>
            """;

        string tmp = Path.Combine(Path.GetTempPath(), "SysWidget-task.xml");
        File.WriteAllText(tmp, xml, Encoding.Unicode); // schtasks expects UTF-16
        try
        {
            if (Schtasks($"/Create /F /TN \"{TaskName}\" /XML \"{tmp}\"") != 0)
                throw new InvalidOperationException("Could not create the startup task (schtasks failed).");
        }
        finally
        {
            File.Delete(tmp);
        }
    }

    private static int Schtasks(string args)
    {
        var psi = new ProcessStartInfo("schtasks.exe", args)
        {
            CreateNoWindow = true,
            UseShellExecute = false,
            RedirectStandardOutput = true,
            RedirectStandardError = true,
        };
        using var p = Process.Start(psi)!;
        p.StandardOutput.ReadToEnd();
        p.StandardError.ReadToEnd();
        p.WaitForExit();
        return p.ExitCode;
    }
}
