//! Programmable-key actions that belong to the signed-in user rather than
//! the hardware: Sticky Keys, opening a program, typing a macro, opening
//! OpenSenses. The service never runs these itself; it starts
//! `opensenses-service user-action …` in the user's session (as that user),
//! the way PredatorSense hands them to its per-user agent.
//!
//! The one exception is typing a macro on Linux: only root can create the
//! virtual keyboard there, so the service does that part (see the service's
//! `uinput` module). It only ever presses the keys the macro lists.

use crate::settings::{HotkeyAction, MacroStep, MAX_MACRO_STEPS};
use anyhow::{bail, Result};

/// A key in a macro: Linux evdev code, PC scan code (set 1) and whether the
/// scan code has the E0 prefix. Keys are named like the browser's
/// `KeyboardEvent.code`, which is what the app records.
pub fn key(code: &str) -> Option<(u16, u16, bool)> {
    let k = |ev: u16, sc: u16| Some((ev, sc, false));
    let e = |ev: u16, sc: u16| Some((ev, sc, true));
    if let Some(c) = code.strip_prefix("Key").filter(|c| c.len() == 1) {
        const ROW: [(char, u16); 26] = [
            ('Q', 0x10),
            ('W', 0x11),
            ('E', 0x12),
            ('R', 0x13),
            ('T', 0x14),
            ('Y', 0x15),
            ('U', 0x16),
            ('I', 0x17),
            ('O', 0x18),
            ('P', 0x19),
            ('A', 0x1E),
            ('S', 0x1F),
            ('D', 0x20),
            ('F', 0x21),
            ('G', 0x22),
            ('H', 0x23),
            ('J', 0x24),
            ('K', 0x25),
            ('L', 0x26),
            ('Z', 0x2C),
            ('X', 0x2D),
            ('C', 0x2E),
            ('V', 0x2F),
            ('B', 0x30),
            ('N', 0x31),
            ('M', 0x32),
        ];
        let ch = c.chars().next()?;
        return ROW.iter().find(|(l, _)| *l == ch).map(|&(_, sc)| (sc, sc, false));
    }
    if let Some(d) = code.strip_prefix("Digit").and_then(|d| d.parse::<u16>().ok()).filter(|d| *d <= 9) {
        let sc = if d == 0 { 0x0B } else { 0x01 + d };
        return k(sc, sc);
    }
    if let Some(n) = code.strip_prefix('F').and_then(|n| n.parse::<u16>().ok()) {
        return match n {
            1..=10 => k(0x3A + n, 0x3A + n),
            11 => k(87, 0x57),
            12 => k(88, 0x58),
            13..=23 => Some((183 + n - 13, 0x64 + n - 13, false)),
            24 => k(194, 0x76),
            _ => None,
        };
    }
    if let Some(d) = code.strip_prefix("Numpad").and_then(|d| d.parse::<u16>().ok()).filter(|d| *d <= 9) {
        const PAD: [u16; 10] = [0x52, 0x4F, 0x50, 0x51, 0x4B, 0x4C, 0x4D, 0x47, 0x48, 0x49];
        let sc = PAD[d as usize];
        return k(sc, sc);
    }
    match code {
        "Escape" => k(1, 0x01),
        "Minus" => k(12, 0x0C),
        "Equal" => k(13, 0x0D),
        "Backspace" => k(14, 0x0E),
        "Tab" => k(15, 0x0F),
        "BracketLeft" => k(26, 0x1A),
        "BracketRight" => k(27, 0x1B),
        "Enter" => k(28, 0x1C),
        "ControlLeft" => k(29, 0x1D),
        "Semicolon" => k(39, 0x27),
        "Quote" => k(40, 0x28),
        "Backquote" => k(41, 0x29),
        "ShiftLeft" => k(42, 0x2A),
        "Backslash" => k(43, 0x2B),
        "Comma" => k(51, 0x33),
        "Period" => k(52, 0x34),
        "Slash" => k(53, 0x35),
        "ShiftRight" => k(54, 0x36),
        "NumpadMultiply" => k(55, 0x37),
        "AltLeft" => k(56, 0x38),
        "Space" => k(57, 0x39),
        "CapsLock" => k(58, 0x3A),
        // Windows wants NumLock with the extended flag (without it, 0x45 is Pause).
        "NumLock" => e(69, 0x45),
        "ScrollLock" => k(70, 0x46),
        "NumpadSubtract" => k(74, 0x4A),
        "NumpadAdd" => k(78, 0x4E),
        "NumpadDecimal" => k(83, 0x53),
        "IntlBackslash" => k(86, 0x56),
        "NumpadEnter" => e(96, 0x1C),
        "ControlRight" => e(97, 0x1D),
        "NumpadDivide" => e(98, 0x35),
        "PrintScreen" => e(99, 0x37),
        "AltRight" => e(100, 0x38),
        "Home" => e(102, 0x47),
        "ArrowUp" => e(103, 0x48),
        "PageUp" => e(104, 0x49),
        "ArrowLeft" => e(105, 0x4B),
        "ArrowRight" => e(106, 0x4D),
        "End" => e(107, 0x4F),
        "ArrowDown" => e(108, 0x50),
        "PageDown" => e(109, 0x51),
        "Insert" => e(110, 0x52),
        "Delete" => e(111, 0x53),
        "AudioVolumeMute" => e(113, 0x20),
        "AudioVolumeDown" => e(114, 0x2E),
        "AudioVolumeUp" => e(115, 0x30),
        "MetaLeft" => e(125, 0x5B),
        "MetaRight" => e(126, 0x5C),
        "ContextMenu" => e(127, 0x5D),
        "MediaTrackNext" => e(163, 0x19),
        "MediaPlayPause" => e(164, 0x22),
        "MediaTrackPrevious" => e(165, 0x10),
        "MediaStop" => e(166, 0x24),
        _ => None,
    }
}

