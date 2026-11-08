//! GPU clock offsets through NVIDIA's driver API (nvapi64.dll), the way
//! PredatorSense applies its Faster/Turbo levels. Only the public
//! "pstates20" calls are used: read the P0 clock deltas, and set new ones.
//!
//! The structure is built by hand as bytes so its layout is explicit:
//! NV_GPU_PERF_PSTATES20_INFO_V2 (7416 bytes, version 0x21CF8).

use anyhow::{anyhow, bail, Result};
use std::ffi::c_void;
use windows::core::{s, w};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

const ID_INITIALIZE: u32 = 0x0150_E828;
const ID_ENUM_PHYSICAL_GPUS: u32 = 0xE5AC_921F;
const ID_GET_PSTATES20: u32 = 0x6FF8_1213;
const ID_SET_PSTATES20: u32 = 0x0F4D_AE6B;
const ID_GET_PCI_IDENTIFIERS: u32 = 0x2DDF_B66E;

const SIZE: usize = 7416;
const VERSION: u32 = SIZE as u32 | (2 << 16);
const PSTATE_SIZE: usize = 456; // id, flags, 8 clocks x 44, 4 voltages x 24
const CLOCK_SIZE: usize = 44;
const PSTATES_AT: usize = 20;

pub const CLOCK_GRAPHICS: u32 = 0;
pub const CLOCK_MEMORY: u32 = 4;

type QueryInterface = unsafe extern "C" fn(u32) -> *const c_void;
type Initialize = unsafe extern "C" fn() -> i32;
type EnumGpus = unsafe extern "C" fn(*mut *mut c_void, *mut u32) -> i32;
type Pstates20 = unsafe extern "C" fn(*mut c_void, *mut u8) -> i32;
type PciIds = unsafe extern "C" fn(*mut c_void, *mut u32, *mut u32, *mut u32, *mut u32) -> i32;

pub struct Nvapi {
    gpu: *mut c_void,
    get: Pstates20,
    set: Pstates20,
    pci: Option<PciIds>,
}

fn put(buf: &mut [u8], at: usize, v: u32) {
    buf[at..at + 4].copy_from_slice(&v.to_le_bytes());
}

fn get(buf: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(buf[at..at + 4].try_into().unwrap())
}

impl Nvapi {
    pub fn open() -> Result<Self> {
        unsafe {
            let lib = LoadLibraryW(w!("nvapi64.dll")).map_err(|e| anyhow!("NVIDIA driver not found: {e}"))?;
            let qi = GetProcAddress(lib, s!("nvapi_QueryInterface")).ok_or_else(|| anyhow!("nvapi_QueryInterface missing"))?;
            let qi: QueryInterface = std::mem::transmute(qi);
            let f = |id: u32| {
                let p = qi(id);
                if p.is_null() {
                    Err(anyhow!("NVAPI function 0x{id:08X} missing"))
                } else {
                    Ok(p)
                }
            };
            let init: Initialize = std::mem::transmute(f(ID_INITIALIZE)?);
            let enumerate: EnumGpus = std::mem::transmute(f(ID_ENUM_PHYSICAL_GPUS)?);
            let get: Pstates20 = std::mem::transmute(f(ID_GET_PSTATES20)?);
            let set: Pstates20 = std::mem::transmute(f(ID_SET_PSTATES20)?);
            let pci: Option<PciIds> = f(ID_GET_PCI_IDENTIFIERS).ok().map(|p| std::mem::transmute(p));
            let st = init();
            if st != 0 {
                bail!("NvAPI_Initialize failed ({st})");
            }
            let mut gpus = [std::ptr::null_mut::<c_void>(); 64];
            let mut n = 0u32;
            let st = enumerate(gpus.as_mut_ptr(), &mut n);
            if st != 0 || n == 0 {
                bail!("no NVIDIA GPU found ({st})");
            }
            Ok(Self { gpu: gpus[0], get, set, pci })
        }
    }

    /// PCI device id (e.g. 0x1F10) and subsystem id (e.g. 0x13421025),
    /// the key PredatorSense used for its per-GPU overclock numbers.
    pub fn pci_ids(&self) -> Result<(u16, u32)> {
        let f = self.pci.ok_or_else(|| anyhow!("NvAPI_GPU_GetPCIIdentifiers missing"))?;
        let (mut dev, mut sub, mut rev, mut ext) = (0u32, 0u32, 0u32, 0u32);
        let st = unsafe { f(self.gpu, &mut dev, &mut sub, &mut rev, &mut ext) };
        if st != 0 {
            bail!("reading GPU ids failed ({st})");
        }
        // NVAPI reports the device id as (device << 16) | vendor.
        Ok(((dev >> 16) as u16, sub))
    }

    /// Current P0 offset of one clock domain, in MHz.
    pub fn offset(&self, domain: u32) -> Result<i32> {
        let mut buf = vec![0u8; SIZE];
        put(&mut buf, 0, VERSION);
        let st = unsafe { (self.get)(self.gpu, buf.as_mut_ptr()) };
        if st != 0 {
            bail!("reading GPU clocks failed ({st})");
        }
        let pstates = (get(&buf, 8) as usize).min(16);
        let clocks = (get(&buf, 12) as usize).min(8);
        for p in 0..pstates {
            let base = PSTATES_AT + p * PSTATE_SIZE;
            if get(&buf, base) != 0 {
                continue; // only P0 matters
            }
            for c in 0..clocks {
                let at = base + 8 + c * CLOCK_SIZE;
                if get(&buf, at) == domain {
                    return Ok(get(&buf, at + 12) as i32 / 1000);
                }
            }
        }
        bail!("clock domain {domain} not reported")
    }

    /// Sets P0 offsets for graphics and memory clocks, in MHz.
    pub fn set_offsets(&self, core_mhz: i32, mem_mhz: i32) -> Result<()> {
        let mut buf = vec![0u8; SIZE];
        put(&mut buf, 0, VERSION);
        put(&mut buf, 8, 1); // one pstate
        put(&mut buf, 12, 2); // two clocks
        let base = PSTATES_AT; // pstate id 0 (P0), flags 0
        for (i, (domain, mhz)) in [(CLOCK_GRAPHICS, core_mhz), (CLOCK_MEMORY, mem_mhz)].into_iter().enumerate() {
            let at = base + 8 + i * CLOCK_SIZE;
            put(&mut buf, at, domain);
            put(&mut buf, at + 12, (mhz * 1000) as u32);
        }
        let st = unsafe { (self.set)(self.gpu, buf.as_mut_ptr()) };
        if st != 0 {
            bail!("setting GPU clock offsets failed (NVAPI status {st})");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn layout_constants() {
        assert_eq!(super::VERSION, 0x21CF8);
        assert_eq!(20 + 16 * super::PSTATE_SIZE + 100, super::SIZE);
        assert_eq!(8 + 8 * super::CLOCK_SIZE + 4 * 24, super::PSTATE_SIZE);
    }
}
