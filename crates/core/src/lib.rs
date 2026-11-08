pub mod backend;
pub mod ipc;
pub mod protocol;
pub mod settings;

use backend::{Backend, Observed};
use settings::{Effect, FanMode, GpuMode, Rgb, Settings, Toggle};

/// Folder name under ProgramData / /var/lib and the IPC endpoint name.
pub const APP_DIR: &str = "OpenSenses";
pub const PIPE_NAME: &str = r"\\.\pipe\opensenses";
pub const SOCKET_PATH: &str = "/run/opensenses.sock";

/// Sends every setting to the hardware.
pub fn apply(hw: &mut dyn Backend, s: &Settings) -> Vec<String> {
    apply_changes(hw, None, s)
}

/// Sends the settings that differ from `old` (everything when `old` is
/// None). Dragging a colour picker then only repaints the keyboard instead
/// of also re-sending fans, GPU clocks and screen settings each time. Keeps
/// going when one part fails so that, say, a lighting error never stops the
/// fans from being set.
pub fn apply_changes(hw: &mut dyn Backend, old: Option<&Settings>, s: &Settings) -> Vec<String> {
    let mut errors = Vec::new();
    let mut note = |what: &str, r: anyhow::Result<()>| {
        if let Err(e) = r {
            errors.push(format!("{what}: {e:#}"));
        }
    };
    let caps = hw.capabilities();
    let all = old.is_none();
    let lighting_changed = all || old.map(|o| o.lighting != s.lighting).unwrap_or(true);
    let fans_changed = all || old.map(|o| o.fans != s.fans).unwrap_or(true);
    let gpu_changed = all || old.map(|o| o.gpu != s.gpu).unwrap_or(true);

    if caps.lighting && lighting_changed {
        let l = &s.lighting;
        let payload = protocol::kb_backlight(l.effect, l.speed, l.brightness, l.direction, l.colour);
        note("lighting effect", hw.set_effect(payload));
        if l.effect == Effect::Static {
            for z in 0..4u8 {
                let c = if l.zone_on[z as usize] { l.zones[z as usize] } else { Rgb::default() };
                note("zone colour", hw.set_zone(z + 1, c));
            }
        }
    }

    if caps.fans && fans_changed {
        let f = &s.fans;
        for (fan, pct) in [(protocol::FAN_CPU, f.cpu_percent), (protocol::FAN_GPU, f.gpu_percent)] {
            match f.mode {
                FanMode::Auto | FanMode::Max => note("fan mode", hw.set_fan_mode(fan, f.mode)),
                FanMode::Custom => match pct {
                    Some(p) => {
                        note("fan mode", hw.set_fan_mode(fan, FanMode::Custom));
                        note("fan speed", hw.set_fan_speed(fan, p));
                    }
                    None => note("fan mode", hw.set_fan_mode(fan, FanMode::Auto)),
                },
            }
        }
    }

    if caps.gpu_modes && gpu_changed {
        note("GPU mode", hw.set_gpu(s.gpu));
        // Some models (the PH317-53 included) have no turbo LED and reject
        // the call, so it is best-effort and never reported as an error.
        let _ = hw.set_turbo_led(s.gpu == GpuMode::Turbo);
    }

    if caps.toggles {
        for t in Toggle::ALL {
            let changed = old.map(|o| o.toggles.get(t) != s.toggles.get(t)).unwrap_or(true);
            if let (Some(on), true) = (s.toggles.get(t), changed) {
                note("setting", hw.set_toggle(t, on));
            }
        }
    }
    errors
}

/// Fan modes the firmware should report for these settings.
pub fn expected_fans(s: &Settings) -> (FanMode, FanMode) {
    let f = &s.fans;
    let one = |p: Option<u8>| match f.mode {
        FanMode::Custom if p.is_none() => FanMode::Auto,
        m => m,
    };
    (one(f.cpu_percent), one(f.gpu_percent))
}

/// True when what the firmware reports no longer matches the settings.
pub fn drifted(s: &Settings, seen: &Observed) -> bool {
    if s.lighting.effect == Effect::Static {
        if let Some(z) = seen.zones {
            for (i, seen_colour) in z.iter().enumerate() {
                let want = if s.lighting.zone_on[i] { s.lighting.zones[i] } else { Rgb::default() };
                if *seen_colour != want {
                    return true;
                }
            }
        }
    }
    let (cpu, gpu) = expected_fans(s);
    if let (Some(c), Some(g)) = seen.fans {
        if c != cpu || g != gpu {
            return true;
        }
    }
    if let Some(g) = seen.gpu {
        if g != s.gpu {
            return true;
        }
    }
    for t in Toggle::ALL {
        if let (Some(want), Some(have)) = (s.toggles.get(t), seen.toggles.get(t)) {
            if want != have {
                return true;
            }
        }
    }
    // A driver update or reinstall drops clock offsets without touching
    // the firmware value.
    if let Some(core) = seen.gpu_core_offset {
        if core != protocol::gpu_offsets(s.gpu).0 {
            return true;
        }
    }
    false
}

