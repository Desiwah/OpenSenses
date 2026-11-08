//! Encoding of the values sent to the firmware's `AcerGamingFunction` WMI
//! methods. See docs/protocol.md for how each layout was worked out.

use crate::settings::{Direction, Effect, Fan, FanMode, Rgb};

/// The two firmware interfaces. On Windows they are the WMI classes of the
/// same names; on Linux the OpenSenses driver reaches them by GUID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Iface {
    /// `AcerGamingFunction`, GUID 7A4DDFE7-5B5D-40B4-8595-4408E0CC7F56.
    Gaming,
    /// `APGeAction`, GUID 61EF69EA-865C-4BC3-A502-A0DEBA0CB531.
    Apge,
}

/// WMI method ids, as listed in the firmware's own class description
/// (read on a PH317-53, version 2.63). Linux calls methods by these ids.
pub fn method_id(iface: Iface, name: &str) -> Option<u32> {
    const GAMING: &[&str] = &[
        "SetGamingProfile",
        "SetGamingLED",
        "GetGamingProfile",
        "GetGamingLED",
        "GetGamingSysInfo",
        "SetGamingRgbKb",
        "GetGamingRgbKb",
        "SetGamingProfileSetting",
        "GetGamingProfileSetting",
        "SetGamingLEDBehavior",
        "GetGamingLEDBehavior",
        "SetGamingLEDColor",
        "GetGamingLEDColor",
        "SetGamingFanBehavior",
        "GetGamingFanBehavior",
        "SetGamingFanSpeed",
        "GetGamingFanSpeed",
        "SetGamingFanTable",
        "GetGamingFanTable",
        "SetGamingKBBacklight",
        "GetGamingKBBacklight",
        "SetGamingMiscSetting",
        "GetGamingMiscSetting",
    ];
    const APGE: &[&str] = &["SetFunction", "GetFunction"];
    let list = match iface {
        Iface::Gaming => GAMING,
        Iface::Apge => APGE,
    };
    list.iter().position(|m| *m == name).map(|i| i as u32 + 1)
}

/// A method's input: its size is what the firmware's class description
/// declares (uint32, uint64 or a 16-byte array).
#[derive(Debug, Clone, Copy)]
pub enum Input {
    U32(u32),
    U64(u64),
    Bytes([u8; 16]),
}

impl Input {
    /// Little-endian bytes, as the ACPI method receives them.
    pub fn bytes(&self) -> Vec<u8> {
        match self {
            Input::U32(n) => n.to_le_bytes().to_vec(),
            Input::U64(n) => n.to_le_bytes().to_vec(),
            Input::Bytes(b) => b.to_vec(),
        }
    }
}

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

/// Fan bit in `SetGamingFanBehavior`'s low 16 bits.
pub fn fan_mask(fan: Fan) -> u64 {
    match fan {
        Fan::Cpu => 0x1,
        Fan::Gpu => 0x8,
        Fan::Gpu2 => 0x10,
    }
}

/// Bit where a fan's 2-bit mode sits in `SetGamingFanBehavior`'s input;
/// `GetGamingFanBehavior` reports it 8 bits lower.
fn fan_mode_shift(fan: Fan) -> u32 {
    match fan {
        Fan::Cpu => 16,
        Fan::Gpu => 22,
        Fan::Gpu2 => 24,
    }
}

fn fan_mode_value(m: FanMode) -> u64 {
    match m {
        FanMode::Auto => 1,
        FanMode::Max => 2,
        FanMode::Custom => 3,
    }
}

/// `SetGamingFanBehavior` input for one fan.
pub fn fan_behavior(fan: Fan, m: FanMode) -> u64 {
    fan_mask(fan) | fan_mode_value(m) << fan_mode_shift(fan)
}

/// `SetGamingFanSpeed` input: fan id (CPU 1, GPU 4, second GPU 5) and percentage.
pub fn fan_speed(fan: Fan, percent: u8) -> u64 {
    let id = match fan {
        Fan::Cpu => 1,
        Fan::Gpu => 4,
        Fan::Gpu2 => 5,
    };
    id | (percent.min(100) as u64) << 8
}

/// `GetGamingFanSpeed` input for one fan (the same ids as `fan_speed`).
pub fn fan_speed_query(fan: Fan) -> u32 {
    (fan_speed(fan, 0) & 0xFF) as u32
}

