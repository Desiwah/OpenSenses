//! Linux: calls the same firmware methods as Windows through the OpenSenses
//! kernel driver (driver/opensenses.c), which exposes them to root at
//! /dev/opensenses. The firmware logic is shared with Windows in `acer.rs`;
//! GPU clock offsets go through NVML.
//!
//! Without the driver, temperatures and fan speeds still come from the
//! kernel's own hwmon sensors, and the app says how to install the driver.

use super::acer::{AcerBackend, GpuClocks, Transport};
use super::{Backend, Capabilities, Observed, Sensors};
use crate::protocol::{self as p, Iface, Input};
use crate::settings::{Fan, FanMode, GpuMode, Rgb, Toggle};
use anyhow::{bail, Context, Result};
use std::fs;
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};

pub const DEVICE: &str = "/dev/opensenses";

/// A driver the service built itself (no DKMS), for one kernel release.
pub fn local_module(release: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(format!("/var/lib/OpenSenses/opensenses-{release}.ko"))
}
const DATA: usize = 32;

/// Must match `struct opensenses_call` in driver/opensenses.c (96 bytes).
#[repr(C)]
struct Call {
    iface: u32,
    method: u32,
    in_len: u32,
    out_type: u32,
    input: [u8; DATA],
    out_int: u64,
    out_len: u32,
    out: [u8; DATA],
    reserved: u32,
}

/// _IOWR('O', 1, struct opensenses_call)
const IOC_CALL: libc::c_ulong = 0xC060_4F01;

pub struct Driver {
    file: fs::File,
    apge: bool,
}

impl Driver {
    pub fn open() -> Result<Self> {
        let file = fs::OpenOptions::new().read(true).write(true).open(DEVICE).with_context(|| format!("{DEVICE} is missing"))?;
        let mut me = Self { file, apge: true };
        // APGeAction answers on every model PredatorSense knew, but check.
        me.apge = me.raw(Iface::Apge, "GetFunction", Input::U32(p::COOL_BOOST_GET)).is_ok();
        Ok(me)
    }

    fn raw(&self, iface: Iface, method: &str, input: Input) -> Result<u64> {
        let id = p::method_id(iface, method).with_context(|| format!("unknown firmware method {method}"))?;
        let bytes = input.bytes();
        let mut c = Call {
            iface: match iface {
                Iface::Gaming => 0,
                Iface::Apge => 1,
            },
            method: id,
            in_len: bytes.len() as u32,
            out_type: 0,
            input: [0; DATA],
            out_int: 0,
            out_len: 0,
            out: [0; DATA],
            reserved: 0,
        };
        c.input[..bytes.len()].copy_from_slice(&bytes);
        let r = unsafe { libc::ioctl(self.file.as_raw_fd(), IOC_CALL as _, &mut c as *mut Call) };
        if r != 0 {
            bail!("{method} failed: {}", std::io::Error::last_os_error());
        }
        Ok(match c.out_type {
            1 => c.out_int,
            2 => {
                let mut b = [0u8; 8];
                let n = (c.out_len as usize).min(8);
                b[..n].copy_from_slice(&c.out[..n]);
                u64::from_le_bytes(b)
            }
            _ => 0,
        })
    }
}

impl Transport for Driver {
    fn name(&self) -> &'static str {
        "Linux (OpenSenses driver)"
    }

    fn call(&self, iface: Iface, method: &str, input: Input) -> Result<u64> {
        self.raw(iface, method, input)
    }

    fn has_apge(&self) -> bool {
        self.apge
    }
}

pub fn dmi(field: &str) -> String {
    fs::read_to_string(format!("/sys/class/dmi/id/{field}")).map(|s| s.trim().to_string()).unwrap_or_default()
}

