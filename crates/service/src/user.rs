//! Running programmable-key actions as the signed-in user.
//!
//! The service runs as SYSTEM / root, but opening a program, Sticky Keys
//! and typing a macro belong to whoever is at the keyboard. As PredatorSense
//! does with its per-user agent, the service starts a copy of itself in the
//! active session, as that user (`opensenses-service user-action …`), and
//! that copy does the work. Nothing a key is set to ever runs as SYSTEM or
//! root.
//!
//! Linux macros are the exception: only root can create a virtual keyboard
//! (/dev/uinput), so the service types those keys itself; it presses only
//! the keys in the macro.
//!
//! Windows also gets a small per-user agent for the PredatorSense key,
//! which reaches only the signed-in user's desktop (Raw Input).

use anyhow::Result;
use pcore::settings::HotkeyAction;

/// Carries out a user action without blocking the caller.
pub fn run(a: &HotkeyAction) -> Result<()> {
    #[cfg(target_os = "linux")]
    if let HotkeyAction::Macro { steps } = a {
        let steps = pcore::useraction::playable(steps);
        std::thread::spawn(move || {
            if let Err(e) = uinput::play(&steps) {
                log::warn!("macro: {e:#}");
            }
        });
        return Ok(());
    }
    let exe = std::env::current_exe()?;
    spawn_in_session(&exe.to_string_lossy(), &["user-action".to_string(), pcore::useraction::encode(a)]).map(|_| ())
}

/// Runs inside the user's session: `opensenses-service user-action <hex>`.
pub fn user_action(hex: &str) -> Result<()> {
    let a = pcore::useraction::decode(hex).ok_or_else(|| anyhow::anyhow!("bad action"))?;
    let app = pcore::settings::load().and_then(|s| s.app_path);
    pcore::useraction::perform(&a, app.as_deref())
}

#[cfg(windows)]
pub use win::{spawn_in_session, Agent};

#[cfg(windows)]
mod win {
    use anyhow::{bail, Result};
    use windows::core::{PCWSTR, PWSTR};
    use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
    use windows::Win32::System::Environment::{CreateEnvironmentBlock, DestroyEnvironmentBlock};
    use windows::Win32::System::RemoteDesktop::{WTSGetActiveConsoleSessionId, WTSQueryUserToken};
    use windows::Win32::System::Threading::{
        CreateProcessAsUserW, TerminateProcess, WaitForSingleObject, CREATE_NO_WINDOW, CREATE_UNICODE_ENVIRONMENT, PROCESS_INFORMATION, STARTUPINFOW,
    };

    fn quote(a: &str) -> String {
        if !a.is_empty() && !a.contains([' ', '\t', '"']) {
            return a.to_string();
        }
        format!("\"{}\"", a.replace('"', "\\\""))
    }

    /// The session at the physical console, if someone is signed in.
    pub fn console_session() -> Option<u32> {
        let id = unsafe { WTSGetActiveConsoleSessionId() };
        (id != u32::MAX).then_some(id)
    }

    /// Starts `exe args…` in the console session as its signed-in user
    /// (with their normal, non-elevated rights). Returns the process.
    pub fn spawn_in_session(exe: &str, args: &[String]) -> Result<HANDLE> {
        let Some(session) = console_session() else { bail!("nobody is signed in") };
        unsafe {
            let mut token = HANDLE::default();
            if WTSQueryUserToken(session, &mut token).is_err() {
                bail!("nobody is signed in");
            }
            let mut env: *mut core::ffi::c_void = std::ptr::null_mut();
            let have_env = CreateEnvironmentBlock(&mut env, token, false).is_ok();
            let mut cmd: Vec<u16> = std::iter::once(quote(exe))
                .chain(args.iter().map(|a| quote(a)))
                .collect::<Vec<_>>()
                .join(" ")
                .encode_utf16()
                .chain([0])
                .collect();
            let mut desktop: Vec<u16> = "winsta0\\default".encode_utf16().chain([0]).collect();
            let si = STARTUPINFOW {
                cb: std::mem::size_of::<STARTUPINFOW>() as u32,
                lpDesktop: PWSTR(desktop.as_mut_ptr()),
                ..Default::default()
            };
            let mut pi = PROCESS_INFORMATION::default();
            let exe_w: Vec<u16> = exe.encode_utf16().chain([0]).collect();
            let r = CreateProcessAsUserW(
                token,
                PCWSTR(exe_w.as_ptr()),
                PWSTR(cmd.as_mut_ptr()),
                None,
                None,
                false,
                CREATE_NO_WINDOW | if have_env { CREATE_UNICODE_ENVIRONMENT } else { Default::default() },
                if have_env { Some(env as *const _) } else { None },
                PCWSTR::null(),
                &si,
                &mut pi,
            );
            if have_env {
                let _ = DestroyEnvironmentBlock(env);
            }
            let _ = CloseHandle(token);
            r?;
            let _ = CloseHandle(pi.hThread);
            Ok(pi.hProcess)
        }
    }

