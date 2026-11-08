//! Everything that talks to an Acer gaming laptop's firmware, shared by
//! Windows and Linux. Only the way a firmware method is called differs:
//! WMI on Windows, the OpenSenses kernel driver on Linux (see `Transport`).
//! GPU clock offsets go through NVIDIA's driver: NVAPI on Windows, NVML on
//! Linux (see `GpuClocks`).

use super::{Backend, Capabilities, GpuLevel, Observed, Sensors};
use crate::models::{self, Model};
use crate::protocol::{self as p, Iface, Input};
use crate::settings::{Fan, FanMode, GpuMode, Rgb, Toggle, Toggles};
use crate::snapshot::{Snapshot, Source};
use anyhow::{anyhow, bail, Result};

/// Calls one firmware method and returns its output as a number.
pub trait Transport {
    fn name(&self) -> &'static str;
    fn call(&self, iface: Iface, method: &str, input: Input) -> Result<u64>;
    /// For the one method that returns a byte array (`GetGamingKBBacklight`).
    fn call_bytes(&self, iface: Iface, method: &str, input: Input) -> Result<Vec<u8>>;
    /// Whether the `APGeAction` interface exists on this laptop.
    fn has_apge(&self) -> bool;
}

/// NVIDIA GPU clock offsets for the overclock levels.
pub trait GpuClocks {
    /// PCI device id (e.g. 0x1F10) and subsystem id (e.g. 0x13421025).
    fn pci_ids(&self) -> Result<(u16, u32)>;
    /// Current graphics clock offset in MHz.
    fn core_offset(&self) -> Result<i32>;
    /// Current memory clock offset, in NVAPI's MHz.
    fn memory_offset(&self) -> Result<i32>;
    /// Sets graphics and memory clock offsets, in PredatorSense's (NVAPI's) MHz.
    fn set_offsets(&self, core_mhz: i32, memory_mhz: i32) -> Result<()>;
}

pub struct AcerBackend<T: Transport> {
    t: T,
    gpu: std::result::Result<Box<dyn GpuClocks>, String>,
    model: Option<&'static Model>,
    caps: Capabilities,
    /// The per-key keyboard's controller, opened when first needed.
    kb: Option<crate::perkey::Keyboard>,
}

impl<T: Transport> AcerBackend<T> {
    /// `product` is the BIOS product name, `family` the BIOS family name
    /// (shown for unknown models). `tested` says whether this OS has been
    /// tried on the tested laptop; when not, everything is experimental.
    /// `cpu` is the processor name ("Intel(R) Core(TM) i9-8950HK CPU ...").
    pub fn new(t: T, gpu: std::result::Result<Box<dyn GpuClocks>, String>, product: String, family: Option<String>, cpu: &str, tested: bool) -> Self {
        let model = models::lookup(&product);
        let mut me = Self {
            t,
            gpu,
            model,
            caps: Capabilities::default(),
            kb: None,
        };
        let mut caps = me.detect(product, family);
        if let Some(m) = model {
            // PredatorSense offered CPU overclocking only on the unlocked HK
            // processors; on the others the CPU power level follows the GPU.
            let unlocked = cpu.to_ascii_uppercase().split_whitespace().any(|w| w.ends_with("HK"));
            if m.cpu_to_firmware && unlocked && !m.cpu_oc_levels.is_empty() {
                caps.cpu_levels = m.cpu_oc_levels.to_vec();
            }
            caps.cpu_follows_gpu = m.cpu_to_firmware && caps.cpu_levels.is_empty() && !caps.gpu_levels.is_empty();
            caps.fan_table = !caps.gpu_levels.is_empty() || !caps.cpu_levels.is_empty();
            caps.turbo_key = m.machine_type != 0 && !caps.gpu_levels.is_empty();
            caps.hotkey_keys = m.hotkey_keys;
            // The Helios 300 / Triton 300 family has no PredatorSense key.
            caps.ps_key = m.machine_type != 1;
            if m.per_key {
                // Helios 700 (MachineType 4) / Triton 900 (3).
                caps.heat_pipe_light = m.machine_type == 4;
                caps.fan_light = m.machine_type == 3;
                caps.touchpad_light = if m.machine_type == 3 { 2 } else { 1 };
                match me.keyboard() {
                    Some(kb) => caps.per_key_layout = kb.layout(),
                    None => {
                        log::warn!("per-key keyboard controller not found");
                        caps.notice
                            .get_or_insert_with(|| "The per-key keyboard's controller wasn't found, so keyboard lighting can't be changed.".into());
                    }
                }
            }
        }
        me.caps = if tested { caps } else { caps.all_experimental().with_experimental("games") };
        log::info!("hardware: {:?}", me.caps);
        me
    }

