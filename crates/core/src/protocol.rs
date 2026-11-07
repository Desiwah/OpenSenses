//! Encoding of the values sent to the firmware's `AcerGamingFunction` WMI
//! methods. See docs/protocol.md for how each layout was worked out.

use crate::settings::{Direction, Effect, FanMode, GpuMode, Rgb};

/// Zone bit for the 4-zone keyboard (zone 1 = left .. zone 4 = right).
pub fn zone_mask(zone: u8) -> u32 {
    match zone {
        1 => 0x1,
        2 => 0x2,
        3 => 0x4,
        _ => 0x8,
    }
}

/// `SetGamingRgbKb` input: one static colour for one zone.
pub fn rgb_zone(zone: u8, c: Rgb) -> u32 {
    zone_mask(zone) | (c.r as u32) << 8 | (c.g as u32) << 16 | (c.b as u32) << 24
}

/// Decodes `GetGamingRgbKb` output (status in byte 0, then R, G, B).
pub fn decode_rgb_zone(out: u64) -> Option<Rgb> {
    if out & 0xFF != 0 {
        return None;
    }
    Some(Rgb {
        r: (out >> 8) as u8,
        g: (out >> 16) as u8,
        b: (out >> 24) as u8,
    })
}

/// Firmware effect id used in byte 0 of `SetGamingKBBacklight`.
pub fn effect_id(e: Effect) -> u8 {
    match e {
        Effect::Static | Effect::Temperature => 0,
        Effect::Breathing => 1,
        Effect::Neon => 2,
        Effect::Wave => 3,
        Effect::Shifting => 4,
        Effect::Zoom => 5,
    }
}

pub fn effect_uses_colour(e: Effect) -> bool {
    matches!(e, Effect::Breathing | Effect::Shifting | Effect::Zoom)
}

pub fn effect_uses_direction(e: Effect) -> bool {
    matches!(e, Effect::Wave | Effect::Shifting)
}

/// `SetGamingKBBacklight` input (16-byte array):
/// [effect, speed, brightness, flags, direction, R, G, B, 0 ...]
pub fn kb_backlight(effect: Effect, speed: u8, brightness: u8, direction: Direction, colour: Rgb) -> [u8; 16] {
    let mut p = [0u8; 16];
    p[0] = effect_id(effect);
    p[2] = brightness.min(100);
    if matches!(effect, Effect::Static | Effect::Temperature) {
        return p;
    }
    p[1] = speed.clamp(1, 9);
    if effect == Effect::Wave {
        p[3] = 0x08;
    }
    if effect_uses_direction(effect) {
        p[4] = match direction {
            Direction::RightToLeft => 1,
            Direction::LeftToRight => 2,
        };
    }
    if effect_uses_colour(effect) {
        p[5] = colour.r;
        p[6] = colour.g;
        p[7] = colour.b;
    }
    p
}

pub const FAN_CPU: u64 = 0x1;
pub const FAN_GPU: u64 = 0x8;

fn fan_mode_value(m: FanMode) -> u64 {
    match m {
        FanMode::Auto => 1,
        FanMode::Max => 2,
        FanMode::Custom => 3,
    }
}

/// `SetGamingFanBehavior` input for one fan.
pub fn fan_behavior(fan: u64, m: FanMode) -> u64 {
    let shift = if fan == FAN_CPU { 16 } else { 22 };
    fan | fan_mode_value(m) << shift
}

/// `SetGamingFanSpeed` input: fan id (CPU 1, GPU 4) and percentage.
pub fn fan_speed(fan: u64, percent: u8) -> u64 {
    let id = if fan == FAN_CPU { 1 } else { 4 };
    id | (percent.min(100) as u64) << 8
}

/// Decodes `GetGamingFanBehavior(0x9)`: (cpu, gpu) mode.
pub fn decode_fan_behavior(out: u64) -> (Option<FanMode>, Option<FanMode>) {
    let m = |v: u64| match v {
        1 => Some(FanMode::Auto),
        2 => Some(FanMode::Max),
        3 => Some(FanMode::Custom),
        _ => None,
    };
    (m((out >> 8) & 3), m((out >> 14) & 3))
}

