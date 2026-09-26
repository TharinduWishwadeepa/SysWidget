//! Position / options in HKCU\Software\SysWidget, and the start-with-Windows task.

use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegGetValueW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE,
    REG_DWORD, REG_OPTION_NON_VOLATILE, RRF_RT_REG_DWORD,
};

use crate::wide;

const KEY: &str = r"Software\SysWidget";
const TASK_NAME: &str = "SysWidget";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub struct Settings {
    pub pos: Option<(i32, i32)>, // physical pixels
    pub topmost: bool,
}

impl Settings {
    pub fn load() -> Self {
        let pos = get_dword("Left").zip(get_dword("Top")).map(|(x, y)| (x as i32, y as i32));
        let topmost = get_dword("Topmost").map_or(true, |v| v != 0);
        Settings { pos, topmost }
    }

    pub fn save(&self) {
        if let Some((x, y)) = self.pos {
            set_dword("Left", x as u32);
            set_dword("Top", y as u32);
        }
        set_dword("Topmost", self.topmost as u32);
    }
}

fn get_dword(name: &str) -> Option<u32> {
    let mut value = 0u32;
    let mut size = 4u32;
    let result = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            wide(KEY).as_ptr(),
            wide(name).as_ptr(),
            RRF_RT_REG_DWORD,
            null_mut(),
            (&mut value as *mut u32).cast(),
            &mut size,
        )
    };
    (result == ERROR_SUCCESS).then_some(value)
}

fn set_dword(name: &str, value: u32) {
    unsafe {
        let mut key: HKEY = null_mut();
        let created = RegCreateKeyExW(
            HKEY_CURRENT_USER,
            wide(KEY).as_ptr(),
            0,
            null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            null(),
            &mut key,
            null_mut(),
        );
        if created == ERROR_SUCCESS {
            RegSetValueExW(key, wide(name).as_ptr(), 0, REG_DWORD, (&value as *const u32).cast(), 4);
            RegCloseKey(key);
        }
    }
}

// --- Start with Windows ------------------------------------------------------
// The app runs as admin, and Windows silently skips elevated apps in the Run key,
// so this is a Task Scheduler logon task with highest privileges instead.

pub fn autostart_enabled() -> bool {
    schtasks(&["/Query", "/TN", TASK_NAME])
}

pub fn disable_autostart() {
    schtasks(&["/Delete", "/F", "/TN", TASK_NAME]);
}

pub fn enable_autostart() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let exe = xml_escape(&exe.to_string_lossy());
    let user = xml_escape(&format!(
        "{}\\{}",
        std::env::var("USERDOMAIN").unwrap_or_default(),
        std::env::var("USERNAME").unwrap_or_default()
    ));

    // Battery conditions are off so it also starts on an unplugged laptop.
    let xml = format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
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
"#
    );

    // schtasks wants UTF-16 with a BOM
    let mut bytes = vec![0xFF, 0xFE];
    bytes.extend(xml.encode_utf16().flat_map(u16::to_le_bytes));
    let path = std::env::temp_dir().join("SysWidget-task.xml");
    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    let ok = schtasks(&["/Create", "/F", "/TN", TASK_NAME, "/XML", &path.to_string_lossy()]);
    let _ = std::fs::remove_file(&path);
    ok.then_some(()).ok_or_else(|| "Could not create the startup task (schtasks failed).".to_string())
}

fn schtasks(args: &[&str]) -> bool {
    Command::new("schtasks.exe")
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map_or(false, |s| s.success())
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
