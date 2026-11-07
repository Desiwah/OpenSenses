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

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![get_state, set_settings, reapply, install_service])
        .run(tauri::generate_context!())
        .expect("error while running the app");
}
