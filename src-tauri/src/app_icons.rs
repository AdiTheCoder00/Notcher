//! Start Menu app icons.
//!
//! The Control Centre lists shortcuts, and a shortcut's icon only exists once
//! the shell resolves it, so the shell is asked for its own 32 px icon and the
//! result is handed to the webview as a PNG data URL. Icons are decoration: any
//! failure drops back to the lettered placeholder in the UI.

use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use base64::Engine;
use windows::core::{Interface, PCWSTR};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits, GetObjectW, BITMAP, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HGDIOBJ,
};
use windows::Win32::Storage::FileSystem::{FILE_FLAGS_AND_ATTRIBUTES, WIN32_FIND_DATAW};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, IPersistFile, CLSCTX_INPROC_SERVER,
    COINIT_APARTMENTTHREADED, STGM_READ,
};
use windows::Win32::UI::Shell::{
    IShellLinkW, SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON, ShellLink,
};
use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo};

/// A PNG data URL for the icon Windows shows for `target`, or `None` when the
/// shell cannot resolve one.
pub fn png_data_url(target: &Path) -> Option<String> {
    let png = render(target)?;
    let mut url = String::with_capacity(png.len() * 2 + 32);
    url.push_str("data:image/png;base64,");
    url.push_str(&base64::engine::general_purpose::STANDARD.encode(png));
    Some(url)
}

/// Icons for many shortcuts at once. Resolving a shortcut through the shell is
/// the expensive part, so the work is spread over the machine's cores.
pub fn png_data_urls(targets: &[std::path::PathBuf]) -> Vec<Option<String>> {
    let threads = std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(4)
        .clamp(1, 8);
    if threads == 1 || targets.len() < 8 {
        return targets.iter().map(|target| png_data_url(target)).collect();
    }

    let chunk = targets.len().div_ceil(threads);
    let mut icons: Vec<Option<String>> = vec![None; targets.len()];
    std::thread::scope(|scope| {
        for (paths, slots) in targets.chunks(chunk).zip(icons.chunks_mut(chunk)) {
            scope.spawn(move || {
                for (path, slot) in paths.iter().zip(slots.iter_mut()) {
                    *slot = png_data_url(path);
                }
            });
        }
    });
    icons
}

/// A `.lnk`'s target. The shell paints a shortcut arrow onto a link's own icon,
/// so links are resolved first and the target's icon is used instead.
fn shortcut_target(target: &Path) -> Option<std::path::PathBuf> {
    if !target
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("lnk"))
    {
        return None;
    }
    // Safety: COM is initialised for the duration of this call, and the path
    // returned is owned data, so nothing outlives the apartment.
    unsafe {
        let initialized = CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok();
        let resolved = resolve_link(target);
        if initialized {
            CoUninitialize();
        }
        resolved
    }
}

/// # Safety
/// In the caller's apartment; `resolve_link` only borrows COM objects locally.
unsafe fn resolve_link(target: &Path) -> Option<std::path::PathBuf> {
    let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
    let file: IPersistFile = link.cast().ok()?;
    let wide: Vec<u16> = target
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    file.Load(PCWSTR(wide.as_ptr()), STGM_READ).ok()?;
    let mut buffer = [0u16; 1024];
    let mut data = WIN32_FIND_DATAW::default();
    link.GetPath(&mut buffer, &mut data, 0).ok()?;
    let length = buffer.iter().position(|unit| *unit == 0).unwrap_or(0);
    if length == 0 {
        return None;
    }
    Some(std::path::PathBuf::from(String::from_utf16_lossy(
        &buffer[..length],
    )))
}

fn render(target: &Path) -> Option<Vec<u8>> {
    let resolved = shortcut_target(target);
    let target = resolved.as_deref().unwrap_or(target);
    let wide: Vec<u16> = target
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut file_info = SHFILEINFOW::default();
    // Safety: `wide` is NUL-terminated and `file_info` outlives the call.
    let found = unsafe {
        SHGetFileInfoW(
            PCWSTR(wide.as_ptr()),
            FILE_FLAGS_AND_ATTRIBUTES(0),
            Some(&mut file_info),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON,
        )
    };
    if found == 0 || file_info.hIcon.is_invalid() {
        return None;
    }

    // Safety: the handle came from the shell above and is destroyed once here.
    let png = unsafe { icon_png(file_info.hIcon) };
    unsafe {
        let _ = DestroyIcon(file_info.hIcon);
    }
    png
}

/// # Safety
/// `icon` must be a live icon handle; this does not take ownership of it.
unsafe fn icon_png(icon: windows::Win32::UI::WindowsAndMessaging::HICON) -> Option<Vec<u8>> {
    let mut icon_info = windows::Win32::UI::WindowsAndMessaging::ICONINFO::default();
    GetIconInfo(icon, &mut icon_info).ok()?;
    let colour = icon_info.hbmColor;
    let mask = icon_info.hbmMask;
    let png = if colour.is_invalid() {
        None
    } else {
        colour_bitmap_png(colour, mask)
    };
    if !colour.is_invalid() {
        let _ = DeleteObject(HGDIOBJ(colour.0));
    }
    if !mask.is_invalid() {
        let _ = DeleteObject(HGDIOBJ(mask.0));
    }
    png
}

