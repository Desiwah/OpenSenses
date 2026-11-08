//! Windows: calls the firmware through the WMI classes in the `root\WMI`
//! namespace, `AcerGamingFunction` and `APGeAction`. Needs administrator /
//! SYSTEM rights. COM objects here are not thread-safe, so the backend must
//! stay on the thread that opened it. The firmware logic itself is shared
//! with Linux in `acer.rs`.

use super::acer::{AcerBackend, GpuClocks, Transport};
use crate::protocol::{Iface, Input};
use anyhow::{anyhow, Context, Result};
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

/// One WMI class with its single instance and the names of its in/out parameters.
struct WmiClass {
    name: &'static str,
    class: IWbemClassObject,
    path: BSTR,
    input: &'static str,
    output: &'static str,
}

pub struct Wmi {
    svc: IWbemServices,
    gaming: WmiClass,
    apge: Option<WmiClass>,
}

pub type WmiBackend = AcerBackend<Wmi>;

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

/// Opens the WMI classes and the NVIDIA driver interface.
pub fn open() -> Result<WmiBackend> {
    let wmi = unsafe { Wmi::open()? };
    let gpu = super::nvapi::Nvapi::open().map(|n| Box::new(n) as Box<dyn GpuClocks>).map_err(|e| format!("{e:#}"));
    let product = bios_value("SystemProductName").unwrap_or_default();
    Ok(AcerBackend::new(wmi, gpu, product, bios_value("SystemFamily"), true))
}

impl Wmi {
    unsafe fn open() -> Result<Self> {
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
        let svc = locator.ConnectServer(&BSTR::from("root\\WMI"), &BSTR::new(), &BSTR::new(), &BSTR::new(), 0, &BSTR::new(), None)?;
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
                log::info!("APGeAction unavailable, no backlight timeout or CoolBoost: {e:#}");
                None
            }
        };
        Ok(Self { svc, gaming, apge })
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
}

impl Transport for Wmi {
    fn name(&self) -> &'static str {
        "Windows WMI"
    }

    fn call(&self, iface: Iface, method: &str, input: Input) -> Result<u64> {
        match iface {
            Iface::Gaming => self.call_on(&self.gaming, method, input),
            Iface::Apge => match &self.apge {
                Some(c) => self.call_on(c, method, input),
                None => Err(anyhow!("APGeAction is not available")),
            },
        }
    }

    fn has_apge(&self) -> bool {
        self.apge.is_some()
    }
}