/// The macro's steps that can be played, with every key that is still
/// down at the end released, so a cut-off recording never leaves Shift held.
pub fn playable(steps: &[MacroStep]) -> Vec<MacroStep> {
    let mut out: Vec<MacroStep> = Vec::new();
    let mut held: Vec<String> = Vec::new();
    for s in steps.iter().take(MAX_MACRO_STEPS) {
        match &s.code {
            Some(c) if key(c).is_some() => {
                if s.down {
                    if !held.contains(c) {
                        held.push(c.clone());
                    }
                } else {
                    held.retain(|h| h != c);
                }
                out.push(s.clone());
            }
            Some(_) => {}
            None => out.push(MacroStep {
                code: None,
                down: false,
                delay_ms: s.delay_ms.min(10_000),
            }),
        }
    }
    for c in held.into_iter().rev() {
        out.push(MacroStep {
            code: Some(c),
            down: false,
            delay_ms: 0,
        });
    }
    out
}

/// Gap between key events that have no recorded pause, so games see them.
pub const KEY_GAP_MS: u64 = 15;

/// Runs an action as the signed-in user (called in the user's session).
pub fn perform(a: &HotkeyAction, app: Option<&str>) -> Result<()> {
    match a {
        HotkeyAction::StickyKeys => {
            let on = sticky_keys().unwrap_or(false);
            set_sticky_keys(!on)
        }
        HotkeyAction::LaunchApp { target } => launch(target),
        HotkeyAction::OpenApp => open_app(app),
        HotkeyAction::Macro { steps } => type_macro(&playable(steps)),
        _ => bail!("not a user action"),
    }
}

fn open_app(app: Option<&str>) -> Result<()> {
    #[cfg(windows)]
    unsafe {
        // Already open: bring it to the front instead of a second window.
        use windows::core::w;
        use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, SetForegroundWindow, ShowWindow, SW_RESTORE};
        if let Ok(h) = FindWindowW(None, w!("OpenSenses")) {
            if !h.is_invalid() {
                let _ = ShowWindow(h, SW_RESTORE);
                let _ = SetForegroundWindow(h);
                return Ok(());
            }
        }
    }
    if let Some(p) = app.filter(|p| std::path::Path::new(p).is_file()) {
        std::process::Command::new(p).spawn()?;
        return Ok(());
    }
    #[cfg(windows)]
    {
        // The service sits in the install folder's bin\ (or next to the app).
        let exe = std::env::current_exe()?;
        for dir in exe.ancestors().skip(1).take(2) {
            let p = dir.join("opensenses.exe");
            if p.is_file() {
                std::process::Command::new(p).spawn()?;
                return Ok(());
            }
        }
        bail!("couldn't find OpenSenses")
    }
    #[cfg(not(windows))]
    {
        for name in ["opensenses", "OpenSenses"] {
            if std::process::Command::new(name).spawn().is_ok() {
                return Ok(());
            }
        }
        launch("opensenses.desktop")
    }
}