/// Colour for the temperature effect: blue when cool, through green and
/// yellow, to red when hot.
pub fn temperature_colour(celsius: u32) -> Rgb {
    const STOPS: [(f32, (f32, f32, f32)); 5] = [
        (45.0, (0.0, 80.0, 255.0)),
        (60.0, (0.0, 255.0, 60.0)),
        (72.0, (255.0, 220.0, 0.0)),
        (82.0, (255.0, 90.0, 0.0)),
        (92.0, (255.0, 0.0, 0.0)),
    ];
    let t = celsius as f32;
    if t <= STOPS[0].0 {
        let (r, g, b) = STOPS[0].1;
        return Rgb { r: r as u8, g: g as u8, b: b as u8 };
    }
    for w in STOPS.windows(2) {
        let ((t0, c0), (t1, c1)) = (w[0], w[1]);
        if t <= t1 {
            let k = (t - t0) / (t1 - t0);
            let mix = |a: f32, b: f32| (a + (b - a) * k).round() as u8;
            return Rgb { r: mix(c0.0, c1.0), g: mix(c0.1, c1.1), b: mix(c0.2, c1.2) };
        }
    }
    Rgb { r: 255, g: 0, b: 0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temperature_colour_ends() {
        assert_eq!(temperature_colour(30), Rgb { r: 0, g: 80, b: 255 });
        assert_eq!(temperature_colour(99), Rgb { r: 255, g: 0, b: 0 });
        assert_eq!(temperature_colour(60), Rgb { r: 0, g: 255, b: 60 });
    }

    #[derive(Default)]
    struct Recorder {
        calls: Vec<&'static str>,
    }

    impl Backend for Recorder {
        fn name(&self) -> &'static str {
            "test"
        }
        fn capabilities(&self) -> backend::Capabilities {
            backend::Capabilities { lighting: true, fans: true, gpu_modes: true, sensors: true, toggles: true }
        }
        fn sensors(&mut self) -> backend::Sensors {
            Default::default()
        }
        fn set_effect(&mut self, _: [u8; 16]) -> anyhow::Result<()> {
            self.calls.push("effect");
            Ok(())
        }
        fn set_zone(&mut self, _: u8, _: Rgb) -> anyhow::Result<()> {
            self.calls.push("zone");
            Ok(())
        }
        fn set_fan_mode(&mut self, _: u64, _: FanMode) -> anyhow::Result<()> {
            self.calls.push("fan");
            Ok(())
        }
        fn set_fan_speed(&mut self, _: u64, _: u8) -> anyhow::Result<()> {
            self.calls.push("fan");
            Ok(())
        }
        fn set_gpu(&mut self, _: GpuMode) -> anyhow::Result<()> {
            self.calls.push("gpu");
            Ok(())
        }
        fn set_turbo_led(&mut self, _: bool) -> anyhow::Result<()> {
            Ok(())
        }
        fn set_toggle(&mut self, _: Toggle, _: bool) -> anyhow::Result<()> {
            self.calls.push("toggle");
            Ok(())
        }
        fn observe(&mut self) -> Observed {
            Observed::default()
        }
    }

    #[test]
    fn colour_change_only_repaints_keyboard() {
        let old = Settings { toggles: settings::Toggles { lcd_overdrive: Some(true), ..Default::default() }, ..Default::default() };
        let mut new = old.clone();
        new.lighting.zones[0] = Rgb { r: 1, g: 2, b: 3 };
        let mut hw = Recorder::default();
        apply_changes(&mut hw, Some(&old), &new);
        assert!(hw.calls.iter().all(|c| *c == "effect" || *c == "zone"), "{:?}", hw.calls);

        let mut hw = Recorder::default();
        apply(&mut hw, &old);
        assert!(hw.calls.contains(&"gpu") && hw.calls.contains(&"toggle") && hw.calls.contains(&"fan"));
    }

    #[test]
    fn drift_detects_reset_colours() {
        let s = Settings::default();
        let mut seen = Observed::default();
        seen.zones = Some([Rgb { r: 0xFF, g: 0xA0, b: 0 }; 4]);
        assert!(drifted(&s, &seen));
        seen.zones = Some(s.lighting.zones);
        seen.fans = (Some(FanMode::Auto), Some(FanMode::Auto));
        seen.gpu = Some(GpuMode::Normal);
        assert!(!drifted(&s, &seen));
    }
}
