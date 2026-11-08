//! The per-key RGB keyboards of the Helios 700 (PH717-71) and Triton 900
//! (PT917-71). They have their own USB controller (Darfon, VID 0x0D62),
//! which PredatorSense drives with 8-byte HID feature reports plus a
//! 512-byte colour frame (see docs/protocol.md).
//!
//! Only the commands PredatorSense itself sends for lighting, the
//! Windows-key lock and the 30-second backlight timeout can be sent: the
//! controller has other commands (key remapping, analog modes) that change
//! how the keyboard works, and unknown commands could upset the controller,
//! so everything else is refused before it reaches the device.

use crate::settings::{Direction, Effect, Lighting, Rgb};
use anyhow::{bail, Result};
use std::time::Duration;

pub const VENDOR: u16 = 0x0D62;

/// The controller's product ids and the layout each one reports
/// (0 = US, 1 = UK, 2/3 = other ISO layouts).
pub const PRODUCTS: [(u16, u8); 11] = [
    (0xFABC, 0),
    (0xFABE, 1),
    (0x0ABA, 0),
    (0x0ABB, 1),
    (0x0ABC, 2),
    (0x0ABD, 3),
    (0x6A0C, 0),
    (0x6A0D, 1),
    (0x9A1A, 0),
    (0x6666, 0),
    (0x9A1B, 1),
];

pub fn layout_of(pid: u16) -> Option<u8> {
    PRODUCTS.iter().find(|(p, _)| *p == pid).map(|(_, l)| *l)
}

/// LED slots in the frame; 126 keys are used.
pub const SLOTS: usize = 128;
pub const KEYS: usize = 126;
/// The Triton 900's touchpad light, stored after the keys (set over WMI).
pub const TOUCHPAD_SLOT: usize = 126;

/// Last byte of every command: 0xFF minus the sum of the others.
pub fn checksum(c: &mut [u8; 8]) {
    c[7] = 0xFFu8.wrapping_sub(c[..7].iter().fold(0u8, |a, b| a.wrapping_add(*b)));
}

/// Commands that may be sent (see the module comment).
pub fn allowed(c: &[u8; 8]) -> bool {
    match c[0] {
        0x03 => c[1] == 0x00 || c[1] == 0x03,
        0x08 | 0x12 | 0x14 => true,
        // Backlight timeout only; 30 01 xx 01 is the MagForce key mode.
        0x30 => c[1] == 0x01 && c[2] <= 1 && c[3] == 0,
        // Read-only queries.
        0x88 | 0xB0 => true,
        _ => false,
    }
}

/// Brightness 0..=100 in the app → the keyboard's 0..=0x32, in
/// PredatorSense's five steps.
pub fn brightness(pct: u8) -> u8 {
    match pct {
        0 => 0x00,
        1..=37 => 0x0C,
        38..=62 => 0x19,
        63..=87 => 0x25,
        _ => 0x32,
    }
}

/// The keyboard's effect number.
pub fn effect_id(e: Effect) -> Option<u8> {
    Some(match e {
        Effect::Breathing => 0x02,
        Effect::Wave => 0x03,
        Effect::Reactive => 0x04,
        Effect::Marquee => 0x05,
        Effect::Ripple => 0x06,
        Effect::Neon => 0x08,
        Effect::RainbowMarquee => 0x09,
        Effect::Raindrop => 0x0A,
        Effect::CircleMarquee => 0x0B,
        Effect::Hedge => 0x0C,
        Effect::Spiral => 0x0D,
        Effect::Shifting => 0x14,
        Effect::Zoom => 0x15,
        Effect::Static | Effect::Temperature => return None,
    })
}

fn direction(e: Effect, d: Direction) -> u8 {
    match e {
        Effect::Wave => match d {
            Direction::RightToLeft => 0x02,
            Direction::LeftToRight => 0x01,
            Direction::Up => 0x03,
            Direction::Down => 0x04,
        },
        Effect::Spiral => match d {
            Direction::RightToLeft | Direction::Up => 0x02,
            _ => 0x01,
        },
        _ => 0x00,
    }
}

fn cmd(bytes: [u8; 7]) -> [u8; 8] {
    let mut c = [0u8; 8];
    c[..7].copy_from_slice(&bytes);
    checksum(&mut c);
    c
}

