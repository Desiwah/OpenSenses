use crate::settings::{Fan, FanMode, GpuMode, Rgb, Toggle, Toggles};
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

/// One GPU overclock level with its clock offsets in MHz.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GpuLevel {
    pub mode: GpuMode,
    pub core_mhz: i32,
    pub memory_mhz: i32,
}

/// What this laptop supports. For the models PredatorSense 3.00 knew this
/// comes from its per-model data; for others, from probing the firmware.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Capabilities {
    /// BIOS product name, e.g. "Predator PH317-53".
    pub product: String,
    /// Marketing name when the model is known, e.g. "Predator Helios 300".
    pub model_name: Option<String>,
    /// False when features were found by probing an unlisted model.
    pub known_model: bool,
    /// 4-zone RGB keyboard.
    pub lighting: bool,
    /// Per-key RGB keyboard (not supported yet).
    pub per_key: bool,
    pub fans: Vec<Fan>,
    pub gpu_levels: Vec<GpuLevel>,
    pub sensors: bool,
    pub toggles: Vec<Toggle>,
    /// Windows' Sticky Keys (set by the app, not the service).
    pub sticky_keys: bool,
    /// Features this model has that OpenSenses doesn't support yet.
    pub coming: Vec<String>,
}

impl Capabilities {
    pub fn has_toggle(&self, t: Toggle) -> bool {
        self.toggles.contains(&t)
    }

    pub fn gpu_level(&self, mode: GpuMode) -> Option<(usize, GpuLevel)> {
        self.gpu_levels.iter().copied().enumerate().find(|(_, l)| l.mode == mode)
    }
}

/// What the firmware reports right now, used to notice when something
/// (a Windows update, a BIOS reset, another tool) changed our settings.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Observed {
    pub zones: Option<[Rgb; 4]>,
    pub fans: Vec<(Fan, FanMode)>,
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
    fn set_fan_mode(&mut self, fan: Fan, mode: FanMode) -> Result<()>;
    fn set_fan_speed(&mut self, fan: Fan, percent: u8) -> Result<()>;
    fn set_gpu(&mut self, mode: GpuMode) -> Result<()>;
    fn set_turbo_led(&mut self, on: bool) -> Result<()>;
    fn set_toggle(&mut self, t: Toggle, on: bool) -> Result<()>;
    fn observe(&mut self) -> Observed;
    /// Raw readings for troubleshooting (`opensenses-service status`).
    fn diagnostics(&mut self) -> Vec<String> {
        Vec::new()
    }
}

/// Features a known model has that aren't supported yet, for the app to list.
pub fn coming_features(m: &crate::models::Model) -> Vec<String> {
    let mut v = Vec::new();
    if m.per_key {
        v.push("Per-key keyboard lighting".to_string());
    }
    if m.cpu_overclock {
        v.push("CPU overclocking".to_string());
    }
    if m.power_limit {
        v.push("CPU power limits".to_string());
    }
    if m.hotkeys {
        v.push("Programmable hotkeys".to_string());
    }
    v
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
