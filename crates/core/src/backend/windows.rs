//! Talks to the firmware through WMI classes in the `root\WMI` namespace:
//! `AcerGamingFunction` for nearly everything, and `APGeAction` for the
//! keyboard backlight timeout. Needs administrator / SYSTEM rights. COM
//! objects here are not thread-safe, so the backend must stay on the thread
//! that opened it.

use super::{Backend, Capabilities, GpuLevel, Observed, Sensors};
use crate::models::{self, Model};
use crate::protocol as p;
use crate::settings::{Fan, FanMode, GpuMode, Rgb, Toggle, Toggles};
use anyhow::{anyhow, bail, Context, Result};
use windows::core::{BSTR, PCWSTR, VARIANT};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoInitializeSecurity, CoSetProxyBlanket, CLSCTX_INPROC_SERVER,
    COINIT_MULTITHREADED, EOAC_NONE, RPC_C_AUTHN_LEVEL_CALL, RPC_C_AUTHN_LEVEL_DEFAULT, RPC_C_IMP_LEVEL_IMPERSONATE,
};
use windows::Win32::System::Rpc::{RPC_C_AUTHN_WINNT, RPC_C_AUTHZ_NONE};
use windows::Win32::System::Variant::{InitVariantFromBuffer, VT_BSTR};
use windows::Win32::System::Wmi::{
    IWbemClassObject, IWbemLocator, IWbemServices, WbemLocator, WBEM_FLAG_FORWARD_ONLY, WBEM_FLAG_RETURN_IMMEDIATELY,
    WBEM_GENERIC_FLAG_TYPE, WBEM_INFINITE,
};

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

enum Input {
    U32(u32),
    U64(u64),
    Bytes([u8; 16]),
}

/// One WMI class with its single instance and the names of its in/out parameters.
struct WmiClass {
    name: &'static str,
    class: IWbemClassObject,
    path: BSTR,
    input: &'static str,
    output: &'static str,
}

pub struct WmiBackend {
    svc: IWbemServices,
    gaming: WmiClass,
    apge: Option<WmiClass>,
    nvapi: std::result::Result<super::nvapi::Nvapi, String>,
    model: Option<&'static Model>,
    caps: Capabilities,
}

/// A string value under HKLM\HARDWARE\DESCRIPTION\System\BIOS.
fn bios_value(name: &str) -> Option<String> {
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ};
    let key = wide(r"HARDWARE\DESCRIPTION\System\BIOS");
    let val = wide(name);
    let mut buf = [0u16; 256];
    let mut size = (buf.len() * 2) as u32;
    let st = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            PCWSTR(key.as_ptr()),
            PCWSTR(val.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr() as *mut _),
            Some(&mut size),
        )
    };
    if st.is_err() {
        return None;
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Some(String::from_utf16_lossy(&buf[..len]).trim().to_string())
}

unsafe fn open_class(svc: &IWbemServices, name: &'static str, input: &'static str, output: &'static str) -> Result<WmiClass> {
    let mut class = None;
    svc.GetObject(&BSTR::from(name), WBEM_GENERIC_FLAG_TYPE(0), None, Some(&mut class), None)
        .with_context(|| format!("{name} class not found (not an Acer gaming laptop?)"))?;
    let class = class.ok_or_else(|| anyhow!("no {name} class object"))?;

    let query = svc.ExecQuery(
        &BSTR::from("WQL"),
        &BSTR::from(format!("SELECT * FROM {name}")),
        WBEM_GENERIC_FLAG_TYPE(WBEM_FLAG_FORWARD_ONLY.0 | WBEM_FLAG_RETURN_IMMEDIATELY.0),
        None,
    )?;
    let mut objs: [Option<IWbemClassObject>; 1] = [None];
    let mut got = 0u32;
    query.Next(WBEM_INFINITE, &mut objs, &mut got).ok()?;
    let inst = objs[0].take().ok_or_else(|| anyhow!("no {name} instance (run as administrator)"))?;
    let mut v = VARIANT::default();
    inst.Get(PCWSTR(wide("__PATH").as_ptr()), 0, &mut v, None, None)?;
    let path = BSTR::try_from(&v).map_err(|e| anyhow!("{name} instance path: {e}"))?;
    Ok(WmiClass { name, class, path, input, output })
}

impl WmiBackend {
    pub fn open() -> Result<Self> {
        unsafe {
            // S_FALSE / RPC_E_CHANGED_MODE just mean COM is already set up on this thread.
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let _ = CoInitializeSecurity(
                None,
                -1,
                None,
                None,
                RPC_C_AUTHN_LEVEL_DEFAULT,
                RPC_C_IMP_LEVEL_IMPERSONATE,
                None,
                EOAC_NONE,
                None,
            );
            let locator: IWbemLocator = CoCreateInstance(&WbemLocator, None, CLSCTX_INPROC_SERVER)?;
            let svc = locator.ConnectServer(
                &BSTR::from("root\\WMI"),
                &BSTR::new(),
                &BSTR::new(),
                &BSTR::new(),
                0,
                &BSTR::new(),
                None,
            )?;
            CoSetProxyBlanket(
                &svc,
                RPC_C_AUTHN_WINNT,
                RPC_C_AUTHZ_NONE,
                PCWSTR::null(),
                RPC_C_AUTHN_LEVEL_CALL,
                RPC_C_IMP_LEVEL_IMPERSONATE,
                None,
                EOAC_NONE,
            )?;

            let gaming = open_class(&svc, "AcerGamingFunction", "gmInput", "gmOutput")?;
            let apge = match open_class(&svc, "APGeAction", "uiInput", "uiOutput") {
                Ok(c) => Some(c),
                Err(e) => {
                    log::info!("APGeAction unavailable, no backlight timeout: {e:#}");
                    None
                }
            };
            let nvapi = super::nvapi::Nvapi::open().map_err(|e| format!("{e:#}"));
            let product = bios_value("SystemProductName").unwrap_or_default();
            let model = models::lookup(&product);
            let mut me = Self { svc, gaming, apge, nvapi, model, caps: Capabilities::default() };
            me.caps = me.detect(product);
            log::info!("hardware: {:?}", me.caps);
            Ok(me)
        }
    }