/// Keys that light more than one LED (long keys), per model and layout:
/// (key's LED, the other LEDs under it).
pub fn extra_leds(product: &str, layout: u8) -> Vec<(usize, Vec<usize>)> {
    let uk = layout != 0;
    let mut v: Vec<(usize, Vec<usize>)>;
    if product.contains("PT917-71") {
        v = vec![
            (54, vec![60]),
            (88, vec![82, 94]),
            (2, vec![8, 14]),
            (95, vec![101]),
            (97, vec![103]),
            (79, vec![85]),
            (83, vec![89]),
            (67, vec![73]),
            (36, vec![30, 42, 48]),
            (3, vec![9]),
        ];
        if uk {
            v.push((93, vec![99, 105, 92]));
        } else {
            v.push((86, vec![92, 98]));
            v.push((1, vec![7]));
        }
    } else if product.contains("PH717-71") {
        v = vec![
            (2, vec![8, 14]),
            (3, vec![9]),
            (24, vec![30, 42, 48, 54, 36]),
            (60, vec![66]),
            (85, vec![91]),
            (88, vec![94, 82]),
            (120, vec![121]),
            (1, vec![7]),
        ];
        if uk {
            v.push((93, vec![99, 87]));
        } else {
            v.push((86, vec![92, 98]));
        }
    } else {
        v = Vec::new();
    }
    v
}

/// The 512-byte frame: 128 slots of [0, R, G, B].
pub fn frame(keys: &[Rgb], fallback: Rgb, extra: &[(usize, Vec<usize>)]) -> [u8; 512] {
    let mut slots = [Rgb::default(); SLOTS];
    for (i, s) in slots.iter_mut().enumerate().take(KEYS) {
        *s = keys.get(i).copied().unwrap_or(fallback);
    }
    for (key, others) in extra {
        let c = slots[*key];
        for o in others {
            if *o < KEYS {
                slots[*o] = c;
            }
        }
    }
    let mut f = [0u8; 512];
    for (i, c) in slots.iter().enumerate() {
        f[i * 4 + 1] = c.r;
        f[i * 4 + 2] = c.g;
        f[i * 4 + 3] = c.b;
    }
    f
}

/// Talks to the keyboard controller.
pub trait Device {
    /// Sends one 8-byte command (feature report 0).
    fn send_raw(&mut self, c: &[u8; 8]) -> Result<()>;
    /// Sends a query and reads the 8-byte answer.
    fn query_raw(&mut self, c: &[u8; 8]) -> Result<[u8; 8]>;
    /// Writes 64 bytes of the colour frame (output report 0).
    fn write_raw(&mut self, chunk: &[u8; 64]) -> Result<()>;
    fn layout(&self) -> u8;
}

pub struct Keyboard {
    dev: Box<dyn Device>,
}

impl Keyboard {
    pub fn open() -> Result<Self> {
        Ok(Self { dev: os::open()? })
    }

    pub fn layout(&self) -> u8 {
        self.dev.layout()
    }

    fn send(&mut self, c: [u8; 8]) -> Result<()> {
        if !allowed(&c) {
            bail!("refused keyboard command {:02X?}", c);
        }
        self.dev.send_raw(&c)
    }

    fn query(&mut self, c: [u8; 8]) -> Result<[u8; 8]> {
        if !matches!(c[0], 0x88 | 0xB0) {
            bail!("refused keyboard query {:02X?}", c);
        }
        self.dev.query_raw(&c)
    }

    /// Static per-key colours (PredatorSense's "custom" mode, effect 0x33).
    pub fn set_keys(&mut self, frame: &[u8; 512], bright: u8) -> Result<()> {
        self.send(cmd([0x12, 0, 0, 0x08, 0, 0, 0]))?;
        std::thread::sleep(Duration::from_millis(100));
        for chunk in frame.chunks_exact(64) {
            self.dev.write_raw(chunk.try_into().expect("64-byte chunk"))?;
            std::thread::sleep(Duration::from_millis(5));
        }
        std::thread::sleep(Duration::from_millis(100));
        self.send(cmd([0x08, 0x02, 0x33, 0x05, bright, 0x08, 0x01]))
    }

