// No console window behind the app on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

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
    None
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
    {
        let _ = on;
        Err("Sticky Keys is a Windows setting".into())
    }
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![get_state, set_settings, reapply, install_service, get_sticky_keys, set_sticky_keys])
        .run(tauri::generate_context!())
        .expect("error while running the app");
}
