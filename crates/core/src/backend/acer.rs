//! Everything that talks to an Acer gaming laptop's firmware, shared by
//! Windows and Linux. Only the way a firmware method is called differs:
//! WMI on Windows, the OpenSenses kernel driver on Linux (see `Transport`).
//! GPU clock offsets go through NVIDIA's driver: NVAPI on Windows, NVML on
//! Linux (see `GpuClocks`).

use super::{Backend, Capabilities, GpuLevel, Observed, Sensors};
use crate::models::{self, Model};
use crate::protocol::{self as p, Iface, Input};
use crate::settings::{Fan, FanMode, GpuMode, Rgb, Toggle, Toggles};
use anyhow::{anyhow, bail, Result};

/// Calls one firmware method and returns its output as a number.
pub trait Transport {
    fn name(&self) -> &'static str;
    fn call(&self, iface: Iface, method: &str, input: Input) -> Result<u64>;
    /// Whether the `APGeAction` interface exists on this laptop.
    fn has_apge(&self) -> bool;
}

/// NVIDIA GPU clock offsets for the overclock levels.
pub trait GpuClocks {
    /// PCI device id (e.g. 0x1F10) and subsystem id (e.g. 0x13421025).
    fn pci_ids(&self) -> Result<(u16, u32)>;
    /// Current graphics clock offset in MHz.
    fn core_offset(&self) -> Result<i32>;
    /// Sets graphics and memory clock offsets, in PredatorSense's (NVAPI's) MHz.
    fn set_offsets(&self, core_mhz: i32, memory_mhz: i32) -> Result<()>;
}

pub struct AcerBackend<T: Transport> {
    t: T,
    gpu: std::result::Result<Box<dyn GpuClocks>, String>,
    model: Option<&'static Model>,
    caps: Capabilities,
}

impl<T: Transport> AcerBackend<T> {
    /// `product` is the BIOS product name, `family` the BIOS family name
    /// (shown for unknown models). `tested` says whether this OS has been
    /// tried on the tested laptop; when not, everything is experimental.
    pub fn new(t: T, gpu: std::result::Result<Box<dyn GpuClocks>, String>, product: String, family: Option<String>, tested: bool) -> Self {
        let model = models::lookup(&product);
        let mut me = Self { t, gpu, model, caps: Capabilities::default() };
        let caps = me.detect(product, family);
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
                    .filter(|t| !matches!(t, Toggle::BacklightTimeout | Toggle::CoolBoost) || apge)
                    .collect(),
                sticky_keys: true,
                coming: super::coming_features(m),
                experimental: Vec::new(),
                notice: None,
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
        if self.call("GetGamingMiscSetting", Input::U32(p::MISC_GPU_MODE as u32)).ok().and_then(p::decode_discrete_gpu).is_some() {
            toggles.push(Toggle::DiscreteGpu);
        }
        if self.call("GetGamingMiscSetting", Input::U32(p::MISC_BOOT_ANIMATION as u32)).ok().and_then(p::decode_misc_flag).is_some() {
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
        }
        .all_experimental()
    }

    fn read_backlight(&self) -> Option<bool> {
        self.apge("GetFunction", Input::U32(p::BACKLIGHT_TIMEOUT_GET)).ok().and_then(p::decode_backlight_timeout)
    }

    fn read_cool_boost(&self) -> Option<bool> {
        self.apge("GetFunction", Input::U32(p::COOL_BOOST_GET)).ok().and_then(p::decode_cool_boost)
    }

    fn gpu_to_firmware(&self) -> bool {
        self.model.map(|m| m.gpu_level_to_firmware).unwrap_or(false)
    }

    fn read_toggles(&self) -> Toggles {
        let profile = self.call("GetGamingProfile", Input::U32(0)).ok().and_then(p::decode_profile);
        let boot = self.call("GetGamingMiscSetting", Input::U32(p::MISC_BOOT_ANIMATION as u32)).ok().and_then(p::decode_misc_flag);
        let has = |t| self.caps.has_toggle(t);
        let discrete = if has(Toggle::DiscreteGpu) {
            self.call("GetGamingMiscSetting", Input::U32(p::MISC_GPU_MODE as u32)).ok().and_then(p::decode_discrete_gpu)
        } else {
            None
        };
        Toggles {
            lcd_overdrive: profile.map(|(lcd, _)| lcd).filter(|_| has(Toggle::LcdOverdrive)),
            win_key: profile.map(|(_, win)| win).filter(|_| has(Toggle::WinKey)),
            boot_animation: boot.filter(|_| has(Toggle::BootAnimation)),
            backlight_timeout: if has(Toggle::BacklightTimeout) { self.read_backlight() } else { None },
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

    fn set_turbo_led(&mut self, on: bool) -> Result<()> {
        let v = if on { p::TURBO_LED_ON } else { p::TURBO_LED_OFF };
        self.set("SetGamingLED", Input::U64(v))
    }

    fn set_toggle(&mut self, t: Toggle, on: bool) -> Result<()> {
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
        Observed { zones: ok.then_some(zones), fans, gpu, gpu_core_offset, toggles }
    }

    fn diagnostics(&mut self) -> Vec<String> {
        let mut out = Vec::new();
        let raw = |r: Result<u64>| r.map(|v| format!("0x{v:X}")).unwrap_or_else(|e| format!("error: {e:#}"));
        out.push(format!("GetGamingProfile(0) = {}", raw(self.call("GetGamingProfile", Input::U32(0)))));
        out.push(format!("GetGamingMiscSetting(6) = {}", raw(self.call("GetGamingMiscSetting", Input::U32(6)))));
        out.push(format!("GetGamingLED(1) = {}", raw(self.call("GetGamingLED", Input::U32(1)))));
        if self.t.has_apge() {
            out.push(format!("APGeAction.GetFunction(0x88401) = {}", raw(self.apge("GetFunction", Input::U32(p::BACKLIGHT_TIMEOUT_GET)))));
            out.push(format!("APGeAction.GetFunction(0x207) = {}", raw(self.apge("GetFunction", Input::U32(p::COOL_BOOST_GET)))));
        }
        match &self.gpu {
            Ok(g) => out.push(format!("GPU ids = {:?}, core offset = {:?}", g.pci_ids().ok(), g.core_offset().ok())),
            Err(e) => out.push(format!("GPU clocks unavailable: {e}")),
        }
        out
    }
}