    /// Only the brightness of the static per-key colours.
    pub fn set_static_brightness(&mut self, bright: u8) -> Result<()> {
        self.send(cmd([0x08, 0x02, 0, 0, 0, 0, 0]))?;
        self.send(cmd([0x08, 0x00, 0x33, 0x05, bright, 0x01, 0x01]))?;
        std::thread::sleep(Duration::from_millis(100));
        Ok(())
    }

    /// A built-in effect with its colour.
    pub fn set_effect(&mut self, effect: u8, speed: u8, colour: Rgb, dir: u8, bright: u8) -> Result<()> {
        let speed = 10 - speed.clamp(1, 9);
        let cc = if effect == 0x09 { 0x08 } else { 0x01 };
        self.send(cmd([0x08, 0x02, 0, 0, 0, 0, 0]))?;
        self.send(cmd([0x14, 0, 0, colour.r, colour.g, colour.b, 0]))?;
        self.send(cmd([0x08, 0x00, effect, speed, bright, cc, dir]))?;
        std::thread::sleep(Duration::from_millis(100));
        Ok(())
    }

    pub fn off(&mut self) -> Result<()> {
        self.send(cmd([0x08, 0x01, 0, 0, 0, 0, 0]))
    }

    /// Windows and Menu keys working (true) or locked.
    pub fn set_win_key(&mut self, enabled: bool) -> Result<()> {
        self.send(cmd([0x03, if enabled { 0x00 } else { 0x03 }, 0, 0, 0, 0, 0]))
    }

    /// Lighting off after 30 seconds without typing.
    pub fn set_timeout(&mut self, on: bool) -> Result<()> {
        self.send(cmd([0x30, 0x01, on as u8, 0, 0, 0, 0]))
    }

    pub fn timeout(&mut self) -> Option<bool> {
        self.query(cmd([0xB0, 0, 0, 0, 0, 0, 0])).ok().map(|r| r[2] != 0)
    }

    /// The keyboard's current brightness (0..=0x32), changed by Fn keys.
    pub fn brightness(&mut self) -> Option<u8> {
        self.query(cmd([0x88, 0, 0, 0, 0, 0, 0])).ok().map(|r| r[4])
    }

    /// Sends what changed from `old` (everything when None).
    pub fn apply(&mut self, product: &str, l: &Lighting, old: Option<&Lighting>) -> Result<()> {
        let bright = brightness(l.brightness);
        if l.brightness == 0 {
            return self.off();
        }
        match effect_id(l.effect) {
            None => {
                let only_brightness = old
                    .map(|o| o.brightness != 0 && o.effect == l.effect && o.keys == l.keys && o.colour == l.colour && o.brightness != l.brightness)
                    .unwrap_or(false);
                if only_brightness {
                    return self.set_static_brightness(bright);
                }
                let extra = extra_leds(product, self.layout());
                let f = frame(&l.keys, l.colour, &extra);
                self.set_keys(&f, bright)
            }
            Some(id) => self.set_effect(id, l.speed, l.colour, direction(l.effect, l.direction), bright),
        }
    }
}

#[cfg(windows)]
mod os {
    use super::{layout_of, Device, VENDOR};
    use anyhow::{bail, Result};
    use windows::core::PCWSTR;
    use windows::Win32::Devices::DeviceAndDriverInstallation::{
        SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInterfaces, SetupDiGetClassDevsW, SetupDiGetDeviceInterfaceDetailW, DIGCF_DEVICEINTERFACE,
        DIGCF_PRESENT, SP_DEVICE_INTERFACE_DATA, SP_DEVICE_INTERFACE_DETAIL_DATA_W,
    };
    use windows::Win32::Devices::HumanInterfaceDevice::{
        HidD_FreePreparsedData, HidD_GetAttributes, HidD_GetFeature, HidD_GetHidGuid, HidD_GetPreparsedData, HidD_SetFeature, HidP_GetCaps, HIDD_ATTRIBUTES,
        HIDP_CAPS, PHIDP_PREPARSED_DATA,
    };
    use windows::Win32::Foundation::{CloseHandle, GENERIC_READ, GENERIC_WRITE, HANDLE};
    use windows::Win32::Storage::FileSystem::{CreateFileW, WriteFile, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING};

    pub struct Hid {
        path: Vec<u16>,
        layout: u8,
    }

