//! Background service. `run` runs it in the foreground (handy for testing),
//! `install` / `uninstall` register it with the OS, and on Windows the
//! Service Control Manager starts it with `service`.

mod daemon;
mod server;
mod user;

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
    // Key events from the firmware (Turbo key, programmable keys).
    let event_tx = tx.clone();
    std::thread::spawn(move || {
        let (etx, erx) = mpsc::channel::<[u8; 8]>();
        std::thread::spawn(move || pcore::backend::listen_events(etx));
        for e in erx {
            if event_tx.send(Job::Firmware(e)).is_err() {
                break;
            }
        }
    });
    #[cfg(target_os = "linux")]
    {
        let key_tx = tx.clone();
        std::thread::spawn(move || user::watch_ps_key(key_tx));
    }
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
    println!("live snapshot: {:?}", hw.snapshot());
    println!("saved original: {:?}", pcore::snapshot::load());
    Ok(())
}

/// Puts the laptop's original settings back; the uninstallers run this.
/// Goes through the running service when there is one (so it also stops
/// re-applying), otherwise talks to the hardware directly.
fn restore() -> anyhow::Result<()> {
    use pcore::ipc::{request, Request, Response};
    match request(&Request::RestoreOriginal) {
        Ok(Response::State(st)) => {
            for e in &st.errors {
                eprintln!("{e}");
            }
            println!("original settings restored");
            return Ok(());
        }
        Ok(Response::Error(e)) => eprintln!("service: {e}"),
        Err(_) => {}
    }
    let snap = pcore::snapshot::load().ok_or_else(|| anyhow::anyhow!("no original settings were saved on this laptop"))?;
    let mut hw = pcore::backend::open()?;
    for e in hw.restore(&snap) {
        eprintln!("{e}");
    }
    if let Some(mut s) = pcore::settings::load() {
        s.restored = true;
        let _ = pcore::settings::save(&s);
    }
    println!("original settings restored");
    Ok(())
}