    /// The per-user PredatorSense key agent, restarted when the signed-in
    /// user changes or it exits.
    #[derive(Default)]
    pub struct Agent {
        running: Option<(u32, HANDLE)>,
        last_try: Option<std::time::Instant>,
    }

    impl Agent {
        pub fn keep(&mut self, wanted: bool) {
            let session = console_session();
            if let Some((s, h)) = self.running {
                let exited = unsafe { WaitForSingleObject(h, 0) } == WAIT_OBJECT_0;
                if !wanted || exited || Some(s) != session {
                    self.stop();
                }
            }
            if !wanted || self.running.is_some() || self.last_try.map(|t| t.elapsed().as_secs() < 15).unwrap_or(false) {
                return;
            }
            self.last_try = Some(std::time::Instant::now());
            let (Some(s), Ok(exe)) = (session, std::env::current_exe()) else {
                return;
            };
            match spawn_in_session(&exe.to_string_lossy(), &["agent".to_string()]) {
                Ok(h) => {
                    log::info!("PredatorSense key agent started in session {s}");
                    self.running = Some((s, h));
                }
                Err(e) => log::debug!("PredatorSense key agent: {e:#}"),
            }
        }

        pub fn stop(&mut self) {
            if let Some((_, h)) = self.running.take() {
                unsafe {
                    let _ = TerminateProcess(h, 0);
                    let _ = CloseHandle(h);
                }
            }
        }
    }

    impl Drop for Agent {
        fn drop(&mut self) {
            self.stop();
        }
    }
}

/// `opensenses-service agent`: runs as the signed-in user and opens
/// OpenSenses when the PredatorSense key is pressed. The key is scan code
/// E0 75 (released), or HID usage page 0x88 report [_, 0x81, 0xFF] on some
/// keyboards, as PredatorSense's agent reads it.
#[cfg(windows)]
pub fn agent() -> Result<()> {
    use windows::core::w;
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::Input::{
        GetRawInputData, RegisterRawInputDevices, HRAWINPUT, RAWINPUT, RAWINPUTDEVICE, RAWINPUTHEADER, RIDEV_INPUTSINK, RID_INPUT, RIM_TYPEHID,
        RIM_TYPEKEYBOARD,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, RegisterClassW, MSG, WINDOW_EX_STYLE, WM_INPUT, WNDCLASSW, WS_OVERLAPPED,
    };

    static LAST: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);

    fn pressed() {
        let mut last = LAST.lock().unwrap();
        if last.map(|t| t.elapsed().as_millis() < 700).unwrap_or(false) {
            return;
        }
        *last = Some(std::time::Instant::now());
        drop(last);
        let app = pcore::settings::load().and_then(|s| s.app_path);
        let _ = pcore::useraction::perform(&HotkeyAction::OpenApp, app.as_deref());
    }

    unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        if msg == WM_INPUT {
            let mut size = 0u32;
            let hdr = std::mem::size_of::<RAWINPUTHEADER>() as u32;
            GetRawInputData(HRAWINPUT(lp.0 as _), RID_INPUT, None, &mut size, hdr);
            if size > 0 && size <= 4096 {
                let mut buf = vec![0u64; (size as usize + 7) / 8];
                if GetRawInputData(HRAWINPUT(lp.0 as _), RID_INPUT, Some(buf.as_mut_ptr() as *mut _), &mut size, hdr) == size {
                    let ri = &*(buf.as_ptr() as *const RAWINPUT);
                    if ri.header.dwType == RIM_TYPEKEYBOARD.0 {
                        let k = ri.data.keyboard;
                        // E0 prefix (2) + key up (1).
                        if k.MakeCode == 0x75 && k.Flags == 3 {
                            pressed();
                        }
                    } else if ri.header.dwType == RIM_TYPEHID.0 {
                        let h = ri.data.hid;
                        let n = (h.dwSizeHid * h.dwCount) as usize;
                        let data = std::slice::from_raw_parts(h.bRawData.as_ptr(), n.min(size as usize));
                        if data.len() >= 3 && data[1] == 0x81 && data[2] == 0xFF {
                            pressed();
                        }
                    }
                }
            }
        }
        DefWindowProcW(hwnd, msg, wp, lp)
    }

    unsafe {
        let inst = GetModuleHandleW(None)?;
        let class = w!("OpenSensesKeyAgent");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(proc),
            hInstance: inst.into(),
            lpszClassName: class,
            ..Default::default()
        };
        RegisterClassW(&wc);
        // Never shown; only there to receive Raw Input.
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class,
            w!("OpenSenses key agent"),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            None,
            None,
            inst,
            None,
        )?;
        let devs = [
            RAWINPUTDEVICE {
                usUsagePage: 0x01,
                usUsage: 0x06,
                dwFlags: RIDEV_INPUTSINK,
                hwndTarget: hwnd,
            },
            RAWINPUTDEVICE {
                usUsagePage: 0x88,
                usUsage: 0x01,
                dwFlags: RIDEV_INPUTSINK,
                hwndTarget: hwnd,
            },
        ];
        RegisterRawInputDevices(&devs, std::mem::size_of::<RAWINPUTDEVICE>() as u32)?;
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            DispatchMessageW(&msg);
        }
    }
    Ok(())
}