    fn open_path(path: &[u16], access: u32) -> Result<HANDLE> {
        Ok(unsafe {
            CreateFileW(
                PCWSTR(path.as_ptr()),
                access,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                FILE_FLAGS_AND_ATTRIBUTES(0),
                None,
            )?
        })
    }

    struct Handle(HANDLE);
    impl Drop for Handle {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }

    /// The controller's interface: VID 0x0D62, a known product id, and an
    /// 8-byte feature report without a report id (9 bytes with it), as
    /// PredatorSense picks it.
    pub fn open() -> Result<Box<dyn Device>> {
        unsafe {
            let guid = HidD_GetHidGuid();
            let set = SetupDiGetClassDevsW(Some(&guid), PCWSTR::null(), None, DIGCF_PRESENT | DIGCF_DEVICEINTERFACE)?;
            let mut found = None;
            let mut i = 0;
            loop {
                let mut ifd = SP_DEVICE_INTERFACE_DATA {
                    cbSize: std::mem::size_of::<SP_DEVICE_INTERFACE_DATA>() as u32,
                    ..Default::default()
                };
                if SetupDiEnumDeviceInterfaces(set, None, &guid, i, &mut ifd).is_err() {
                    break;
                }
                i += 1;
                let mut need = 0u32;
                let _ = SetupDiGetDeviceInterfaceDetailW(set, &ifd, None, 0, Some(&mut need), None);
                if need == 0 || need > 4096 {
                    continue;
                }
                let mut buf = vec![0u64; (need as usize).div_ceil(8)];
                let detail = buf.as_mut_ptr() as *mut SP_DEVICE_INTERFACE_DETAIL_DATA_W;
                (*detail).cbSize = std::mem::size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32;
                if SetupDiGetDeviceInterfaceDetailW(set, &ifd, Some(detail), need, None, None).is_err() {
                    continue;
                }
                let p = std::ptr::addr_of!((*detail).DevicePath) as *const u16;
                let mut len = 0;
                while *p.add(len) != 0 {
                    len += 1;
                }
                let path: Vec<u16> = std::slice::from_raw_parts(p, len).iter().copied().chain([0]).collect();
                let Ok(h) = open_path(&path, 0) else { continue };
                let h = Handle(h);
                let mut attr = HIDD_ATTRIBUTES {
                    Size: std::mem::size_of::<HIDD_ATTRIBUTES>() as u32,
                    ..Default::default()
                };
                if !HidD_GetAttributes(h.0, &mut attr).as_bool() || attr.VendorID != VENDOR {
                    continue;
                }
                let Some(layout) = layout_of(attr.ProductID) else {
                    continue;
                };
                let mut pp = PHIDP_PREPARSED_DATA::default();
                if !HidD_GetPreparsedData(h.0, &mut pp).as_bool() {
                    continue;
                }
                let mut caps = HIDP_CAPS::default();
                let ok = HidP_GetCaps(pp, &mut caps).is_ok();
                let _ = HidD_FreePreparsedData(pp);
                if ok && caps.FeatureReportByteLength == 9 {
                    found = Some(Hid { path, layout });
                    break;
                }
            }
            let _ = SetupDiDestroyDeviceInfoList(set);
            match found {
                Some(h) => Ok(Box::new(h)),
                None => bail!("per-key keyboard controller not found"),
            }
        }
    }

    impl Device for Hid {
        fn send_raw(&mut self, c: &[u8; 8]) -> Result<()> {
            let h = Handle(open_path(&self.path, GENERIC_READ.0 | GENERIC_WRITE.0)?);
            let mut buf = [0u8; 9];
            buf[1..].copy_from_slice(c);
            if !unsafe { HidD_SetFeature(h.0, buf.as_ptr() as *const _, 9) }.as_bool() {
                bail!("keyboard didn't take the command: {}", std::io::Error::last_os_error());
            }
            Ok(())
        }

        fn query_raw(&mut self, c: &[u8; 8]) -> Result<[u8; 8]> {
            self.send_raw(c)?;
            let h = Handle(open_path(&self.path, GENERIC_READ.0 | GENERIC_WRITE.0)?);
            let mut buf = [0u8; 9];
            if !unsafe { HidD_GetFeature(h.0, buf.as_mut_ptr() as *mut _, 9) }.as_bool() {
                bail!("keyboard didn't answer: {}", std::io::Error::last_os_error());
            }
            let mut out = [0u8; 8];
            out.copy_from_slice(&buf[1..]);
            Ok(out)
        }