/// Decodes `GetGamingFanSpeed`: the set percentage in bits 8-15.
pub fn decode_fan_speed(out: u64) -> Option<u8> {
    if out & 0xFF != 0 {
        return None;
    }
    let p = ((out >> 8) & 0xFF) as u8;
    (p <= 100).then_some(p)
}

/// Reads a `SetGamingKBBacklight` payload back into its parts, rejecting
/// anything that isn't a valid effect (so a bad read is never re-sent).
pub fn decode_kb_backlight(p: &[u8]) -> Option<(Effect, u8, u8, Direction, Rgb)> {
    if p.len() < 8 {
        return None;
    }
    let effect = match p[0] {
        0 => Effect::Static,
        1 => Effect::Breathing,
        2 => Effect::Neon,
        3 => Effect::Wave,
        4 => Effect::Shifting,
        5 => Effect::Zoom,
        _ => return None,
    };
    if p[1] > 9 || p[2] > 100 {
        return None;
    }
    let direction = if p[4] == 2 { Direction::LeftToRight } else { Direction::RightToLeft };
    Some((effect, p[1], p[2], direction, Rgb { r: p[5], g: p[6], b: p[7] }))
}

/// Query for `GetGamingFanBehavior` covering these fans.
pub fn fan_query(fans: &[Fan]) -> u32 {
    fans.iter().map(|f| fan_mask(*f)).fold(0, |a, b| a | b) as u32
}