#[cfg(windows)]
fn launch(target: &str) -> Result<()> {
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let t = target.trim().trim_matches('"');
    if t.is_empty() {
        bail!("no program chosen for this key");
    }
    // Store apps are saved as their AUMID, as in PredatorSense.
    let (file, params) = if !t.contains('\\') && !t.contains(':') && t.contains('!') {
        ("explorer.exe".to_string(), format!("shell:AppsFolder\\{t}"))
    } else {
        (t.to_string(), String::new())
    };
    let r = unsafe {
        ShellExecuteW(
            None,
            &HSTRING::from("open"),
            &HSTRING::from(file),
            if params.is_empty() {
                PCWSTR::null()
            } else {
                PCWSTR(HSTRING::from(params).as_ptr())
            },
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    if r.0 as usize <= 32 {
        bail!("couldn't open {t} (error {})", r.0 as usize);
    }
    Ok(())
}

#[cfg(not(windows))]
fn launch(target: &str) -> Result<()> {
    use std::process::{Command, Stdio};
    let t = target.trim();
    if t.is_empty() {
        bail!("no program chosen for this key");
    }
    let path = std::path::Path::new(t);
    let spawn = |c: &mut Command| c.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().map(|_| ());
    // A .desktop entry: start it the way the menu would.
    if t.ends_with(".desktop") {
        let id = path.file_name().and_then(|n| n.to_str()).unwrap_or(t);
        let stem = id.trim_end_matches(".desktop");
        if spawn(Command::new("gtk-launch").arg(stem)).is_ok() || spawn(Command::new("kioclient").args(["exec", t])).is_ok() {
            return Ok(());
        }
    }
    // A Windows game under Wine/Proton (what GameSync picks up).
    if t.to_ascii_lowercase().ends_with(".exe") {
        return spawn(Command::new("wine").arg(t)).map_err(|e| anyhow::anyhow!("couldn't start {t} with Wine: {e}"));
    }
    use std::os::unix::fs::PermissionsExt;
    if path.is_file() && path.metadata().map(|m| m.permissions().mode() & 0o111 != 0).unwrap_or(false) {
        return spawn(&mut Command::new(path)).map_err(Into::into);
    }
    if !path.exists() && !t.contains('/') && !t.contains("://") && spawn(&mut Command::new(t)).is_ok() {
        return Ok(());
    }
    spawn(Command::new("xdg-open").arg(t)).map_err(|e| anyhow::anyhow!("couldn't open {t}: {e}"))
}

#[cfg(windows)]
fn type_macro(steps: &[MacroStep]) -> Result<()> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE, VIRTUAL_KEY,
    };
    for s in steps {
        match &s.code {
            None => std::thread::sleep(std::time::Duration::from_millis(s.delay_ms as u64)),
            Some(c) => {
                let Some((_, sc, ext)) = key(c) else { continue };
                let mut flags = KEYEVENTF_SCANCODE;
                if ext {
                    flags |= KEYEVENTF_EXTENDEDKEY;
                }
                if !s.down {
                    flags |= KEYEVENTF_KEYUP;
                }
                let input = INPUT {
                    r#type: INPUT_KEYBOARD,
                    Anonymous: INPUT_0 {
                        ki: KEYBDINPUT {
                            wVk: VIRTUAL_KEY(0),
                            wScan: sc,
                            dwFlags: flags,
                            time: 0,
                            dwExtraInfo: 0,
                        },
                    },
                };
                unsafe { SendInput(&[input], std::mem::size_of::<INPUT>() as i32) };
                std::thread::sleep(std::time::Duration::from_millis(KEY_GAP_MS));
            }
        }
    }
    Ok(())
}

#[cfg(not(windows))]
fn type_macro(_steps: &[MacroStep]) -> Result<()> {
    bail!("macros are typed by the service on Linux")
}

/// Whether Sticky Keys is on for the signed-in user.
pub fn sticky_keys() -> Option<bool> {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::UI::Accessibility::{SKF_STICKYKEYSON, STICKYKEYS};
        use windows::Win32::UI::WindowsAndMessaging::{SystemParametersInfoW, SPI_GETSTICKYKEYS, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS};
        let mut sk = STICKYKEYS {
            cbSize: std::mem::size_of::<STICKYKEYS>() as u32,
            ..Default::default()
        };
        SystemParametersInfoW(
            SPI_GETSTICKYKEYS,
            sk.cbSize,
            Some(&mut sk as *mut _ as *mut _),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
        .ok()?;
        Some(sk.dwFlags.0 & SKF_STICKYKEYSON.0 != 0)
    }
    #[cfg(not(windows))]
    linux_a11y::get()
}

pub fn set_sticky_keys(on: bool) -> Result<()> {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::UI::Accessibility::{SKF_STICKYKEYSON, STICKYKEYS, STICKYKEYS_FLAGS};
        use windows::Win32::UI::WindowsAndMessaging::{
            SystemParametersInfoW, SPIF_SENDCHANGE, SPIF_UPDATEINIFILE, SPI_GETSTICKYKEYS, SPI_SETSTICKYKEYS, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
        };
        let mut sk = STICKYKEYS {
            cbSize: std::mem::size_of::<STICKYKEYS>() as u32,
            ..Default::default()
        };
        SystemParametersInfoW(
            SPI_GETSTICKYKEYS,
            sk.cbSize,
            Some(&mut sk as *mut _ as *mut _),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )?;
        sk.dwFlags = if on {
            STICKYKEYS_FLAGS(sk.dwFlags.0 | SKF_STICKYKEYSON.0)
        } else {
            STICKYKEYS_FLAGS(sk.dwFlags.0 & !SKF_STICKYKEYSON.0)
        };
        SystemParametersInfoW(
            SPI_SETSTICKYKEYS,
            sk.cbSize,
            Some(&mut sk as *mut _ as *mut _),
            SPIF_UPDATEINIFILE | SPIF_SENDCHANGE,
        )?;
        Ok(())
    }
    #[cfg(not(windows))]
    linux_a11y::set(on)
}

