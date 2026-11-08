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
    // The per-key keyboards' own effects.
    Reactive,
    Marquee,
    Ripple,
    RainbowMarquee,
    Raindrop,
    CircleMarquee,
    Hedge,
    Spiral,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    RightToLeft,
    LeftToRight,
    /// Per-key keyboards' Wave only.
    Up,
    Down,
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
    /// Only the Helios 700 has this fourth level.
    Boosted,
    Turbo,
}

/// What pressing the Turbo key does (it toggles on and off).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TurboAction {
    /// PredatorSense's behaviour: highest GPU (and CPU) level and fans on
    /// Max; only on the charger.
    #[default]
    Performance,
    /// Only the highest GPU (and CPU) level.
    Overclock,
    /// Only the fans on Max.
    Fans,
    /// Nothing.
    Off,
}

/// One step of a keyboard macro: a key going down or up (by its
/// `KeyboardEvent.code` name, e.g. "KeyA", "ShiftLeft"), or a pause.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct MacroStep {
    pub code: Option<String>,
    pub down: bool,
    /// Pause in milliseconds when `code` is empty.
    pub delay_ms: u32,
}

/// Longest macro, as in PredatorSense.
pub const MAX_MACRO_STEPS: usize = 50;

/// What a programmable key does (PredatorSense's "key assignment" types).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HotkeyAction {
    #[default]
    None,
    /// Steps through these GPU levels.
    GpuLevels { cycle: Vec<GpuMode> },
    /// Steps through these CPU levels (models with a CPU overclock).
    CpuLevels { cycle: Vec<GpuMode> },
    /// Steps through these fan modes.
    FanModes { cycle: Vec<FanMode> },
    /// Normal <-> highest CPU and GPU level together.
    Performance,
    /// The same as pressing the Turbo key.
    Turbo,
    /// Windows and Menu keys on/off.
    WinKey,
    /// LCD overdrive on/off.
    LcdOverdrive,
    /// Sticky Keys on/off (a setting of the signed-in user).
    StickyKeys,
    /// Opens a program, file or web address as the signed-in user.
    LaunchApp { target: String },
    /// Types a recorded key sequence.
    Macro { steps: Vec<MacroStep> },
    /// Opens OpenSenses.
    OpenApp,
}

/// Programmable keys: three groups, switched with the laptop's group key.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct HotkeyGroup {
    pub name: String,
    pub keys: Vec<HotkeyAction>,
}

/// The fans a model can have, in PredatorSense's terms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Fan {
    Cpu,
    Gpu,
    /// Second GPU fan (Triton 500).
    Gpu2,
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
    /// Per-key keyboards: one colour per LED slot (static mode). Missing
    /// slots use `colour`. Slot 126 is the Triton 900's touchpad.
    pub keys: Vec<Rgb>,
    /// Lights besides the keyboard (Helios 700 / Triton 900).
    pub extras: ExtraLights,
}

/// Lights besides the keyboard. `None` leaves a light as it is.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExtraLights {
    /// Helios 700: the light strip on the heat pipes, and its colour.
    pub heat_pipe: Option<bool>,
    pub heat_pipe_colour: Option<Rgb>,
    /// Triton 900: the fan window light, and its colour.
    pub fan_led: Option<bool>,
    pub fan_led_colour: Option<Rgb>,
    /// Touchpad light (Triton 900: its colour is key slot 126).
    pub touchpad: Option<bool>,
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
            keys: Vec::new(),
            extras: ExtraLights::default(),
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
    pub gpu2_percent: Option<u8>,
}

impl Default for Fans {
    fn default() -> Self {
        Self {
            mode: FanMode::Auto,
            cpu_percent: Some(50),
            gpu_percent: Some(50),
            gpu2_percent: Some(50),
        }
    }
}

impl Fans {
    pub fn percent(&self, fan: Fan) -> Option<u8> {
        match fan {
            Fan::Cpu => self.cpu_percent,
            Fan::Gpu => self.gpu_percent,
            Fan::Gpu2 => self.gpu2_percent,
        }
    }

    /// Mode the firmware should report for one fan with these settings.
    pub fn expected(&self, fan: Fan) -> FanMode {
        match self.mode {
            FanMode::Custom if self.percent(fan).is_none() => FanMode::Auto,
            m => m,
        }
    }
}

