//! NVIDIA GPU readings via NVML (nvml.dll ships with the NVIDIA driver).
//!
//! - Each value (load, temperature, VRAM) is read separately, so one failing
//!   call doesn't hide the others.
//! - NVML is shut down while on battery so the GPU can power off (Optimus laptops).
//! - If the GPU can't be reached (ASUS Eco mode) or returns nothing (asleep),
//!   it is retried every 5 s.

use std::ffi::c_void;
use std::ptr::null_mut;
use std::time::{Duration, Instant};
use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_SYSTEM32};

use crate::wide;

const RETRY_EVERY: Duration = Duration::from_secs(5);
const NVML_SUCCESS: i32 = 0;
const NVML_TEMPERATURE_GPU: u32 = 0;
const GB: f32 = 1024.0 * 1024.0 * 1024.0;

type Device = *mut c_void;

#[repr(C)]
#[derive(Default)]
struct Utilization {
    gpu: u32,
    memory: u32,
}

#[repr(C)]
#[derive(Default)]
struct MemoryV1 {
    total: u64,
    free: u64,
    used: u64,
}

#[repr(C)]
#[derive(Default)]
struct MemoryV2 {
    version: u32,
    total: u64,
    reserved: u64,
    free: u64,
    used: u64,
}

#[repr(C)]
#[derive(Default)]
struct TemperatureV1 {
    version: u32,
    sensor_type: u32,
    temperature: i32,
}

/// NVML_STRUCT_VERSION(type, ver) = sizeof(type) | (ver << 24)
const fn struct_version<T>(ver: u32) -> u32 {
    std::mem::size_of::<T>() as u32 | (ver << 24)
}

type Fn0 = unsafe extern "C" fn() -> i32;

#[derive(Clone, Copy)]
struct Api {
    init: Fn0,
    shutdown: Fn0,
    handle_by_index: unsafe extern "C" fn(u32, *mut Device) -> i32,
    name: unsafe extern "C" fn(Device, *mut u8, u32) -> i32,
    // Optional: newer/older drivers may only export some of these.
    utilization: Option<unsafe extern "C" fn(Device, *mut Utilization) -> i32>,
    temperature: Option<unsafe extern "C" fn(Device, u32, *mut u32) -> i32>,
    temperature_v: Option<unsafe extern "C" fn(Device, *mut TemperatureV1) -> i32>,
    memory: Option<unsafe extern "C" fn(Device, *mut MemoryV1) -> i32>,
    memory_v2: Option<unsafe extern "C" fn(Device, *mut MemoryV2) -> i32>,
}

unsafe fn cast<T: Copy>(f: unsafe extern "system" fn() -> isize) -> T {
    std::mem::transmute_copy(&f)
}

impl Api {
    fn load() -> Option<Api> {
        unsafe {
            let mut lib = LoadLibraryExW(wide("nvml.dll").as_ptr(), null_mut(), LOAD_LIBRARY_SEARCH_SYSTEM32);
            if lib.is_null() {
                // Older drivers
                let pf = std::env::var("ProgramFiles").unwrap_or_else(|_| r"C:\Program Files".into());
                let path = format!(r"{pf}\NVIDIA Corporation\NVSMI\nvml.dll");
                lib = LoadLibraryExW(wide(&path).as_ptr(), null_mut(), 0);
            }
            if lib.is_null() {
                return None;
            }
            let sym = |name: &str| GetProcAddress(lib, format!("{name}\0").as_ptr());
            Some(Api {
                init: cast(sym("nvmlInit_v2")?),
                shutdown: cast(sym("nvmlShutdown")?),
                handle_by_index: cast(sym("nvmlDeviceGetHandleByIndex_v2")?),
                name: cast(sym("nvmlDeviceGetName")?),
                utilization: sym("nvmlDeviceGetUtilizationRates").map(|f| cast(f)),
                temperature: sym("nvmlDeviceGetTemperature").map(|f| cast(f)),
                temperature_v: sym("nvmlDeviceGetTemperatureV").map(|f| cast(f)),
                memory: sym("nvmlDeviceGetMemoryInfo").map(|f| cast(f)),
                memory_v2: sym("nvmlDeviceGetMemoryInfo_v2").map(|f| cast(f)),
            })
        }
    }