        fn write_raw(&mut self, chunk: &[u8; 64]) -> Result<()> {
            let h = Handle(open_path(&self.path, GENERIC_WRITE.0)?);
            let mut buf = [0u8; 65];
            buf[1..].copy_from_slice(chunk);
            let mut n = 0u32;
            unsafe { WriteFile(h.0, Some(&buf), Some(&mut n), None)? };
            if n != 65 {
                bail!("short write to the keyboard ({n} bytes)");
            }
            Ok(())
        }

        fn layout(&self) -> u8 {
            self.layout
        }
    }
}

#[cfg(target_os = "linux")]
mod os {
    use super::{layout_of, Device, VENDOR};
    use anyhow::{bail, Result};
    use std::fs;
    use std::io::Write;
    use std::os::fd::AsRawFd;

    pub struct Hidraw {
        file: fs::File,
        layout: u8,
    }

    // _IOC(_IOC_READ|_IOC_WRITE, 'H', 0x06 / 0x07, 9)
    const HIDIOCSFEATURE_9: libc::c_ulong = 0xC009_4806;
    const HIDIOCGFEATURE_9: libc::c_ulong = 0xC009_4807;

    /// Bits of feature report 0 (reports without an id) in a HID report
    /// descriptor; PredatorSense's controller interface has 64.
    pub fn feature_bits(desc: &[u8]) -> (u32, bool) {
        let (mut size, mut count, mut id) = (0u32, 0u32, 0u8);
        let mut bits = 0u32;
        let mut has_ids = false;
        let mut i = 0;
        while i < desc.len() {
            let b = desc[i];
            if b == 0xFE {
                // Long item.
                let n = desc.get(i + 1).copied().unwrap_or(0) as usize;
                i += 3 + n;
                continue;
            }
            let n = match b & 3 {
                3 => 4,
                k => k as usize,
            };
            let mut v = 0u32;
            for k in 0..n {
                v |= (desc.get(i + 1 + k).copied().unwrap_or(0) as u32) << (8 * k);
            }
            match b & 0xFC {
                0x74 => size = v,
                0x94 => count = v,
                0x84 => {
                    id = v as u8;
                    has_ids = true;
                }
                0xB0 if id == 0 => bits += size * count,
                _ => {}
            }
            i += 1 + n;
        }
        (bits, has_ids)
    }

    pub fn open() -> Result<Box<dyn Device>> {
        for e in fs::read_dir("/sys/class/hidraw")?.flatten() {
            let dev = e.path().join("device");
            let uevent = fs::read_to_string(dev.join("uevent")).unwrap_or_default();
            // HID_ID=0003:00000D62:0000FABC
            let Some(id) = uevent.lines().find_map(|l| l.strip_prefix("HID_ID=")) else {
                continue;
            };
            let parts: Vec<&str> = id.split(':').collect();
            if parts.len() != 3 {
                continue;
            }
            let (Ok(vid), Ok(pid)) = (u32::from_str_radix(parts[1], 16), u32::from_str_radix(parts[2], 16)) else {
                continue;
            };
            if vid != VENDOR as u32 {
                continue;
            }
            let Some(layout) = layout_of(pid as u16) else {
                continue;
            };
            let desc = fs::read(dev.join("report_descriptor")).unwrap_or_default();
            let (bits, has_ids) = feature_bits(&desc);
            if bits != 64 || has_ids {
                continue;
            }
            let name = e.file_name();
            let file = fs::OpenOptions::new().read(true).write(true).open(std::path::Path::new("/dev").join(name))?;
            return Ok(Box::new(Hidraw { file, layout }));
        }
        bail!("per-key keyboard controller not found")
    }

    impl Device for Hidraw {
        fn send_raw(&mut self, c: &[u8; 8]) -> Result<()> {
            let mut buf = [0u8; 9];
            buf[1..].copy_from_slice(c);
            if unsafe { libc::ioctl(self.file.as_raw_fd(), HIDIOCSFEATURE_9, buf.as_mut_ptr()) } < 0 {
                bail!("keyboard didn't take the command: {}", std::io::Error::last_os_error());
            }
            Ok(())
        }