    fn call(&self, method: &str, input: Input) -> Result<u64> {
        self.t.call(Iface::Gaming, method, input)
    }

    fn apge(&self, method: &str, input: Input) -> Result<u64> {
        if !self.t.has_apge() {
            bail!("APGeAction is not available on this laptop");
        }
        self.t.call(Iface::Apge, method, input)
    }

    /// Set* methods report failure in the low byte of their output.
    fn set(&self, method: &str, input: Input) -> Result<()> {
        let out = self.call(method, input)?;
        if out & 0xFF != 0 {
            bail!("{method} returned status 0x{:X}", out & 0xFF);
        }
        Ok(())
    }

    fn sensor(&self, id: u32) -> Option<u32> {
        self.call("GetGamingSysInfo", Input::U32(p::sensor_query(id))).ok().and_then(p::decode_sensor)
    }

    /// Works out what this laptop supports: from the model table when the
    /// model is known, otherwise by asking the firmware what answers.
    fn detect(&self, product: String, family: Option<String>) -> Capabilities {
        let gpu_ids = self.gpu.as_ref().ok().and_then(|g| g.pci_ids().ok());
        let table = gpu_ids.and_then(|(dev, sub)| models::gpu_table(self.model, dev, sub));
        let levels = |modes: &[GpuMode]| -> Vec<GpuLevel> {
            match table {
                Some(t) => modes
                    .iter()
                    .zip(t.core.iter().zip(t.memory.iter()))
                    .map(|(&mode, (&core_mhz, &memory_mhz))| GpuLevel { mode, core_mhz, memory_mhz })
                    .collect(),
                None => Vec::new(),
            }
        };
        let apge = self.t.has_apge();

        if let Some(m) = self.model {
            let mut caps = Capabilities {
                product,
                model_name: Some(m.name.to_string()),
                known_model: true,
                lighting: m.four_zone,
                per_key: m.per_key,
                fans: m.fans.to_vec(),
                gpu_levels: levels(m.gpu_levels),
                sensors: true,
                // PredatorSense showed CoolBoost on every one of these models'
                // fan pages; both it and the backlight timeout need APGeAction.
                toggles: m
                    .toggles
                    .iter()
                    .copied()
                    .chain((!m.fans.is_empty()).then_some(Toggle::CoolBoost))
                    .filter(|t| !matches!(t, Toggle::BacklightTimeout | Toggle::CoolBoost) || apge || (m.per_key && *t == Toggle::BacklightTimeout))
                    .collect(),
                sticky_keys: true,
                coming: super::coming_features(m),
                experimental: Vec::new(),
                notice: None,
                ..Default::default()
            };
            if m.product != models::TESTED_PRODUCT {
                return caps.all_experimental();
            }
            // Same laptop, different GPU: its overclock numbers are untested.
            if gpu_ids != Some(models::TESTED_GPU) {
                caps.experimental.push("gpu".into());
            }
            return caps;
        }

        // Unknown model: keep only what the firmware answers to.
        let ok = |r: Result<u64>| r.map(|v| v & 0xFF == 0).unwrap_or(false);
        let lighting = ok(self.call("GetGamingRgbKb", Input::U32(1)));
        let mut fans = Vec::new();
        if ok(self.call("GetGamingFanBehavior", Input::U32(p::fan_query(&[Fan::Cpu, Fan::Gpu])))) {
            fans.extend([Fan::Cpu, Fan::Gpu]);
        }
        let mut toggles = Vec::new();
        if self.read_backlight().is_some() {
            toggles.push(Toggle::BacklightTimeout);
        }
        if !fans.is_empty() && self.read_cool_boost().is_some() {
            toggles.push(Toggle::CoolBoost);
        }
        if self.call("GetGamingProfile", Input::U32(0)).ok().and_then(p::decode_profile).is_some() {
            toggles.extend([Toggle::LcdOverdrive, Toggle::WinKey]);
        }
        if self
            .call("GetGamingMiscSetting", Input::U32(p::MISC_GPU_MODE as u32))
            .ok()
            .and_then(p::decode_discrete_gpu)
            .is_some()
        {
            toggles.push(Toggle::DiscreteGpu);
        }
        if self
            .call("GetGamingMiscSetting", Input::U32(p::MISC_BOOT_ANIMATION as u32))
            .ok()
            .and_then(p::decode_misc_flag)
            .is_some()
        {
            toggles.push(Toggle::BootAnimation);
        }
        let sensors = self.sensor(p::SENSOR_CPU_TEMP).is_some();
        Capabilities {
            product,
            model_name: family.filter(|s| !s.is_empty()),
            known_model: false,
            lighting,
            per_key: false,
            fans,
            // Only GPUs PredatorSense had numbers for; the firmware value is
            // skipped since this model's convention is unknown.
            gpu_levels: levels(&[GpuMode::Normal, GpuMode::Faster, GpuMode::Turbo]),
            sensors,
            toggles,
            sticky_keys: true,
            coming: Vec::new(),
            experimental: Vec::new(),
            notice: None,
            ..Default::default()
        }
        .all_experimental()
    }

