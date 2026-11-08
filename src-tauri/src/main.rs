// No console window behind the app on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod icon;

use pcore::ipc::{request, Request, Response, State};
use pcore::settings::Settings;

fn call(req: Request) -> Result<State, String> {
    // The pipe can be briefly busy between connections; retry a few times.
    let mut last = String::new();
    for _ in 0..10 {
        match request(&req) {
            Ok(Response::State(s)) => return Ok(s),
            Ok(Response::Error(e)) => return Err(e),
            Err(e) => last = format!("{e:#}"),
        }
        std::thread::sleep(std::time::Duration::from_millis(60));
    }
    Err(last)
}

#[tauri::command]
async fn get_state() -> Result<State, String> {
    tauri::async_runtime::spawn_blocking(|| call(Request::GetState)).await.map_err(|e| e.to_string())?
}

#[tauri::command]
async fn set_settings(settings: Settings) -> Result<State, String> {
    tauri::async_runtime::spawn_blocking(move || call(Request::SetSettings { settings }))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn reapply() -> Result<State, String> {
    tauri::async_runtime::spawn_blocking(|| call(Request::Reapply)).await.map_err(|e| e.to_string())?
}

/// Programs running in this Windows session, for picking a game to add.
#[tauri::command]
async fn running_apps() -> Vec<pcore::procs::App> {
    tauri::async_runtime::spawn_blocking(pcore::procs::user_apps).await.unwrap_or_default()
}

/// Where a game's program lives, from Windows' records of programs that
/// have been run, for games added by name.
#[tauri::command]
async fn find_program(exe: String) -> Option<String> {
    tauri::async_runtime::spawn_blocking(move || pcore::procs::find_program(&exe)).await.ok().flatten()
}

/// Lets the user pick a game's program file.
#[tauri::command]
async fn pick_program(app: tauri::AppHandle) -> Option<String> {
    use tauri_plugin_dialog::DialogExt;
    let mut d = app.dialog().file().set_title("Choose the game's program file");
    if cfg!(windows) {
        d = d.add_filter("Programs", &["exe"]);
    }
    tauri::async_runtime::spawn_blocking(move || d.blocking_pick_file())
        .await
        .ok()
        .flatten()
        .and_then(|p| p.into_path().ok())
        .map(|p| p.to_string_lossy().into_owned())
}

/// A program's icon as a data URL, for the game list.
#[tauri::command]
async fn app_icon(path: String) -> Option<String> {
    tauri::async_runtime::spawn_blocking(move || icon::data_url(&path)).await.ok().flatten()
}

/// Registers the background service when it is missing (asks for admin).
#[tauri::command]
fn install_service(app: tauri::AppHandle) -> Result<(), String> {
    use tauri::Manager;
    let dir = app.path().resource_dir().map_err(|e| e.to_string())?;
    #[cfg(windows)]
    {
        let svc = dir.join("opensenses-service.exe");
        if !svc.exists() {
            return Err(format!("{} is missing; reinstall the app", svc.display()));
        }
        let arg = format!(
            "Start-Process -FilePath '{}' -ArgumentList install -Verb RunAs -Wait -WindowStyle Hidden",
            svc.display().to_string().replace('\'', "''")
        );
        std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", &arg])
            .status()
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let svc = dir.join("opensenses-service");
        std::process::Command::new("pkexec")
            .arg(&svc)
            .arg("install")
            .status()
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}

/// Windows Sticky Keys belong to the signed-in user, not the hardware, so
/// the app sets them itself instead of going through the service.
#[tauri::command]
fn get_sticky_keys() -> Option<bool> {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::UI::Accessibility::{SKF_STICKYKEYSON, STICKYKEYS};
        use windows::Win32::UI::WindowsAndMessaging::{SystemParametersInfoW, SPI_GETSTICKYKEYS, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS};
        let mut sk = STICKYKEYS { cbSize: std::mem::size_of::<STICKYKEYS>() as u32, ..Default::default() };
        SystemParametersInfoW(SPI_GETSTICKYKEYS, sk.cbSize, Some(&mut sk as *mut _ as *mut _), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0)).ok()?;
        Some(sk.dwFlags.0 & SKF_STICKYKEYSON.0 != 0)
    }
    #[cfg(not(windows))]
    linux_a11y::get()
}

#[tauri::command]
fn set_sticky_keys(on: bool) -> Result<(), String> {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::UI::Accessibility::{SKF_STICKYKEYSON, STICKYKEYS, STICKYKEYS_FLAGS};
        use windows::Win32::UI::WindowsAndMessaging::{
            SystemParametersInfoW, SPIF_SENDCHANGE, SPIF_UPDATEINIFILE, SPI_GETSTICKYKEYS, SPI_SETSTICKYKEYS,
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
        };
        let mut sk = STICKYKEYS { cbSize: std::mem::size_of::<STICKYKEYS>() as u32, ..Default::default() };
        SystemParametersInfoW(SPI_GETSTICKYKEYS, sk.cbSize, Some(&mut sk as *mut _ as *mut _), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0))
            .map_err(|e| e.to_string())?;
        sk.dwFlags = if on {
            STICKYKEYS_FLAGS(sk.dwFlags.0 | SKF_STICKYKEYSON.0)
        } else {
            STICKYKEYS_FLAGS(sk.dwFlags.0 & !SKF_STICKYKEYSON.0)
        };
        SystemParametersInfoW(SPI_SETSTICKYKEYS, sk.cbSize, Some(&mut sk as *mut _ as *mut _), SPIF_UPDATEINIFILE | SPIF_SENDCHANGE)
            .map_err(|e| e.to_string())
    }
    #[cfg(not(windows))]
    linux_a11y::set(on)
}

/// Sticky Keys on Linux is a desktop setting: KDE Plasma keeps it in
/// kaccessrc, GNOME in its accessibility settings.
#[cfg(not(windows))]
mod linux_a11y {
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

    pub fn set(on: bool) -> Result<(), String> {
        let v = if on { "true" } else { "false" };
        let ok = match desktop().ok_or("Sticky Keys can be changed on KDE Plasma and GNOME desktops")? {
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
            Err("couldn't change the desktop's Sticky Keys setting".into())
        }
    }
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![get_state, set_settings, reapply, running_apps, app_icon, find_program, pick_program, install_service, get_sticky_keys, set_sticky_keys])
        .run(tauri::generate_context!())
        .expect("error while running the app");
}
