//! A program's icon as a PNG data URL, for the game list.

#[cfg(windows)]
pub fn data_url(path: &str) -> Option<String> {
    use base64::Engine;
    let rgba = windows_icon(path, 48)?;
    let mut png = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut png, rgba.0, rgba.1);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header().ok()?;
        w.write_image_data(&rgba.2).ok()?;
    }
    Some(format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(png)))
}

/// Linux: Windows games (run through Wine or Proton) carry their icon in
/// the .exe, read here without Windows; native apps get the icon of their
/// desktop entry from the icon theme.
#[cfg(not(windows))]
pub fn data_url(path: &str) -> Option<String> {
    use base64::Engine;
    let b64 = |mime: &str, bytes: &[u8]| format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes));
    if path.to_lowercase().ends_with(".exe") {
        if let Some(png) = exe_icon_png(std::path::Path::new(path)) {
            return Some(b64("image/png", &png));
        }
    }
    static APPS: std::sync::OnceLock<std::collections::HashMap<String, String>> = std::sync::OnceLock::new();
    let apps = APPS.get_or_init(pcore::desktop::apps);
    let icon = apps.get(&pcore::settings::exe_name(path))?;
    let file = pcore::desktop::icon_file(icon)?;
    let bytes = std::fs::read(&file).ok()?;
    let mime = if file.extension().and_then(|e| e.to_str()) == Some("svg") { "image/svg+xml" } else { "image/png" };
    Some(b64(mime, &bytes))
}

/// The program's main icon from its PE resources, as PNG, picking the size
/// closest to 48 pixels.
#[cfg(not(windows))]
fn exe_icon_png(path: &std::path::Path) -> Option<Vec<u8>> {
    let map = pelite::FileMap::open(path).ok()?;
    let pe = pelite::PeFile::from_bytes(&map).ok()?;
    let res = pe.resources().ok()?;
    let (_, group) = res.icons().find_map(|r| r.ok())?;
    let mut ico = Vec::new();
    group.write(&mut ico).ok()?;
    let dir = ico::IconDir::read(std::io::Cursor::new(ico)).ok()?;
    let entry = dir.entries().iter().min_by_key(|e| {
        let w = e.width() as i32;
        // Prefer 48 and up to 64; then larger ones over smaller ones.
        if (48..=64).contains(&w) { 0 } else if w > 64 { w - 64 } else { 1000 - w }
    })?;
    let img = entry.decode().ok()?;
    let mut png = Vec::new();
    img.write_png(&mut png).ok()?;
    Some(png)
}

#[cfg(all(test, not(windows)))]
mod tests {
    /// Uses the Windows build of this app, which carries the app icon.
    #[test]
    fn reads_an_exe_icon() {
        let exe = concat!(env!("CARGO_MANIFEST_DIR"), "/../target/x86_64-pc-windows-gnu/release/opensenses.exe");
        if !std::path::Path::new(exe).exists() {
            return;
        }
        let png = super::exe_icon_png(std::path::Path::new(exe)).expect("icon");
        assert_eq!(&png[1..4], b"PNG");
        if let Ok(out) = std::env::var("ICON_OUT") {
            std::fs::write(out, &png).unwrap();
        }
    }
}

/// Width, height and RGBA pixels of the program's first icon.
#[cfg(windows)]
fn windows_icon(path: &str, size: u32) -> Option<(u32, u32, Vec<u8>)> {
    use windows::core::HSTRING;
    use windows::Win32::Graphics::Gdi::{
        CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits, GetObjectW, BITMAP, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
        DIB_RGB_COLORS,
    };
    use windows::Win32::UI::Shell::SHDefExtractIconW;
    use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, HICON, ICONINFO};

    unsafe {
        let mut icon = HICON::default();
        SHDefExtractIconW(&HSTRING::from(path), 0, 0, Some(&mut icon), None, size).ok().ok()?;
        if icon.is_invalid() {
            return None;
        }
        let mut info = ICONINFO::default();
        let got = GetIconInfo(icon, &mut info).is_ok();
        let _ = DestroyIcon(icon);
        if !got {
            return None;
        }
        let result = (|| {
            let mut bm = BITMAP::default();
            if GetObjectW(info.hbmColor, std::mem::size_of::<BITMAP>() as i32, Some(&mut bm as *mut _ as *mut _)) == 0 {
                return None;
            }
            let (w, h) = (bm.bmWidth, bm.bmHeight);
            if w <= 0 || h <= 0 || w > 256 || h > 256 {
                return None;
            }
            let read = |bmp| -> Option<Vec<u8>> {
                let dc = CreateCompatibleDC(None);
                let mut bi = BITMAPINFO {
                    bmiHeader: BITMAPINFOHEADER {
                        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                        biWidth: w,
                        biHeight: -h, // top-down rows
                        biPlanes: 1,
                        biBitCount: 32,
                        biCompression: BI_RGB.0,
                        ..Default::default()
                    },
                    ..Default::default()
                };
                let mut buf = vec![0u8; (w * h * 4) as usize];
                let lines = GetDIBits(dc, bmp, 0, h as u32, Some(buf.as_mut_ptr() as *mut _), &mut bi, DIB_RGB_COLORS);
                let _ = DeleteDC(dc);
                (lines == h).then_some(buf)
            };
            let mut px = read(info.hbmColor)?;
            // Old icons have no alpha channel; their transparency is in the mask.
            if px.chunks(4).all(|p| p[3] == 0) {
                if let Some(mask) = read(info.hbmMask) {
                    for (p, m) in px.chunks_mut(4).zip(mask.chunks(4)) {
                        p[3] = if m[0] == 0 { 255 } else { 0 };
                    }
                }
            }
            for p in px.chunks_mut(4) {
                p.swap(0, 2); // BGRA -> RGBA
            }
            Some((w as u32, h as u32, px))
        })();
        let _ = DeleteObject(info.hbmColor);
        let _ = DeleteObject(info.hbmMask);
        result
    }
}