    fn keyboard(&mut self) -> Option<&mut crate::perkey::Keyboard> {
        if self.kb.is_none() && self.model.map(|m| m.per_key).unwrap_or(false) {
            match crate::perkey::Keyboard::open() {
                Ok(k) => self.kb = Some(k),
                Err(e) => log::debug!("per-key keyboard: {e:#}"),
            }
        }
        self.kb.as_mut()
    }

    /// Runs `f` on the keyboard controller, opening it again once if the
    /// first try fails (it may have been re-enumerated after sleep).
    fn with_keyboard<R>(&mut self, f: impl Fn(&mut crate::perkey::Keyboard) -> Result<R>) -> Result<R> {
        if let Some(kb) = self.keyboard() {
            if let Ok(r) = f(kb) {
                return Ok(r);
            }
        }
        self.kb = None;
        let kb = self.keyboard().ok_or_else(|| anyhow!("per-key keyboard controller not found"))?;
        f(kb)
    }

    fn per_key(&self) -> bool {
        self.model.map(|m| m.per_key).unwrap_or(false)
    }

    fn read_backlight(&self) -> Option<bool> {
        self.apge("GetFunction", Input::U32(p::BACKLIGHT_TIMEOUT_GET))
            .ok()
            .and_then(p::decode_backlight_timeout)
    }

    fn read_cool_boost(&self) -> Option<bool> {
        self.apge("GetFunction", Input::U32(p::COOL_BOOST_GET)).ok().and_then(p::decode_cool_boost)
    }

    fn gpu_to_firmware(&self) -> bool {
        self.model.map(|m| m.gpu_level_to_firmware).unwrap_or(false)
    }

    fn read_toggles(&self) -> Toggles {
        let profile = self.call("GetGamingProfile", Input::U32(0)).ok().and_then(p::decode_profile);
        let boot = self
            .call("GetGamingMiscSetting", Input::U32(p::MISC_BOOT_ANIMATION as u32))
            .ok()
            .and_then(p::decode_misc_flag);
        let has = |t| self.caps.has_toggle(t);
        let discrete = if has(Toggle::DiscreteGpu) {
            self.call("GetGamingMiscSetting", Input::U32(p::MISC_GPU_MODE as u32))
                .ok()
                .and_then(p::decode_discrete_gpu)
        } else {
            None
        };
        Toggles {
            lcd_overdrive: profile.map(|(lcd, _)| lcd).filter(|_| has(Toggle::LcdOverdrive)),
            win_key: profile.map(|(_, win)| win).filter(|_| has(Toggle::WinKey)),
            boot_animation: boot.filter(|_| has(Toggle::BootAnimation)),
            // On per-key models the timeout lives in the keyboard controller.
            backlight_timeout: if has(Toggle::BacklightTimeout) && !self.per_key() {
                self.read_backlight()
            } else {
                None
            },
            discrete_gpu: discrete,
            cool_boost: if has(Toggle::CoolBoost) { self.read_cool_boost() } else { None },
        }
    }
}