/// On/off settings PredatorSense showed; which ones a laptop has depends on
/// the model.
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
    /// Discrete GPU only (on) or hybrid graphics (off); needs a restart.
    DiscreteGpu,
    /// Fans run faster in Auto mode.
    CoolBoost,
}

impl Toggle {
    /// Name used in settings files and by the app.
    pub fn key(self) -> &'static str {
        match self {
            Toggle::LcdOverdrive => "lcd_overdrive",
            Toggle::BootAnimation => "boot_animation",
            Toggle::BacklightTimeout => "backlight_timeout",
            Toggle::WinKey => "win_key",
            Toggle::DiscreteGpu => "discrete_gpu",
            Toggle::CoolBoost => "cool_boost",
        }
    }

    pub const ALL: [Toggle; 6] = [
        Toggle::LcdOverdrive,
        Toggle::BootAnimation,
        Toggle::BacklightTimeout,
        Toggle::WinKey,
        Toggle::DiscreteGpu,
        Toggle::CoolBoost,
    ];
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
    pub discrete_gpu: Option<bool>,
    pub cool_boost: Option<bool>,
}

impl Toggles {
    pub fn get(&self, t: Toggle) -> Option<bool> {
        match t {
            Toggle::LcdOverdrive => self.lcd_overdrive,
            Toggle::BootAnimation => self.boot_animation,
            Toggle::BacklightTimeout => self.backlight_timeout,
            Toggle::WinKey => self.win_key,
            Toggle::DiscreteGpu => self.discrete_gpu,
            Toggle::CoolBoost => self.cool_boost,
        }
    }

    pub fn set(&mut self, t: Toggle, v: Option<bool>) {
        match t {
            Toggle::LcdOverdrive => self.lcd_overdrive = v,
            Toggle::BootAnimation => self.boot_animation = v,
            Toggle::BacklightTimeout => self.backlight_timeout = v,
            Toggle::WinKey => self.win_key = v,
            Toggle::DiscreteGpu => self.discrete_gpu = v,
            Toggle::CoolBoost => self.cool_boost = v,
        }
    }
}

/// GameSync: settings used while a game is running, PredatorSense-style.
/// Each part left as `None` keeps the everyday setting.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GameProfile {
    /// Program file name, e.g. "eldenring.exe" (case is ignored).
    pub exe: String,
    /// Full path of the game's program when known, for showing its icon.
    pub path: Option<String>,
    pub enabled: bool,
    pub lighting: Option<Lighting>,
    pub fans: Option<Fans>,
    pub gpu: Option<GpuMode>,
    /// Windows and Menu keys while playing (usually off).
    pub win_key: Option<bool>,
}

impl Default for GameProfile {
    fn default() -> Self {
        Self {
            exe: String::new(),
            path: None,
            enabled: true,
            lighting: None,
            fans: None,
            gpu: None,
            win_key: None,
        }
    }
}

/// Most game profiles kept; more is almost certainly a misbehaving client.
pub const MAX_GAMES: usize = 64;

/// Lower-case file name of a program path, the form game profiles and
/// running programs are compared in ("C:\\Games\\X.exe" -> "x.exe").
pub fn exe_name(path: &str) -> String {
    path.trim().rsplit(['\\', '/']).next().unwrap_or("").to_lowercase()
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
    /// Separate CPU overclock level, on models that have one.
    pub cpu: GpuMode,
    /// What the Turbo key does.
    pub turbo_key: TurboAction,
    /// Programmable keys, three groups (models with hotkeys).
    pub hotkeys: Vec<HotkeyGroup>,
    /// Switch to a game's profile while it runs.
    pub game_sync: bool,
    pub games: Vec<GameProfile>,
    /// OpenSenses has sent settings to this laptop's hardware. Settings
    /// files from versions before this field always had, hence the default.
    #[serde(default = "yes")]
    pub touched: bool,
    /// The laptop's original settings were restored: OpenSenses changes
    /// nothing until the user changes a setting again.
    pub restored: bool,
    /// Where the OpenSenses app is, for the "Open OpenSenses" key.
    pub app_path: Option<String>,
}

fn yes() -> bool {
    true
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
            cpu: GpuMode::Normal,
            turbo_key: TurboAction::default(),
            hotkeys: Vec::new(),
            game_sync: true,
            games: Vec::new(),
            touched: false,
            restored: false,
            app_path: None,
        }
    }
}

