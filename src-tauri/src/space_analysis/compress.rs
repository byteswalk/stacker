//! Making big files smaller without deleting them.
//!
//! Three ways, by what a file is:
//! - anything not already compressed: Windows' own transparent compression (`compact /EXE:LZX`).
//!   The file keeps its name, format and contents; NTFS stores it smaller and every program
//!   reads it as before. Writing to the file undoes it, so it suits files that stay as they are.
//! - videos: encoded again as H.265 by ffmpeg, at one of three quality levels, and no larger
//!   than a chosen resolution (never made larger).
//! - photos: encoded again as JPEG, WebP or AVIF, keeping their EXIF and colour profile where
//!   the format holds them.
//!
//! A video or photo is replaced only once the new file is there, plays (its length matches)
//! and is clearly smaller; the original goes to the Recycle Bin, so it can be restored.

use super::file_removal::{is_protected_with, system_roots};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

const FFMPEG_ZIP: &str = "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip";
const FFMPEG_SHA: &str = "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip.sha256";
/// Seconds of a video encoded to estimate the whole.
const SAMPLE_SECONDS: f64 = 8.0;
/// A new file is kept only below this share of the old one.
const WORTH: f64 = 0.92;

static CANCEL: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Video,
    Image,
    Other,
}

const VIDEO: &[&str] = &[
    "mp4", "mkv", "mov", "avi", "wmv", "flv", "m4v", "webm", "ts", "mts", "m2ts", "3gp", "mpg",
    "mpeg",
];
// No TIFF: a scan is often many pages, and the encoder keeps only the first.
const IMAGE: &[&str] = &["jpg", "jpeg", "png", "bmp"];
/// Formats that are compressed already: transparent compression saves next to nothing on them.
const PACKED: &[&str] = &[
    "zip", "7z", "rar", "gz", "tgz", "xz", "bz2", "zst", "lz4", "cab", "msi", "jar", "apk", "mp3",
    "aac", "m4a", "flac", "ogg", "opus", "wma", "jpg", "jpeg", "png", "gif", "webp", "avif",
    "heic", "heif", "mp4", "mkv", "mov", "avi", "wmv", "flv", "m4v", "webm", "ts", "mts", "m2ts",
    "3gp", "mpg", "mpeg", "docx", "xlsx", "pptx", "pdf", "iso", "vhdx", "esd", "wim",
];

fn extension(path: &Path) -> String {
    path.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

pub fn kind_of(path: &Path) -> Kind {
    let ext = extension(path);
    if VIDEO.contains(&ext.as_str()) {
        Kind::Video
    } else if IMAGE.contains(&ext.as_str()) {
        Kind::Image
    } else {
        Kind::Other
    }
}

/// How the user wants videos and photos done.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Options {
    /// "high", "standard" or "small".
    pub video_quality: String,
    /// The most the shorter side may be (720, 1080, 1440, 2160); 0 keeps it.
    pub video_height: u32,
    /// "x265", "nvenc", "qsv" or "amf".
    pub video_encoder: String,
    /// "jpeg", "webp" or "avif".
    pub image_format: String,
    pub image_quality: String,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            video_quality: "standard".into(),
            video_height: 0,
            video_encoder: "x265".into(),
            image_format: "jpeg".into(),
            image_quality: "standard".into(),
        }
    }
}

fn level(quality: &str, high: u32, standard: u32, small: u32) -> u32 {
    match quality {
        "high" => high,
        "small" => small,
        _ => standard,
    }
}

// ── Processes ────────────────────────────────────────────────────────────────────────────

fn hide_console(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
}

pub fn ffmpeg_dir() -> PathBuf {
    crate::installer::managed_tools_root().join("ffmpeg")
}

fn tool(name: &str) -> Option<PathBuf> {
    let own = ffmpeg_dir().join("bin").join(format!("{name}.exe"));
    if own.is_file() {
        return Some(own);
    }
    crate::agents::process::resolve_command(&[&format!("{name}.exe")])
}

/// Runs a short command to its end and returns stdout, or why it failed.
fn run(program: &Path, args: &[String], timeout: Duration) -> Result<String, String> {
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    hide_console(&mut cmd);
    let mut child = cmd.spawn().map_err(|e| e.to_string())?;
    let mut out = child.stdout.take().ok_or("E_PIPE")?;
    let mut err = child.stderr.take().ok_or("E_PIPE")?;
    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = out.read_to_string(&mut text);
        text
    });
    let errors = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = err.read_to_string(&mut text);
        text
    });
    let started = std::time::Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            break status;
        }
        if CANCEL.load(Ordering::Relaxed) || started.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(if CANCEL.load(Ordering::Relaxed) {
                "E_CANCELLED".into()
            } else {
                "E_TIMEOUT".into()
            });
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let text = reader.join().unwrap_or_default();
    let errors = errors.join().unwrap_or_default();
    if status.success() {
        Ok(text)
    } else {
        Err(errors
            .lines()
            .rev()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("failed")
            .trim()
            .to_string())
    }
}

/// What this ffmpeg can make: x265 always, a graphics card's encoder when the card answers,
/// WebP and AVIF when the build has them.
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaTools {
    pub ffmpeg: Option<String>,
    pub version: String,
    pub video_encoders: Vec<String>,
    pub image_formats: Vec<String>,
}

static TOOLS: Mutex<Option<MediaTools>> = Mutex::new(None);

