use crate::settings::{FanMode, GpuMode, Rgb, Toggle, Toggles};
use anyhow::Result;
use serde::{Deserialize, Serialize};

#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(windows)]
pub mod nvapi;
#[cfg(windows)]
pub mod windows;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Sensors {
    pub cpu_temp: Option<u32>,
    pub gpu_temp: Option<u32>,
    pub sys_temp: Option<u32>,
    pub cpu_fan_rpm: Option<u32>,
    pub gpu_fan_rpm: Option<u32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Capabilities {
    pub lighting: bool,
    pub fans: bool,
    pub gpu_modes: bool,
    pub sensors: bool,
    pub toggles: bool,
}

/// What the firmware reports right now, used to notice when something
/// (a Windows update, a BIOS reset, another tool) changed our settings.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Observed {
    pub zones: Option<[Rgb; 4]>,
    pub fans: (Option<FanMode>, Option<FanMode>),
    pub gpu: Option<GpuMode>,
    /// P0 graphics clock offset in MHz, when the GPU driver can be read.
    pub gpu_core_offset: Option<i32>,
    /// Current state of each toggle, where the firmware reports it.
    pub toggles: Toggles,
}

pub trait Backend {
    fn name(&self) -> &'static str;
    fn capabilities(&self) -> Capabilities;
    fn sensors(&mut self) -> Sensors;
    /// 16-byte `SetGamingKBBacklight` payload (see protocol::kb_backlight).
    fn set_effect(&mut self, payload: [u8; 16]) -> Result<()>;
    fn set_zone(&mut self, zone: u8, colour: Rgb) -> Result<()>;
    fn set_fan_mode(&mut self, fan: u64, mode: FanMode) -> Result<()>;
    fn set_fan_speed(&mut self, fan: u64, percent: u8) -> Result<()>;
    fn set_gpu(&mut self, mode: GpuMode) -> Result<()>;
    fn set_turbo_led(&mut self, on: bool) -> Result<()>;
    fn set_toggle(&mut self, t: Toggle, on: bool) -> Result<()>;
    fn observe(&mut self) -> Observed;
    /// Raw readings for troubleshooting (`opensenses-service status`).
    fn diagnostics(&mut self) -> Vec<String> {
        Vec::new()
    }
}

/// Opens the hardware backend for this OS.
pub fn open() -> Result<Box<dyn Backend>> {
    #[cfg(windows)]
    {
        Ok(Box::new(windows::WmiBackend::open()?))
    }
    #[cfg(target_os = "linux")]
    {
        Ok(Box::new(linux::LinuxBackend::open()?))
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        anyhow::bail!("unsupported operating system")
    }
}