impl Settings {
    /// Brings every value into the range the firmware accepts. The service
    /// runs with full rights, so it never trusts what a client sends.
    pub fn sanitized(mut self) -> Self {
        sanitize_lighting(&mut self.lighting);
        sanitize_fans(&mut self.fans);
        self.games.truncate(MAX_GAMES);
        for g in &mut self.games {
            g.exe = exe_name(&g.exe).chars().take(260).collect();
            // Only kept for the app's icon; drop anything that isn't a
            // plausible path to this same program.
            if g.path.as_deref().map(|p| p.len() > 1024 || exe_name(p) != g.exe).unwrap_or(false) {
                g.path = None;
            }
            if let Some(l) = g.lighting.as_mut() {
                sanitize_lighting(l);
            }
            if let Some(f) = g.fans.as_mut() {
                sanitize_fans(f);
            }
        }
        self.games.retain(|g| !g.exe.is_empty());
        if self.app_path.as_deref().map(|p| p.len() > 1024).unwrap_or(false) {
            self.app_path = None;
        }
        self.hotkeys.truncate(3);
        for g in &mut self.hotkeys {
            g.name = g.name.chars().take(40).collect();
            g.keys.truncate(5);
            for k in &mut g.keys {
                match k {
                    HotkeyAction::GpuLevels { cycle } | HotkeyAction::CpuLevels { cycle } => cycle.truncate(4),
                    HotkeyAction::FanModes { cycle } => cycle.truncate(3),
                    HotkeyAction::LaunchApp { target } => *target = target.chars().take(1024).collect(),
                    HotkeyAction::Macro { steps } => {
                        steps.truncate(MAX_MACRO_STEPS);
                        for st in steps {
                            st.delay_ms = st.delay_ms.min(10_000);
                            st.code = st.code.take().map(|c| c.chars().take(32).collect());
                        }
                    }
                    _ => {}
                }
            }
        }
        self
    }

    /// The first enabled game profile whose program is running.
    /// `running` holds lower-case program file names.
    pub fn active_game(&self, running: &std::collections::HashSet<String>) -> Option<usize> {
        if !self.game_sync {
            return None;
        }
        self.games.iter().position(|g| g.enabled && running.contains(&g.exe))
    }

    /// These settings with game profile `i` laid over them.
    pub fn with_game(&self, i: usize) -> Settings {
        let mut s = self.clone();
        if let Some(g) = self.games.get(i) {
            if let Some(l) = &g.lighting {
                s.lighting = l.clone();
            }
            if let Some(f) = &g.fans {
                s.fans = f.clone();
            }
            if let Some(m) = g.gpu {
                s.gpu = m;
            }
            if g.win_key.is_some() {
                s.toggles.win_key = g.win_key;
            }
        }
        s
    }
}

fn sanitize_lighting(l: &mut Lighting) {
    l.keys.truncate(crate::perkey::TOUCHPAD_SLOT + 1);
    l.speed = l.speed.clamp(1, 9);
    l.brightness = l.brightness.min(100);
}

fn sanitize_fans(f: &mut Fans) {
    f.cpu_percent = f.cpu_percent.map(|p| p.min(100));
    f.gpu_percent = f.gpu_percent.map(|p| p.min(100));
    f.gpu2_percent = f.gpu2_percent.map(|p| p.min(100));
}

pub fn settings_path() -> PathBuf {
    #[cfg(windows)]
    {
        let base = std::env::var_os("ProgramData")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn game_profiles() {
        assert_eq!(exe_name(r"C:\Games\Elden Ring\eldenring.EXE"), "eldenring.exe");
        assert_eq!(exe_name("/home/u/.steam/game.x86_64"), "game.x86_64");
        let mut s = Settings::default();
        s.games.push(GameProfile {
            exe: r"D:\x\Game.exe".into(),
            gpu: Some(GpuMode::Turbo),
            win_key: Some(false),
            ..Default::default()
        });
        s.games.push(GameProfile {
            exe: "  ".into(),
            ..Default::default()
        });
        let s = s.sanitized();
        assert_eq!(s.games.len(), 1);
        assert_eq!(s.games[0].exe, "game.exe");
        let running: std::collections::HashSet<String> = ["explorer.exe", "game.exe"].iter().map(|x| x.to_string()).collect();
        let i = s.active_game(&running).unwrap();
        let g = s.with_game(i);
        assert_eq!((g.gpu, g.toggles.win_key, &g.lighting), (GpuMode::Turbo, Some(false), &s.lighting));
        let mut off = s.clone();
        off.game_sync = false;
        assert_eq!(off.active_game(&running), None);
    }
}
