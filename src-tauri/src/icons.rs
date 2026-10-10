//! App icons for the bar's rows, extracted once per app and size and cached as PNG in the
//! cache folder. Each platform supplies `extract`; where there is none yet, rows keep the
//! generic glyph.

use std::path::PathBuf;
use std::sync::OnceLock;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use sha2::{Digest, Sha256};

static DIR: OnceLock<PathBuf> = OnceLock::new();

pub fn init(dir: PathBuf) {
    let _ = std::fs::create_dir_all(&dir);
    let _ = DIR.set(dir);
}

/// The icon for an app id (an AppUserModelID on Windows) as a `data:` URL, at `size` pixels.
/// Slow on a miss, so callers run it off the UI thread.
pub fn app_icon(app_id: &str, size: u32) -> Option<String> {
    let size = size.clamp(16, 256);
    let key = hex(&Sha256::digest(format!("{app_id}\n{size}").as_bytes()));
    let cached = DIR.get().map(|dir| dir.join(format!("{key}.png")));
    let png = match cached.as_ref().and_then(|path| std::fs::read(path).ok()) {
        Some(png) => png,
        None => {
            let png = extract(app_id, size)?;
            if let Some(path) = &cached {
                let _ = std::fs::write(path, &png);
            }
            png
        }
    };
    Some(format!("data:image/png;base64,{}", STANDARD.encode(png)))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(windows)]
fn extract(app_id: &str, size: u32) -> Option<Vec<u8>> {
    use windows::Win32::Foundation::SIZE;
    use windows::Win32::Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC,
        DeleteObject, GetDIBits,
    };
    use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, IBindCtx};
    use windows::Win32::UI::Shell::{
        IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_ICONONLY,
    };
    use windows::core::PCWSTR;

    // The same item Explorer launches the app from, so packaged and desktop apps both work.
    let path: Vec<u16> = format!("shell:AppsFolder\\{app_id}")
        .encode_utf16()
        .chain(Some(0))
        .collect();
    unsafe {
        // Already initialized on this thread is fine; the shell works in either apartment.
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let factory: IShellItemImageFactory =
            SHCreateItemFromParsingName(PCWSTR(path.as_ptr()), None::<&IBindCtx>).ok()?;
        let side = size as i32;
        let bitmap = factory
            .GetImage(SIZE { cx: side, cy: side }, SIIGBF_ICONONLY)
            .ok()?;

        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: side,
                biHeight: -side, // top row first
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bgra = vec![0u8; (size * size * 4) as usize];
        let dc = CreateCompatibleDC(None);
        let rows = GetDIBits(
            dc,
            bitmap,
            0,
            size,
            Some(bgra.as_mut_ptr().cast()),
            &mut info,
            DIB_RGB_COLORS,
        );
        let _ = DeleteDC(dc);
        let _ = DeleteObject(bitmap.into());
        if rows != side {
            return None;
        }
        encode(&unpremultiply(bgra), size)
    }
}

#[cfg(not(windows))]
fn extract(_app_id: &str, _size: u32) -> Option<Vec<u8>> {
    None
}

/// The shell hands back premultiplied BGRA; PNG wants straight RGBA.
#[cfg_attr(not(windows), allow(dead_code))]
fn unpremultiply(mut bgra: Vec<u8>) -> Vec<u8> {
    for pixel in bgra.as_chunks_mut::<4>().0.iter_mut() {
        let [blue, green, red, alpha] = *pixel;
        let straight = |channel: u8| match alpha {
            0 => 0,
            255 => channel,
            _ => ((channel as u32 * 255 + alpha as u32 / 2) / alpha as u32).min(255) as u8,
        };
        *pixel = [straight(red), straight(green), straight(blue), alpha];
    }
    bgra
}

#[cfg(windows)]
fn encode(rgba: &[u8], size: u32) -> Option<Vec<u8>> {
    let mut png = Vec::new();
    let mut encoder = png::Encoder::new(&mut png, size, size);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().ok()?;
    writer.write_image_data(rgba).ok()?;
    writer.finish().ok()?;
    Some(png)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn premultiplied_pixels_come_out_straight() {
        // Half-transparent pure red, premultiplied: red 128 at alpha 128.
        assert_eq!(unpremultiply(vec![0, 0, 128, 128]), vec![255, 0, 0, 128]);
        assert_eq!(unpremultiply(vec![10, 20, 30, 0]), vec![0, 0, 0, 0]);
        assert_eq!(unpremultiply(vec![10, 20, 30, 255]), vec![30, 20, 10, 255]);
    }

    #[cfg(windows)]
    #[test]
    fn extracts_an_icon_for_a_start_menu_app() {
        // File Explorer is on every PC under this id.
        let png = extract("Microsoft.Windows.Explorer", 32).expect("an icon");
        assert!(png.starts_with(b"\x89PNG"));
    }
}