/// Sticky Keys on Linux is a desktop setting: KDE Plasma keeps it in
/// kaccessrc, GNOME in its accessibility settings.
#[cfg(not(windows))]
mod linux_a11y {
    use anyhow::{anyhow, Result};
    use std::process::Command;

    enum Desktop {
        Kde,
        Gnome,
    }

    fn desktop() -> Option<Desktop> {
        let d = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default().to_uppercase();
        if d.contains("KDE") {
            Some(Desktop::Kde)
        } else if d.contains("GNOME") || d.contains("UNITY") || d.contains("CINNAMON") || d.contains("BUDGIE") {
            Some(Desktop::Gnome)
        } else {
            None
        }
    }

    fn output(cmd: &str, args: &[&str]) -> Option<String> {
        let o = Command::new(cmd).args(args).output().ok()?;
        o.status.success().then(|| String::from_utf8_lossy(&o.stdout).trim().to_string())
    }

    pub fn get() -> Option<bool> {
        match desktop()? {
            Desktop::Kde => {
                let args = ["--file", "kaccessrc", "--group", "Keyboard", "--key", "StickyKeys", "--default", "false"];
                output("kreadconfig6", &args).or_else(|| output("kreadconfig5", &args)).map(|v| v == "true")
            }
            Desktop::Gnome => output("gsettings", &["get", "org.gnome.desktop.a11y.keyboard", "stickykeys-enable"]).map(|v| v == "true"),
        }
    }

    pub fn set(on: bool) -> Result<()> {
        let v = if on { "true" } else { "false" };
        let ok = match desktop().ok_or_else(|| anyhow!("Sticky Keys can be changed on KDE Plasma and GNOME desktops"))? {
            Desktop::Kde => {
                // --notify makes Plasma pick the change up right away.
                let args = ["--file", "kaccessrc", "--group", "Keyboard", "--key", "StickyKeys", "--notify", v];
                output("kwriteconfig6", &args).or_else(|| output("kwriteconfig5", &args)).is_some()
            }
            Desktop::Gnome => output("gsettings", &["set", "org.gnome.desktop.a11y.keyboard", "stickykeys-enable", v]).is_some(),
        };
        if ok {
            Ok(())
        } else {
            Err(anyhow!("couldn't change the desktop's Sticky Keys setting"))
        }
    }
}

/// Hex text for passing an action on a command line without quoting trouble.
pub fn encode(a: &HotkeyAction) -> String {
    serde_json::to_vec(a).unwrap_or_default().iter().map(|b| format!("{b:02x}")).collect()
}

pub fn decode(hex: &str) -> Option<HotkeyAction> {
    if !hex.len().is_multiple_of(2) || hex.len() > 64 * 1024 {
        return None;
    }
    let bytes: Option<Vec<u8>> = (0..hex.len()).step_by(2).map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok()).collect();
    serde_json::from_slice(&bytes?).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys() {
        assert_eq!(key("KeyA"), Some((30, 0x1E, false)));
        assert_eq!(key("Digit0"), Some((11, 0x0B, false)));
        assert_eq!(key("Digit1"), Some((2, 0x02, false)));
        assert_eq!(key("F1"), Some((59, 0x3B, false)));
        assert_eq!(key("F12"), Some((88, 0x58, false)));
        assert_eq!(key("F13"), Some((183, 0x64, false)));
        assert_eq!(key("ArrowUp"), Some((103, 0x48, true)));
        assert_eq!(key("Numpad7"), Some((71, 0x47, false)));
        assert_eq!(key("Bogus"), None);
    }

    #[test]
    fn releases_held_keys() {
        let st = |c: &str, down| MacroStep {
            code: Some(c.into()),
            down,
            delay_ms: 0,
        };
        let p = playable(&[st("ShiftLeft", true), st("KeyA", true), st("KeyA", false), st("Nope", true)]);
        assert_eq!(p.len(), 4);
        assert_eq!(p[3], st("ShiftLeft", false));
    }

    #[test]
    fn round_trip() {
        let a = HotkeyAction::LaunchApp {
            target: "C:\\Games\\a \"b\".exe".into(),
        };
        assert_eq!(decode(&encode(&a)), Some(a));
    }
}
