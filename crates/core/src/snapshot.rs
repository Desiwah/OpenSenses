//! The laptop's settings from before OpenSenses changed anything, so they
//! can be put back: from the app's Restore button, and automatically when
//! OpenSenses is uninstalled.
//!
//! The snapshot is taken once, the first time the service runs, before it
//! sends anything to the hardware, and is never overwritten. On a laptop
//! where an older OpenSenses (without snapshots) already changed things,
//! Acer's factory defaults for the model are used instead.

use crate::settings::{Direction, Effect, Fan, FanMode, Rgb, Toggles};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// Read from this laptop before OpenSenses changed anything.
    #[default]
    Laptop,
    /// Acer's defaults for the model (PredatorSense's own defaults).
    Factory,
}

/// Everything OpenSenses can change. `None` / empty means "not read" and is
/// left alone when restoring.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Snapshot {
    /// Seconds since 1970 (UTC) when it was taken.
    pub taken: u64,
    pub source: Source,
    pub product: String,
    /// The 16-byte `SetGamingKBBacklight` payload (effect, speed, brightness...).
    pub kb_effect: Option<Vec<u8>>,
    pub zones: Option<[Rgb; 4]>,
    pub fans: Vec<(Fan, FanMode)>,
    /// Custom-mode speeds, for fans that were in Custom mode.
    pub fan_speeds: Vec<(Fan, u8)>,
    /// The raw GPU level value the firmware held (`GetGamingMiscSetting(5)`).
    pub gpu_level: Option<u8>,
    /// NVIDIA clock offsets in MHz (NVAPI's units).
    pub gpu_core: Option<i32>,
    pub gpu_memory: Option<i32>,
    pub turbo_led: Option<bool>,
    /// The firmware fan curve (`GetGamingFanTable`), 1 = normal.
    pub fan_curve: Option<u8>,
    /// Whether the CPU level was set back to normal on restore. It can't be
    /// read back, so it's always normal (what the laptop boots with).
    pub cpu_level: Option<u8>,
    pub toggles: Toggles,
}

/// What the app shows about the snapshot.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Info {
    pub taken: u64,
    pub source: Source,
}

impl Snapshot {
    pub fn info(&self) -> Info {
        Info {
            taken: self.taken,
            source: self.source.clone(),
        }
    }

    /// Acer's defaults for a known model: static lighting in the model's
    /// colour at full brightness, fans on Auto, no GPU overclock, CoolBoost
    /// off, Windows key on and the idle backlight timeout on. LCD overdrive,
    /// the boot animation and the NVIDIA-only mode are left as they are,
    /// since their factory values differ between units.
    pub fn factory(model: &crate::models::Model, toggles: &[crate::settings::Toggle]) -> Self {
        use crate::settings::Toggle;
        let c = model.default_colour;
        let mut t = Toggles::default();
        for &tg in toggles {
            match tg {
                Toggle::CoolBoost => t.cool_boost = Some(false),
                Toggle::WinKey => t.win_key = Some(true),
                Toggle::BacklightTimeout => t.backlight_timeout = Some(true),
                _ => {}
            }
        }
        Snapshot {
            taken: now(),
            source: Source::Factory,
            product: model.product.to_string(),
            kb_effect: model
                .four_zone
                .then(|| crate::protocol::kb_backlight(Effect::Static, 0, 100, Direction::RightToLeft, c).to_vec()),
            zones: model.four_zone.then_some([c; 4]),
            fans: model.fans.iter().map(|&f| (f, FanMode::Auto)).collect(),
            fan_speeds: Vec::new(),
            gpu_level: (!model.gpu_levels.is_empty()).then_some(0),
            gpu_core: (!model.gpu_tables.is_empty()).then_some(0),
            gpu_memory: (!model.gpu_tables.is_empty()).then_some(0),
            turbo_led: Some(false),
            fan_curve: (!model.gpu_levels.is_empty()).then_some(1),
            cpu_level: model.cpu_to_firmware.then_some(0),
            toggles: t,
        }
    }
}

pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn path() -> PathBuf {
    crate::settings::settings_path().with_file_name("original.json")
}

pub fn load() -> Option<Snapshot> {
    std::fs::read_to_string(path()).ok().and_then(|s| serde_json::from_str(&s).ok())
}

/// Saves the snapshot unless one already exists: the first one is the one
/// that matters.
pub fn save_once(s: &Snapshot) -> anyhow::Result<()> {
    let p = path();
    if p.exists() {
        return Ok(());
    }
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
    fn factory_defaults_for_this_laptop() {
        let m = crate::models::lookup("Predator PH317-53").unwrap();
        let all = crate::settings::Toggle::ALL;
        let s = Snapshot::factory(m, &all);
        assert_eq!(s.zones.unwrap()[0], Rgb { r: 0x00, g: 0xAE, b: 0xC7 });
        assert_eq!(s.fans, vec![(Fan::Cpu, FanMode::Auto), (Fan::Gpu, FanMode::Auto)]);
        assert_eq!((s.gpu_core, s.gpu_memory, s.gpu_level), (Some(0), Some(0), Some(0)));
        assert_eq!(
            (s.toggles.cool_boost, s.toggles.win_key, s.toggles.lcd_overdrive),
            (Some(false), Some(true), None)
        );
        let json = serde_json::to_string(&s).unwrap();
        let back: Snapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(back.kb_effect, s.kb_effect);
    }
}