fn detect_tools() -> MediaTools {
    let Some(ffmpeg) = tool("ffmpeg") else {
        return MediaTools::default();
    };
    let args = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let version = run(
        &ffmpeg,
        &args(&["-hide_banner", "-version"]),
        Duration::from_secs(10),
    )
    .ok()
    .and_then(|text| text.lines().next().map(str::to_string))
    .unwrap_or_default();
    let encoders = run(
        &ffmpeg,
        &args(&["-hide_banner", "-encoders"]),
        Duration::from_secs(10),
    )
    .unwrap_or_default();
    let has = |name: &str| encoders.split_whitespace().any(|word| word == name);
    let mut video = Vec::new();
    if has("libx265") {
        video.push("x265".to_string());
    }
    // A card's encoder is listed whether or not the card is there; a tiny encode tells.
    for (id, encoder) in [
        ("nvenc", "hevc_nvenc"),
        ("qsv", "hevc_qsv"),
        ("amf", "hevc_amf"),
    ] {
        if has(encoder)
            && run(
                &ffmpeg,
                &args(&[
                    "-hide_banner",
                    "-loglevel",
                    "error",
                    "-f",
                    "lavfi",
                    "-i",
                    "color=c=black:s=320x240:d=0.2",
                    "-c:v",
                    encoder,
                    "-f",
                    "null",
                    "-",
                ]),
                Duration::from_secs(15),
            )
            .is_ok()
        {
            video.push(id.to_string());
        }
    }
    let mut images = vec!["jpeg".to_string()];
    if has("libwebp") {
        images.push("webp".into());
    }
    if has("libaom-av1") {
        images.push("avif".into());
    }
    MediaTools {
        ffmpeg: Some(ffmpeg.to_string_lossy().into_owned()),
        version,
        video_encoders: video,
        image_formats: images,
    }
}

#[tauri::command]
pub async fn space_media_tools(refresh: Option<bool>) -> Result<MediaTools, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut cached = TOOLS.lock().map_err(|e| e.to_string())?;
        if refresh.unwrap_or(false) || cached.is_none() {
            *cached = Some(detect_tools());
        }
        Ok(cached.clone().unwrap_or_default())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Downloads ffmpeg (gyan.dev's release essentials build), checks it against the SHA-256 the
/// site publishes, and unpacks it into Stacker's tools folder.
/// Downloads ffmpeg (gyan.dev's release essentials build), checks it against the SHA-256 the
/// site publishes, and unpacks it into Stacker's tools folder.
pub fn install_ffmpeg(mut progress: impl FnMut(String)) -> Result<MediaTools, String> {
    crate::installer::op_reset();
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(30))
        .timeout_read(Duration::from_secs(30))
        .build();
    let expected = agent
        .get(FFMPEG_SHA)
        .set("User-Agent", "Stacker")
        .call()
        .map_err(|e| format!("无法获取 ffmpeg 的校验值：{e}"))?
        .into_string()
        .map_err(|e| e.to_string())?
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    if expected.len() != 64 {
        return Err("ffmpeg 的校验值格式不对".into());
    }
    let archive = std::env::temp_dir().join(format!("stacker_ffmpeg_{}.zip", std::process::id()));
    crate::installer::download_file_candidates_with_agent(
        &agent,
        &[FFMPEG_ZIP.to_string()],
        &archive,
        50 * 1024 * 1024,
        &mut progress,
    )?;
    progress("正在校验下载的文件…".to_string());
    let actual = sha256_of(&archive)?;
    if actual != expected {
        let _ = std::fs::remove_file(&archive);
        return Err("下载的 ffmpeg 与官方校验值不一致，已丢弃，请重试".into());
    }
    progress("正在解压…".to_string());
    let dest = ffmpeg_dir();
    let _ = std::fs::remove_dir_all(&dest);
    let extracted = crate::installer::extract_zip(&archive, &dest, true);
    let _ = std::fs::remove_file(&archive);
    extracted.map_err(|e| format!("解压失败：{e}"))?;
    let tools = detect_tools();
    if let Ok(mut cached) = TOOLS.lock() {
        *cached = Some(tools.clone());
    }
    tools
        .ffmpeg
        .as_ref()
        .ok_or("ffmpeg 解压后没有找到 ffmpeg.exe")?;
    Ok(tools)
}

#[tauri::command]
pub async fn space_ffmpeg_install(window: tauri::Window) -> Result<MediaTools, String> {
    use tauri::Emitter;
    tauri::async_runtime::spawn_blocking(move || {
        let tools = install_ffmpeg(|line| {
            let _ = window.emit("install-progress", line);
        });
        let _ = window.emit("install-progress", "__done__".to_string());
        tools
    })
    .await
    .map_err(|e| e.to_string())?
}

fn sha256_of(path: &Path) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = file.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

// ── Sizes on disk ────────────────────────────────────────────────────────────────────────

/// What a file takes on disk, which transparent compression lowers while its length stays.
#[cfg(windows)]
pub fn allocated(path: &Path) -> u64 {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetCompressedFileSizeW;
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut high = 0u32;
    // SAFETY: a NUL-terminated path and a valid out pointer.
    let low = unsafe { GetCompressedFileSizeW(wide.as_ptr(), &mut high) };
    if low == u32::MAX && std::io::Error::last_os_error().raw_os_error().unwrap_or(0) != 0 {
        return std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    }
    (u64::from(high) << 32) | u64::from(low)
}

#[cfg(not(windows))]
pub fn allocated(path: &Path) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

