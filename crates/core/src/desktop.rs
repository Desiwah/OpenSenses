//! Linux desktop entries (.desktop files) and icon themes: which installed
//! apps exist, and where their icons are. Used to list running apps and to
//! show their icons, the way Windows shows a program's own icon.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Directories holding `applications/` and `icons/`, most specific first.
fn data_dirs() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let mut v = Vec::new();
    match std::env::var_os("XDG_DATA_HOME") {
        Some(d) => v.push(PathBuf::from(d)),
        None => v.extend(home.iter().map(|h| h.join(".local/share"))),
    }
    v.extend(home.iter().map(|h| h.join(".local/share/flatpak/exports/share")));
    v.push(PathBuf::from("/var/lib/flatpak/exports/share"));
    let sys = std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    v.extend(sys.split(':').filter(|s| !s.is_empty()).map(PathBuf::from));
    v.dedup();
    v
}

/// Installed apps by the lower-case names their running program may have:
/// the program file in `Exec=`, `StartupWMClass=` and the desktop file id.
/// The value is the entry's `Icon=`.
pub fn apps() -> HashMap<String, String> {
    let mut map = HashMap::new();
    for dir in data_dirs().iter().map(|d| d.join("applications")) {
        let Ok(rd) = fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) != Some("desktop") {
                continue;
            }
            let Ok(text) = fs::read_to_string(&p) else { continue };
            let entry = parse(&text);
            let Some(icon) = entry.get("Icon").cloned() else { continue };
            let mut keys = Vec::new();
            if let Some(exec) = entry.get("Exec") {
                if let Some(prog) = exec_program(exec) {
                    keys.push(prog);
                }
            }
            if let Some(class) = entry.get("StartupWMClass") {
                keys.push(class.to_lowercase());
            }
            if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
                // "com.discordapp.Discord" -> "discord"
                keys.push(stem.rsplit('.').next().unwrap_or(stem).to_lowercase());
            }
            for k in keys {
                map.entry(k).or_insert_with(|| icon.clone());
            }
        }
    }
    map
}

/// Keys of the `[Desktop Entry]` group.
fn parse(text: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let mut in_entry = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            out.entry(k.trim().to_string()).or_insert_with(|| v.trim().to_string());
        }
    }
    out
}

/// The program an `Exec=` line runs, lower-case file name. `env VAR=x prog`
/// and quotes are skipped; Flatpak and Steam launchers aren't the program,
/// so those entries are found by their other names instead.
fn exec_program(exec: &str) -> Option<String> {
    let mut words = split_exec(exec).into_iter();
    let mut first = words.next()?;
    if first == "env" {
        first = words.find(|w| !w.contains('='))?;
    }
    let name = crate::settings::exe_name(&first);
    match name.as_str() {
        "flatpak" | "steam" | "sh" | "bash" | "gtk-launch" | "xdg-open" => None,
        _ => Some(name),
    }
}

