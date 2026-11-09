use anyhow::{anyhow, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::Serialize;
use std::sync::atomic::{AtomicU32, Ordering};
use sysinfo::{Disks, Networks, System};

static LAST_CLIPBOARD_SEQUENCE: AtomicU32 = AtomicU32::new(0);

#[derive(Serialize)]
pub struct ClipboardItem {
    kind: &'static str,
    data: String,
}

pub fn clipboard() -> Result<ClipboardItem> {
    let sequence = unsafe { windows::Win32::System::DataExchange::GetClipboardSequenceNumber() };
    if sequence != 0 && sequence == LAST_CLIPBOARD_SEQUENCE.load(Ordering::Relaxed) {
        return Ok(ClipboardItem {
            kind: "text",
            data: String::new(),
        });
    }
    let mut clipboard = arboard::Clipboard::new()?;
    if let Ok(image) = clipboard.get_image() {
        if image.width > 4096 || image.height > 4096 {
            return Err(anyhow!("Image is too large for history"));
        }
        let mut encoded = Vec::new();
        {
            let mut encoder =
                png::Encoder::new(&mut encoded, image.width as u32, image.height as u32);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder.write_header()?.write_image_data(&image.bytes)?;
        }
        if encoded.len() > 1_500_000 {
            return Err(anyhow!("Image exceeds the history size limit"));
        }
        LAST_CLIPBOARD_SEQUENCE.store(sequence, Ordering::Relaxed);
        return Ok(ClipboardItem {
            kind: "image",
            data: STANDARD.encode(encoded),
        });
    }
    let text = clipboard.get_text()?;
    LAST_CLIPBOARD_SEQUENCE.store(sequence, Ordering::Relaxed);
    Ok(ClipboardItem {
        kind: "text",
        data: text,
    })
}

pub fn copy_image(data: &str) -> Result<()> {
    let encoded = STANDARD.decode(data)?;
    if encoded.len() > 1_500_000 {
        return Err(anyhow!("Image exceeds the history size limit"));
    }
    let decoder = png::Decoder::new(encoded.as_slice());
    let mut reader = decoder.read_info()?;
    if reader.info().width > 4096 || reader.info().height > 4096 {
        return Err(anyhow!("Image is too large"));
    }
    let mut pixels = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut pixels)?;
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        return Err(anyhow!("Unsupported image format"));
    }
    arboard::Clipboard::new()?.set_image(arboard::ImageData {
        width: info.width as usize,
        height: info.height as usize,
        bytes: pixels.into(),
    })?;
    Ok(())
}

#[derive(Serialize)]
pub struct ProcessInfo {
    name: String,
    memory: u64,
}

#[derive(Serialize)]
pub struct Stats {
    cpu: f32,
    memory_used: u64,
    memory_total: u64,
    uptime: u64,
    disk_used: u64,
    disk_total: u64,
    received: u64,
    transmitted: u64,
    processes: Vec<ProcessInfo>,
    gpu: Option<GpuInfo>,
}

#[derive(Serialize)]
pub struct GpuInfo {
    name: String,
    usage: f32,
    used_mb: u64,
    total_mb: u64,
}

fn gpu_info() -> Option<GpuInfo> {
    use std::os::windows::process::CommandExt;
    let executable = std::env::var_os("SystemRoot")
        .map(|root| std::path::PathBuf::from(root).join("System32/nvidia-smi.exe"))
        .filter(|path| path.is_file())
        .or_else(|| {
            std::env::var_os("ProgramFiles").map(|root| {
                std::path::PathBuf::from(root).join("NVIDIA Corporation/NVSMI/nvidia-smi.exe")
            })
        })?;
    let output = std::process::Command::new(executable)
        .args([
            "--query-gpu=name,utilization.gpu,memory.used,memory.total",
            "--format=csv,noheader,nounits",
        ])
        .creation_flags(0x08000000)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    parse_gpu(&text)
}

fn parse_gpu(text: &str) -> Option<GpuInfo> {
    let mut parts = text.lines().next()?.split(',').map(str::trim);
    Some(GpuInfo {
        name: parts.next()?.to_string(),
        usage: parts.next()?.parse().ok()?,
        used_mb: parts.next()?.parse().ok()?,
        total_mb: parts.next()?.parse().ok()?,
    })
}

#[cfg(test)]
mod tests {
    use super::parse_gpu;

    #[test]
    fn gpu_reading_requires_all_numeric_fields() {
        let gpu = parse_gpu("NVIDIA Example, 23, 500, 8000\n").unwrap();
        assert_eq!(gpu.name, "NVIDIA Example");
        assert_eq!(gpu.usage, 23.0);
        assert_eq!(gpu.total_mb, 8000);
        assert!(parse_gpu("NVIDIA Example, N/A, 500, 8000").is_none());
        assert!(parse_gpu("").is_none());
    }
}

#[derive(Serialize)]
pub struct SpeedResult {
    download_mbps: f64,
    upload_mbps: f64,
}

pub async fn speed_test() -> Result<SpeedResult> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;
    let start = std::time::Instant::now();
    let download = client
        .get("https://speed.cloudflare.com/__down?bytes=10000000")
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;
    let download_mbps = download.len() as f64 * 8.0 / start.elapsed().as_secs_f64() / 1_000_000.0;
    let start = std::time::Instant::now();
    client
        .post("https://speed.cloudflare.com/__up")
        .body(vec![0u8; 2_000_000])
        .send()
        .await?
        .error_for_status()?;
    let upload_mbps = 16.0 / start.elapsed().as_secs_f64();
    Ok(SpeedResult {
        download_mbps,
        upload_mbps,
    })
}

pub fn stats() -> Stats {
    let mut system = System::new_all();
    let mut networks = Networks::new_with_refreshed_list();
    std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
    system.refresh_all();
    networks.refresh(true);
    let disks = Disks::new_with_refreshed_list();
    let mut processes: Vec<_> = system
        .processes()
        .values()
        .map(|p| ProcessInfo {
            name: p.name().to_string_lossy().into_owned(),
            memory: p.memory(),
        })
        .collect();
    processes.sort_by_key(|p| std::cmp::Reverse(p.memory));
    processes.truncate(4);
    Stats {
        cpu: system.global_cpu_usage(),
        memory_used: system.used_memory(),
        memory_total: system.total_memory(),
        uptime: System::uptime(),
        disk_used: disks
            .list()
            .iter()
            .map(|d| d.total_space().saturating_sub(d.available_space()))
            .sum(),
        disk_total: disks.list().iter().map(|d| d.total_space()).sum(),
        received: (networks.values().map(|n| n.received()).sum::<u64>() as f64
            / sysinfo::MINIMUM_CPU_UPDATE_INTERVAL.as_secs_f64()) as u64,
        transmitted: (networks.values().map(|n| n.transmitted()).sum::<u64>() as f64
            / sysinfo::MINIMUM_CPU_UPDATE_INTERVAL.as_secs_f64()) as u64,
        processes,
        gpu: gpu_info(),
    }
}