/// Sensor ids for `GetGamingSysInfo`.
pub const SENSOR_CPU_TEMP: u32 = 0x01;
pub const SENSOR_CPU_FAN: u32 = 0x02;
pub const SENSOR_SYS_TEMP: u32 = 0x03;
pub const SENSOR_GPU_FAN: u32 = 0x06;
pub const SENSOR_GPU_TEMP: u32 = 0x0A;

pub fn sensor_query(sensor: u32) -> u32 {
    0x1 | sensor << 8
}

pub fn decode_sensor(out: u64) -> Option<u32> {
    if out & 0xFF != 0 {
        return None;
    }
    Some(((out >> 8) & 0xFFFF) as u32)
}

/// Clock offsets (MHz) PredatorSense uses for the PH317-53's RTX 20-series
/// GPUs (its PredatorSense.ini): (core, memory) for Normal / Faster / Turbo.
pub fn gpu_offsets(mode: GpuMode) -> (i32, i32) {
    match mode {
        GpuMode::Normal => (0, 0),
        GpuMode::Faster => (100, 160),
        GpuMode::Turbo => (160, 320),
    }
}

/// Misc setting 5 tells the firmware which GPU level is active.
pub const MISC_GPU_OC: u64 = 0x05;

pub fn gpu_oc(mode: GpuMode) -> u64 {
    let v: u64 = match mode {
        GpuMode::Normal => 0,
        GpuMode::Faster => 1,
        GpuMode::Turbo => 2,
    };
    MISC_GPU_OC | v << 8
}

pub fn decode_gpu_oc(out: u64) -> Option<GpuMode> {
    if out & 0xFF != 0 {
        return None;
    }
    match (out >> 8) & 0xFF {
        0 => Some(GpuMode::Normal),
        1 => Some(GpuMode::Faster),
        2 => Some(GpuMode::Turbo),
        _ => None,
    }
}

/// `SetGamingProfile` input for LCD overdrive (flag at bit 48).
pub fn lcd_overdrive(on: bool) -> u64 {
    0x10 | (on as u64) << 48
}

/// `SetGamingProfile` input for the Windows and Menu keys (flag at bit 24).
pub fn win_key(on: bool) -> u64 {
    0x2 | (on as u64) << 24
}

/// Decodes `GetGamingProfile(0)`: (LCD overdrive, Windows key).
pub fn decode_profile(out: u64) -> Option<(bool, bool)> {
    if out & 0xFF != 0 {
        return None;
    }
    Some(((out >> 48) & 0xFF == 1, (out >> 24) & 0xFF == 1))
}

/// Misc setting 6: boot logo animation and sound.
pub const MISC_BOOT_ANIMATION: u64 = 0x06;

pub fn boot_animation(on: bool) -> u64 {
    MISC_BOOT_ANIMATION | (on as u64) << 8
}

pub fn decode_misc_flag(out: u64) -> Option<bool> {
    if out & 0xFF != 0 {
        return None;
    }
    match (out >> 8) & 0xFF {
        0 => Some(false),
        1 => Some(true),
        _ => None,
    }
}

/// `APGeAction.SetFunction` input for the 30-second keyboard backlight
/// timeout, in PredatorSense's format: 0x84 is the backlight hotkey number
/// on these models, byte 4 the backlight brightness (100%), byte 5 the
/// timeout in seconds (0 = never). Sending brightness 0 instead (as some
/// Linux tools do) left the PH317-53 reporting a different state.
pub fn backlight_timeout(on: bool) -> u64 {
    0x64_0008_8402 | if on { 30u64 << 40 } else { 0 }
}

pub const BACKLIGHT_TIMEOUT_GET: u32 = 0x88401;