/// Splits an `Exec=` value into words, keeping "quoted parts" together.
fn split_exec(exec: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut chars = exec.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => quoted = !quoted,
            '\\' if quoted => {
                if let Some(n) = chars.next() {
                    cur.push(n);
                }
            }
            c if c.is_whitespace() && !quoted => {
                if !cur.is_empty() {
                    words.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        words.push(cur);
    }
    words
}

/// Where the icon named `icon` lives (an absolute path is used as it is).
pub fn icon_file(icon: &str) -> Option<PathBuf> {
    let p = Path::new(icon);
    if p.is_absolute() {
        return p.is_file().then(|| p.to_path_buf());
    }
    const SIZES: &[&str] = &["256x256", "512x512", "128x128", "96x96", "64x64", "48x48", "scalable", "32x32"];
    for base in data_dirs() {
        for size in SIZES {
            for ext in ["png", "svg"] {
                let f = base.join("icons/hicolor").join(size).join("apps").join(format!("{icon}.{ext}"));
                if f.is_file() {
                    return Some(f);
                }
            }
        }
        for ext in ["png", "svg"] {
            let f = base.join("pixmaps").join(format!("{icon}.{ext}"));
            if f.is_file() {
                return Some(f);
            }
        }
    }
    None
}

/// The Linux path of a Windows program path seen in a Wine or Proton
/// process ("Z:\home\me\Game\game.exe", "C:\Program Files\..."), using the
/// process's Wine prefix to resolve drive letters.
pub fn wine_path(win: &str, environ: &HashMap<String, String>) -> Option<PathBuf> {
    let bytes = win.as_bytes();
    if bytes.len() < 3 || bytes[1] != b':' || !(bytes[2] == b'\\' || bytes[2] == b'/') {
        return None;
    }
    let letter = (bytes[0] as char).to_ascii_lowercase();
    let rest = win[3..].replace('\\', "/");
    let home = environ.get("HOME").cloned().or_else(|| std::env::var("HOME").ok()).unwrap_or_default();
    let prefix = environ
        .get("WINEPREFIX")
        .map(PathBuf::from)
        .or_else(|| environ.get("STEAM_COMPAT_DATA_PATH").map(|p| Path::new(p).join("pfx")))
        .unwrap_or_else(|| Path::new(&home).join(".wine"));
    let drive = prefix.join("dosdevices").join(format!("{letter}:"));
    let root = match fs::read_link(&drive) {
        Ok(t) if t.is_absolute() => t,
        Ok(t) => prefix.join("dosdevices").join(t),
        Err(_) if letter == 'z' => PathBuf::from("/"),
        Err(_) if letter == 'c' => prefix.join("drive_c"),
        Err(_) => return None,
    };
    let full = root.join(rest);
    full.is_file().then_some(full)
}

/// A process's environment variables (only readable for one's own processes).
pub fn environ(pid_dir: &Path) -> HashMap<String, String> {
    fs::read(pid_dir.join("environ"))
        .map(|b| {
            b.split(|&c| c == 0)
                .filter_map(|kv| {
                    let s = String::from_utf8_lossy(kv);
                    s.split_once('=').map(|(k, v)| (k.to_string(), v.to_string()))
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_desktop_entries() {
        let e = parse("[Desktop Entry]\nName=Game\nExec=env DXVK=1 \"/opt/My Game/game\" %U\nIcon=mygame\n[Desktop Action x]\nExec=other\n");
        assert_eq!(e.get("Icon").map(String::as_str), Some("mygame"));
        assert_eq!(exec_program(e.get("Exec").unwrap()).as_deref(), Some("game"));
        assert_eq!(exec_program("env DXVK=1 /opt/game/Game %U").as_deref(), Some("game"));
        assert_eq!(exec_program("flatpak run com.discordapp.Discord"), None);
    }

    #[test]
    fn maps_wine_paths() {
        let dir = std::env::temp_dir().join(format!("os-wine-{}", std::process::id()));
        let pfx = dir.join("pfx");
        std::fs::create_dir_all(pfx.join("dosdevices")).unwrap();
        std::fs::create_dir_all(pfx.join("drive_c/Games")).unwrap();
        std::fs::write(pfx.join("drive_c/Games/x.exe"), b"MZ").unwrap();
        std::os::unix::fs::symlink("../drive_c", pfx.join("dosdevices/c:")).unwrap();
        let env: HashMap<String, String> = [("STEAM_COMPAT_DATA_PATH".to_string(), dir.to_string_lossy().into_owned())].into();
        assert_eq!(wine_path(r"C:\Games\x.exe", &env), Some(pfx.join("dosdevices/../drive_c").join("Games/x.exe")));
        let z = format!("Z:{}", pfx.join("drive_c/Games/x.exe").to_string_lossy().replace('/', "\\"));
        assert!(wine_path(&z, &env).is_some());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
