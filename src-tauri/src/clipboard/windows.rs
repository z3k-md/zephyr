//! Windows capture and paste: the Win32 clipboard for reading and writing, and SendInput for
//! Ctrl+V. Text in images isn't recognized here yet.

use std::path::Path;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::time::Duration;

use ::windows::Win32::Foundation::{CloseHandle, GlobalFree, HANDLE, HGLOBAL, HWND};
use ::windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, GetClipboardOwner,
    GetClipboardSequenceNumber, IsClipboardFormatAvailable, OpenClipboard,
    RegisterClipboardFormatW, SetClipboardData,
};
use ::windows::Win32::System::Memory::{
    GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock,
};
use ::windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use ::windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput,
    VIRTUAL_KEY, VK_CONTROL, VK_V,
};
use ::windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
use ::windows::core::{PCWSTR, PWSTR};

use super::{Capture, Content, Platform};

pub struct WinClipboard;

// Standard formats, by number so the Ole feature isn't needed for three constants.
const CF_UNICODETEXT: u32 = 13;
const CF_HDROP: u32 = 15;
const CF_DIB: u32 = 8;
const CF_DIBV5: u32 = 17;

/// Kept small: a huge image would make every list and save slow.
const MAX_IMAGE_BYTES: usize = 25 << 20;

/// The window that owns what Zephyr writes. Writing needs an owner, and it must be a window
/// whose thread handles messages, or other apps stall when they next take the clipboard.
static OWNER: AtomicIsize = AtomicIsize::new(0);

pub fn set_owner(hwnd: HWND) {
    OWNER.store(hwnd.0 as isize, Ordering::SeqCst);
}

fn format(name: &str) -> u32 {
    let wide: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
    unsafe { RegisterClipboardFormatW(PCWSTR(wide.as_ptr())) }
}

fn available(format: u32) -> bool {
    format != 0 && unsafe { IsClipboardFormatAvailable(format) }.is_ok()
}

/// Holds the clipboard open; it closes when dropped, whatever path the code takes.
struct Open;

impl Open {
    /// Another app may have the clipboard open for a moment, so this retries briefly. It
    /// never waits long: the watcher polls again shortly anyway.
    fn new(owner: Option<HWND>) -> Option<Self> {
        for attempt in 0..5 {
            if unsafe { OpenClipboard(owner) }.is_ok() {
                return Some(Open);
            }
            std::thread::sleep(Duration::from_millis(10 * (attempt + 1)));
        }
        None
    }

    /// The bytes of a format, copied out of the clipboard's memory.
    fn bytes(&self, format: u32) -> Option<Vec<u8>> {
        let handle = unsafe { GetClipboardData(format) }.ok()?;
        let global = HGLOBAL(handle.0);
        unsafe {
            let size = GlobalSize(global);
            let data = GlobalLock(global) as *const u8;
            if data.is_null() || size == 0 {
                return None;
            }
            let bytes = std::slice::from_raw_parts(data, size).to_vec();
            let _ = GlobalUnlock(global);
            Some(bytes)
        }
    }

    /// Puts `bytes` on the clipboard as `format`; the clipboard owns the memory afterwards.
    fn put(&self, format: u32, bytes: &[u8]) -> Result<(), String> {
        unsafe {
            let global =
                GlobalAlloc(GMEM_MOVEABLE, bytes.len().max(1)).map_err(|err| err.to_string())?;
            let data = GlobalLock(global) as *mut u8;
            if data.is_null() {
                let _ = GlobalFree(Some(global));
                return Err("Couldn't lock clipboard memory".into());
            }
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), data, bytes.len());
            let _ = GlobalUnlock(global);
            if let Err(err) = SetClipboardData(format, Some(HANDLE(global.0))) {
                let _ = GlobalFree(Some(global));
                return Err(err.to_string());
            }
        }
        Ok(())
    }
}

