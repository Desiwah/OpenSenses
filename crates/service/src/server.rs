//! Local-only IPC endpoint. Each connection sends one request line and gets
//! one reply line. Requests are only typed settings (never raw firmware
//! calls), and settings are range-checked before use.

use crate::daemon::Job;
use anyhow::Result;
use pcore::ipc::{read_line, Request, Response};
use std::io::{BufReader, Write};
use std::sync::mpsc::{self, Sender};
use std::time::Duration;

fn handle<S: std::io::Read + Write>(stream: S, jobs: &Sender<Job>) {
    let mut reader = BufReader::new(stream);
    let reply = match read_line(&mut reader) {
        Ok(Some(line)) => match serde_json::from_str::<Request>(&line) {
            Ok(req) => {
                let (tx, rx) = mpsc::channel();
                if jobs.send(Job::Ipc(req, tx)).is_err() {
                    return;
                }
                rx.recv_timeout(Duration::from_secs(20))
                    .unwrap_or_else(|_| Response::Error("hardware did not answer in time".into()))
            }
            Err(e) => Response::Error(format!("bad request: {e}")),
        },
        Ok(None) => return,
        Err(e) => Response::Error(format!("bad request: {e}")),
    };
    let mut out = serde_json::to_string(&reply).unwrap_or_default();
    out.push('\n');
    let stream = reader.get_mut();
    let _ = stream.write_all(out.as_bytes());
    let _ = stream.flush();
}

#[cfg(unix)]
pub fn serve(jobs: Sender<Job>) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::UnixListener;
    let path = pcore::SOCKET_PATH;
    let _ = std::fs::remove_file(path);
    let listener = UnixListener::bind(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o666))?;
    for conn in listener.incoming() {
        match conn {
            Ok(s) => {
                let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
                let jobs = jobs.clone();
                std::thread::spawn(move || handle(s, &jobs));
            }
            Err(e) => log::warn!("accept: {e}"),
        }
    }
    Ok(())
}

#[cfg(windows)]
pub fn serve(jobs: Sender<Job>) -> Result<()> {
    use std::fs::File;
    use std::os::windows::io::FromRawHandle;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{ERROR_PIPE_CONNECTED, INVALID_HANDLE_VALUE};
    use windows::Win32::Security::Authorization::{
        ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
    };
    use windows::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
    use windows::Win32::Storage::FileSystem::PIPE_ACCESS_DUPLEX;
    use windows::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE,
        PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
    };

    // SYSTEM and Administrators: full access. Interactive (logged-on) users:
    // read/write so the app can talk to us without admin rights. Nobody
    // else, and remote clients are rejected outright.
    let sddl: Vec<u16> = "D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;IU)\0".encode_utf16().collect();
    let mut sd = PSECURITY_DESCRIPTOR::default();
    unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(PCWSTR(sddl.as_ptr()), SDDL_REVISION_1, &mut sd, None)?;
    }
    let sa = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: sd.0,
        bInheritHandle: false.into(),
    };
    let name: Vec<u16> = format!("{}\0", pcore::PIPE_NAME).encode_utf16().collect();

    loop {
        let pipe = unsafe {
            CreateNamedPipeW(
                PCWSTR(name.as_ptr()),
                PIPE_ACCESS_DUPLEX,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                PIPE_UNLIMITED_INSTANCES,
                64 * 1024,
                64 * 1024,
                0,
                Some(&sa),
            )
        };
        if pipe == INVALID_HANDLE_VALUE {
            let e = windows::core::Error::from_win32();
            log::error!("CreateNamedPipe: {e}");
            std::thread::sleep(Duration::from_secs(1));
            continue;
        }
        let connected = unsafe { ConnectNamedPipe(pipe, None) };
        if let Err(e) = connected {
            if e.code() != ERROR_PIPE_CONNECTED.to_hresult() {
                unsafe {
                    let _ = windows::Win32::Foundation::CloseHandle(pipe);
                }
                continue;
            }
        }
        let file = unsafe { File::from_raw_handle(pipe.0 as _) };
        let jobs = jobs.clone();
        std::thread::spawn(move || handle(file, &jobs));
    }
}