    fn call_on(&self, c: &WmiClass, method: &str, input: Input) -> Result<u64> {
        unsafe {
            let m = wide(method);
            let mut in_def: Option<IWbemClassObject> = None;
            c.class.GetMethod(PCWSTR(m.as_ptr()), 0, &mut in_def, std::ptr::null_mut())?;
            let in_def = in_def.ok_or_else(|| anyhow!("{}.{method}: no input signature", c.name))?;
            let params = in_def.SpawnInstance(0)?;

            // WMI expects uint64 as a decimal string and uint32 as VT_I4.
            let value = match input {
                Input::U32(n) => VARIANT::from(n as i32),
                Input::U64(n) => VARIANT::from(BSTR::from(n.to_string())),
                Input::Bytes(b) => InitVariantFromBuffer(b.as_ptr() as *const _, b.len() as u32)?,
            };
            params.Put(PCWSTR(wide(c.input).as_ptr()), 0, &value, 0)?;

            let mut out: Option<IWbemClassObject> = None;
            self.svc.ExecMethod(&c.path, &BSTR::from(method), WBEM_GENERIC_FLAG_TYPE(0), None, &params, Some(&mut out), None)?;
            let out = out.ok_or_else(|| anyhow!("{}.{method}: no output", c.name))?;
            let mut v = VARIANT::default();
            out.Get(PCWSTR(wide(c.output).as_ptr()), 0, &mut v, None, None)?;
            if v.as_raw().Anonymous.Anonymous.vt == VT_BSTR.0 {
                let s = BSTR::try_from(&v).map_err(|e| anyhow!("{method}: {e}"))?;
                return s.to_string().trim().parse::<u64>().map_err(|e| anyhow!("{method}: {e}"));
            }
            if let Ok(n) = i32::try_from(&v) {
                return Ok(n as u32 as u64);
            }
            if let Ok(n) = u64::try_from(&v) {
                return Ok(n);
            }
            Ok(0)
        }
    }

    fn call(&self, method: &str, input: Input) -> Result<u64> {
        self.call_on(&self.gaming, method, input)
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
    fn detect(&self, product: String) -> Capabilities {
        let gpu_ids = self.nvapi.as_ref().ok().and_then(|nv| nv.pci_ids().ok());
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

        if let Some(m) = self.model {
            return Capabilities {
                product,
                model_name: Some(m.name.to_string()),
                known_model: true,
                lighting: m.four_zone,
                per_key: m.per_key,
                fans: m.fans.to_vec(),
                gpu_levels: levels(m.gpu_levels),
                sensors: true,
                toggles: m.toggles.iter().copied().filter(|t| *t != Toggle::BacklightTimeout || self.apge.is_some()).collect(),
                sticky_keys: true,
                coming: super::coming_features(m),
            };
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
            model_name: bios_value("SystemFamily").filter(|s| !s.is_empty()),
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
        }
    }

    fn read_backlight(&self) -> Option<bool> {
        self.apge
            .as_ref()
            .and_then(|c| self.call_on(c, "GetFunction", Input::U32(p::BACKLIGHT_TIMEOUT_GET)).ok())
            .and_then(p::decode_backlight_timeout)
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
        }
    }
}

impl Backend for WmiBackend {
    fn name(&self) -> &'static str {
        "Windows WMI"
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
        match &self.nvapi {
            Ok(nv) => nv.set_offsets(level.core_mhz, level.memory_mhz),
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
            Toggle::BacklightTimeout => {
                let c = self.apge.as_ref().ok_or_else(|| anyhow!("backlight timeout not supported"))?;
                let read = |me: &Self| -> Result<u64> {
                    let v = me.call_on(c, "GetFunction", Input::U32(p::BACKLIGHT_TIMEOUT_GET))?;
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
                let out = self.call_on(c, "SetFunction", Input::U64(p::backlight_timeout(current, on)))?;
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
        let gpu_core_offset = self.nvapi.as_ref().ok().and_then(|nv| nv.offset(super::nvapi::CLOCK_GRAPHICS).ok());
        let toggles = self.read_toggles();
        Observed { zones: ok.then_some(zones), fans, gpu, gpu_core_offset, toggles }
    }

    fn diagnostics(&mut self) -> Vec<String> {
        let mut out = Vec::new();
        let raw = |r: Result<u64>| r.map(|v| format!("0x{v:X}")).unwrap_or_else(|e| format!("error: {e:#}"));
        out.push(format!("GetGamingProfile(0) = {}", raw(self.call("GetGamingProfile", Input::U32(0)))));
        out.push(format!("GetGamingMiscSetting(6) = {}", raw(self.call("GetGamingMiscSetting", Input::U32(6)))));
        out.push(format!("GetGamingLED(1) = {}", raw(self.call("GetGamingLED", Input::U32(1)))));
        if let Some(c) = self.apge.as_ref() {
            out.push(format!("APGeAction.GetFunction(0x88401) = {}", raw(self.call_on(c, "GetFunction", Input::U32(p::BACKLIGHT_TIMEOUT_GET)))));
            out.push(format!("APGeAction.GetFunction(0xFFFF8401) = {}", raw(self.call_on(c, "GetFunction", Input::U32(0xFFFF_8401)))));
        }
        out
    }
}