impl Drop for Open {
    fn drop(&mut self) {
        let _ = unsafe { CloseClipboard() };
    }
}

/// What password managers and other private copies set. Windows' own clipboard history
/// honors the same markers.
fn concealed(open: &Open) -> bool {
    if available(format("ExcludeClipboardContentFromMonitorProcessing"))
        || available(format("Clipboard Viewer Ignore"))
    {
        return true;
    }
    ["CanIncludeInClipboardHistory", "CanUploadToCloudClipboard"]
        .iter()
        .any(|name| {
            let id = format(name);
            available(id)
                && open
                    .bytes(id)
                    .is_some_and(|bytes| bytes.len() >= 4 && bytes[..4] == [0, 0, 0, 0])
        })
}

fn utf16(bytes: &[u8]) -> Vec<u16> {
    bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect()
}

fn text(bytes: &[u8]) -> String {
    let wide = utf16(bytes);
    let end = wide
        .iter()
        .position(|&unit| unit == 0)
        .unwrap_or(wide.len());
    String::from_utf16_lossy(&wide[..end])
}

/// Paths from a DROPFILES block: a 20-byte header, then null-terminated names and an extra
/// null at the end.
fn dropped_files(bytes: &[u8]) -> Vec<String> {
    if bytes.len() < 20 {
        return Vec::new();
    }
    let offset = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
    let wide = u32::from_le_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]) != 0;
    let Some(list) = bytes.get(offset..) else {
        return Vec::new();
    };
    let joined = if wide {
        String::from_utf16_lossy(&utf16(list))
    } else {
        list.iter().map(|&byte| byte as char).collect()
    };
    joined
        .split('\0')
        .take_while(|name| !name.is_empty())
        .map(str::to_string)
        .collect()
}