#[cfg(target_os = "linux")]
pub use lin::spawn_in_session;

#[cfg(target_os = "linux")]
mod lin {
    use anyhow::{anyhow, bail, Result};
    use std::process::{Command, Stdio};

    fn out(cmd: &str, args: &[&str]) -> Option<String> {
        let o = Command::new(cmd).args(args).stderr(Stdio::null()).output().ok()?;
        o.status.success().then(|| String::from_utf8_lossy(&o.stdout).trim().to_string())
    }

    /// The user of the active graphical session on seat0: (name, uid).
    fn active_user() -> Option<(String, u32)> {
        let session = out("loginctl", &["show-seat", "seat0", "-p", "ActiveSession", "--value"]).filter(|s| !s.is_empty())?;
        let props = out("loginctl", &["show-session", &session, "-p", "Name", "-p", "User"])?;
        let mut name = None;
        let mut uid = None;
        for l in props.lines() {
            match l.split_once('=') {
                Some(("Name", v)) => name = Some(v.to_string()),
                Some(("User", v)) => uid = v.parse().ok(),
                _ => {}
            }
        }
        Some((name?, uid?)).filter(|(_, u)| *u != 0)
    }

    /// Starts `exe args…` as the active desktop user, inside their own
    /// systemd user manager so it sees their display and desktop.
    pub fn spawn_in_session(exe: &str, args: &[String]) -> Result<()> {
        let (name, uid) = active_user().ok_or_else(|| anyhow!("nobody is signed in to a desktop"))?;
        let machine = format!("--machine={name}@.host");
        let ok = Command::new("systemd-run")
            // KillMode=process: a program the action opens keeps running
            // after the action itself exits.
            .args(["--user", &machine, "--collect", "--quiet", "--no-block", "-p", "KillMode=process", "--"])
            .arg(exe)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if ok {
            return Ok(());
        }
        // Older systemd: run it as the user with their session bus.
        let runtime = format!("/run/user/{uid}");
        let bus = format!("DBUS_SESSION_BUS_ADDRESS=unix:path={runtime}/bus");
        let rt = format!("XDG_RUNTIME_DIR={runtime}");
        let status = Command::new("runuser")
            .args(["-u", &name, "--", "env", &rt, &bus])
            .arg(exe)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        match status {
            Ok(_) => Ok(()),
            Err(e) => bail!("couldn't start the action as {name}: {e}"),
        }
    }
}

/// Typing macros on Linux through a short-lived virtual keyboard.
#[cfg(target_os = "linux")]
mod uinput {
    use anyhow::{bail, Result};
    use pcore::settings::MacroStep;
    use std::io::Write;
    use std::os::fd::AsRawFd;
    use std::time::Duration;

    const UI_SET_EVBIT: libc::c_ulong = 0x4004_5564;
    const UI_SET_KEYBIT: libc::c_ulong = 0x4004_5565;
    const UI_DEV_SETUP: libc::c_ulong = 0x405C_5503;
    const UI_DEV_CREATE: libc::c_ulong = 0x5501;
    const UI_DEV_DESTROY: libc::c_ulong = 0x5502;
    const EV_SYN: u16 = 0;
    const EV_KEY: u16 = 1;

    #[repr(C)]
    struct Setup {
        bustype: u16,
        vendor: u16,
        product: u16,
        version: u16,
        name: [u8; 80],
        ff_effects_max: u32,
    }

    #[repr(C)]
    struct Event {
        time: libc::timeval,
        kind: u16,
        code: u16,
        value: i32,
    }

    fn emit(f: &mut std::fs::File, kind: u16, code: u16, value: i32) -> std::io::Result<()> {
        let ev = Event {
            time: libc::timeval { tv_sec: 0, tv_usec: 0 },
            kind,
            code,
            value,
        };
        let bytes = unsafe { std::slice::from_raw_parts(&ev as *const Event as *const u8, std::mem::size_of::<Event>()) };
        f.write_all(bytes)
    }