/// Only the two shapes the firmware is known to report are trusted;
/// anything else is treated as "unknown" so it never triggers a re-apply.
pub fn decode_backlight_timeout(out: u64) -> Option<bool> {
    match out {
        0x1E00_0008_0000 => Some(true),
        0x8_0000 => Some(false),
        _ => None,
    }
}

/// `SetGamingLED` values for the turbo LED.
pub const TURBO_LED_ON: u64 = 0x10001;
pub const TURBO_LED_OFF: u64 = 0x1;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zone_colour_matches_predatorsense() {
        // zone 2, #FFA000 -> 2 | FF<<8 | A0<<16 | 00<<24
        assert_eq!(rgb_zone(2, Rgb { r: 0xFF, g: 0xA0, b: 0 }), 0x00A0FF02);
    }

    #[test]
    fn decodes_probe_reading() {
        // read from the laptop: all zones orange
        assert_eq!(decode_rgb_zone(0xA0FF00), Some(Rgb { r: 0xFF, g: 0xA0, b: 0 }));
        // CPU temperature 72 C, CPU fan 3000 rpm
        assert_eq!(decode_sensor(0x4800), Some(72));
        assert_eq!(decode_sensor(0xBB800), Some(3000));
        assert_eq!(decode_fan_behavior(0x4100), (Some(FanMode::Auto), Some(FanMode::Auto)));
        assert_eq!(decode_gpu_oc(0x0), Some(GpuMode::Normal));
    }

    #[test]
    fn fan_values_match_predatorsense() {
        // PredatorSense sends 0x410009 for Auto and 0x820009 for Max (both fans at once)
        assert_eq!(fan_behavior(FAN_CPU, FanMode::Auto) | fan_behavior(FAN_GPU, FanMode::Auto), 0x410009);
        assert_eq!(fan_behavior(FAN_CPU, FanMode::Max) | fan_behavior(FAN_GPU, FanMode::Max), 0x820009);
        assert_eq!(fan_behavior(FAN_CPU, FanMode::Custom), 0x30001);
        assert_eq!(fan_behavior(FAN_GPU, FanMode::Custom), 0xC00008);
        assert_eq!(fan_speed(FAN_GPU, 60), 4 | 60 << 8);
    }

    #[test]
    fn effect_payloads() {
        let c = Rgb { r: 1, g: 2, b: 3 };
        let w = kb_backlight(Effect::Wave, 5, 100, Direction::LeftToRight, c);
        assert_eq!(&w[..8], &[3, 5, 100, 0x08, 2, 0, 0, 0]);
        let b = kb_backlight(Effect::Breathing, 4, 50, Direction::RightToLeft, c);
        assert_eq!(&b[..8], &[1, 4, 50, 0, 0, 1, 2, 3]);
        let s = kb_backlight(Effect::Static, 9, 75, Direction::RightToLeft, c);
        assert_eq!(&s[..8], &[0, 0, 75, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn toggles_match_laptop_readings() {
        // values read from the PH317-53
        assert_eq!(decode_profile(0x1000000), Some((false, true)));
        assert_eq!(decode_profile(0x1000001000000), Some((true, true)));
        assert_eq!(lcd_overdrive(true), 0x1000000000010);
        assert_eq!(win_key(true), 0x1000002);
        assert_eq!(backlight_timeout(true), 0x1E6400088402);
        assert_eq!(backlight_timeout(false), 0x6400088402);
        assert_eq!(decode_backlight_timeout(0x1E0000080000), Some(true));
        assert_eq!(decode_backlight_timeout(0x6400000000), None);
        assert_eq!(boot_animation(true), 0x106);
        assert_eq!(decode_misc_flag(0x0), Some(false));
    }

    #[test]
    fn gpu_turbo_matches_linux_driver() {
        assert_eq!(gpu_oc(GpuMode::Turbo), 0x205);
        assert_eq!(gpu_oc(GpuMode::Normal), 0x005);
    }
}
