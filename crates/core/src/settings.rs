use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Effect {
    Static,
    Breathing,
    Neon,
    Wave,
    Shifting,
    Zoom,
    /// Software effect: whole keyboard follows CPU temperature.
    Temperature,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    RightToLeft,
    LeftToRight,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FanMode {
    Auto,
    Max,
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GpuMode {
    Normal,
    Faster,
    Turbo,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Lighting {
    pub effect: Effect,
    /// One colour per zone, used by `Static`.
    pub zones: [Rgb; 4],
    /// Zones switched off in `Static` (shown black).
    pub zone_on: [bool; 4],
    /// Colour for Breathing / Shifting / Zoom.
    pub colour: Rgb,
    /// 1 (slowest) ..= 9 (fastest)
    pub speed: u8,
    /// 0 ..= 100
    pub brightness: u8,
    pub direction: Direction,
}

impl Default for Lighting {
    fn default() -> Self {
        // PredatorSense's PH317-53 default: all zones #00AEC7
        let teal = Rgb { r: 0x00, g: 0xAE, b: 0xC7 };
        Self {
            effect: Effect::Static,
            zones: [teal; 4],
            zone_on: [true; 4],
            colour: teal,
            speed: 4,
            brightness: 100,
            direction: Direction::RightToLeft,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Fans {
    pub mode: FanMode,
    /// Custom mode: Some(percent) = fixed speed, None = that fan stays on Auto.
    pub cpu_percent: Option<u8>,
    pub gpu_percent: Option<u8>,
}

impl Default for Fans {
    fn default() -> Self {
        Self { mode: FanMode::Auto, cpu_percent: Some(50), gpu_percent: Some(50) }
    }
}

/// On/off settings PredatorSense showed for the PH317-53.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Toggle {
    /// Faster pixel response on the built-in screen.
    LcdOverdrive,
    /// Logo animation and sound when the laptop starts.
    BootAnimation,
    /// Keyboard lighting goes dark after 30 seconds without typing.
    BacklightTimeout,
    /// Windows and Menu keys work (off blocks them, e.g. while gaming).
    WinKey,
}

impl Toggle {
    pub const ALL: [Toggle; 4] = [Toggle::LcdOverdrive, Toggle::BootAnimation, Toggle::BacklightTimeout, Toggle::WinKey];
}

/// `None` means "leave it as the laptop has it": nothing is changed until
/// the user actually flips a toggle in the app.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Toggles {
    pub lcd_overdrive: Option<bool>,
    pub boot_animation: Option<bool>,
    pub backlight_timeout: Option<bool>,
    pub win_key: Option<bool>,
}

impl Toggles {
    pub fn get(&self, t: Toggle) -> Option<bool> {
        match t {
            Toggle::LcdOverdrive => self.lcd_overdrive,
            Toggle::BootAnimation => self.boot_animation,
            Toggle::BacklightTimeout => self.backlight_timeout,
            Toggle::WinKey => self.win_key,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub lighting: Lighting,
    pub fans: Fans,
    pub gpu: GpuMode,
    pub toggles: Toggles,
    /// Show temperatures in Fahrenheit in the app.
    pub fahrenheit: bool,
    /// Re-apply these settings at startup, after sleep and when something
    /// (e.g. a Windows update) resets them.
    pub keep_applied: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            lighting: Lighting::default(),
            fans: Fans::default(),
            gpu: GpuMode::Normal,
            toggles: Toggles::default(),
            fahrenheit: false,
            keep_applied: true,
        }
    }
}

impl Settings {
    /// Brings every value into the range the firmware accepts. The service
    /// runs with full rights, so it never trusts what a client sends.
    pub fn sanitized(mut self) -> Self {
        let l = &mut self.lighting;
        l.speed = l.speed.clamp(1, 9);
        l.brightness = l.brightness.min(100);
        self.fans.cpu_percent = self.fans.cpu_percent.map(|p| p.min(100));
        self.fans.gpu_percent = self.fans.gpu_percent.map(|p| p.min(100));
        self
    }
}

pub fn settings_path() -> PathBuf {
    #[cfg(windows)]
    {
        let base = std::env::var_os("ProgramData").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"));
        base.join(crate::APP_DIR).join("settings.json")
    }
    #[cfg(not(windows))]
    {
        PathBuf::from("/var/lib").join(crate::APP_DIR).join("settings.json")
    }
}

/// Saved settings, or None on first run (no file yet).
pub fn load() -> Option<Settings> {
    std::fs::read_to_string(settings_path())
        .ok()
        .and_then(|s| serde_json::from_str::<Settings>(&s).ok())
        .map(Settings::sanitized)
}

pub fn save(s: &Settings) -> anyhow::Result<()> {
    let p = settings_path();
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = p.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(s)?)?;
    std::fs::rename(tmp, p)?;
    Ok(())
}