    pub fn play(steps: &[MacroStep]) -> Result<()> {
        let keys: Vec<u16> = steps
            .iter()
            .filter_map(|s| s.code.as_deref().and_then(pcore::useraction::key))
            .map(|k| k.0)
            .collect();
        if keys.is_empty() {
            return Ok(());
        }
        let mut f = std::fs::OpenOptions::new().write(true).open("/dev/uinput")?;
        let fd = f.as_raw_fd();
        let mut name = [0u8; 80];
        name[..19].copy_from_slice(b"OpenSenses macro kb");
        let setup = Setup {
            bustype: 0x06,
            vendor: 0x1d6b,
            product: 0x0105,
            version: 1,
            name,
            ff_effects_max: 0,
        };
        unsafe {
            if libc::ioctl(fd, UI_SET_EVBIT, EV_KEY as libc::c_int) < 0 {
                bail!("uinput: {}", std::io::Error::last_os_error());
            }
            for k in &keys {
                libc::ioctl(fd, UI_SET_KEYBIT, *k as libc::c_int);
            }
            if libc::ioctl(fd, UI_DEV_SETUP, &setup as *const Setup) < 0 || libc::ioctl(fd, UI_DEV_CREATE) < 0 {
                bail!("uinput: {}", std::io::Error::last_os_error());
            }
        }
        // Give the desktop a moment to pick up the new keyboard.
        std::thread::sleep(Duration::from_millis(300));
        let mut result = Ok(());
        for s in steps {
            match s.code.as_deref().and_then(pcore::useraction::key) {
                Some((code, _, _)) => {
                    if let Err(e) = emit(&mut f, EV_KEY, code, s.down as i32).and_then(|_| emit(&mut f, EV_SYN, 0, 0)) {
                        result = Err(e.into());
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(pcore::useraction::KEY_GAP_MS));
                }
                None => std::thread::sleep(Duration::from_millis(s.delay_ms as u64)),
            }
        }
        std::thread::sleep(Duration::from_millis(50));
        unsafe { libc::ioctl(fd, UI_DEV_DESTROY) };
        result
    }
}

/// Watches the keyboards for the PredatorSense key, which Linux reports as
/// KEY_PRESENTATION on Acer laptops (systemd's keyboard database).
#[cfg(target_os = "linux")]
pub fn watch_ps_key(tx: std::sync::mpsc::Sender<crate::daemon::Job>) {
    use std::io::Read;
    use std::os::fd::AsRawFd;
    const KEY_PRESENTATION: usize = 425;
    // EVIOCGBIT(EV_KEY, 96)
    const EVIOCGBIT_KEY: libc::c_ulong = 0x8060_4521;
    let has_key = |f: &std::fs::File| -> bool {
        let mut bits = [0u8; 96];
        let n = unsafe { libc::ioctl(f.as_raw_fd(), EVIOCGBIT_KEY, bits.as_mut_ptr()) };
        n > 0 && bits[KEY_PRESENTATION / 8] & (1 << (KEY_PRESENTATION % 8)) != 0
    };
    loop {
        let mut found = Vec::new();
        if let Ok(dir) = std::fs::read_dir("/dev/input") {
            for e in dir.flatten() {
                let p = e.path();
                if !p.file_name().and_then(|n| n.to_str()).map(|n| n.starts_with("event")).unwrap_or(false) {
                    continue;
                }
                if let Ok(f) = std::fs::File::open(&p) {
                    if has_key(&f) {
                        found.push(f);
                    }
                }
            }
        }
        if found.is_empty() {
            std::thread::sleep(std::time::Duration::from_secs(60));
            continue;
        }
        let mut fds: Vec<libc::pollfd> = found
            .iter()
            .map(|f| libc::pollfd {
                fd: f.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            })
            .collect();
        // struct input_event is 24 bytes on 64-bit.
        let mut buf = [0u8; 24 * 16];
        'watch: loop {
            if unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, -1) } < 0 {
                if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                break;
            }
            for (i, pfd) in fds.iter_mut().enumerate() {
                if pfd.revents & (libc::POLLERR | libc::POLLHUP) != 0 {
                    break 'watch;
                }
                if pfd.revents & libc::POLLIN == 0 {
                    continue;
                }
                let Ok(n) = (&found[i]).read(&mut buf) else {
                    break 'watch;
                };
                for ev in buf[..n].chunks_exact(24) {
                    let kind = u16::from_ne_bytes([ev[16], ev[17]]);
                    let code = u16::from_ne_bytes([ev[18], ev[19]]);
                    let value = i32::from_ne_bytes([ev[20], ev[21], ev[22], ev[23]]);
                    if kind == 1 && code as usize == KEY_PRESENTATION && value == 1 && tx.send(crate::daemon::Job::PredatorSenseKey).is_err() {
                        return;
                    }
                }
            }
        }
        std::thread::sleep(std::time::Duration::from_secs(5));
    }
}
