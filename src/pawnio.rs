//! Intel CPU package temperature through the PawnIO driver.
//!
//! Talks to the driver directly (same protocol LibreHardwareMonitor uses) and loads
//! PawnIO's signed IntelMSR module, which only allows reading safe, whitelisted MSRs.

use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::{CloseHandle, GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows_sys::Win32::System::IO::DeviceIoControl;

use crate::wide;

/// Signed module from https://github.com/namazso/PawnIO.Modules (LGPL-2.1, see pawnio/COPYING).
static INTEL_MSR_MODULE: &[u8] = include_bytes!("../pawnio/IntelMSR.bin");

const DEVICE_TYPE: u32 = 41394 << 16;
const IOCTL_LOAD_BINARY: u32 = DEVICE_TYPE | (0x821 << 2);
const IOCTL_EXECUTE_FN: u32 = DEVICE_TYPE | (0x841 << 2);
const FN_NAME_LEN: usize = 32;

const MSR_TEMPERATURE_TARGET: u32 = 0x1A2; // TjMax in bits 23:16
const MSR_PACKAGE_THERM_STATUS: u32 = 0x1B1; // degrees below TjMax in bits 22:16

pub struct IntelTemp {
    handle: HANDLE,
    tjmax: f32,
}

impl IntelTemp {
    /// None if PawnIO isn't installed, we're not admin, or the CPU isn't Intel.
    pub fn open() -> Option<Self> {
        let handle = unsafe {
            CreateFileW(
                wide(r"\\?\GLOBALROOT\Device\PawnIO").as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                null(),
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return None;
        }
        let mut returned = 0u32;
        let loaded = unsafe {
            DeviceIoControl(
                handle,
                IOCTL_LOAD_BINARY,
                INTEL_MSR_MODULE.as_ptr().cast(),
                INTEL_MSR_MODULE.len() as u32,
                null_mut(),
                0,
                &mut returned,
                null_mut(),
            )
        };
        let mut this = IntelTemp { handle, tjmax: 100.0 }; // Drop closes the handle on failure
        if loaded == 0 {
            return None;
        }
        if let Some(v) = this.read_msr(MSR_TEMPERATURE_TARGET) {
            let tjmax = ((v >> 16) & 0xFF) as f32;
            if tjmax > 0.0 {
                this.tjmax = tjmax;
            }
        }
        this.package_temp()?; // make sure reading actually works
        Some(this)
    }

    pub fn package_temp(&self) -> Option<f32> {
        let v = self.read_msr(MSR_PACKAGE_THERM_STATUS)?;
        let below_tjmax = ((v >> 16) & 0x7F) as f32;
        Some(self.tjmax - below_tjmax)
    }

    fn read_msr(&self, index: u32) -> Option<u64> {
        let fn_name = b"ioctl_read_msr";
        let mut input = [0u8; FN_NAME_LEN + 8];
        input[..fn_name.len()].copy_from_slice(fn_name);
        input[FN_NAME_LEN..].copy_from_slice(&(index as u64).to_le_bytes());
        let mut output = [0u8; 8];
        let mut returned = 0u32;
        let ok = unsafe {
            DeviceIoControl(
                self.handle,
                IOCTL_EXECUTE_FN,
                input.as_ptr().cast(),
                input.len() as u32,
                output.as_mut_ptr().cast(),
                output.len() as u32,
                &mut returned,
                null_mut(),
            )
        };
        (ok != 0 && returned >= 8).then(|| u64::from_le_bytes(output))
    }
}

impl Drop for IntelTemp {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.handle) };
    }
}
