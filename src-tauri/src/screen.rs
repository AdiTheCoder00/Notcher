//! Screen capture for circle-to-ask, in memory only.
//!
//! Frames live in a `Vec<u8>`, are encoded to PNG in RAM, are handed to Gemini,
//! and are dropped. Nothing here writes a file, touches the clipboard or keeps a
//! history.
//!
//! Geometry: the process is per-monitor DPI aware v2, so `GetCursorPos`,
//! `GetMonitorInfoW` and `BitBlt` all speak physical pixels. The overlay sends
//! its selection normalised (0..1), which keeps CSS pixels out of the crop maths.

use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits,
    GetMonitorInfoW, MonitorFromPoint, ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER,
    DIB_RGB_COLORS, HGDIOBJ, MONITORINFO, MONITOR_DEFAULTTONEAREST, RGBQUAD, SRCCOPY,
};
use windows::Win32::UI::HiDpi::{
    GetDpiForMonitor, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
    MDT_EFFECTIVE_DPI,
};
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

/// Padding added around the drawn loop, in physical pixels.
pub const PADDING: i32 = 16;
/// Width of the downscaled full-screen context image sent with the crop.
pub const CONTEXT_WIDTH: u32 = 1280;

/// The monitor under the cursor, in physical pixels, plus its DPI scale.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MonitorRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub scale: f64,
}

/// The overlay's selection, normalised to 0..1 of the monitor.
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct NormalizedRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// A rectangle inside a captured frame, in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PixelRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl PixelRect {
    pub fn is_tiny(&self, minimum: u32) -> bool {
        self.width < minimum || self.height < minimum
    }
}

/// A captured frame as tightly packed RGBA. In memory only.
#[derive(Debug, Clone)]
pub struct Capture {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Capture {
    pub fn crop(&self, rect: PixelRect) -> Capture {
        let x = rect.x.min(self.width.saturating_sub(1));
        let y = rect.y.min(self.height.saturating_sub(1));
        let width = rect.width.min(self.width - x).max(1);
        let height = rect.height.min(self.height - y).max(1);

        let mut rgba = Vec::with_capacity((width * height * 4) as usize);
        for row in 0..height {
            let start = (((y + row) * self.width + x) * 4) as usize;
            let end = start + (width * 4) as usize;
            if let Some(slice) = self.rgba.get(start..end) {
                rgba.extend_from_slice(slice);
            }
        }

        Capture {
            width,
            height,
            rgba,
        }
    }

    /// Area-averaged downscale, used for the full-screen context image. A box
    /// filter avoids the shimmer of nearest-neighbour on text-heavy screenshots.
    pub fn downscaled_to_width(&self, target_width: u32) -> Capture {
        if self.width == 0 || self.height == 0 || self.width <= target_width {
            return self.clone();
        }

        let target_width = target_width.max(1);
        let target_height = ((f64::from(self.height) * f64::from(target_width)
            / f64::from(self.width))
        .round() as u32)
            .max(1);

        let mut rgba = vec![0u8; (target_width * target_height * 4) as usize];
        let scale_x = f64::from(self.width) / f64::from(target_width);
        let scale_y = f64::from(self.height) / f64::from(target_height);

        for ty in 0..target_height {
            let y0 = (f64::from(ty) * scale_y).floor() as u32;
            let y1 = ((f64::from(ty + 1) * scale_y).ceil() as u32).min(self.height);

            for tx in 0..target_width {
                let x0 = (f64::from(tx) * scale_x).floor() as u32;
                let x1 = ((f64::from(tx + 1) * scale_x).ceil() as u32).min(self.width);

                let mut sums = [0u32; 4];
                let mut count = 0u32;

                for y in y0..y1.max(y0 + 1).min(self.height) {
                    for x in x0..x1.max(x0 + 1).min(self.width) {
                        let index = ((y * self.width + x) * 4) as usize;
                        if let Some(pixel) = self.rgba.get(index..index + 4) {
                            for channel in 0..4 {
                                sums[channel] += u32::from(pixel[channel]);
                            }
                            count += 1;
                        }
                    }
                }

                let out = ((ty * target_width + tx) * 4) as usize;
                let divisor = count.max(1);
                for channel in 0..4 {
                    rgba[out + channel] = (sums[channel] / divisor) as u8;
                }
            }
        }

        Capture {
            width: target_width,
            height: target_height,
            rgba,
        }
    }

    pub fn to_png(&self) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut out, self.width, self.height);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().context("start the PNG stream")?;
            writer
                .write_image_data(&self.rgba)
                .context("encode the captured frame")?;
            writer.finish().context("finish the PNG stream")?;
        }
        Ok(out)
    }
}

/// Tauri's manifest normally requests per-monitor v2 already; setting it again
/// fails harmlessly. It guarantees every coordinate below is a physical pixel.
pub fn ensure_dpi_awareness() {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
}