/// Transparent compression works in 32 KB pieces, so a few pieces deflated on their own say
/// what the whole would come to, near enough.
fn estimate_packed(path: &Path, size: u64) -> Result<u64, String> {
    use flate2::write::DeflateEncoder;
    use flate2::Compression;
    use std::io::{Seek, SeekFrom, Write};
    const PIECE: usize = 32 * 1024;
    const PIECES: u64 = 24;
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let (mut read, mut packed) = (0u64, 0u64);
    let mut buf = vec![0u8; PIECE];
    for i in 0..PIECES {
        let at = size.saturating_sub(PIECE as u64) * i / PIECES.max(1);
        file.seek(SeekFrom::Start(at)).map_err(|e| e.to_string())?;
        let n = file.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        let mut encoder = DeflateEncoder::new(Vec::new(), Compression::new(6));
        encoder.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        let out = encoder.finish().map_err(|e| e.to_string())?;
        // A piece that does not shrink is stored as it is.
        packed += (out.len() as u64).min(n as u64);
        read += n as u64;
        if (n as u64) < PIECE as u64 {
            break;
        }
    }
    if read == 0 {
        return Ok(size);
    }
    // NTFS stores in 4 KB clusters, and LZX does a little better than deflate.
    Ok(((size as f64) * (packed as f64 / read as f64) * 0.95) as u64)
}

// ── Media ────────────────────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Probe {
    pub duration: f64,
    pub width: u32,
    pub height: u32,
    pub audio: Option<String>,
    /// Streams besides the first video and the audio: subtitles, data (GPS telemetry),
    /// timecode, further video. An MP4 re-encode maps none of them.
    pub others: usize,
}

pub fn parse_probe(json: &str) -> Option<Probe> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    let streams = value.get("streams")?.as_array()?;
    let video = streams
        .iter()
        .find(|s| s.get("codec_type").and_then(|t| t.as_str()) == Some("video"))?;
    let audio = streams
        .iter()
        .find(|s| s.get("codec_type").and_then(|t| t.as_str()) == Some("audio"))
        .and_then(|s| s.get("codec_name").and_then(|c| c.as_str()))
        .map(str::to_string);
    let number = |v: Option<&serde_json::Value>| {
        v.and_then(|v| {
            v.as_f64()
                .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        })
    };
    let audio_streams = streams
        .iter()
        .filter(|s| s.get("codec_type").and_then(|t| t.as_str()) == Some("audio"))
        .count();
    Some(Probe {
        duration: number(value.get("format").and_then(|f| f.get("duration"))).unwrap_or(0.0),
        width: number(video.get("width")).unwrap_or(0.0) as u32,
        height: number(video.get("height")).unwrap_or(0.0) as u32,
        audio,
        others: streams.len().saturating_sub(audio_streams + 1),
    })
}

fn probe(path: &Path) -> Result<Probe, String> {
    let ffprobe = tool("ffprobe").ok_or("E_NO_FFMPEG")?;
    let args = [
        "-v",
        "error",
        "-show_entries",
        "stream=codec_type,codec_name,width,height:format=duration",
        "-of",
        "json",
    ]
    .iter()
    .map(|s| s.to_string())
    .chain(Some(path.to_string_lossy().into_owned()))
    .collect::<Vec<_>>();
    let text = run(&ffprobe, &args, Duration::from_secs(60))?;
    parse_probe(&text).ok_or_else(|| "无法读取视频信息".to_string())
}

/// The scale filter that brings the shorter side down to `cap`, never up; none when it fits.
pub fn scale_filter(width: u32, height: u32, cap: u32) -> Option<String> {
    if cap == 0 || width.min(height) <= cap {
        return None;
    }
    Some(if width >= height {
        format!("scale=-2:{cap}")
    } else {
        format!("scale={cap}:-2")
    })
}

/// Audio a container takes as it is; anything else is made AAC.
fn audio_fits(container: &str, audio: &str) -> bool {
    container == "mkv" || ["aac", "mp3", "ac3", "eac3", "alac", "opus", "flac"].contains(&audio)
}

/// The output container: a Matroska file stays one (it holds anything), the rest become MP4.
pub fn video_container(path: &Path) -> &'static str {
    if extension(path) == "mkv" {
        "mkv"
    } else {
        "mp4"
    }
}

/// ffmpeg's arguments for a video, between the input and the output names.
pub fn video_args(options: &Options, probe: &Probe, container: &str) -> Vec<String> {
    let mut args: Vec<String> = Vec::new();
    if container == "mkv" {
        args.extend(["-map", "0"].map(String::from));
    } else {
        args.extend(["-map", "0:v:0", "-map", "0:a?"].map(String::from));
    }
    let q =
        |high, standard, small| level(&options.video_quality, high, standard, small).to_string();
    match options.video_encoder.as_str() {
        "nvenc" => args.extend(
            [
                "-c:v",
                "hevc_nvenc",
                "-preset",
                "p5",
                "-rc",
                "vbr",
                "-b:v",
                "0",
                "-cq",
            ]
            .map(String::from)
            .into_iter()
            .chain([q(23, 28, 32)]),
        ),
        "qsv" => args.extend(
            ["-c:v", "hevc_qsv", "-preset", "medium", "-global_quality"]
                .map(String::from)
                .into_iter()
                .chain([q(22, 26, 30)]),
        ),
        "amf" => {
            let level = q(22, 26, 30);
            args.extend(
                ["-c:v", "hevc_amf", "-rc", "cqp", "-qp_i"]
                    .map(String::from)
                    .into_iter()
                    .chain([level.clone(), "-qp_p".into(), level]),
            )
        }
        _ => args.extend(
            [
                "-c:v",
                "libx265",
                "-preset",
                "medium",
                "-x265-params",
                "log-level=error",
                "-crf",
            ]
            .map(String::from)
            .into_iter()
            .chain([q(22, 26, 30)]),
        ),
    }
    if container == "mp4" {
        // Apple's players and Windows' own play H.265 in MP4 only with this tag.
        args.extend(["-tag:v", "hvc1", "-movflags", "+faststart"].map(String::from));
    }
    if let Some(filter) = scale_filter(probe.width, probe.height, options.video_height) {
        args.extend(["-vf".to_string(), filter]);
    }
    match &probe.audio {
        Some(audio) if !audio_fits(container, audio) => {
            args.extend(["-c:a", "aac", "-b:a", "192k"].map(String::from))
        }
        Some(_) => args.extend(["-c:a", "copy"].map(String::from)),
        None => {}
    }
    if container == "mkv" {
        args.extend(["-c:s", "copy"].map(String::from));
    }
    args.extend(["-map_metadata", "0"].map(String::from));
    args
}