fn dropfiles_block(files: &[String]) -> Vec<u8> {
    let mut block = Vec::new();
    block.extend_from_slice(&20u32.to_le_bytes()); // pFiles
    block.extend_from_slice(&[0; 8]); // pt
    block.extend_from_slice(&0u32.to_le_bytes()); // fNC
    block.extend_from_slice(&1u32.to_le_bytes()); // fWide
    for file in files {
        for unit in file.encode_utf16().chain(Some(0)) {
            block.extend_from_slice(&unit.to_le_bytes());
        }
    }
    block.extend_from_slice(&[0, 0]);
    block
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

/// A packed DIB (BITMAPINFOHEADER or BITMAPV5HEADER, then pixels) as RGBA rows, top first.
/// Handles the 24- and 32-bit images screenshots and browsers copy.
fn dib_to_rgba(bytes: &[u8]) -> Option<(Vec<u8>, u32, u32)> {
    let header = u32_at(bytes, 0)? as usize;
    let width = i32::from_le_bytes(bytes.get(4..8)?.try_into().ok()?);
    let height = i32::from_le_bytes(bytes.get(8..12)?.try_into().ok()?);
    let bits = u16::from_le_bytes(bytes.get(14..16)?.try_into().ok()?);
    let compression = u32_at(bytes, 16)?;
    const BI_RGB: u32 = 0;
    const BI_BITFIELDS: u32 = 3;
    if width <= 0 || height == 0 || !matches!(bits, 24 | 32) {
        return None;
    }
    let (masks, mut pixels_at) = match compression {
        BI_RGB => (None, header),
        BI_BITFIELDS if bits == 32 => {
            // A V4/V5 header carries the masks; a plain one is followed by three of them.
            let at = if header >= 52 { 40 } else { header };
            let masks = (
                u32_at(bytes, at)?,
                u32_at(bytes, at + 4)?,
                u32_at(bytes, at + 8)?,
            );
            let alpha = if header >= 56 { u32_at(bytes, 52)? } else { 0 };
            (
                Some((masks, alpha)),
                if header >= 52 { header } else { header + 12 },
            )
        }
        _ => return None,
    };
    let colors_used = u32_at(bytes, 32).unwrap_or(0) as usize;
    pixels_at += colors_used * 4;

    let (width, rows) = (width as usize, height.unsigned_abs() as usize);
    let stride = (width * bits as usize).div_ceil(32) * 4;
    let pixels = bytes.get(pixels_at..pixels_at + stride * rows)?;
    let mut rgba = Vec::with_capacity(width * rows * 4);
    let shift = |mask: u32| mask.trailing_zeros().min(31);
    for row in 0..rows {
        // Positive heights are stored bottom row first.
        let source = if height > 0 { rows - 1 - row } else { row };
        let line = &pixels[source * stride..source * stride + stride];
        for x in 0..width {
            if bits == 24 {
                let pixel = &line[x * 3..x * 3 + 3];
                rgba.extend_from_slice(&[pixel[2], pixel[1], pixel[0], 255]);
                continue;
            }
            let value = u32::from_le_bytes(line[x * 4..x * 4 + 4].try_into().ok()?);
            match masks {
                Some(((red, green, blue), alpha)) => rgba.extend_from_slice(&[
                    ((value & red) >> shift(red)) as u8,
                    ((value & green) >> shift(green)) as u8,
                    ((value & blue) >> shift(blue)) as u8,
                    if alpha == 0 {
                        255
                    } else {
                        ((value & alpha) >> shift(alpha)) as u8
                    },
                ]),
                None => {
                    let [blue, green, red, alpha] = value.to_le_bytes();
                    rgba.extend_from_slice(&[red, green, blue, alpha]);
                }
            }
        }
    }
    // Plain 32-bit DIBs usually leave alpha at zero, which means opaque, not invisible.
    if masks.is_none() && bits == 32 && rgba.as_chunks::<4>().0.iter().all(|pixel| pixel[3] == 0) {
        rgba.as_chunks_mut::<4>()
            .0
            .iter_mut()
            .for_each(|pixel| pixel[3] = 255);
    }
    Some((rgba, width as u32, rows as u32))
}

/// A bottom-up 32-bit BITMAPINFOHEADER DIB, which every app that pastes images reads.
fn rgba_to_dib(rgba: &[u8], width: u32, height: u32) -> Vec<u8> {
    let mut dib = Vec::with_capacity(40 + rgba.len());
    dib.extend_from_slice(&40u32.to_le_bytes());
    dib.extend_from_slice(&(width as i32).to_le_bytes());
    dib.extend_from_slice(&(height as i32).to_le_bytes());
    dib.extend_from_slice(&1u16.to_le_bytes()); // planes
    dib.extend_from_slice(&32u16.to_le_bytes()); // bits
    dib.extend_from_slice(&0u32.to_le_bytes()); // BI_RGB
    dib.extend_from_slice(&(rgba.len() as u32).to_le_bytes());
    dib.extend_from_slice(&[0; 16]); // resolution and palette
    let row = width as usize * 4;
    for line in rgba.chunks_exact(row.max(1)).rev() {
        for pixel in line.as_chunks::<4>().0 {
            dib.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
        }
    }
    dib
}

fn encode_png(rgba: &[u8], width: u32, height: u32) -> Option<Vec<u8>> {
    let mut png = Vec::new();
    let mut encoder = png::Encoder::new(&mut png, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().ok()?;
    writer.write_image_data(rgba).ok()?;
    writer.finish().ok()?;
    Some(png)
}

/// A PNG's pixels as 8-bit RGBA.
fn decode_png(bytes: &[u8]) -> Option<(Vec<u8>, u32, u32)> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().ok()?;
    let mut buffer = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buffer).ok()?;
    buffer.truncate(info.buffer_size());
    let rgba = match info.color_type {
        png::ColorType::Rgba => buffer,
        png::ColorType::Rgb => buffer
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|pixel| [pixel[0], pixel[1], pixel[2], 255])
            .collect(),
        png::ColorType::GrayscaleAlpha => buffer
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|pixel| [pixel[0], pixel[0], pixel[0], pixel[1]])
            .collect(),
        png::ColorType::Grayscale => buffer
            .iter()
            .flat_map(|&gray| [gray, gray, gray, 255])
            .collect(),
        png::ColorType::Indexed => return None,
    };
    Some((rgba, info.width, info.height))
}

