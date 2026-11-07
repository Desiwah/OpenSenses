//! Talks to the firmware through the `AcerGamingFunction` class in the
//! `root\WMI` namespace. Needs administrator / SYSTEM rights. COM objects
//! here are not thread-safe, so the backend must stay on the thread that
//! opened it.

use super::{Backend, Capabilities, Observed, Sensors};
use crate::protocol as p;
use crate::settings::{FanMode, GpuMode, Rgb};
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

const CLASS: &str = "AcerGamingFunction";

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

enum Input {
    U32(u32),
    U64(u64),
    Bytes([u8; 16]),
}

pub struct WmiBackend {
    svc: IWbemServices,
    class: IWbemClassObject,
    path: BSTR,
    nvapi: std::result::Result<super::nvapi::Nvapi, String>,
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

            let mut class = None;
            svc.GetObject(&BSTR::from(CLASS), WBEM_GENERIC_FLAG_TYPE(0), None, Some(&mut class), None)
                .context("AcerGamingFunction class not found (not an Acer gaming laptop?)")?;
            let class = class.ok_or_else(|| anyhow!("no class object"))?;

            let query = svc.ExecQuery(
                &BSTR::from("WQL"),
                &BSTR::from(format!("SELECT * FROM {CLASS}")),
                WBEM_GENERIC_FLAG_TYPE(WBEM_FLAG_FORWARD_ONLY.0 | WBEM_FLAG_RETURN_IMMEDIATELY.0),
                None,
            )?;
            let mut objs: [Option<IWbemClassObject>; 1] = [None];
            let mut got = 0u32;
            query.Next(WBEM_INFINITE, &mut objs, &mut got).ok()?;
            let inst = objs[0].take().ok_or_else(|| anyhow!("no AcerGamingFunction instance (run as administrator)"))?;
            let mut v = VARIANT::default();
            inst.Get(PCWSTR(wide("__PATH").as_ptr()), 0, &mut v, None, None)?;
            let path = BSTR::try_from(&v).map_err(|e| anyhow!("instance path: {e}"))?;

            let nvapi = super::nvapi::Nvapi::open().map_err(|e| format!("{e:#}"));
            Ok(Self { svc, class, path, nvapi })
        }
    }

    fn call(&self, method: &str, input: Input) -> Result<u64> {
        unsafe {
            let m = wide(method);
            let mut in_def: Option<IWbemClassObject> = None;
            self.class.GetMethod(PCWSTR(m.as_ptr()), 0, &mut in_def, std::ptr::null_mut())?;
            let in_def = in_def.ok_or_else(|| anyhow!("{method}: no input signature"))?;
            let params = in_def.SpawnInstance(0)?;

            // WMI expects uint64 as a decimal string and uint32 as VT_I4.
            let value = match input {
                Input::U32(n) => VARIANT::from(n as i32),
                Input::U64(n) => VARIANT::from(BSTR::from(n.to_string())),
                Input::Bytes(b) => InitVariantFromBuffer(b.as_ptr() as *const _, b.len() as u32)?,
            };
            params.Put(PCWSTR(wide("gmInput").as_ptr()), 0, &value, 0)?;

            let mut out: Option<IWbemClassObject> = None;
            self.svc.ExecMethod(
                &self.path,
                &BSTR::from(method),
                WBEM_GENERIC_FLAG_TYPE(0),
                None,
                &params,
                Some(&mut out),
                None,
            )?;
            let out = out.ok_or_else(|| anyhow!("{method}: no output"))?;
            let mut v = VARIANT::default();
            out.Get(PCWSTR(wide("gmOutput").as_ptr()), 0, &mut v, None, None)?;
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
}

impl Backend for WmiBackend {
    fn name(&self) -> &'static str {
        "Windows WMI"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities { lighting: true, fans: true, gpu_modes: true, sensors: true }
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

    fn set_fan_mode(&mut self, fan: u64, mode: FanMode) -> Result<()> {
        self.set("SetGamingFanBehavior", Input::U64(p::fan_behavior(fan, mode)))
    }

    fn set_fan_speed(&mut self, fan: u64, percent: u8) -> Result<()> {
        self.set("SetGamingFanSpeed", Input::U64(p::fan_speed(fan, percent)))
    }

    fn set_gpu(&mut self, mode: GpuMode) -> Result<()> {
        // The firmware value tells the laptop which level is active; the
        // actual clock offsets go through the NVIDIA driver.
        self.set("SetGamingMiscSetting", Input::U64(p::gpu_oc(mode)))?;
        let (core, mem) = p::gpu_offsets(mode);
        match &self.nvapi {
            Ok(nv) => nv.set_offsets(core, mem),
            Err(e) if mode == GpuMode::Normal => {
                log::info!("NVIDIA driver interface unavailable ({e}); nothing to reset");
                Ok(())
            }
            Err(e) => bail!("GPU clock offsets unavailable: {e}"),
        }
    }

    fn set_turbo_led(&mut self, on: bool) -> Result<()> {
        let v = if on { p::TURBO_LED_ON } else { p::TURBO_LED_OFF };
        self.set("SetGamingLEDBehavior", Input::U64(v))
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
        let fans = self
            .call("GetGamingFanBehavior", Input::U32((p::FAN_CPU | p::FAN_GPU) as u32))
            .map(p::decode_fan_behavior)
            .unwrap_or((None, None));
        let gpu = self.call("GetGamingMiscSetting", Input::U32(p::MISC_GPU_OC as u32)).ok().and_then(p::decode_gpu_oc);
        let gpu_core_offset = self.nvapi.as_ref().ok().and_then(|nv| nv.offset(super::nvapi::CLOCK_GRAPHICS).ok());
        Observed { zones: ok.then_some(zones), fans, gpu, gpu_core_offset }
    }
}
