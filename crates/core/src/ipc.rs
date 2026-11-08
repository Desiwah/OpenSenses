//! The app (running as a normal user) talks to the background service
//! (running with admin rights) over a local named pipe / unix socket, one
//! JSON message per line.

use crate::backend::{Capabilities, Sensors};
use crate::settings::{Settings, Toggles};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Request {
    GetState,
    SetSettings { settings: Settings },
    /// Re-send the saved settings to the hardware.
    Reapply,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct State {
    pub settings: Settings,
    pub sensors: Sensors,
    pub capabilities: Capabilities,
    pub backend: String,
    /// Errors from the most recent apply, empty when everything worked.
    pub errors: Vec<String>,
    /// Set when the service had to force fans to Max because of heat.
    pub overheat_guard: bool,
    /// What each toggle is actually set to on the laptop right now.
    pub toggles_now: Toggles,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Response {
    State(State),
    Error(String),
}

/// Largest message the service accepts, so a bad client can't make it buffer forever.
pub const MAX_LINE: usize = 64 * 1024;

pub fn read_line<R: BufRead>(r: &mut R) -> Result<Option<String>> {
    let mut buf = Vec::new();
    let n = std::io::Read::take(r, MAX_LINE as u64 + 1).read_until(b'\n', &mut buf)?;
    if n == 0 {
        return Ok(None);
    }
    anyhow::ensure!(buf.len() <= MAX_LINE, "message too long");
    Ok(Some(String::from_utf8(buf)?.trim_end().to_string()))
}

/// Client side: sends one request to the service and waits for the reply.
pub fn request(req: &Request) -> Result<Response> {
    let mut line = serde_json::to_string(req)?;
    line.push('\n');

    #[cfg(windows)]
    let stream = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(crate::PIPE_NAME)
        .context("background service is not running")?;
    #[cfg(unix)]
    let stream = std::os::unix::net::UnixStream::connect(crate::SOCKET_PATH).context("background service is not running")?;

    let mut stream = stream;
    stream.write_all(line.as_bytes())?;
    stream.flush()?;
    let mut reader = BufReader::new(stream);
    let reply = read_line(&mut reader)?.context("service closed the connection")?;
    Ok(serde_json::from_str(&reply)?)
}
