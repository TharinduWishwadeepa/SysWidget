//! NVIDIA GPU readings via NVML (nvml.dll ships with the NVIDIA driver).
//!
//! NVML is shut down while on battery so the GPU can power off (Optimus laptops),
//! and init is retried every 30 s while the GPU is unavailable (e.g. ASUS Eco mode).

use std::ffi::c_void;
use std::ptr::null_mut;
use std::time::{Duration, Instant};
use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_SYSTEM32};

use crate::wide;

const RETRY_EVERY: Duration = Duration::from_secs(30);
const NVML_SUCCESS: i32 = 0;
const NVML_TEMPERATURE_GPU: u32 = 0;

type Device = *mut c_void;

#[repr(C)]
#[derive(Default)]
struct Utilization {
    gpu: u32,
    memory: u32,
}

#[repr(C)]
#[derive(Default)]
struct Memory {
    total: u64,
    free: u64,
    used: u64,
}

#[derive(Clone, Copy)]
struct Api {
    init: unsafe extern "C" fn() -> i32,
    shutdown: unsafe extern "C" fn() -> i32,
    handle_by_index: unsafe extern "C" fn(u32, *mut Device) -> i32,
    name: unsafe extern "C" fn(Device, *mut u8, u32) -> i32,
    utilization: unsafe extern "C" fn(Device, *mut Utilization) -> i32,
    temperature: unsafe extern "C" fn(Device, u32, *mut u32) -> i32,
    memory: unsafe extern "C" fn(Device, *mut Memory) -> i32,
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
            macro_rules! sym {
                ($name:literal) => {
                    std::mem::transmute(GetProcAddress(lib, concat!($name, "\0").as_ptr())?)
                };
            }
            Some(Api {
                init: sym!("nvmlInit_v2"),
                shutdown: sym!("nvmlShutdown"),
                handle_by_index: sym!("nvmlDeviceGetHandleByIndex_v2"),
                name: sym!("nvmlDeviceGetName"),
                utilization: sym!("nvmlDeviceGetUtilizationRates"),
                temperature: sym!("nvmlDeviceGetTemperature"),
                memory: sym!("nvmlDeviceGetMemoryInfo"),
            })
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum GpuReading {
    Active { load: f32, temp: f32, vram_used_gb: f32, vram_total_gb: f32 },
    PausedOnBattery,
    Unavailable,
}

pub struct Gpu {
    api: Option<Api>,
    device: Option<Device>,
    pub name: String,
    retry_at: Instant,
}

impl Gpu {
    pub fn new() -> Self {
        Gpu { api: Api::load(), device: None, name: String::new(), retry_at: Instant::now() }
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
                    return GpuReading::Unavailable;
                }
                match self.start(api) {
                    Some(d) => d,
                    None => {
                        self.retry_at = Instant::now() + RETRY_EVERY;
                        return GpuReading::Unavailable;
                    }
                }
            }
        };

        let (mut util, mut temp, mut mem) = (Utilization::default(), 0u32, Memory::default());
        let ok = unsafe {
            (api.utilization)(device, &mut util) == NVML_SUCCESS
                && (api.temperature)(device, NVML_TEMPERATURE_GPU, &mut temp) == NVML_SUCCESS
                && (api.memory)(device, &mut mem) == NVML_SUCCESS
        };
        if !ok {
            // GPU switched off (Eco mode) or driver reset - try again later.
            self.stop();
            self.retry_at = Instant::now() + RETRY_EVERY;
            return GpuReading::Unavailable;
        }

        const GB: f32 = 1024.0 * 1024.0 * 1024.0;
        GpuReading::Active {
            load: util.gpu as f32,
            temp: temp as f32,
            vram_used_gb: mem.used as f32 / GB,
            vram_total_gb: mem.total as f32 / GB,
        }
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
