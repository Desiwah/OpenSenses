use crate::settings::{Fan, FanMode, GpuMode, Rgb, Toggle, Toggles};
use anyhow::Result;
use serde::{Deserialize, Serialize};

pub mod acer;
#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(windows)]
pub mod nvapi;
#[cfg(target_os = "linux")]
pub mod nvml;
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
    /// Per-key RGB keyboard (its own USB controller).
    pub per_key: bool,
    /// Per-key keyboard layout: 0 US, 1 UK, 2/3 other ISO (from the controller).
    pub per_key_layout: u8,
    /// Helios 700: light strip on the heat pipes (on/off and colour).
    pub heat_pipe_light: bool,
    /// Triton 900: fan window light (on/off and colour).
    pub fan_light: bool,
    /// Touchpad light: 1 = on/off (Helios 700), 2 = with colour (Triton 900).
    pub touchpad_light: u8,
    pub fans: Vec<Fan>,
    pub gpu_levels: Vec<GpuLevel>,
    pub sensors: bool,
    pub toggles: Vec<Toggle>,
    /// Windows' Sticky Keys (set by the app, not the service).
    pub sticky_keys: bool,
    /// Features this model has that OpenSenses doesn't support yet.
    pub coming: Vec<String>,
    /// Separate CPU overclock levels (unlocked HK CPUs on some models).
    pub cpu_levels: Vec<GpuMode>,
    /// The CPU power level is sent with the GPU level, as PredatorSense does.
    pub cpu_follows_gpu: bool,
    /// The firmware's fan curve follows the overclock level.
    pub fan_table: bool,
    /// The laptop has a Turbo key that OpenSenses handles.
    pub turbo_key: bool,
    /// Programmable keys per hotkey group (0 = none).
    pub hotkey_keys: u8,
    /// The laptop has a PredatorSense key (it opens OpenSenses).
    pub ps_key: bool,
    /// Features not yet tested on this model's hardware: "lighting",
    /// "fans", "gpu", "monitor", "games" and toggle keys ("cool_boost").
    /// GameSync itself only depends on the OS, so it is only listed on Linux.
    pub experimental: Vec<String>,
    /// Something the user should know or do, e.g. install the Linux driver.
    pub notice: Option<String>,
}

impl Capabilities {
    pub fn has_toggle(&self, t: Toggle) -> bool {
        self.toggles.contains(&t)
    }

    pub fn gpu_level(&self, mode: GpuMode) -> Option<(usize, GpuLevel)> {
        self.gpu_levels.iter().copied().enumerate().find(|(_, l)| l.mode == mode)
    }

    /// Marks every feature this laptop shows as experimental.
    pub fn all_experimental(mut self) -> Self {
        let mut v: Vec<String> = ["lighting", "fans", "gpu", "monitor", "keys"].iter().map(|s| s.to_string()).collect();
        v.extend(self.toggles.iter().map(|t| t.key().to_string()));
        self.experimental = v;
        self
    }

    pub fn with_experimental(mut self, key: &str) -> Self {
        self.experimental.push(key.to_string());
        self
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
    /// Per-key keyboards: sends what changed since `old` (all when None).
    fn set_per_key(&mut self, _l: &crate::settings::Lighting, _old: Option<&crate::settings::Lighting>) -> Result<()> {
        Ok(())
    }
    /// Lights besides the keyboard. `touchpad` is the Triton 900's touchpad colour.
    fn set_extra_lights(&mut self, _e: &crate::settings::ExtraLights, _touchpad: Option<Rgb>) -> Vec<String> {
        Vec::new()
    }
    /// The programmable keys' active group (1-3), on models that have them.
    fn hotkey_group(&mut self) -> Option<u8> {
        None
    }
    /// CPU overclock / power level, by index into the model's levels.
    fn set_cpu_level(&mut self, _index: usize) -> Result<()> {
        Ok(())
    }
    /// Firmware fan curve (1 = normal); only sent when it differs.
    fn set_fan_table(&mut self, _curve: u8) -> Result<()> {
        Ok(())
    }
    fn set_toggle(&mut self, t: Toggle, on: bool) -> Result<()>;
    fn observe(&mut self) -> Observed;
    /// Raw readings for troubleshooting (`opensenses-service status`).
    fn diagnostics(&mut self) -> Vec<String> {
        Vec::new()
    }
    /// Reads everything OpenSenses can change, to restore it later. `None`
    /// when this backend can't read the hardware.
    fn snapshot(&mut self) -> Option<crate::snapshot::Snapshot> {
        None
    }
    /// Acer's defaults for this model, when it's a known one.
    fn factory_snapshot(&self) -> Option<crate::snapshot::Snapshot> {
        None
    }
    /// Puts a snapshot back; returns the errors.
    fn restore(&mut self, _s: &crate::snapshot::Snapshot) -> Vec<String> {
        vec!["restoring isn't possible without the hardware interface".into()]
    }
}

/// Features a known model has that OpenSenses can't offer, for the app to
/// list.
pub fn coming_features(m: &crate::models::Model) -> Vec<String> {
    let mut v = Vec::new();
    if m.machine_type == 0 {
        // The Helios 500 did its CPU overclock and power limits only through
        // encrypted Intel XTU profiles, with no firmware part.
        v.push("CPU overclocking and power limits (PredatorSense used Intel XTU for these)".to_string());
    }
    v
}

/// Listens for firmware key events (`EventDetail` bytes) until `tx` closes.
pub fn listen_events(tx: std::sync::mpsc::Sender<[u8; 8]>) {
    #[cfg(windows)]
    windows::listen_events(tx);
    #[cfg(target_os = "linux")]
    linux::listen_events(tx);
    #[cfg(not(any(windows, target_os = "linux")))]
    drop(tx);
}

/// Whether the laptop is on the charger, and the battery percentage.
pub fn power_status() -> Option<(bool, u8)> {
    #[cfg(windows)]
    {
        windows::power_status()
    }
    #[cfg(target_os = "linux")]
    {
        linux::power_status()
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        None
    }
}

/// Opens the hardware backend for this OS.
pub fn open() -> Result<Box<dyn Backend>> {
    #[cfg(windows)]
    {
        Ok(Box::new(windows::open()?))
    }
    #[cfg(target_os = "linux")]
    {
        linux::open()
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        anyhow::bail!("unsupported operating system")
    }
}