fn png_size(bytes: &[u8]) -> Option<(u32, u32)> {
    let reader = png::Decoder::new(std::io::Cursor::new(bytes))
        .read_info()
        .ok()?;
    let info = reader.info();
    Some((info.width, info.height))
}

/// The app that put this on the clipboard: (name, exe), e.g. ("KeePass", "KeePass.exe").
fn source() -> Option<(String, String)> {
    let owner = unsafe { GetClipboardOwner() }
        .ok()
        .filter(|hwnd| !hwnd.is_invalid())
        .unwrap_or_else(|| unsafe { GetForegroundWindow() });
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(owner, Some(&mut pid)) };
    if pid == 0 {
        return None;
    }
    let path = unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buffer = [0u16; 1024];
        let mut size = buffer.len() as u32;
        let found = QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut size,
        );
        let _ = CloseHandle(process);
        found.ok()?;
        String::from_utf16_lossy(&buffer[..size as usize])
    };
    let path = Path::new(&path);
    let exe = path.file_name()?.to_string_lossy().into_owned();
    let name = path.file_stem()?.to_string_lossy().into_owned();
    Some((name, exe))
}

fn key(key: VIRTUAL_KEY, up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: key,
                wScan: 0,
                dwFlags: if up {
                    KEYEVENTF_KEYUP
                } else {
                    KEYBD_EVENT_FLAGS(0)
                },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

impl Platform for WinClipboard {
    fn change_count(&self) -> i64 {
        i64::from(unsafe { GetClipboardSequenceNumber() })
    }

    fn read(&self) -> Option<Capture> {
        let source = source();
        let open = Open::new(None)?;
        if concealed(&open) {
            return None;
        }

        if available(CF_HDROP) {
            let files = open
                .bytes(CF_HDROP)
                .map(|bytes| dropped_files(&bytes))
                .unwrap_or_default();
            if !files.is_empty() {
                return Some(Capture {
                    content: Content::Files(files),
                    source,
                });
            }
        }

        // Prefer text when an app offers both (a spreadsheet cell copies as text and an image).
        if available(CF_UNICODETEXT)
            && let Some(text) = open.bytes(CF_UNICODETEXT).map(|bytes| text(&bytes))
            && !text.trim().is_empty()
        {
            return Some(Capture {
                content: Content::Text(text),
                source,
            });
        }

        let png_format = format("PNG");
        if available(png_format)
            && let Some(png) = open
                .bytes(png_format)
                .filter(|png| png.len() <= MAX_IMAGE_BYTES)
            && let Some((width, height)) = png_size(&png)
        {
            return Some(Capture {
                content: Content::Image { png, width, height },
                source,
            });
        }

        let dib = [CF_DIBV5, CF_DIB]
            .into_iter()
            .filter(|&format| available(format))
            .find_map(|format| open.bytes(format))
            .filter(|dib| dib.len() <= MAX_IMAGE_BYTES)?;
        drop(open);
        let (rgba, width, height) = dib_to_rgba(&dib)?;
        let png = encode_png(&rgba, width, height)?;
        Some(Capture {
            content: Content::Image { png, width, height },
            source,
        })
    }

    fn write(&self, content: &Content, plain: bool) -> Result<(), String> {
        // Decode before opening, so the clipboard isn't held while an image converts.
        let image = match content {
            Content::Image { .. } if plain => {
                return Err("Images can't be pasted as plain text".into());
            }
            Content::Image { png, .. } => Some(decode_png(png).ok_or("Couldn't read that image")?),
            _ => None,
        };
        let owner = HWND(OWNER.load(Ordering::SeqCst) as *mut _);
        let owner = (!owner.is_invalid()).then_some(owner);
        let open = Open::new(owner).ok_or("Another app is using the clipboard")?;
        unsafe { EmptyClipboard() }.map_err(|err| err.to_string())?;
        let wide = |text: &str| -> Vec<u8> {
            text.encode_utf16()
                .chain(Some(0))
                .flat_map(u16::to_le_bytes)
                .collect()
        };
        match content {
            Content::Text(text) => open.put(CF_UNICODETEXT, &wide(text)),
            Content::Files(files) if plain => open.put(CF_UNICODETEXT, &wide(&files.join("\r\n"))),
            Content::Files(files) => {
                open.put(CF_HDROP, &dropfiles_block(files))?;
                // Explorer copies rather than moves what is pasted.
                let _ = open.put(format("Preferred DropEffect"), &1u32.to_le_bytes());
                open.put(CF_UNICODETEXT, &wide(&files.join("\r\n")))
            }
            Content::Image { png, .. } => {
                let (rgba, width, height) = image.ok_or("Couldn't read that image")?;
                open.put(format("PNG"), png)?;
                open.put(CF_DIB, &rgba_to_dib(&rgba, width, height))
            }
        }
    }

    fn can_paste(&self, _prompt: bool) -> bool {
        true
    }

    fn send_paste(&self) -> Result<(), String> {
        let inputs = [
            key(VK_CONTROL, false),
            key(VK_V, false),
            key(VK_V, true),
            key(VK_CONTROL, true),
        ];
        let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
        if sent as usize == inputs.len() {
            Ok(())
        } else {
            Err("Windows blocked the paste keystroke".into())
        }
    }

    fn recognize_text(&self, _png: &[u8]) -> Option<String> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dropped_file_lists_round_trip() {
        let files = vec![
            r"C:\Users\Zach\a.txt".to_string(),
            r"D:\b c\é.png".to_string(),
        ];
        assert_eq!(dropped_files(&dropfiles_block(&files)), files);
        assert!(dropped_files(&[0; 4]).is_empty());
    }

    #[test]
    fn images_survive_dib_and_png() {
        // 3x2 with distinct colors, including a translucent pixel.
        let rgba: Vec<u8> = vec![
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, //
            10, 20, 30, 128, 40, 50, 60, 255, 70, 80, 90, 255,
        ];
        let dib = rgba_to_dib(&rgba, 3, 2);
        assert_eq!(dib_to_rgba(&dib), Some((rgba.clone(), 3, 2)));
        let png = encode_png(&rgba, 3, 2).unwrap();
        assert_eq!(png_size(&png), Some((3, 2)));
        assert_eq!(decode_png(&png), Some((rgba, 3, 2)));
    }

    #[test]
    fn a_24_bit_dib_reads_bottom_up_with_padded_rows() {
        // 1x2: bottom row blue, top row red; each 3-byte row pads to 4.
        let mut dib = Vec::new();
        dib.extend_from_slice(&40u32.to_le_bytes());
        dib.extend_from_slice(&1i32.to_le_bytes());
        dib.extend_from_slice(&2i32.to_le_bytes());
        dib.extend_from_slice(&1u16.to_le_bytes());
        dib.extend_from_slice(&24u16.to_le_bytes());
        dib.extend_from_slice(&[0; 24]);
        dib.extend_from_slice(&[255, 0, 0, 0]); // blue, stored first (bottom)
        dib.extend_from_slice(&[0, 0, 255, 0]); // red
        assert_eq!(
            dib_to_rgba(&dib),
            Some((vec![255, 0, 0, 255, 0, 0, 255, 255], 1, 2))
        );
    }

    #[test]
    fn utf16_text_stops_at_the_terminator() {
        let bytes: Vec<u8> = "héllo\0junk"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        assert_eq!(text(&bytes), "héllo");
    }
}
