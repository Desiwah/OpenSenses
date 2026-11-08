//! Background service. `run` runs it in the foreground (handy for testing),
//! `install` / `uninstall` register it with the OS, and on Windows the
//! Service Control Manager starts it with `service`.

mod daemon;
mod server;

use daemon::Job;
use std::sync::mpsc;

fn init_log() {
    use simplelog::{ConfigBuilder, LevelFilter, WriteLogger};
    let dir = pcore::settings::settings_path();
    let dir = dir.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("service.log");
    // Keep the log small: start over once it passes 1 MB.
    if std::fs::metadata(&path).map(|m| m.len() > 1_000_000).unwrap_or(false) {
        let _ = std::fs::remove_file(&path);
    }
    if let Ok(f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = WriteLogger::init(LevelFilter::Info, ConfigBuilder::new().build(), f);
    }
}

/// Starts the IPC server and runs the hardware loop until `stop` fires.
pub fn run_until(stop: mpsc::Receiver<()>) {
    let (tx, rx) = mpsc::channel::<Job>();
    let server_tx = tx.clone();
    std::thread::spawn(move || {
        if let Err(e) = server::serve(server_tx) {
            log::error!("IPC server: {e:#}");
        }
    });
    std::thread::spawn(move || {
        let _ = stop.recv();
        let _ = tx.send(Job::Stop);
    });
    daemon::run(rx);
}

/// Prints what the hardware reports right now (needs admin on Windows).
fn status() -> anyhow::Result<()> {
    let mut hw = pcore::backend::open()?;
    println!("backend: {}", hw.name());
    println!("capabilities: {:?}", hw.capabilities());
    println!("sensors: {:?}", hw.sensors());
    println!("observed: {:?}", hw.observe());
    for line in hw.diagnostics() {
        println!("{line}");
    }
    Ok(())
}

fn main() {
    let arg = std::env::args().nth(1).unwrap_or_default();
    let result = match arg.as_str() {
        "install" => platform::install(),
        "uninstall" => platform::uninstall(),
        "run" => {
            init_log();
            let (_keep, stop) = mpsc::channel();
            run_until(stop);
            Ok(())
        }
        "status" => status(),
        "service" => {
            init_log();
            platform::run_as_service()
        }
        _ => {
            eprintln!("usage: opensenses-service <install | uninstall | run | status>");
            std::process::exit(2);
        }
    };
    if let Err(e) = result {
        eprintln!("error: {e:#}");
        log::error!("{e:#}");
        std::process::exit(1);
    }
}