/// The monitor containing the cursor.
pub fn monitor_under_cursor() -> Result<MonitorRect> {
    unsafe {
        let mut point = POINT { x: 0, y: 0 };
        GetCursorPos(&mut point).context("read the cursor position")?;

        let monitor = MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST);
        if monitor.0.is_null() {
            return Err(anyhow!("no monitor under the cursor"));
        }

        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            rcMonitor: RECT {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            },
            rcWork: RECT {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            },
            dwFlags: 0,
        };

        if GetMonitorInfoW(monitor, &mut info).0 == 0 {
            return Err(anyhow!("could not read the monitor rectangle"));
        }

        let rect = info.rcMonitor;
        Ok(MonitorRect {
            x: rect.left,
            y: rect.top,
            width: (rect.right - rect.left).max(1) as u32,
            height: (rect.bottom - rect.top).max(1) as u32,
            scale: monitor_scale(monitor),
        })
    }
}

fn monitor_scale(monitor: windows::Win32::Graphics::Gdi::HMONITOR) -> f64 {
    let mut dpi_x = 96u32;
    let mut dpi_y = 96u32;

    // Fails on Windows versions without shcore; 100% is the safe assumption.
    let first = unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) };
    if first.is_err() {
        return 1.0;
    }

    f64::from(dpi_x.max(1)) / 96.0
}

/// Maps the loop's normalised box onto physical monitor pixels and pads it.
pub fn crop_rect(selection: NormalizedRect, monitor: MonitorRect, padding: i32) -> PixelRect {
    let width = monitor.width as i32;
    let height = monitor.height as i32;

    let left = (selection.x * f64::from(width)).round() as i32 - padding;
    let top = (selection.y * f64::from(height)).round() as i32 - padding;
    let right = ((selection.x + selection.width) * f64::from(width)).round() as i32 + padding;
    let bottom = ((selection.y + selection.height) * f64::from(height)).round() as i32 + padding;

    let left = left.clamp(0, width);
    let top = top.clamp(0, height);
    let right = right.clamp(left, width);
    let bottom = bottom.clamp(top, height);

    PixelRect {
        x: left as u32,
        y: top as u32,
        width: (right - left).max(0) as u32,
        height: (bottom - top).max(0) as u32,
    }
}

/// Captures the monitor exactly as it is now.
pub fn capture(monitor: MonitorRect) -> Result<Capture> {
    let width = monitor.width.max(1) as i32;
    let height = monitor.height.max(1) as i32;

    unsafe {
        let screen = GetDC(None::<&HWND>);
        if screen.0.is_null() {
            return Err(anyhow!("could not open the screen device context"));
        }

        let result = capture_with(screen, monitor, width, height);
        ReleaseDC(None::<&HWND>, screen);
        result
    }
}

unsafe fn capture_with(
    screen: windows::Win32::Graphics::Gdi::HDC,
    monitor: MonitorRect,
    width: i32,
    height: i32,
) -> Result<Capture> {
    let memory = CreateCompatibleDC(screen);
    if memory.0.is_null() {
        return Err(anyhow!("could not create a capture device context"));
    }

    let bitmap = CreateCompatibleBitmap(screen, width, height);
    if bitmap.0.is_null() {
        let _ = DeleteDC(memory);
        return Err(anyhow!("could not allocate a capture bitmap"));
    }

    let previous = SelectObject(memory, HGDIOBJ(bitmap.0));

    let result = (|| -> Result<Capture> {
        // Source coordinates are virtual-desktop physical pixels, so a
        // secondary monitor at x = 2560 captures correctly.
        BitBlt(
            memory, 0, 0, width, height, screen, monitor.x, monitor.y, SRCCOPY,
        )
        .context("copy the screen pixels")?;

        // GetDIBits refuses a bitmap that is still selected into a DC, so put
        // the original object back before reading the pixels.
        SelectObject(memory, previous);
        read_pixels(memory, bitmap, width, height)
    })();

    SelectObject(memory, previous);
    let _ = DeleteObject(HGDIOBJ(bitmap.0));
    let _ = DeleteDC(memory);
    result
}

unsafe fn read_pixels(
    memory: windows::Win32::Graphics::Gdi::HDC,
    bitmap: windows::Win32::Graphics::Gdi::HBITMAP,
    width: i32,
    height: i32,
) -> Result<Capture> {
    let mut info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            // Negative height asks for a top-down DIB, matching the row order
            // PNG and the crop maths expect.
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: 0, // BI_RGB
            biSizeImage: 0,
            biXPelsPerMeter: 0,
            biYPelsPerMeter: 0,
            biClrUsed: 0,
            biClrImportant: 0,
        },
        bmiColors: [RGBQUAD {
            rgbBlue: 0,
            rgbGreen: 0,
            rgbRed: 0,
            rgbReserved: 0,
        }],
    };

    let mut bgra = vec![0u8; (width * height * 4) as usize];

    let copied = GetDIBits(
        memory,
        bitmap,
        0,
        height as u32,
        Some(bgra.as_mut_ptr().cast::<std::ffi::c_void>()),
        &mut info,
        DIB_RGB_COLORS,
    );

    if copied != height {
        return Err(anyhow!(
            "could not read the captured pixels ({copied} of {height} rows)"
        ));
    }

    // GDI hands back BGRA with a meaningless alpha channel.
    for pixel in bgra.chunks_exact_mut(4) {
        pixel.swap(0, 2);
        pixel[3] = 255;
    }

    Ok(Capture {
        width: width as u32,
        height: height as u32,
        rgba: bgra,
    })
}
