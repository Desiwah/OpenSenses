//! GPU clock offsets on Linux through NVIDIA's management library
//! (libnvidia-ml.so.1, part of the NVIDIA driver), the Linux counterpart of
//! the NVAPI calls used on Windows. Needs root, which the service has.
//!
//! NVML counts memory clock offsets in transfer rate, twice the number NVAPI
//! (and so PredatorSense and MSI Afterburner) uses, so memory offsets are
//! doubled on the way in.

use anyhow::{anyhow, bail, Result};
use std::ffi::{c_char, c_int, c_uint, c_void, CStr};

type Device = *mut c_void;

#[repr(C)]
struct PciInfo {
    bus_id_legacy: [c_char; 16],
    domain: c_uint,
    bus: c_uint,
    device: c_uint,
    pci_device_id: c_uint,
    pci_sub_system_id: c_uint,
    bus_id: [c_char; 32],
}

/// nvmlClockOffset_t (driver 555 and later).
#[repr(C)]
struct ClockOffset {
    version: c_uint,
    clock_type: c_uint,
    pstate: c_uint,
    offset_mhz: c_int,
    min_offset_mhz: c_int,
    max_offset_mhz: c_int,
}

const CLOCK_GRAPHICS: c_uint = 0;
const CLOCK_MEM: c_uint = 2;
const PSTATE_0: c_uint = 0;
const CLOCK_OFFSET_V1: c_uint = std::mem::size_of::<ClockOffset>() as c_uint | (1 << 24);

pub struct Nvml {
    dev: Device,
    pci: unsafe extern "C" fn(Device, *mut PciInfo) -> c_int,
    set_offsets: Option<unsafe extern "C" fn(Device, *mut ClockOffset) -> c_int>,
    get_offsets: Option<unsafe extern "C" fn(Device, *mut ClockOffset) -> c_int>,
    set_gpc: Option<unsafe extern "C" fn(Device, c_int) -> c_int>,
    get_gpc: Option<unsafe extern "C" fn(Device, *mut c_int) -> c_int>,
    set_mem: Option<unsafe extern "C" fn(Device, c_int) -> c_int>,
    get_mem: Option<unsafe extern "C" fn(Device, *mut c_int) -> c_int>,
}

unsafe fn sym<T>(lib: *mut c_void, name: &CStr) -> Option<T> {
    let p = libc::dlsym(lib, name.as_ptr());
    (!p.is_null()).then(|| std::mem::transmute_copy(&p))
}

impl Nvml {
    pub fn open() -> Result<Self> {
        unsafe {
            let lib = libc::dlopen(c"libnvidia-ml.so.1".as_ptr(), libc::RTLD_NOW);
            if lib.is_null() {
                bail!("NVIDIA driver not found (libnvidia-ml.so.1)");
            }
            let init: unsafe extern "C" fn() -> c_int = sym(lib, c"nvmlInit_v2").ok_or_else(|| anyhow!("nvmlInit_v2 missing"))?;
            let by_index: unsafe extern "C" fn(c_uint, *mut Device) -> c_int =
                sym(lib, c"nvmlDeviceGetHandleByIndex_v2").ok_or_else(|| anyhow!("nvmlDeviceGetHandleByIndex_v2 missing"))?;
            let pci = sym(lib, c"nvmlDeviceGetPciInfo_v3").ok_or_else(|| anyhow!("nvmlDeviceGetPciInfo_v3 missing"))?;
            let st = init();
            if st != 0 {
                bail!("nvmlInit failed ({st})");
            }
            let mut dev: Device = std::ptr::null_mut();
            let st = by_index(0, &mut dev);
            if st != 0 {
                bail!("no NVIDIA GPU found ({st})");
            }
            let me = Self {
                dev,
                pci,
                set_offsets: sym(lib, c"nvmlDeviceSetClockOffsets"),
                get_offsets: sym(lib, c"nvmlDeviceGetClockOffsets"),
                set_gpc: sym(lib, c"nvmlDeviceSetGpcClkVfOffset"),
                get_gpc: sym(lib, c"nvmlDeviceGetGpcClkVfOffset"),
                set_mem: sym(lib, c"nvmlDeviceSetMemClkVfOffset"),
                get_mem: sym(lib, c"nvmlDeviceGetMemClkVfOffset"),
            };
            if me.set_offsets.is_none() && (me.set_gpc.is_none() || me.set_mem.is_none()) {
                bail!("this NVIDIA driver can't set clock offsets");
            }
            Ok(me)
        }
    }

    fn set_one(&self, clock_type: c_uint, mhz: i32) -> Result<()> {
        // The newer call first; some drivers only accept the older one.
        let mut st = -1;
        if let Some(f) = self.set_offsets {
            let mut o = ClockOffset {
                version: CLOCK_OFFSET_V1,
                clock_type,
                pstate: PSTATE_0,
                offset_mhz: mhz,
                min_offset_mhz: 0,
                max_offset_mhz: 0,
            };
            st = unsafe { f(self.dev, &mut o) };
        }
        if st != 0 {
            let old = if clock_type == CLOCK_GRAPHICS { self.set_gpc } else { self.set_mem };
            if let Some(f) = old {
                st = unsafe { f(self.dev, mhz) };
            }
        }
        if st != 0 {
            bail!("setting GPU clock offsets failed (NVML status {st})");
        }
        Ok(())
    }
}

impl Nvml {
    fn get_one(&self, clock_type: c_uint) -> Result<i32> {
        unsafe {
            if let Some(f) = self.get_offsets {
                let mut o = ClockOffset { version: CLOCK_OFFSET_V1, clock_type, pstate: PSTATE_0, offset_mhz: 0, min_offset_mhz: 0, max_offset_mhz: 0 };
                if f(self.dev, &mut o) == 0 {
                    return Ok(o.offset_mhz);
                }
            }
            let old = if clock_type == CLOCK_GRAPHICS { self.get_gpc } else { self.get_mem };
            if let Some(f) = old {
                let mut v: c_int = 0;
                if f(self.dev, &mut v) == 0 {
                    return Ok(v);
                }
            }
        }
        bail!("reading the GPU clock offset failed")
    }
}

impl super::acer::GpuClocks for Nvml {
    fn pci_ids(&self) -> Result<(u16, u32)> {
        let mut info: PciInfo = unsafe { std::mem::zeroed() };
        let st = unsafe { (self.pci)(self.dev, &mut info) };
        if st != 0 {
            bail!("reading GPU ids failed ({st})");
        }
        // Same layout as NVAPI: (device << 16) | vendor.
        Ok(((info.pci_device_id >> 16) as u16, info.pci_sub_system_id))
    }

    fn core_offset(&self) -> Result<i32> {
        self.get_one(CLOCK_GRAPHICS)
    }

    fn memory_offset(&self) -> Result<i32> {
        Ok(self.get_one(CLOCK_MEM)? / 2)
    }

    fn set_offsets(&self, core_mhz: i32, memory_mhz: i32) -> Result<()> {
        self.set_one(CLOCK_GRAPHICS, core_mhz)?;
        self.set_one(CLOCK_MEM, memory_mhz * 2)
    }
}