/// The format a photo becomes, and its extension: a PNG keeps its sharp edges and
/// transparency as lossless WebP, or stays as it is when JPEG is asked for.
pub fn image_target(path: &Path, format: &str) -> Option<(&'static str, &'static str)> {
    let png = extension(path) == "png";
    match (format, png) {
        ("webp", _) | ("avif", true) => Some(("webp", "webp")),
        ("avif", false) => Some(("avif", "avif")),
        ("jpeg", true) => None,
        _ => Some(("jpeg", "jpg")),
    }
}

pub fn image_args(path: &Path, options: &Options) -> Option<(Vec<String>, &'static str)> {
    let (format, ext) = image_target(path, &options.image_format)?;
    let q =
        |high, standard, small| level(&options.image_quality, high, standard, small).to_string();
    let png = extension(path) == "png";
    let args: Vec<String> = match format {
        "webp" if png => [
            "-c:v",
            "libwebp",
            "-lossless",
            "1",
            "-compression_level",
            "6",
        ]
        .map(String::from)
        .to_vec(),
        "webp" => ["-c:v", "libwebp", "-quality"]
            .map(String::from)
            .into_iter()
            .chain([q(90, 80, 68)])
            .collect(),
        "avif" => [
            "-c:v",
            "libaom-av1",
            "-still-picture",
            "1",
            "-cpu-used",
            "6",
            "-crf",
        ]
        .map(String::from)
        .into_iter()
        .chain([q(24, 30, 36)])
        .collect(),
        _ => ["-c:v", "mjpeg", "-q:v"]
            .map(String::from)
            .into_iter()
            .chain([q(3, 5, 8)])
            .collect(),
    };
    Some((args, ext))
}

/// An APNG names its animation (`acTL`) before the first image data (`IDAT`).
fn is_animated_png(path: &Path) -> bool {
    if extension(path) != "png" {
        return false;
    }
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let mut head = vec![0u8; 64 * 1024];
    let n = std::io::Read::read(&mut file, &mut head).unwrap_or(0);
    let head = &head[..n];
    let at = |tag: &[u8]| head.windows(4).position(|w| w == tag);
    match (at(b"acTL"), at(b"IDAT")) {
        (Some(actl), Some(idat)) => actl < idat,
        (Some(_), None) => true,
        _ => false,
    }
}

/// Copies the photo's EXIF and colour profile into the new file (JPEG and WebP hold them).
fn carry_metadata(from: &Path, to: &Path) {
    use img_parts::{Bytes, DynImage, ImageEXIF, ImageICC};
    let (Ok(old), Ok(new)) = (std::fs::read(from), std::fs::read(to)) else {
        return;
    };
    let Ok(Some(old)) = DynImage::from_bytes(Bytes::from(old)) else {
        return;
    };
    let Ok(Some(mut new)) = DynImage::from_bytes(Bytes::from(new)) else {
        return;
    };
    new.set_exif(old.exif());
    if old.icc_profile().is_some() {
        new.set_icc_profile(old.icc_profile());
    }
    // Built in memory and swapped in whole: truncating the file and writing it in place left
    // a cut-off image whenever a write failed (a nearly full disk), and that image then
    // replaced the original.
    let mut bytes = Vec::new();
    if new.encoder().write_to(&mut bytes).is_err() {
        return;
    }
    let staged = to.with_extension("stacker-meta");
    if std::fs::write(&staged, &bytes).is_err() || std::fs::rename(&staged, to).is_err() {
        let _ = std::fs::remove_file(&staged);
    }
}

// ── Estimate and run ─────────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Target {
    pub path: String,
    /// The size the scan saw; a file that has changed since is left alone.
    pub bytes: u64,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Outcome {
    pub path: String,
    /// Where the file is now (a photo may change its extension).
    pub new_path: String,
    /// "packed" (transparent), "video", "image".
    pub method: String,
    pub before: u64,
    pub after: u64,
    /// "ok", "estimated", or why it was left: "already", "notWorth", "keepPng", "noFfmpeg",
    /// "protected", "changed", "cancelled", or ffmpeg's own words.
    pub status: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Progress {
    done: usize,
    total: usize,
    path: String,
    percent: f64,
}

fn report(window: &tauri::Window, done: usize, total: usize, path: &str, percent: f64) {
    use tauri::Emitter;
    let _ = window.emit(
        "space-compress-progress",
        Progress {
            done,
            total,
            path: path.to_string(),
            percent,
        },
    );
}

fn method_of(kind: Kind) -> &'static str {
    match kind {
        Kind::Video => "video",
        Kind::Image => "image",
        Kind::Other => "packed",
    }
}

/// A name beside `original` with `ext`, free to write.
fn sibling(original: &Path, tag: &str, ext: &str) -> PathBuf {
    let stem = original
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let dir = original.parent().unwrap_or(Path::new("."));
    dir.join(format!("{stem}{tag}.{ext}"))
}

