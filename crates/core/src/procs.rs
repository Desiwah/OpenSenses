//! Which programs are running, for GameSync. Names are lower-case file
//! names ("eldenring.exe"), the same form `settings::exe_name` produces.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

/// A running program: its lower-case file name and, when it can be read,
/// the full path of its executable (used for the game's icon).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct App {
    pub name: String,
    pub path: Option<String>,
}

/// Every running program, in any session (the service sees them all).
pub fn running() -> HashSet<String> {
    list(false).into_iter().map(|a| a.name).collect()
}

/// Programs running in the signed-in user's session, sorted, for the app's
/// "pick a running game" list.
pub fn user_apps() -> Vec<App> {
    let mut by_name: BTreeMap<String, Option<String>> = BTreeMap::new();
    for a in list(true) {
        if IGNORED.contains(&a.name.as_str()) {
            continue;
        }
        let slot = by_name.entry(a.name).or_default();
        if slot.is_none() {
            *slot = a.path;
        }
    }
    by_name.into_iter().map(|(name, path)| App { name, path }).collect()
}

/// Windows and desktop plumbing nobody would make a game profile for.
const IGNORED: &[&str] = &[
    "applicationframehost.exe", "conhost.exe", "ctfmon.exe", "dllhost.exe", "explorer.exe", "fontdrvhost.exe",
    "msedgewebview2.exe", "opensenses.exe", "runtimebroker.exe", "searchhost.exe", "searchindexer.exe",
    "shellexperiencehost.exe", "sihost.exe", "smartscreen.exe", "startmenuexperiencehost.exe", "svchost.exe",
    "taskhostw.exe", "textinputhost.exe", "widgets.exe", "dwm.exe", "csrss.exe", "winlogon.exe",
    "systemsettings.exe", "securityhealthsystray.exe", "lockapp.exe", "useroobebroker.exe",
];

#[cfg(windows)]
fn list(this_session_only: bool) -> Vec<App> {
    use windows::core::PWSTR;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
    };
    use windows::Win32::System::RemoteDesktop::ProcessIdToSessionId;
    use windows::Win32::System::Threading::{
        GetCurrentProcessId, OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    // Full path only for the app's list; the service just needs names.
    let path_of = |pid: u32| -> Option<String> {
        unsafe {
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
            let mut buf = [0u16; 1024];
            let mut len = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
            let _ = CloseHandle(h);
            ok.then(|| String::from_utf16_lossy(&buf[..len as usize]))
        }
    };

    let mut out = Vec::new();
    unsafe {
        let mine = {
            let mut s = 0u32;
            let _ = ProcessIdToSessionId(GetCurrentProcessId(), &mut s);
            s
        };
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return out };
        let mut e = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut ok = Process32FirstW(snap, &mut e).is_ok();
        while ok {
            let keep = !this_session_only || {
                let mut s = u32::MAX;
                ProcessIdToSessionId(e.th32ProcessID, &mut s).is_ok() && s == mine
            };
            if keep {
                let len = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(e.szExeFile.len());
                let name = String::from_utf16_lossy(&e.szExeFile[..len]).to_lowercase();
                if !name.is_empty() {
                    let path = if this_session_only { path_of(e.th32ProcessID) } else { None };
                    out.push(App { name, path });
                }
            }
            ok = Process32NextW(snap, &mut e).is_ok();
        }
        let _ = CloseHandle(snap);
    }
    out
}

#[cfg(unix)]
fn list(this_user_only: bool) -> Vec<App> {
    use std::os::unix::fs::MetadataExt;
    let me = std::fs::metadata("/proc/self").map(|m| m.uid()).ok();
    let mut out = Vec::new();
    let Ok(dir) = std::fs::read_dir("/proc") else { return out };
    for entry in dir.flatten() {
        let name = entry.file_name();
        if !name.to_string_lossy().bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let path = entry.path();
        if this_user_only {
            let uid = std::fs::metadata(&path).map(|m| m.uid()).ok();
            if uid.is_none() || uid != me {
                continue;
            }
        }
        // The first command-line argument holds the full name, including
        // Windows games run through Wine or Proton ("Z:\...\game.exe");
        // `comm` is cut to 15 characters, so it is only the fallback.
        let arg0 = std::fs::read(path.join("cmdline"))
            .ok()
            .and_then(|b| b.split(|&c| c == 0).next().map(|a| String::from_utf8_lossy(a).into_owned()))
            .filter(|a| !a.is_empty());
        let n = match arg0 {
            Some(a) => crate::settings::exe_name(&a),
            None => std::fs::read_to_string(path.join("comm")).map(|c| c.trim().to_lowercase()).unwrap_or_default(),
        };
        if !n.is_empty() {
            let path = if this_user_only {
                std::fs::read_link(path.join("exe")).ok().map(|p| p.to_string_lossy().into_owned())
            } else {
                None
            };
            out.push(App { name: n, path });
        }
    }
    out
}

#[cfg(not(any(windows, unix)))]
fn list(_: bool) -> Vec<App> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    #[test]
    fn finds_this_test() {
        let me = std::env::current_exe().unwrap();
        let name = crate::settings::exe_name(&me.to_string_lossy());
        assert!(super::running().contains(&name), "{name} not in the process list");
    }
}