#[cfg(windows)]
mod platform {
    use anyhow::{Context, Result};
    use std::ffi::OsString;
    use std::sync::mpsc;
    use std::time::Duration;
    use windows_service::service::{
        ServiceAccess, ServiceControl, ServiceControlAccept, ServiceErrorControl, ServiceExitCode, ServiceInfo,
        ServiceStartType, ServiceState, ServiceStatus, ServiceType,
    };
    use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
    use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};
    use windows_service::{define_windows_service, service_dispatcher};

    pub const NAME: &str = "OpenSenses";
    const DISPLAY: &str = "OpenSenses hardware service";

    define_windows_service!(ffi_main, service_main);

    pub fn run_as_service() -> Result<()> {
        service_dispatcher::start(NAME, ffi_main).context("not started by the Service Control Manager")
    }

    fn service_main(_args: Vec<OsString>) {
        let (stop_tx, stop_rx) = mpsc::channel();
        let handler = move |c| match c {
            ServiceControl::Stop | ServiceControl::Shutdown => {
                let _ = stop_tx.send(());
                ServiceControlHandlerResult::NoError
            }
            ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
            _ => ServiceControlHandlerResult::NotImplemented,
        };
        let Ok(status) = service_control_handler::register(NAME, handler) else { return };
        let set = |state, accept| {
            let _ = status.set_service_status(ServiceStatus {
                service_type: ServiceType::OWN_PROCESS,
                current_state: state,
                controls_accepted: accept,
                exit_code: ServiceExitCode::Win32(0),
                checkpoint: 0,
                wait_hint: Duration::from_secs(5),
                process_id: None,
            });
        };
        set(ServiceState::Running, ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN);
        crate::run_until(stop_rx);
        set(ServiceState::Stopped, ServiceControlAccept::empty());
    }

    pub fn install() -> Result<()> {
        let mgr = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE)
            .context("run this as administrator")?;
        let exe = std::env::current_exe()?;
        let info = ServiceInfo {
            name: NAME.into(),
            display_name: DISPLAY.into(),
            service_type: ServiceType::OWN_PROCESS,
            start_type: ServiceStartType::AutoStart,
            error_control: ServiceErrorControl::Normal,
            executable_path: exe,
            launch_arguments: vec!["service".into()],
            dependencies: vec![],
            account_name: None, // LocalSystem
            account_password: None,
        };
        let access = ServiceAccess::QUERY_STATUS | ServiceAccess::START | ServiceAccess::STOP | ServiceAccess::CHANGE_CONFIG;
        let svc = match mgr.open_service(NAME, access) {
            Ok(s) => {
                // Upgrade: point the existing service at this exe.
                let _ = s.stop();
                std::thread::sleep(Duration::from_secs(1));
                s.change_config(&info)?;
                s
            }
            Err(_) => mgr.create_service(&info, access)?,
        };
        svc.set_description("Applies keyboard lighting, fan and GPU settings and keeps them applied.")?;
        svc.start::<&str>(&[])?;
        println!("service installed and started");
        Ok(())
    }

    pub fn uninstall() -> Result<()> {
        let mgr = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)?;
        let svc = mgr.open_service(NAME, ServiceAccess::QUERY_STATUS | ServiceAccess::STOP | ServiceAccess::DELETE)?;
        if svc.query_status()?.current_state != ServiceState::Stopped {
            let _ = svc.stop();
            std::thread::sleep(Duration::from_secs(2));
        }
        svc.delete()?;
        println!("service removed");
        Ok(())
    }
}

#[cfg(not(windows))]
mod platform {
    use anyhow::{Context, Result};
    use std::process::Command;

    const UNIT: &str = "/etc/systemd/system/opensenses.service";
    /// Where the service keeps its own copy when it isn't installed by a
    /// package: an AppImage runs from a temporary mount that disappears
    /// when the app closes, and /usr is read-only on Bazzite and other
    /// image-based distros, while /usr/local is writable on all of them.
    const COPY: &str = "/usr/local/lib/opensenses/opensenses-service";

    pub fn run_as_service() -> Result<()> {
        let (_keep, stop) = std::sync::mpsc::channel();
        crate::run_until(stop);
        Ok(())
    }

    pub fn install() -> Result<()> {
        let mut exe = std::env::current_exe()?;
        let packaged = exe.starts_with("/usr/bin") || exe.starts_with("/usr/lib");
        if !packaged {
            let dest = std::path::Path::new(COPY);
            std::fs::create_dir_all(dest.parent().unwrap()).context("run this with sudo")?;
            // Replace rather than overwrite, so a running copy isn't disturbed.
            let tmp = dest.with_extension("new");
            std::fs::copy(&exe, &tmp).context("copying the service")?;
            std::fs::rename(&tmp, dest)?;
            exe = dest.to_path_buf();
        }
        let unit = format!(
            "[Unit]\nDescription=OpenSenses hardware service\nAfter=multi-user.target\n\n\
             [Service]\nExecStart={} run\nRestart=on-failure\n\n[Install]\nWantedBy=multi-user.target\n",
            exe.display()
        );
        std::fs::write(UNIT, unit).context("run this with sudo")?;
        Command::new("systemctl").args(["daemon-reload"]).status()?;
        Command::new("systemctl").args(["enable", "--now", "opensenses.service"]).status()?;
        println!("service installed and started");
        Ok(())
    }

    pub fn uninstall() -> Result<()> {
        let _ = Command::new("systemctl").args(["disable", "--now", "opensenses.service"]).status();
        let _ = std::fs::remove_file(UNIT);
        let _ = std::fs::remove_dir_all(std::path::Path::new(COPY).parent().unwrap());
        Command::new("systemctl").args(["daemon-reload"]).status()?;
        println!("service removed");
        Ok(())
    }
}