/// Where the new file goes: the old name when the extension stays, else the same name with the
/// new extension, numbered when that is taken.
fn final_path(original: &Path, ext: &str) -> PathBuf {
    if extension(original) == ext || (ext == "jpg" && extension(original) == "jpeg") {
        return original.to_path_buf();
    }
    let first = sibling(original, "", ext);
    if !first.exists() {
        return first;
    }
    (1..1000)
        .map(|n| sibling(original, &format!(" ({n})"), ext))
        .find(|p| !p.exists())
        .unwrap_or(first)
}

/// Runs ffmpeg to `out`, reporting how far it is through `duration` seconds.
fn encode(
    input_args: &[String],
    input: &Path,
    args: &[String],
    out: &Path,
    duration: f64,
    on_percent: &mut dyn FnMut(f64),
) -> Result<(), String> {
    let ffmpeg = tool("ffmpeg").ok_or("E_NO_FFMPEG")?;
    let mut cmd = Command::new(ffmpeg);
    hide_console(&mut cmd);
    cmd.args([
        "-hide_banner",
        "-loglevel",
        "error",
        "-y",
        "-nostats",
        "-progress",
        "pipe:1",
    ])
    .args(input_args)
    .arg("-i")
    .arg(input)
    .args(args)
    .arg(out)
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| e.to_string())?;
    let job = crate::agents::process::ProcessJob::attach(&child);
    let stdout = child.stdout.take().ok_or("E_PIPE")?;
    let mut stderr = child.stderr.take().ok_or("E_PIPE")?;
    let errors = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = stderr.read_to_string(&mut text);
        text
    });
    let (tx, rx) = std::sync::mpsc::channel::<f64>();
    let lines = std::thread::spawn(move || {
        for line in std::io::BufReader::new(stdout)
            .lines()
            .map_while(Result::ok)
        {
            if let Some(us) = line
                .strip_prefix("out_time_us=")
                .and_then(|v| v.trim().parse::<f64>().ok())
            {
                let _ = tx.send(us / 1_000_000.0);
            }
        }
    });
    let status = loop {
        while let Ok(seconds) = rx.try_recv() {
            if duration > 0.0 {
                on_percent((seconds / duration * 100.0).clamp(0.0, 100.0));
            }
        }
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            break status;
        }
        if CANCEL.load(Ordering::Relaxed) {
            match &job {
                Some(job) => job.terminate(),
                None => {
                    let _ = child.kill();
                }
            }
            let _ = child.wait();
            let _ = lines.join();
            let _ = std::fs::remove_file(out);
            return Err("cancelled".into());
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    let _ = lines.join();
    let errors = errors.join().unwrap_or_default();
    if status.success() && out.is_file() {
        Ok(())
    } else {
        let _ = std::fs::remove_file(out);
        Err(errors
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("ffmpeg failed")
            .trim()
            .to_string())
    }
}

/// A video's new size: a few seconds from its middle, encoded, scaled to its length.
fn estimate_video(path: &Path, options: &Options, size: u64) -> Result<u64, String> {
    let info = probe(path)?;
    let container = video_container(path);
    let out =
        std::env::temp_dir().join(format!("stacker_sample_{}.{container}", std::process::id()));
    let (start, length) = if info.duration > SAMPLE_SECONDS * 2.0 {
        (info.duration / 2.0 - SAMPLE_SECONDS / 2.0, SAMPLE_SECONDS)
    } else {
        (0.0, info.duration.max(0.1))
    };
    let input = vec![
        "-ss".to_string(),
        format!("{start:.2}"),
        "-t".into(),
        format!("{length:.2}"),
    ];
    let result = encode(
        &input,
        path,
        &video_args(options, &info, container),
        &out,
        length,
        &mut |_| {},
    );
    let made = std::fs::metadata(&out).map(|m| m.len()).unwrap_or(0);
    let _ = std::fs::remove_file(&out);
    result?;
    if info.duration <= 0.0 || length <= 0.0 {
        return Ok(size);
    }
    Ok((made as f64 / length * info.duration) as u64)
}

fn encode_image(path: &Path, options: &Options, out: &Path) -> Result<(), String> {
    let (args, ext) = image_args(path, options).ok_or("keepPng")?;
    if is_animated_png(path) {
        return Err("动画 PNG 压缩后只剩第一帧，已跳过".into());
    }
    // EXIF keeps the orientation for JPEG and WebP, so their pixels stay as stored; AVIF
    // loses EXIF, so its pixels are turned upright instead.
    let input: Vec<String> = if ext == "avif" {
        vec![]
    } else {
        vec!["-noautorotate".into()]
    };
    let mut args = args;
    args.extend(["-frames:v", "1", "-map_metadata", "-1"].map(String::from));
    encode(&input, path, &args, out, 0.0, &mut |_| {})?;
    if ext != "avif" {
        carry_metadata(path, out);
    }
    // Decoded again before the original goes: a file that does not open, or opens at another
    // size, never replaces it.
    let unreadable = || "压缩后的图片无法正常读取，已放弃，原图保留".to_string();
    let before = probe(path).map_err(|_| unreadable())?;
    let after = probe(out).map_err(|_| unreadable())?;
    let same = (after.width, after.height) == (before.width, before.height)
        || (after.width, after.height) == (before.height, before.width);
    if after.width == 0 || !same {
        return Err(unreadable());
    }
    Ok(())
}

fn check(target: &Target, roots: &[PathBuf]) -> Result<PathBuf, &'static str> {
    let path = PathBuf::from(&target.path);
    if is_protected_with(&path, roots) {
        return Err("protected");
    }
    let meta = std::fs::metadata(&path).map_err(|_| "changed")?;
    if !meta.is_file() || meta.len() != target.bytes {
        return Err("changed");
    }
    Ok(path)
}