        fn query_raw(&mut self, c: &[u8; 8]) -> Result<[u8; 8]> {
            self.send_raw(c)?;
            let mut buf = [0u8; 9];
            if unsafe { libc::ioctl(self.file.as_raw_fd(), HIDIOCGFEATURE_9, buf.as_mut_ptr()) } < 0 {
                bail!("keyboard didn't answer: {}", std::io::Error::last_os_error());
            }
            let mut out = [0u8; 8];
            out.copy_from_slice(&buf[1..]);
            Ok(out)
        }

        fn write_raw(&mut self, chunk: &[u8; 64]) -> Result<()> {
            let mut buf = [0u8; 65];
            buf[1..].copy_from_slice(chunk);
            self.file.write_all(&buf)?;
            Ok(())
        }

        fn layout(&self) -> u8 {
            self.layout
        }
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
mod os {
    use super::Device;
    pub fn open() -> anyhow::Result<Box<dyn Device>> {
        anyhow::bail!("not supported on this system")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_packets() {
        // From PredatorSense's agent and OpenRGB's identical protocol.
        assert_eq!(cmd([0x12, 0, 0, 0x08, 0, 0, 0]), [0x12, 0, 0, 0x08, 0, 0, 0, 0xE5]);
        assert_eq!(cmd([0x08, 0x01, 0, 0, 0, 0, 0]), [0x08, 0x01, 0, 0, 0, 0, 0, 0xF6]);
        assert_eq!(cmd([0x08, 0x02, 0, 0, 0, 0, 0]), [0x08, 0x02, 0, 0, 0, 0, 0, 0xF5]);
        assert_eq!(cmd([0x03, 0, 0, 0, 0, 0, 0])[7], 0xFC);
        assert_eq!(cmd([0x03, 3, 0, 0, 0, 0, 0])[7], 0xF9);
        assert_eq!(cmd([0x30, 1, 1, 0, 0, 0, 0])[7], 0xCD);
        assert_eq!(cmd([0x88, 0, 0, 0, 0, 0, 0])[7], 0x77);
        assert_eq!(cmd([0xB0, 0, 0, 0, 0, 0, 0])[7], 0x4F);
    }

    #[test]
    fn whitelist() {
        assert!(allowed(&cmd([0x08, 0, 0x33, 5, 0x32, 1, 1])));
        assert!(allowed(&cmd([0x30, 1, 0, 0, 0, 0, 0])));
        assert!(!allowed(&cmd([0x30, 1, 0, 1, 0, 0, 0])));
        assert!(!allowed(&cmd([0x31, 0, 0, 0, 0, 0, 0])));
        assert!(!allowed(&cmd([0x32, 0, 0, 0, 0, 0, 0])));
        assert!(!allowed(&cmd([0x92, 0, 0, 0, 0, 0, 0])));
        assert!(!allowed(&cmd([0x03, 7, 0, 0, 0, 0, 0])));
    }

    #[test]
    fn frame_layout() {
        let red = Rgb { r: 255, g: 0, b: 0 };
        let blue = Rgb { r: 0, g: 0, b: 255 };
        let mut keys = vec![blue; KEYS];
        keys[24] = red; // Helios 700 space bar
        let f = frame(&keys, blue, &extra_leds("Predator PH717-71", 0));
        assert_eq!(&f[24 * 4..24 * 4 + 4], &[0, 255, 0, 0]);
        assert_eq!(&f[30 * 4..30 * 4 + 4], &[0, 255, 0, 0]);
        assert_eq!(&f[25 * 4..25 * 4 + 4], &[0, 0, 0, 255]);
        assert_eq!(&f[127 * 4..], &[0, 0, 0, 0]);
    }

    #[test]
    fn brightness_steps() {
        assert_eq!([0, 25, 50, 75, 100].map(brightness), [0x00, 0x0C, 0x19, 0x25, 0x32]);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn descriptor() {
        // Vendor page, 8-byte feature report and 64-byte output report, no ids.
        let d = [
            0x06, 0x00, 0xFF, 0x09, 0x01, 0xA1, 0x01, 0x75, 0x08, 0x95, 0x08, 0xB1, 0x02, 0x95, 0x40, 0x91, 0x02, 0xC0,
        ];
        assert_eq!(os::feature_bits(&d), (64, false));
    }
}
