//! CPU load, RAM, power source and CPU name - all straight from Windows.

use std::mem::{size_of, zeroed};
use std::ptr::null_mut;
use windows_sys::Win32::Foundation::{ERROR_SUCCESS, FILETIME};
use windows_sys::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ};
use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows_sys::Win32::System::Threading::GetSystemTimes;

use crate::wide;

#[derive(Default)]
pub struct CpuLoad {
    prev: Option<(u64, u64)>, // (idle, total)
}

impl CpuLoad {
    /// Percentage busy since the previous call. The first call returns None.
    pub fn sample(&mut self) -> Option<f32> {
        let (mut idle, mut kernel, mut user): (FILETIME, FILETIME, FILETIME) = unsafe { zeroed() };
        if unsafe { GetSystemTimes(&mut idle, &mut kernel, &mut user) } == 0 {
            return None;
        }
        let idle = ft(idle);
        let total = ft(kernel) + ft(user); // kernel time includes idle time
        let load = self.prev.and_then(|(prev_idle, prev_total)| {
            let dt = total.saturating_sub(prev_total);
            let di = idle.saturating_sub(prev_idle);
            (dt > 0).then(|| (100.0 * (1.0 - di as f64 / dt as f64)).clamp(0.0, 100.0) as f32)
        });
        self.prev = Some((idle, total));
        load
    }
}

fn ft(t: FILETIME) -> u64 {
    ((t.dwHighDateTime as u64) << 32) | t.dwLowDateTime as u64
}

#[derive(Default, Clone, Copy)]
pub struct Ram {
    pub load: f32,
    pub used_gb: f32,
    pub total_gb: f32,
}

pub fn ram() -> Ram {
    let mut status: MEMORYSTATUSEX = unsafe { zeroed() };
    status.dwLength = size_of::<MEMORYSTATUSEX>() as u32;
    if unsafe { GlobalMemoryStatusEx(&mut status) } == 0 || status.ullTotalPhys == 0 {
        return Ram::default();
    }
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;
    let total = status.ullTotalPhys as f64;
    let used = total - status.ullAvailPhys as f64;
    Ram {
        load: (used * 100.0 / total) as f32,
        used_gb: (used / GB) as f32,
        total_gb: (total / GB) as f32,
    }
}

pub fn on_battery() -> bool {
    let mut status: SYSTEM_POWER_STATUS = unsafe { zeroed() };
    unsafe { GetSystemPowerStatus(&mut status) != 0 && status.ACLineStatus == 0 }
}

/// "Intel(R) Core(TM) 5 210H" -> "Intel Core 5 210H"
pub fn cpu_name() -> String {
    let mut buf = [0u16; 128];
    let mut size = (buf.len() * 2) as u32;
    let result = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            wide(r"HARDWARE\DESCRIPTION\System\CentralProcessor\0").as_ptr(),
            wide("ProcessorNameString").as_ptr(),
            RRF_RT_REG_SZ,
            null_mut(),
            buf.as_mut_ptr().cast(),
            &mut size,
        )
    };
    if result != ERROR_SUCCESS {
        return String::new();
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    let raw = String::from_utf16_lossy(&buf[..len]);
    let raw = raw.split(" @ ").next().unwrap_or("");
    raw.replace("(R)", "")
        .replace("(TM)", "")
        .replace(" CPU", "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