fn estimate_one(target: &Target, options: &Options, roots: &[PathBuf]) -> Outcome {
    let kind = kind_of(Path::new(&target.path));
    let mut outcome = Outcome {
        path: target.path.clone(),
        new_path: target.path.clone(),
        method: method_of(kind).into(),
        before: target.bytes,
        after: target.bytes,
        status: "estimated".into(),
    };
    let path = match check(target, roots) {
        Ok(path) => path,
        Err(reason) => {
            outcome.status = reason.into();
            return outcome;
        }
    };
    let estimated = match kind {
        Kind::Other => {
            outcome.before = allocated(&path).min(target.bytes).max(1);
            if PACKED.contains(&extension(&path).as_str()) {
                Err("already".to_string())
            } else if outcome.before < target.bytes * 9 / 10 {
                // Already stored compressed.
                Err("already".to_string())
            } else {
                estimate_packed(&path, target.bytes)
            }
        }
        _ if tool("ffmpeg").is_none() => Err("noFfmpeg".to_string()),
        Kind::Video => estimate_video(&path, options, target.bytes),
        Kind::Image => match image_args(&path, options) {
            None => Err("keepPng".to_string()),
            Some((_, ext)) => {
                let out = std::env::temp_dir()
                    .join(format!("stacker_sample_{}.{ext}", std::process::id()));
                let made = encode_image(&path, options, &out).map(|_| {
                    std::fs::metadata(&out)
                        .map(|m| m.len())
                        .unwrap_or(target.bytes)
                });
                let _ = std::fs::remove_file(&out);
                made
            }
        },
    };
    match estimated {
        Ok(after) => {
            outcome.after = after;
            if (after as f64) > outcome.before as f64 * WORTH {
                outcome.status = "notWorth".into();
            }
        }
        Err(reason) => outcome.status = reason,
    }
    outcome
}

/// Keeps the file's times on its replacement, so it sorts where it did.
fn keep_times(from: &std::fs::Metadata, to: &Path) {
    let Ok(file) = std::fs::OpenOptions::new().write(true).open(to) else {
        return;
    };
    let mut times = std::fs::FileTimes::new();
    if let Ok(modified) = from.modified() {
        times = times.set_modified(modified);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::FileTimesExt;
        if let Ok(created) = from.created() {
            times = times.set_created(created);
        }
    }
    let _ = file.set_times(times);
}

fn compact(path: &Path) -> Result<(), String> {
    let program = std::env::var_os("SystemRoot")
        .map(|root| PathBuf::from(root).join("System32").join("compact.exe"))
        .unwrap_or_else(|| PathBuf::from("compact.exe"));
    let args = ["/C", "/EXE:LZX", "/Q"]
        .iter()
        .map(|s| s.to_string())
        .chain(Some(path.to_string_lossy().into_owned()))
        .collect::<Vec<_>>();
    run(&program, &args, Duration::from_secs(3600)).map(|_| ())
}

fn run_one(
    target: &Target,
    options: &Options,
    roots: &[PathBuf],
    on_percent: &mut dyn FnMut(f64),
) -> Outcome {
    let kind = kind_of(Path::new(&target.path));
    let mut outcome = Outcome {
        path: target.path.clone(),
        new_path: target.path.clone(),
        method: method_of(kind).into(),
        before: target.bytes,
        after: target.bytes,
        status: "ok".into(),
    };
    let path = match check(target, roots) {
        Ok(path) => path,
        Err(reason) => {
            outcome.status = reason.into();
            return outcome;
        }
    };
    if kind == Kind::Other {
        outcome.before = allocated(&path).min(target.bytes);
        if PACKED.contains(&extension(&path).as_str()) {
            outcome.status = "already".into();
            return outcome;
        }
        if let Err(error) = compact(&path) {
            outcome.status = error;
            return outcome;
        }
        outcome.after = allocated(&path);
        if outcome.after >= outcome.before {
            outcome.status = "notWorth".into();
        }
        return outcome;
    }
    if tool("ffmpeg").is_none() {
        outcome.status = "noFfmpeg".into();
        return outcome;
    }
    let Ok(meta) = std::fs::metadata(&path) else {
        outcome.status = "changed".into();
        return outcome;
    };
    // Replacing one name of a hard-linked file keeps the old data under the others and adds
    // the new file on top: the disk only fills further.
    if super::windows_fs::file_link_count(&path).is_ok_and(|links| links > 1) {
        outcome.status = "这个文件还有其他硬链接，替换它不会省出空间，已跳过".into();
        return outcome;
    }
    // Written beside the original, so the swap is a rename on the same disk.
    let (ext, made) = if kind == Kind::Video {
        let info = match probe(&path) {
            Ok(info) => info,
            Err(error) => {
                outcome.status = error;
                return outcome;
            }
        };
        let container = video_container(&path);
        // The length is how the new file is checked: without it nothing proves it whole.
        if info.duration <= 0.0 {
            outcome.status = "读不出视频时长，无法核对压缩结果，已跳过".into();
            return outcome;
        }
        if container == "mp4" && info.others > 0 {
            outcome.status = "视频带有字幕、数据或多条视频流，转成 MP4 会丢失，已跳过".into();
            return outcome;
        }
        let out = sibling(&path, ".stacker-compressing", container);
        let made = encode(
            &[],
            &path,
            &video_args(options, &info, container),
            &out,
            info.duration,
            on_percent,
        )
        .and_then(|_| {
            // Checked by playing it back: a new file much shorter than the old is broken.
            let again = probe(&out)?;
            if (again.duration - info.duration).abs() > (info.duration * 0.02).max(1.5) {
                return Err("压缩后的视频时长不对，已放弃".into());
            }
            if info.audio.is_some() && again.audio.is_none() {
                return Err("压缩后的视频没有声音，已放弃".into());
            }
            Ok(out)
        });
        (container, made)
    } else {
        let Some((_, ext)) = image_args(&path, options) else {
            outcome.status = "keepPng".into();
            return outcome;
        };
        let out = sibling(&path, ".stacker-compressing", ext);
        (ext, encode_image(&path, options, &out).map(|_| out))
    };
    let out = match made {
        Ok(out) => out,
        Err(error) => {
            // Encoded but failed its check (or stopped): the full-size working file goes.
            let _ = std::fs::remove_file(sibling(&path, ".stacker-compressing", ext));
            outcome.status = error;
            return outcome;
        }
    };
    let after = std::fs::metadata(&out).map(|m| m.len()).unwrap_or(u64::MAX);
    if after as f64 > target.bytes as f64 * WORTH {
        let _ = std::fs::remove_file(&out);
        outcome.status = "notWorth".into();
        outcome.after = after;
        return outcome;
    }
    keep_times(&meta, &out);
    let destination = final_path(&path, ext);
    if let Err(error) = crate::python_env::recycle(&path) {
        let _ = std::fs::remove_file(&out);
        outcome.status = error;
        return outcome;
    }
    if let Err(error) = std::fs::rename(&out, &destination) {
        // The original is in the Recycle Bin; the new file stays under its working name.
        outcome.status = format!("{error}");
        outcome.new_path = out.to_string_lossy().into_owned();
        return outcome;
    }
    outcome.after = after;
    outcome.new_path = destination.to_string_lossy().into_owned();
    outcome
}

