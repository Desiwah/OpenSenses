//! Linux backend. Lighting goes through the character devices created by the
//! acer-predator-turbo-and-rgb-keyboard-linux-module ("facer") kernel module;
//! temperatures and fan speeds come from hwmon. Fan modes and GPU levels are
//! not exposed to user space by that module, so they are reported as
//! unsupported instead of guessed.

use super::{Backend, Capabilities, Observed, Sensors};
use crate::settings::{FanMode, GpuMode, Rgb, Toggle};
use anyhow::{bail, Context, Result};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

const DYNAMIC_DEV: &str = "/dev/acer-gkbbl-0";
const STATIC_DEV: &str = "/dev/acer-gkbbl-static-0";

pub struct LinuxBackend {
    hwmon_acer: Option<PathBuf>,
    hwmon_cpu: Option<PathBuf>,
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

fn write_dev(dev: &str, bytes: &[u8]) -> Result<()> {
    let mut f = fs::OpenOptions::new()
        .write(true)
        .open(dev)
        .with_context(|| format!("{dev} not found: install and load the facer kernel module"))?;
    f.write_all(bytes)?;
    Ok(())
}

impl LinuxBackend {
    pub fn open() -> Result<Self> {
        Ok(Self { hwmon_acer: hwmon_named(&["acer"]), hwmon_cpu: hwmon_named(&["coretemp", "k10temp"]) })
    }

    fn acer(&self, file: &str) -> Option<u32> {
        self.hwmon_acer.as_ref().and_then(|d| read_num(&d.join(file)))
    }
}

impl Backend for LinuxBackend {
    fn name(&self) -> &'static str {
        "Linux (facer module + hwmon)"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            lighting: Path::new(DYNAMIC_DEV).exists() && Path::new(STATIC_DEV).exists(),
            fans: false,
            gpu_modes: false,
            sensors: self.hwmon_acer.is_some() || self.hwmon_cpu.is_some(),
            toggles: false,
        }
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

    fn set_effect(&mut self, payload: [u8; 16]) -> Result<()> {
        write_dev(DYNAMIC_DEV, &payload)
    }

    fn set_zone(&mut self, zone: u8, c: Rgb) -> Result<()> {
        write_dev(STATIC_DEV, &[crate::protocol::zone_mask(zone) as u8, c.r, c.g, c.b])
    }

    fn set_fan_mode(&mut self, _fan: u64, _mode: FanMode) -> Result<()> {
        bail!("fan control is not available on Linux yet")
    }

    fn set_fan_speed(&mut self, _fan: u64, _percent: u8) -> Result<()> {
        bail!("fan control is not available on Linux yet")
    }

    fn set_gpu(&mut self, _mode: GpuMode) -> Result<()> {
        bail!("GPU modes are not available on Linux yet")
    }

    fn set_turbo_led(&mut self, _on: bool) -> Result<()> {
        Ok(())
    }

    fn set_toggle(&mut self, _t: Toggle, _on: bool) -> Result<()> {
        bail!("these settings are not available on Linux yet")
    }

    fn observe(&mut self) -> Observed {
        // The module cannot read state back, so nothing is ever "drifted".
        Observed::default()
    }
}