impl<T: Transport> Backend for AcerBackend<T> {
    fn name(&self) -> &'static str {
        self.t.name()
    }

    fn capabilities(&self) -> Capabilities {
        self.caps.clone()
    }

    fn sensors(&mut self) -> Sensors {
        Sensors {
            cpu_temp: self.sensor(p::SENSOR_CPU_TEMP),
            gpu_temp: self.sensor(p::SENSOR_GPU_TEMP),
            sys_temp: self.sensor(p::SENSOR_SYS_TEMP),
            cpu_fan_rpm: self.sensor(p::SENSOR_CPU_FAN),
            gpu_fan_rpm: self.sensor(p::SENSOR_GPU_FAN),
        }
    }

    fn set_effect(&mut self, payload: [u8; 16]) -> Result<()> {
        self.set("SetGamingKBBacklight", Input::Bytes(payload))
    }

    fn set_zone(&mut self, zone: u8, colour: Rgb) -> Result<()> {
        // SetGamingRgbKb returns uint64; status is still the low byte.
        self.set("SetGamingRgbKb", Input::U32(p::rgb_zone(zone, colour)))
    }

    fn set_fan_mode(&mut self, fan: Fan, mode: FanMode) -> Result<()> {
        self.set("SetGamingFanBehavior", Input::U64(p::fan_behavior(fan, mode)))
    }

    fn set_fan_speed(&mut self, fan: Fan, percent: u8) -> Result<()> {
        self.set("SetGamingFanSpeed", Input::U64(p::fan_speed(fan, percent)))
    }

    fn set_gpu(&mut self, mode: GpuMode) -> Result<()> {
        let (index, level) = self.caps.gpu_level(mode).ok_or_else(|| anyhow!("this GPU has no {mode:?} level"))?;
        // The firmware value tells the laptop which level is active (on the
        // families where PredatorSense sends it); the actual clock offsets
        // go through the NVIDIA driver.
        if self.gpu_to_firmware() {
            self.set("SetGamingMiscSetting", Input::U64(p::gpu_oc(index)))?;
        }
        match &self.gpu {
            Ok(g) => g.set_offsets(level.core_mhz, level.memory_mhz),
            Err(e) if level.core_mhz == 0 && level.memory_mhz == 0 => {
                log::info!("NVIDIA driver interface unavailable ({e}); nothing to reset");
                Ok(())
            }
            Err(e) => bail!("GPU clock offsets unavailable: {e}"),
        }
    }

    fn set_cpu_level(&mut self, index: usize) -> Result<()> {
        if !self.model.map(|m| m.cpu_to_firmware).unwrap_or(false) || !(self.caps.cpu_follows_gpu || !self.caps.cpu_levels.is_empty()) {
            return Ok(());
        }
        let out = self.call("SetGamingMiscSetting", Input::U64(p::cpu_oc(index)))?;
        if out & 0xFF != 0 {
            // Some BIOS versions don't take the CPU level (the PH317-53
            // answers 2, "not supported"); then there's nothing to show.
            log::info!("this BIOS doesn't take a CPU level (status 0x{:X}); leaving the CPU alone", out & 0xFF);
            self.caps.cpu_follows_gpu = false;
            self.caps.cpu_levels.clear();
        }
        Ok(())
    }

    fn set_fan_table(&mut self, curve: u8) -> Result<()> {
        // Read first, as PredatorSense does: sleep or the firmware may have
        // changed it.
        if self.call("GetGamingFanTable", Input::U32(0)).ok().and_then(p::decode_fan_table) == Some(curve) {
            return Ok(());
        }
        self.set("SetGamingFanTable", Input::U64(curve as u64))
    }

    fn hotkey_group(&mut self) -> Option<u8> {
        let v = self.call("GetGamingProfile", Input::U32(0)).ok().filter(|v| v & 0xFF == 0)?;
        Some(((v >> 32) & 0xFF) as u8).filter(|g| (1..=3).contains(g))
    }

    fn set_turbo_led(&mut self, on: bool) -> Result<()> {
        let v = if on { p::TURBO_LED_ON } else { p::TURBO_LED_OFF };
        self.set("SetGamingLED", Input::U64(v))
    }

    fn set_per_key(&mut self, l: &crate::settings::Lighting, old: Option<&crate::settings::Lighting>) -> Result<()> {
        if !self.per_key() {
            return Ok(());
        }
        let product = self.caps.product.clone();
        self.with_keyboard(|kb| kb.apply(&product, l, old))
    }

    fn set_extra_lights(&mut self, e: &crate::settings::ExtraLights, touchpad: Option<Rgb>) -> Vec<String> {
        let mut errors = Vec::new();
        let colour = |group: u64, c: Rgb| group | (c.r as u64) << 8 | (c.g as u64) << 16 | (c.b as u64) << 24;
        let mut send = |me: &Self, what: &str, method: &str, v: u64| {
            if let Err(e) = me.set(method, Input::U64(v)) {
                errors.push(format!("{what}: {e:#}"));
            }
        };
        if self.caps.heat_pipe_light {
            if let Some(c) = e.heat_pipe_colour {
                send(self, "heat pipe light", "SetGamingLEDColor", colour(7, c));
            }
            if let Some(on) = e.heat_pipe {
                // Bit 29 keeps the start-up / Turbo-key light show on, as PredatorSense's default.
                send(self, "heat pipe light", "SetGamingLEDBehavior", 0x40 | (on as u64) << 28 | 1 << 29);
            }
        }
        if self.caps.fan_light {
            if let Some(c) = e.fan_led_colour {
                send(self, "fan light", "SetGamingLEDColor", colour(6, c));
            }
            if let Some(on) = e.fan_led {
                send(self, "fan light", "SetGamingLEDBehavior", 0x20 | (on as u64) << 26);
            }
        }
        if self.caps.touchpad_light > 0 {
            if let (Some(c), 2) = (touchpad, self.caps.touchpad_light) {
                send(self, "touchpad light", "SetGamingLEDColor", colour(8, c));
            }
            if let Some(on) = e.touchpad {
                send(self, "touchpad light", "SetGamingLEDBehavior", 0x80 | (on as u64) << 30);
            }
        }
        errors
    }

    fn set_toggle(&mut self, t: Toggle, on: bool) -> Result<()> {
        if self.per_key() {
            match t {
                Toggle::WinKey => return self.with_keyboard(|kb| kb.set_win_key(on)),
                Toggle::BacklightTimeout => return self.with_keyboard(|kb| kb.set_timeout(on)),
                _ => {}
            }
        }
        match t {
            Toggle::LcdOverdrive => self.set("SetGamingProfile", Input::U64(p::lcd_overdrive(on))),
            Toggle::WinKey => self.set("SetGamingProfile", Input::U64(p::win_key(on))),
            Toggle::BootAnimation => self.set("SetGamingMiscSetting", Input::U64(p::boot_animation(on))),
            Toggle::DiscreteGpu => self.set("SetGamingMiscSetting", Input::U64(p::discrete_gpu(on))),
            Toggle::CoolBoost => {
                let out = self.apge("SetFunction", Input::U64(p::cool_boost(on)))?;
                if out & 0xFF != 0 {
                    bail!("SetFunction returned status 0x{:X}", out & 0xFF);
                }
                Ok(())
            }
            Toggle::BacklightTimeout => {
                let read = |me: &Self| -> Result<u64> {
                    let v = me.apge("GetFunction", Input::U32(p::BACKLIGHT_TIMEOUT_GET))?;
                    if v & 0xFF != 0 {
                        bail!("GetFunction returned status 0x{:X}", v & 0xFF);
                    }
                    Ok(v)
                };
                let current = read(self)?;
                // The firmware flips this setting on every accepted call, so
                // only send when it actually differs.
                if p::decode_backlight_timeout(current) == Some(on) {
                    return Ok(());
                }
                let out = self.apge("SetFunction", Input::U64(p::backlight_timeout(current, on)))?;
                if out & 0xFF != 0 {
                    bail!("SetFunction returned status 0x{:X}", out & 0xFF);
                }
                if p::decode_backlight_timeout(read(self)?) != Some(on) {
                    bail!("keyboard backlight timeout didn't change");
                }
                Ok(())
            }
        }
    }

    fn observe(&mut self) -> Observed {
        let mut zones = [Rgb::default(); 4];
        let mut ok = true;
        for z in 1..=4u8 {
            match self.call("GetGamingRgbKb", Input::U32(p::zone_mask(z))).ok().and_then(p::decode_rgb_zone) {
                Some(c) => zones[(z - 1) as usize] = c,
                None => ok = false,
            }
        }
        let fans = match self.call("GetGamingFanBehavior", Input::U32(p::fan_query(&self.caps.fans))) {
            Ok(out) => self.caps.fans.iter().filter_map(|&f| p::decode_fan_behavior(out, f).map(|m| (f, m))).collect(),
            Err(_) => Vec::new(),
        };
        let gpu = if self.gpu_to_firmware() {
            self.call("GetGamingMiscSetting", Input::U32(p::MISC_GPU_OC as u32))
                .ok()
                .and_then(p::decode_gpu_oc)
                .and_then(|i| self.caps.gpu_levels.get(i).map(|l| l.mode))
        } else {
            None
        };
        let gpu_core_offset = self.gpu.as_ref().ok().and_then(|g| g.core_offset().ok());
        let toggles = self.read_toggles();
        Observed {
            zones: ok.then_some(zones),
            fans,
            gpu,
            gpu_core_offset,
            toggles,
        }
    }

    fn snapshot(&mut self) -> Option<Snapshot> {
        let seen = self.observe();
        let kb_effect = self
            .t
            .call_bytes(Iface::Gaming, "GetGamingKBBacklight", Input::U32(0))
            .ok()
            .filter(|b| p::decode_kb_backlight(b).is_some())
            .map(|b| b[..16.min(b.len())].to_vec());
        let fan_speeds = seen
            .fans
            .iter()
            .filter(|(_, m)| *m == FanMode::Custom)
            .filter_map(|&(f, _)| {
                self.call("GetGamingFanSpeed", Input::U32(p::fan_speed_query(f)))
                    .ok()
                    .and_then(p::decode_fan_speed)
                    .map(|s| (f, s))
            })
            .collect();
        let gpu_level = if self.gpu_to_firmware() {
            self.call("GetGamingMiscSetting", Input::U32(p::MISC_GPU_OC as u32))
                .ok()
                .filter(|v| v & 0xFF == 0)
                .map(|v| ((v >> 8) & 0xFF) as u8)
        } else {
            None
        };
        let (gpu_core, gpu_memory) = match &self.gpu {
            Ok(g) if !self.caps.gpu_levels.is_empty() => (g.core_offset().ok(), g.memory_offset().ok()),
            _ => (None, None),
        };
        let turbo_led = self.call("GetGamingLED", Input::U32(1)).ok().filter(|v| v & 0xFF == 0).map(|v| v & 0xFF00 != 0);
        let fan_curve = if self.caps.fan_table {
            self.call("GetGamingFanTable", Input::U32(0)).ok().and_then(p::decode_fan_table)
        } else {
            None
        };
        let cpu_level = self.model.filter(|m| m.cpu_to_firmware).map(|_| 0);
        Some(Snapshot {
            taken: crate::snapshot::now(),
            source: Source::Laptop,
            product: self.caps.product.clone(),
            kb_effect,
            zones: seen.zones.filter(|_| self.caps.lighting),
            fans: seen.fans,
            fan_speeds,
            gpu_level,
            gpu_core,
            gpu_memory,
            turbo_led,
            fan_curve,
            cpu_level,
            toggles: seen.toggles,
        })
    }

    fn factory_snapshot(&self) -> Option<Snapshot> {
        self.model.map(|m| Snapshot::factory(m, &self.caps.toggles))
    }

    fn restore(&mut self, s: &Snapshot) -> Vec<String> {
        let mut errors = Vec::new();
        let mut note = |what: &str, r: Result<()>| {
            if let Err(e) = r {
                errors.push(format!("{what}: {e:#}"));
            }
        };
        if self.caps.lighting {
            let fx = s.kb_effect.as_deref().and_then(|b| <[u8; 16]>::try_from(&b[..16.min(b.len())]).ok());
            if let Some(fx) = fx {
                note("lighting effect", self.set_effect(fx));
            }
            // Zone colours only show (and only make sense to send) with the static effect.
            if fx.map(|f| f[0] == 0).unwrap_or(true) {
                if let Some(z) = s.zones {
                    for (i, c) in z.iter().enumerate() {
                        note("zone colour", self.set_zone(i as u8 + 1, *c));
                    }
                }
            }
        }
        for &(fan, mode) in &s.fans {
            if !self.caps.fans.contains(&fan) {
                continue;
            }
            let speed = s.fan_speeds.iter().find(|(f, _)| *f == fan).map(|(_, p)| *p);
            match (mode, speed) {
                (FanMode::Custom, Some(pct)) => {
                    note("fan mode", self.set_fan_mode(fan, FanMode::Custom));
                    note("fan speed", self.set_fan_speed(fan, pct));
                }
                (FanMode::Custom, None) => note("fan mode", self.set_fan_mode(fan, FanMode::Auto)),
                (m, _) => note("fan mode", self.set_fan_mode(fan, m)),
            }
        }
        if let (Some(level), true) = (s.gpu_level, self.gpu_to_firmware()) {
            note("GPU level", self.set("SetGamingMiscSetting", Input::U64(p::MISC_GPU_OC | (level as u64) << 8)));
        }
        if s.gpu_core.is_some() || s.gpu_memory.is_some() {
            if let Ok(g) = &self.gpu {
                note("GPU clocks", g.set_offsets(s.gpu_core.unwrap_or(0), s.gpu_memory.unwrap_or(0)));
            }
        }
        if let Some(on) = s.turbo_led {
            let _ = self.set_turbo_led(on);
        }
        if let Some(level) = s.cpu_level {
            note("CPU level", self.set_cpu_level(level as usize));
        }
        if let (Some(curve), true) = (s.fan_curve, self.caps.fan_table) {
            note("fan curve", self.set_fan_table(curve));
        }
        for t in self.caps.toggles.clone() {
            if let Some(on) = s.toggles.get(t) {
                note("setting", self.set_toggle(t, on));
            }
        }
        errors
    }

    fn diagnostics(&mut self) -> Vec<String> {
        let mut out = Vec::new();
        let raw = |r: Result<u64>| r.map(|v| format!("0x{v:X}")).unwrap_or_else(|e| format!("error: {e:#}"));
        out.push(format!("GetGamingProfile(0) = {}", raw(self.call("GetGamingProfile", Input::U32(0)))));
        out.push(format!("GetGamingMiscSetting(6) = {}", raw(self.call("GetGamingMiscSetting", Input::U32(6)))));
        out.push(format!("GetGamingLED(1) = {}", raw(self.call("GetGamingLED", Input::U32(1)))));
        if self.t.has_apge() {
            out.push(format!(
                "APGeAction.GetFunction(0x88401) = {}",
                raw(self.apge("GetFunction", Input::U32(p::BACKLIGHT_TIMEOUT_GET)))
            ));
            out.push(format!(
                "APGeAction.GetFunction(0x207) = {}",
                raw(self.apge("GetFunction", Input::U32(p::COOL_BOOST_GET)))
            ));
        }
        let kb = self.t.call_bytes(Iface::Gaming, "GetGamingKBBacklight", Input::U32(0));
        out.push(format!("GetGamingKBBacklight(0) = {kb:02X?}"));
        match &self.gpu {
            Ok(g) => out.push(format!(
                "GPU ids = {:?}, core offset = {:?}, memory offset = {:?}",
                g.pci_ids().ok(),
                g.core_offset().ok(),
                g.memory_offset().ok()
            )),
            Err(e) => out.push(format!("GPU clocks unavailable: {e}")),
        }
        out
    }
}