#[tauri::command]
pub async fn space_compress_estimate(
    window: tauri::Window,
    targets: Vec<Target>,
    options: Options,
) -> Result<Vec<Outcome>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        CANCEL.store(false, Ordering::Relaxed);
        let roots = system_roots();
        let total = targets.len();
        let mut out = Vec::new();
        for (i, target) in targets.iter().enumerate() {
            if CANCEL.load(Ordering::Relaxed) {
                break;
            }
            report(&window, i, total, &target.path, 0.0);
            out.push(estimate_one(target, &options, &roots));
        }
        report(&window, total, total, "", 100.0);
        Ok(out)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn space_compress_run(
    window: tauri::Window,
    targets: Vec<Target>,
    options: Options,
) -> Result<Vec<Outcome>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        CANCEL.store(false, Ordering::Relaxed);
        let roots = system_roots();
        let total = targets.len();
        let mut out = Vec::new();
        for (i, target) in targets.iter().enumerate() {
            if CANCEL.load(Ordering::Relaxed) {
                out.push(Outcome {
                    path: target.path.clone(),
                    new_path: target.path.clone(),
                    method: method_of(kind_of(Path::new(&target.path))).into(),
                    before: target.bytes,
                    after: target.bytes,
                    status: "cancelled".into(),
                });
                continue;
            }
            report(&window, i, total, &target.path, 0.0);
            let path = target.path.clone();
            let outcome = run_one(target, &options, &roots, &mut |percent| {
                report(&window, i, total, &path, percent)
            });
            log::info!(
                target: "stacker::compress",
                "compressed method={} status={} before={} after={}",
                outcome.method,
                outcome.status,
                outcome.before,
                outcome.after
            );
            out.push(outcome);
        }
        report(&window, total, total, "", 100.0);
        Ok(out)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn space_compress_cancel() {
    CANCEL.store(true, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_are_sorted_by_what_can_be_done_with_them() {
        assert_eq!(kind_of(Path::new(r"D:\v\Trip.MP4")), Kind::Video);
        assert_eq!(kind_of(Path::new(r"D:\p\a.jpeg")), Kind::Image);
        assert_eq!(kind_of(Path::new(r"D:\db\dump.sql")), Kind::Other);
    }

    #[test]
    fn a_video_is_only_made_smaller_and_never_larger() {
        assert_eq!(
            scale_filter(3840, 2160, 1080).as_deref(),
            Some("scale=-2:1080")
        );
        // Portrait: the shorter side is the width.
        assert_eq!(
            scale_filter(1080, 1920, 720).as_deref(),
            Some("scale=720:-2")
        );
        assert_eq!(scale_filter(1280, 720, 1080), None);
        assert_eq!(scale_filter(3840, 2160, 0), None);
    }

    #[test]
    fn video_arguments_follow_the_quality_encoder_and_container() {
        let options = Options {
            video_quality: "high".into(),
            video_height: 1080,
            ..Options::default()
        };
        let probe = Probe {
            duration: 60.0,
            width: 3840,
            height: 2160,
            audio: Some("wmav2".into()),
            others: 0,
        };
        let args = video_args(&options, &probe, "mp4").join(" ");
        assert!(
            args.contains("-c:v libx265") && args.contains("-crf 22"),
            "{args}"
        );
        assert!(args.contains("-vf scale=-2:1080") && args.contains("-tag:v hvc1"));
        assert!(
            args.contains("-c:a aac"),
            "WMA audio does not go in MP4: {args}"
        );
        let mkv = video_args(
            &Options {
                video_encoder: "nvenc".into(),
                ..Options::default()
            },
            &Probe {
                audio: Some("dts".into()),
                others: 0,
                ..probe
            },
            "mkv",
        )
        .join(" ");
        assert!(mkv.contains("-map 0 ") && mkv.contains("hevc_nvenc") && mkv.contains("-cq 28"));
        assert!(mkv.contains("-c:a copy") && mkv.contains("-c:s copy"));
    }

    #[test]
    fn a_png_becomes_lossless_webp_or_stays_and_a_photo_takes_the_format_asked_for() {
        assert_eq!(image_target(Path::new("a.png"), "jpeg"), None);
        assert_eq!(
            image_target(Path::new("a.png"), "avif"),
            Some(("webp", "webp"))
        );
        assert_eq!(
            image_target(Path::new("a.jpg"), "avif"),
            Some(("avif", "avif"))
        );
        let (args, ext) = image_args(
            Path::new("a.png"),
            &Options {
                image_format: "webp".into(),
                ..Options::default()
            },
        )
        .unwrap();
        assert_eq!(ext, "webp");
        assert!(args.join(" ").contains("-lossless 1"));
        let (args, ext) = image_args(Path::new("a.jpg"), &Options::default()).unwrap();
        assert_eq!((args.join(" ").as_str(), ext), ("-c:v mjpeg -q:v 5", "jpg"));
    }

    #[test]
    fn the_probe_reads_length_size_and_sound() {
        let json = r#"{"streams":[{"codec_type":"video","codec_name":"h264","width":1920,"height":1080},
            {"codec_type":"audio","codec_name":"aac"}],"format":{"duration":"12.5"}}"#;
        assert_eq!(
            parse_probe(json),
            Some(Probe {
                duration: 12.5,
                width: 1920,
                height: 1080,
                audio: Some("aac".into()),
                others: 0,
            })
        );
        assert_eq!(parse_probe(r#"{"streams":[],"format":{}}"#), None);
        // A subtitle and a data track are what an MP4 re-encode would drop.
        let json = r#"{"streams":[{"codec_type":"video","width":1,"height":1},{"codec_type":"audio"},
            {"codec_type":"subtitle"},{"codec_type":"data"}],"format":{"duration":"3"}}"#;
        assert_eq!(parse_probe(json).unwrap().others, 2);
    }

    #[test]
    fn a_new_name_keeps_the_old_one_or_takes_a_free_one() {
        let dir = tempfile::tempdir().unwrap();
        let photo = dir.path().join("p.jpeg");
        std::fs::write(&photo, b"x").unwrap();
        assert_eq!(final_path(&photo, "jpg"), photo);
        assert_eq!(final_path(&photo, "webp"), dir.path().join("p.webp"));
        std::fs::write(dir.path().join("p.webp"), b"x").unwrap();
        assert_eq!(final_path(&photo, "webp"), dir.path().join("p (1).webp"));
    }

    #[test]
    fn text_is_estimated_to_shrink_and_packed_files_are_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("big.log");
        let line = "2026-10-08 12:00:00 INFO request served in 12 ms\n";
        std::fs::write(&log, line.repeat(40_000)).unwrap();
        let size = std::fs::metadata(&log).unwrap().len();
        let estimate = estimate_packed(&log, size).unwrap();
        assert!(estimate < size / 4, "{estimate} of {size}");
        let zip = dir.path().join("a.zip");
        std::fs::write(&zip, vec![7u8; 4096]).unwrap();
        let outcome = estimate_one(
            &Target {
                path: zip.to_string_lossy().into(),
                bytes: 4096,
            },
            &Options::default(),
            &[],
        );
        assert_eq!(outcome.status, "already");
    }

    /// Live: downloads ffmpeg into Stacker's tools folder, then compresses a made-up video,
    /// photo and text file; their originals go to the Recycle Bin as in real use.
    /// `cargo test --lib live_compress -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn live_compress() {
        if tool("ffmpeg").is_none() {
            let tools = install_ffmpeg(|line| println!("{line}")).unwrap();
            println!("{tools:?}");
        }
        let tools = detect_tools();
        println!("{tools:?}");
        let ffmpeg = tool("ffmpeg").unwrap();
        let dir = tempfile::tempdir().unwrap();
        let video = dir.path().join("clip.mov");
        let photo = dir.path().join("photo.jpg");
        let args = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        run(
            &ffmpeg,
            &args(&[
                "-hide_banner",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=s=3840x2160:d=6:r=30",
                "-f",
                "lavfi",
                "-i",
                "sine=d=6",
                "-c:v",
                "libx264",
                "-crf",
                "14",
                "-c:a",
                "pcm_s16le",
                &video.to_string_lossy(),
            ]),
            Duration::from_secs(300),
        )
        .unwrap();
        run(
            &ffmpeg,
            &args(&[
                "-hide_banner",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=s=4000x3000:d=1",
                "-frames:v",
                "1",
                "-q:v",
                "1",
                &photo.to_string_lossy(),
            ]),
            Duration::from_secs(60),
        )
        .unwrap();
        let log = dir.path().join("server.log");
        std::fs::write(
            &log,
            "2026-10-08 12:00:00 INFO request served in 12 ms\n".repeat(200_000),
        )
        .unwrap();
        let targets: Vec<Target> = [&video, &photo, &log]
            .iter()
            .map(|p| Target {
                path: p.to_string_lossy().into(),
                bytes: std::fs::metadata(p).unwrap().len(),
            })
            .collect();
        let options = Options {
            video_height: 1080,
            image_format: "webp".into(),
            ..Options::default()
        };
        for target in &targets {
            println!("estimate {:?}", estimate_one(target, &options, &[]));
        }
        for target in &targets {
            let outcome = run_one(target, &options, &[], &mut |p| print!("{p:.0}% "));
            println!("\nrun {outcome:?}");
            assert_eq!(outcome.status, "ok", "{outcome:?}");
            assert!(outcome.after < outcome.before);
        }
        let again = probe(Path::new(&run_one_path(dir.path(), "clip.mp4"))).unwrap();
        assert_eq!((again.width, again.height), (1920, 1080));
    }

    fn run_one_path(dir: &Path, name: &str) -> String {
        dir.join(name).to_string_lossy().into_owned()
    }
}
