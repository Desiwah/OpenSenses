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
    SetSettings {
        settings: Settings,
    },
    /// Re-send the saved settings to the hardware.
    Reapply,
    /// Put back the laptop's settings from before OpenSenses changed them.
    RestoreOriginal,
    /// Turn Turbo on or off, as the Turbo key does.
    SetTurbo {
        on: bool,
    },
    /// Acts as if the firmware had sent this key event (for trying out
    /// hotkey assignments without the hardware key).
    TestKey {
        detail: [u8; 8],
    },
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
    /// Program name of the game whose GameSync profile is in use.
    pub active_game: Option<String>,
    /// When the original settings were saved and where they came from.
    pub original: Option<crate::snapshot::Info>,
    /// Turbo (from the Turbo key or the app) is on.
    pub turbo: bool,
    /// The CPU level the firmware was given (index into the levels).
    pub cpu_level: Option<usize>,
    /// On the charger, and battery percentage.
    pub power: Option<(bool, u8)>,
    /// Current hotkey group (1..3) on models with programmable keys.
    pub hotkey_group: u8,
    /// A short message about the last key press (e.g. "Turbo needs the
    /// charger"), with a counter so the app can tell new ones apart.
    pub notice: Option<(u32, String)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[allow(clippy::large_enum_variant)] // one reply per request; boxing buys nothing
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