unsafe fn colour_bitmap_png(
    colour: windows::Win32::Graphics::Gdi::HBITMAP,
    mask: windows::Win32::Graphics::Gdi::HBITMAP,
) -> Option<Vec<u8>> {
    let mut bitmap = BITMAP::default();
    let read = GetObjectW(
        HGDIOBJ(colour.0),
        std::mem::size_of::<BITMAP>() as i32,
        Some(&mut bitmap as *mut BITMAP as *mut std::ffi::c_void),
    );
    if read == 0 {
        return None;
    }
    let (width, height) = (bitmap.bmWidth, bitmap.bmHeight);
    if width <= 0 || height <= 0 || width > 512 || height > 512 {
        return None;
    }

    let mut pixels = vec![0u8; width as usize * height as usize * 4];
    let mut header = BITMAPINFO::default();
    header.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
    header.bmiHeader.biWidth = width;
    // Negative height asks for top-down rows, matching the RGBA buffer.
    header.bmiHeader.biHeight = -height;
    header.bmiHeader.biPlanes = 1;
    header.bmiHeader.biBitCount = 32;
    header.bmiHeader.biCompression = BI_RGB.0;

    let device = CreateCompatibleDC(None);
    if device.is_invalid() {
        return None;
    }
    let lines = GetDIBits(
        device,
        colour,
        0,
        height as u32,
        Some(pixels.as_mut_ptr() as *mut std::ffi::c_void),
        &mut header,
        DIB_RGB_COLORS,
    );
    let _ = DeleteDC(device);
    if lines == 0 {
        return None;
    }

    for pixel in pixels.chunks_exact_mut(4) {
        pixel.swap(0, 2); // BGRA -> RGBA
    }

    // Legacy icons carry transparency in the AND mask rather than an alpha
    // channel; without it they would render as an opaque square.
    if pixels.chunks_exact(4).all(|pixel| pixel[3] == 0) {
        match mask_alpha(mask, width, height) {
            Some(alpha) => {
                for (pixel, value) in pixels.chunks_exact_mut(4).zip(alpha) {
                    pixel[3] = value;
                }
            }
            None => {
                for pixel in pixels.chunks_exact_mut(4) {
                    pixel[3] = 255;
                }
            }
        }
    }

    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, width as u32, height as u32);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().ok()?;
        writer.write_image_data(&pixels).ok()?;
        writer.finish().ok()?;
    }
    Some(out)
}

/// Reads the 1-bit AND mask as per-pixel alpha: a set bit means transparent.
unsafe fn mask_alpha(
    mask: windows::Win32::Graphics::Gdi::HBITMAP,
    width: i32,
    height: i32,
) -> Option<Vec<u8>> {
    if mask.is_invalid() {
        return None;
    }
    let mut header = BITMAPINFO::default();
    header.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
    header.bmiHeader.biWidth = width;
    header.bmiHeader.biHeight = -height;
    header.bmiHeader.biPlanes = 1;
    header.bmiHeader.biBitCount = 1;
    header.bmiHeader.biCompression = BI_RGB.0;

    let stride = (width as usize).div_ceil(32) * 4;
    let mut bits = vec![0u8; stride * height as usize];
    let device = CreateCompatibleDC(None);
    if device.is_invalid() {
        return None;
    }
    let lines = GetDIBits(
        device,
        mask,
        0,
        height as u32,
        Some(bits.as_mut_ptr() as *mut std::ffi::c_void),
        &mut header,
        DIB_RGB_COLORS,
    );
    let _ = DeleteDC(device);
    if lines == 0 {
        return None;
    }

    let mut alpha = vec![255u8; width as usize * height as usize];
    for row in 0..height as usize {
        for column in 0..width as usize {
            let byte = bits[row * stride + column / 8];
            if byte >> (7 - column % 8) & 1 == 1 {
                alpha[row * width as usize + column] = 0;
            }
        }
    }
    Some(alpha)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_a_shell_icon_as_png() {
        let exe = Path::new(r"C:\Windows\System32\notepad.exe");
        if !exe.exists() {
            return;
        }
        let url = png_data_url(exe).expect("notepad has an icon");
        assert!(url.starts_with("data:image/png;base64,"));
        let encoded = url.trim_start_matches("data:image/png;base64,");
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .expect("valid base64");
        assert!(bytes.starts_with(&[0x89, b'P', b'N', b'G']));
        assert!(bytes.len() > 200, "icon PNG looks empty");
    }
}

#[cfg(test)]
mod shortcut_tests {
    use super::*;

    fn collect_links(dir: &Path, depth: usize, out: &mut Vec<std::path::PathBuf>) {
        if depth > 4 || out.len() >= 10 {
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_links(&path, depth + 1, out);
            } else if path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("lnk"))
            {
                out.push(path);
            }
        }
    }

    #[test]
    fn resolves_start_menu_shortcuts_to_their_target() {
        let Some(start) = std::env::var_os("APPDATA")
            .map(|dir| Path::new(&dir).join("Microsoft/Windows/Start Menu/Programs"))
            .filter(|path| path.is_dir())
        else {
            return;
        };

        let mut links = Vec::new();
        collect_links(&start, 0, &mut links);
        if links.is_empty() {
            return;
        }

        let resolved: Vec<_> = links
            .iter()
            .take(6)
            .filter_map(|link| shortcut_target(link))
            .collect();
        assert!(
            !resolved.is_empty(),
            "no Start Menu shortcut resolved its target"
        );
        assert!(resolved.iter().all(|target| target.is_absolute()));
    }
}