/// Decodes one fan's mode from `GetGamingFanBehavior`.
pub fn decode_fan_behavior(out: u64, fan: Fan) -> Option<FanMode> {
    if out & 0xFF != 0 {
        return None;
    }
    match (out >> (fan_mode_shift(fan) - 8)) & 3 {
        1 => Some(FanMode::Auto),
        2 => Some(FanMode::Max),
        3 => Some(FanMode::Custom),
        _ => None,
    }
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

/// Misc setting 5 tells the firmware which GPU level is active; the value
/// is the level's index in the model's list (0 = Normal).
pub const MISC_GPU_OC: u64 = 0x05;

pub fn gpu_oc(level_index: usize) -> u64 {
    MISC_GPU_OC | (level_index as u64 & 0xFF) << 8
}

pub fn decode_gpu_oc(out: u64) -> Option<usize> {
    if out & 0xFF != 0 {
        return None;
    }
    Some(((out >> 8) & 0xFF) as usize)
}

/// Misc setting 2: graphics mode, 1 = hybrid, 2 = discrete GPU only.
pub const MISC_GPU_MODE: u64 = 0x02;

pub fn discrete_gpu(on: bool) -> u64 {
    MISC_GPU_MODE | if on { 2 << 8 } else { 1 << 8 }
}

pub fn decode_discrete_gpu(out: u64) -> Option<bool> {
    if out & 0xFF != 0 {
        return None;
    }
    match (out >> 8) & 0xFF {
        1 => Some(false),
        2 => Some(true),
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

/// Keyboard backlight timeout through `APGeAction` (0x84 is the backlight
/// hotkey number on these models). Read with `GetFunction(0x88401)`: byte 5
/// is the timeout in seconds (30 = on, 0 = off) and byte 4 a value the
/// firmware wants echoed back. A `SetFunction` is only accepted when byte 4
/// matches what was just read, and an accepted call flips the setting, so
/// it must only be sent when the state actually needs to change. This is
/// how PredatorSense does it; tested on a PH317-53.
pub const BACKLIGHT_TIMEOUT_GET: u32 = 0x88401;

pub fn decode_backlight_timeout(out: u64) -> Option<bool> {
    if out & 0xFF != 0 {
        return None;
    }
    Some((out >> 40) & 0xFF != 0)
}

/// `SetFunction` input to switch the timeout, given the current reading.
pub fn backlight_timeout(current: u64, on: bool) -> u64 {
    0x88402 | (current & 0xFF_0000_0000) | if on { 30u64 << 40 } else { 0 }
}

/// CoolBoost (fans run harder in Auto mode) through `APGeAction`:
/// `SetFunction(7 | on << 16)`, read with `GetFunction(0x207)`, where byte 1
/// is 1 while it is on. Unlike the backlight timeout, sending the same value
/// twice leaves it as it is. Tested on a PH317-53 (about +1000 RPM per fan).
pub const COOL_BOOST_GET: u32 = 0x207;

pub fn cool_boost(on: bool) -> u64 {
    7 | (on as u64) << 16
}

pub fn decode_cool_boost(out: u64) -> Option<bool> {
    if out & 0xFF != 0 {
        return None;
    }
    Some((out >> 8) & 0xFF == 1)
}

/// `SetGamingLED` values for the turbo LED.
pub const TURBO_LED_ON: u64 = 0x10001;
pub const TURBO_LED_OFF: u64 = 0x1;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn method_ids_match_the_firmware() {
        assert_eq!(method_id(Iface::Gaming, "SetGamingLED"), Some(2));
        assert_eq!(method_id(Iface::Gaming, "SetGamingRgbKb"), Some(6));
        assert_eq!(method_id(Iface::Gaming, "SetGamingFanBehavior"), Some(14));
        assert_eq!(method_id(Iface::Gaming, "SetGamingKBBacklight"), Some(20));
        assert_eq!(method_id(Iface::Gaming, "GetGamingMiscSetting"), Some(23));
        assert_eq!(method_id(Iface::Apge, "GetFunction"), Some(2));
        assert_eq!(method_id(Iface::Gaming, "Nope"), None);
        assert_eq!(Input::U32(0x0A0B0C0D).bytes(), vec![0x0D, 0x0C, 0x0B, 0x0A]);
    }

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
        assert_eq!(decode_fan_behavior(0x4100, Fan::Cpu), Some(FanMode::Auto));
        assert_eq!(decode_fan_behavior(0x4100, Fan::Gpu), Some(FanMode::Auto));
        assert_eq!(decode_fan_behavior(0xC300, Fan::Gpu), Some(FanMode::Custom));
        assert_eq!(decode_gpu_oc(0x0), Some(0));
    }

    #[test]
    fn fan_values_match_predatorsense() {
        // PredatorSense sends 0x410009 for Auto and 0x820009 for Max (both fans at once)
        assert_eq!(fan_behavior(Fan::Cpu, FanMode::Auto) | fan_behavior(Fan::Gpu, FanMode::Auto), 0x410009);
        assert_eq!(fan_behavior(Fan::Cpu, FanMode::Max) | fan_behavior(Fan::Gpu, FanMode::Max), 0x820009);
        assert_eq!(fan_behavior(Fan::Cpu, FanMode::Custom), 0x30001);
        assert_eq!(fan_behavior(Fan::Gpu, FanMode::Custom), 0xC00008);
        assert_eq!(fan_speed(Fan::Gpu, 60), 4 | 60 << 8);
        // second GPU fan, as PredatorSense sends it for the Triton 500
        assert_eq!(fan_behavior(Fan::Gpu2, FanMode::Custom), 0x3000010);
        assert_eq!(fan_behavior(Fan::Gpu2, FanMode::Auto), 0x1000010);
        assert_eq!(fan_speed(Fan::Gpu2, 40), 5 | 40 << 8);
        assert_eq!(fan_query(&[Fan::Cpu, Fan::Gpu]), 0x9);
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
        // readings and the commands that switched them on the laptop
        assert_eq!(decode_backlight_timeout(0x1E0000080000), Some(true));
        assert_eq!(decode_backlight_timeout(0x6400000000), Some(false));
        assert_eq!(backlight_timeout(0x6400000000, true), 0x1E6400088402);
        assert_eq!(backlight_timeout(0x1E0000080000, false), 0x88402);
        assert_eq!(boot_animation(true), 0x106);
        assert_eq!(decode_misc_flag(0x0), Some(false));
        assert_eq!(decode_cool_boost(0x1010000), Some(false));
        let fx = kb_backlight(Effect::Wave, 5, 75, Direction::LeftToRight, Rgb::default());
        let back = decode_kb_backlight(&fx).unwrap();
        assert_eq!((back.0, back.1, back.2, back.3), (Effect::Wave, 5, 75, Direction::LeftToRight));
        assert_eq!(decode_kb_backlight(&[9; 16]), None);
        assert_eq!(fan_speed_query(Fan::Gpu), 4);
        assert_eq!(decode_fan_speed(0x3200), Some(50));
        assert_eq!(decode_cool_boost(0x1010100), Some(true));
        assert_eq!((cool_boost(true), cool_boost(false)), (0x10007, 0x7));
    }

    #[test]
    fn gpu_levels_match_predatorsense() {
        // Helios 300 Turbo is level 2 (also what the Linux driver sends)
        assert_eq!(gpu_oc(2), 0x205);
        assert_eq!(gpu_oc(0), 0x005);
        // Helios 700 has a fourth level
        assert_eq!(gpu_oc(3), 0x305);
        assert_eq!(discrete_gpu(true), 0x202);
        assert_eq!(decode_discrete_gpu(0x100), Some(false));
    }
}