fn main() {
    let arg = std::env::args().nth(1).unwrap_or_default();
    let result = match arg.as_str() {
        "restore" => restore(),
        "install" => platform::install(),
        "uninstall" => platform::uninstall(),
        "run" => {
            init_log();
            let (_keep, stop) = mpsc::channel();
            run_until(stop);
            Ok(())
        }
        "status" => status(),
        // Started by the service in the signed-in user's session.
        "user-action" => user::user_action(&std::env::args().nth(2).unwrap_or_default()),
        #[cfg(windows)]
        "agent" => user::agent(),
        "service" => {
            init_log();
            platform::run_as_service()
        }
        _ => {
            eprintln!("usage: opensenses-service <install | uninstall | restore | run | status>");
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
        ServiceAccess, ServiceControl, ServiceControlAccept, ServiceErrorControl, ServiceExitCode, ServiceInfo, ServiceStartType, ServiceState, ServiceStatus,
        ServiceType,
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
        let Ok(status) = service_control_handler::register(NAME, handler) else {
            return;
        };
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

/// Builds and loads the OpenSenses kernel driver on Linux (driver/ in the
/// source tree; shipped next to the service in every package).
#[cfg(target_os = "linux")]
mod driver {
    use std::path::{Path, PathBuf};
    use std::process::Command;

    const VERSION: &str = "0.1.0";
    const FILES: &[&str] = &["opensenses.c", "Makefile", "dkms.conf"];

    fn loaded() -> bool {
        Path::new(pcore::backend::linux::DEVICE).exists()
    }

    fn run(cmd: &str, args: &[&str]) -> Result<(), String> {
        match Command::new(cmd).args(args).output() {
            Ok(o) if o.status.success() => Ok(()),
            Ok(o) => Err(String::from_utf8_lossy(&o.stderr).trim().to_string()),
            Err(e) => Err(format!("{cmd}: {e}")),
        }
    }

    fn have(cmd: &str) -> bool {
        Command::new("sh")
            .args(["-c", &format!("command -v {cmd}")])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    /// The driver sources: next to the service (AppImage, packages) or
    /// where a package or an earlier install put them.
    pub fn sources(exe: &Path) -> Option<PathBuf> {
        let dir = exe.parent()?;
        [
            dir.join("driver"),
            PathBuf::from(format!("/usr/src/opensenses-{VERSION}")),
            PathBuf::from("/usr/local/lib/opensenses/driver"),
        ]
        .into_iter()
        .find(|d| d.join("opensenses.c").is_file())
    }

    fn copy_sources(from: &Path, to: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(to)?;
        for f in FILES {
            if from != to {
                std::fs::copy(from.join(f), to.join(f))?;
            }
        }
        Ok(())
    }

    fn load() -> Result<(), String> {
        run("modprobe", &["opensenses"])?;
        // udev creates the device a moment later.
        for _ in 0..20 {
            if loaded() {
                return Ok(());
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        Err("the driver loaded but /dev/opensenses didn't appear".into())
    }

    /// Makes sure the driver is installed and loaded; returns what happened.
    pub fn ensure(sources: Option<&Path>) -> String {
        if loaded() || load().is_ok() {
            return "OpenSenses driver: loaded".into();
        }
        let Some(src) = sources else {
            return "OpenSenses driver: sources not found next to the service".into();
        };
        // Keep a copy beside the installed service, for reinstalls.
        let keep = Path::new("/usr/local/lib/opensenses/driver");
        let _ = copy_sources(src, keep);

        // DKMS rebuilds the driver for every new kernel, and on most
        // distributions signs it for Secure Boot.
        if have("dkms") {
            let tree = PathBuf::from(format!("/usr/src/opensenses-{VERSION}"));
            if copy_sources(src, &tree).is_ok() {
                let _ = run("dkms", &["add", "-m", "opensenses", "-v", VERSION]);
                return match run("dkms", &["install", "-m", "opensenses", "-v", VERSION]).and_then(|_| load()) {
                    Ok(()) => "OpenSenses driver: installed with DKMS and loaded".into(),
                    Err(e) => format!(
                        "OpenSenses driver: DKMS couldn't build or load it ({e}). Install your kernel's headers \
                         (e.g. linux-headers / kernel-devel) and run this again; with Secure Boot on, enroll the \
                         DKMS signing key when asked."
                    ),
                };
            }
        }

        // No DKMS (or /usr is read-only): build for the running kernel and
        // keep the module under /var, where the service loads it at start.
        let release = std::fs::read_to_string("/proc/sys/kernel/osrelease").unwrap_or_default().trim().to_string();
        let kdir = format!("/lib/modules/{release}/build");
        if !Path::new(&kdir).exists() || !have("make") {
            return "OpenSenses driver: not installed. This needs DKMS or your kernel's headers and a compiler; \
                    on image-based systems such as Bazzite or SteamOS the driver has to come with the system."
                .into();
        }
        let build = Path::new("/var/lib/OpenSenses/driver-build");
        let _ = std::fs::remove_dir_all(build);
        if let Err(e) = copy_sources(src, build) {
            return format!("OpenSenses driver: couldn't prepare the build ({e})");
        }
        let m = format!("M={}", build.display());
        if let Err(e) = run("make", &["-C", &kdir, &m, "modules"]) {
            return format!("OpenSenses driver: building failed ({e})");
        }
        let ko = pcore::backend::linux::local_module(&release);
        if let Err(e) = std::fs::copy(build.join("opensenses.ko"), &ko) {
            return format!("OpenSenses driver: couldn't keep the built driver ({e})");
        }
        match run("insmod", &[&ko.to_string_lossy()]) {
            Ok(()) => "OpenSenses driver: built for this kernel and loaded".into(),
            Err(e) => format!("OpenSenses driver: built, but the kernel refused to load it ({e}); with Secure Boot on it has to be signed"),
        }
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
        let sources = crate::driver::sources(&exe);
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
        println!("{}", crate::driver::ensure(sources.as_deref()));
        let unit = format!(
            "[Unit]\nDescription=OpenSenses hardware service\nAfter=multi-user.target\n\n\
             [Service]\nExecStart={} run\nRestart=on-failure\n\n[Install]\nWantedBy=multi-user.target\n",
            exe.display()
        );
        std::fs::write(UNIT, unit).context("run this with sudo")?;
        Command::new("systemctl").args(["daemon-reload"]).status()?;
        Command::new("systemctl").args(["enable", "opensenses.service"]).status()?;
        // Restart rather than start, so a running service picks up a newly
        // built driver.
        Command::new("systemctl").args(["restart", "opensenses.service"]).status()?;
        println!("service installed and started");
        Ok(())
    }

    /// On Linux this is only used for a real removal (package removal or
    /// by hand), so it puts the laptop's original settings back first.
    pub fn uninstall() -> Result<()> {
        if let Err(e) = crate::restore() {
            eprintln!("restoring the original settings: {e:#}");
        }
        let _ = Command::new("systemctl").args(["disable", "--now", "opensenses.service"]).status();
        let _ = std::fs::remove_file(UNIT);
        let _ = std::fs::remove_dir_all(std::path::Path::new(COPY).parent().unwrap());
        Command::new("systemctl").args(["daemon-reload"]).status()?;
        println!("service removed");
        Ok(())
    }
}