    unsafe fn load_pct(&self, d: Device) -> Option<f32> {
        let mut u = Utilization::default();
        (self.utilization?(d, &mut u) == NVML_SUCCESS).then_some(u.gpu as f32)
    }

    unsafe fn temp(&self, d: Device) -> Option<f32> {
        let mut t = 0u32;
        if let Some(f) = self.temperature {
            if f(d, NVML_TEMPERATURE_GPU, &mut t) == NVML_SUCCESS && t > 0 {
                return Some(t as f32);
            }
        }
        let mut tv = TemperatureV1 { version: struct_version::<TemperatureV1>(1), ..Default::default() };
        if let Some(f) = self.temperature_v {
            if f(d, &mut tv) == NVML_SUCCESS && tv.temperature > 0 {
                return Some(tv.temperature as f32);
            }
        }
        None
    }

    /// (used GB, total GB)
    unsafe fn vram(&self, d: Device) -> Option<(f32, f32)> {
        let mut m = MemoryV1::default();
        if let Some(f) = self.memory {
            if f(d, &mut m) == NVML_SUCCESS && m.total > 0 {
                return Some((m.used as f32 / GB, m.total as f32 / GB));
            }
        }
        let mut m2 = MemoryV2 { version: struct_version::<MemoryV2>(2), ..Default::default() };
        if let Some(f) = self.memory_v2 {
            if f(d, &mut m2) == NVML_SUCCESS && m2.total > 0 {
                return Some((m2.used as f32 / GB, m2.total as f32 / GB));
            }
        }
        None
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum GpuReading {
    /// Any value that couldn't be read is None and shown as "--".
    Active { load: Option<f32>, temp: Option<f32>, vram: Option<(f32, f32)> },
    /// Driver reachable but reporting nothing - GPU powered down while idle.
    Sleeping,
    PausedOnBattery,
    /// No NVIDIA driver, or GPU switched off (Eco mode).
    Unavailable,
}

pub struct Gpu {
    api: Option<Api>,
    device: Option<Device>,
    pub name: String,
    retry_at: Instant,
    waiting: GpuReading, // shown until the next retry
}

impl Gpu {
    pub fn new() -> Self {
        Gpu {
            api: Api::load(),
            device: None,
            name: String::new(),
            retry_at: Instant::now(),
            waiting: GpuReading::Unavailable,
        }
    }

    pub fn poll(&mut self, on_battery: bool) -> GpuReading {
        let Some(api) = self.api else { return GpuReading::Unavailable };

        if on_battery {
            self.stop();
            return GpuReading::PausedOnBattery;
        }

        let device = match self.device {
            Some(d) => d,
            None => {
                if Instant::now() < self.retry_at {
                    return self.waiting;
                }
                match self.start(api) {
                    Some(d) => d,
                    None => return self.retry_later(GpuReading::Unavailable),
                }
            }
        };

        let (load, temp, vram) = unsafe { (api.load_pct(device), api.temp(device), api.vram(device)) };
        if load.is_none() && temp.is_none() && vram.is_none() {
            self.stop();
            return self.retry_later(GpuReading::Sleeping);
        }
        GpuReading::Active { load, temp, vram }
    }

    fn retry_later(&mut self, state: GpuReading) -> GpuReading {
        self.waiting = state;
        self.retry_at = Instant::now() + RETRY_EVERY;
        state
    }

    fn start(&mut self, api: Api) -> Option<Device> {
        unsafe {
            if (api.init)() != NVML_SUCCESS {
                return None;
            }
            let mut device: Device = null_mut();
            if (api.handle_by_index)(0, &mut device) != NVML_SUCCESS {
                (api.shutdown)();
                return None;
            }
            let mut buf = [0u8; 96];
            if (api.name)(device, buf.as_mut_ptr(), buf.len() as u32) == NVML_SUCCESS {
                let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
                let name = String::from_utf8_lossy(&buf[..len]);
                self.name = name.trim_start_matches("NVIDIA ").to_string();
            }
            self.device = Some(device);
            Some(device)
        }
    }

    fn stop(&mut self) {
        if self.device.take().is_some() {
            if let Some(api) = self.api {
                unsafe { (api.shutdown)() };
            }
        }
    }
}

impl Drop for Gpu {
    fn drop(&mut self) {
        self.stop();
    }
}