/// The firmware backend through the driver, or, when the driver isn't
/// there, a sensors-only backend that tells the user how to get it.
pub fn open() -> Result<Box<dyn Backend>> {
    if !Path::new(DEVICE).exists() {
        // Installed but not loaded yet (e.g. right after installing), or
        // built by the service itself without DKMS.
        let ok = std::process::Command::new("modprobe").arg("opensenses").status().map(|s| s.success()).unwrap_or(false);
        let release = fs::read_to_string("/proc/sys/kernel/osrelease").unwrap_or_default();
        let ko = local_module(release.trim());
        if !ok && ko.is_file() {
            let _ = std::process::Command::new("insmod").arg(&ko).status();
        }
        for _ in 0..20 {
            if Path::new(DEVICE).exists() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
    let product = dmi("product_name");
    match Driver::open() {
        Ok(d) => {
            let gpu = super::nvml::Nvml::open().map(|n| Box::new(n) as Box<dyn GpuClocks>).map_err(|e| format!("{e:#}"));
            if let Err(e) = &gpu {
                log::info!("GPU clock offsets unavailable: {e}");
            }
            let family = Some(dmi("product_family"));
            // Linux hasn't been tried on real hardware yet: everything is
            // marked experimental until it has.
            Ok(Box::new(AcerBackend::new(d, gpu, product, family, false)))
        }
        Err(e) => {
            log::warn!("OpenSenses driver unavailable ({e:#}); only sensors will work");
            Ok(Box::new(SensorsOnly::new(product)))
        }
    }
}

/// Used when the OpenSenses driver isn't installed: shows temperatures and
/// fan speeds from the kernel's own drivers and nothing else.
pub struct SensorsOnly {
    hwmon_acer: Option<PathBuf>,
    hwmon_cpu: Option<PathBuf>,
    product: String,
}

fn hwmon_named(names: &[&str]) -> Option<PathBuf> {
    let dir = fs::read_dir("/sys/class/hwmon").ok()?;
    for e in dir.flatten() {
        let p = e.path();
        if let Ok(n) = fs::read_to_string(p.join("name")) {
            if names.contains(&n.trim()) {
                return Some(p);
            }
        }
    }
    None
}

fn read_num(p: &Path) -> Option<u32> {
    fs::read_to_string(p).ok()?.trim().parse::<u64>().ok().map(|v| v as u32)
}

impl SensorsOnly {
    fn new(product: String) -> Self {
        Self { hwmon_acer: hwmon_named(&["acer"]), hwmon_cpu: hwmon_named(&["coretemp", "k10temp"]), product }
    }

    fn acer(&self, file: &str) -> Option<u32> {
        self.hwmon_acer.as_ref().and_then(|d| read_num(&d.join(file)))
    }
}

impl Backend for SensorsOnly {
    fn name(&self) -> &'static str {
        "Linux (sensors only)"
    }

    fn capabilities(&self) -> Capabilities {
        let model = crate::models::lookup(&self.product);
        Capabilities {
            product: self.product.clone(),
            model_name: model.map(|m| m.name.to_string()).or_else(|| Some(dmi("product_family")).filter(|s| !s.is_empty())),
            known_model: model.is_some(),
            sensors: self.hwmon_acer.is_some() || self.hwmon_cpu.is_some(),
            sticky_keys: true,
            coming: model.map(super::coming_features).unwrap_or_default(),
            notice: Some(
                "The OpenSenses driver isn't installed, so only temperatures are shown. \
                 Reinstall OpenSenses or click Set up again to build it; see the README if your distribution doesn't allow extra drivers."
                    .into(),
            ),
            ..Default::default()
        }
        .all_experimental()
    }

    fn sensors(&mut self) -> Sensors {
        // acer hwmon channel order: temp1 CPU, temp2 system, temp3 GPU; fan1 CPU, fan2 GPU
        let milli = |v: Option<u32>| v.map(|t| t / 1000);
        let cpu = milli(self.acer("temp1_input"))
            .or_else(|| milli(self.hwmon_cpu.as_ref().and_then(|d| read_num(&d.join("temp1_input")))));
        Sensors {
            cpu_temp: cpu,
            sys_temp: milli(self.acer("temp2_input")),
            gpu_temp: milli(self.acer("temp3_input")),
            cpu_fan_rpm: self.acer("fan1_input"),
            gpu_fan_rpm: self.acer("fan2_input"),
        }
    }

    fn set_effect(&mut self, _: [u8; 16]) -> Result<()> {
        bail!("the OpenSenses driver isn't installed")
    }
    fn set_zone(&mut self, _: u8, _: Rgb) -> Result<()> {
        bail!("the OpenSenses driver isn't installed")
    }
    fn set_fan_mode(&mut self, _: Fan, _: FanMode) -> Result<()> {
        bail!("the OpenSenses driver isn't installed")
    }
    fn set_fan_speed(&mut self, _: Fan, _: u8) -> Result<()> {
        bail!("the OpenSenses driver isn't installed")
    }
    fn set_gpu(&mut self, _: GpuMode) -> Result<()> {
        bail!("the OpenSenses driver isn't installed")
    }
    fn set_turbo_led(&mut self, _: bool) -> Result<()> {
        Ok(())
    }
    fn set_toggle(&mut self, _: Toggle, _: bool) -> Result<()> {
        bail!("the OpenSenses driver isn't installed")
    }
    fn observe(&mut self) -> Observed {
        Observed::default()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn call_layout_matches_the_driver() {
        assert_eq!(std::mem::size_of::<super::Call>(), 96);
        assert_eq!(std::mem::offset_of!(super::Call, out_int), 48);
        assert_eq!(std::mem::offset_of!(super::Call, out), 60);
    }
}
