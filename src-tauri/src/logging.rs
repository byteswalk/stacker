use chrono::{Local, NaiveDate};
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

pub const MAX_LOG_FILE_BYTES: u64 = 20 * 1024 * 1024;

pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        log::error!(
            target: "stacker::panic",
            "Unhandled panic: {panic_info}\nBacktrace:\n{}",
            std::backtrace::Backtrace::force_capture()
        );
        previous(panic_info);
    }));
}

pub fn target(log_dir: PathBuf) -> io::Result<tauri_plugin_log::Target> {
    let writer = DailyLogWriter::new(log_dir, MAX_LOG_FILE_BYTES)?;
    let dispatch = fern::Dispatch::new().chain(fern::Output::writer(Box::new(writer), "\n"));
    Ok(tauri_plugin_log::Target::new(
        tauri_plugin_log::TargetKind::Dispatch(dispatch),
    ))
}

pub fn current_log_path(log_dir: &Path) -> PathBuf {
    let today = Local::now().date_naive();
    latest_volume_for_date(log_dir, today)
        .map(|(_, path, _)| path)
        .unwrap_or_else(|| volume_path(log_dir, today, 0))
}

struct DailyLogWriter {
    log_dir: PathBuf,
    max_file_bytes: u64,
    date: NaiveDate,
    volume: u32,
    bytes_written: u64,
    file: File,
}

impl DailyLogWriter {
    fn new(log_dir: PathBuf, max_file_bytes: u64) -> io::Result<Self> {
        std::fs::create_dir_all(&log_dir)?;
        let date = chrono::Local::now().date_naive();
        let (volume, path, bytes_written) =
            writable_volume_for_date(&log_dir, date, max_file_bytes);
        let file = open_append(&path)?;
        Ok(Self {
            log_dir,
            max_file_bytes,
            date,
            volume,
            bytes_written,
            file,
        })
    }

    fn prepare_for_write(&mut self, incoming_bytes: u64) -> io::Result<()> {
        let today = Local::now().date_naive();
        if today != self.date {
            let (volume, path, bytes_written) =
                writable_volume_for_date(&self.log_dir, today, self.max_file_bytes);
            self.file.flush()?;
            self.file = open_append(&path)?;
            self.date = today;
            self.volume = volume;
            self.bytes_written = bytes_written;
        }

        if self.bytes_written > 0
            && self.bytes_written.saturating_add(incoming_bytes) > self.max_file_bytes
        {
            self.file.flush()?;
            self.volume = self.volume.saturating_add(1);
            let path = volume_path(&self.log_dir, self.date, self.volume);
            self.file = open_append(&path)?;
            self.bytes_written = self.file.metadata()?.len();
        }
        Ok(())
    }
}

impl Write for DailyLogWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.prepare_for_write(buffer.len() as u64)?;
        let written = self.file.write(buffer)?;
        self.bytes_written = self.bytes_written.saturating_add(written as u64);
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

fn open_append(path: &Path) -> io::Result<File> {
    OpenOptions::new().create(true).append(true).open(path)
}

fn writable_volume_for_date(
    log_dir: &Path,
    date: NaiveDate,
    max_file_bytes: u64,
) -> (u32, PathBuf, u64) {
    let (mut volume, mut path, mut bytes) = latest_volume_for_date(log_dir, date)
        .unwrap_or_else(|| (0, volume_path(log_dir, date, 0), 0));
    if bytes >= max_file_bytes {
        volume = volume.saturating_add(1);
        path = volume_path(log_dir, date, volume);
        bytes = path.metadata().map(|metadata| metadata.len()).unwrap_or(0);
    }
    (volume, path, bytes)
}

fn latest_volume_for_date(log_dir: &Path, date: NaiveDate) -> Option<(u32, PathBuf, u64)> {
    let prefix = format!("stacker-{date}");
    std::fs::read_dir(log_dir)
        .ok()?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            let file_name = path.file_name()?.to_str()?;
            let volume = parse_volume(file_name, &prefix)?;
            let bytes = entry.metadata().ok()?.len();
            Some((volume, path, bytes))
        })
        .max_by_key(|(volume, _, _)| *volume)
}

fn parse_volume(file_name: &str, prefix: &str) -> Option<u32> {
    if file_name == format!("{prefix}.log") {
        return Some(0);
    }
    file_name
        .strip_prefix(prefix)?
        .strip_prefix('.')?
        .strip_suffix(".log")?
        .parse()
        .ok()
}

fn volume_path(log_dir: &Path, date: NaiveDate, volume: u32) -> PathBuf {
    let suffix = if volume == 0 {
        String::new()
    } else {
        format!(".{volume}")
    };
    log_dir.join(format!("stacker-{date}{suffix}.log"))
}

#[cfg(test)]
mod tests {
    use super::{latest_volume_for_date, parse_volume, volume_path, DailyLogWriter};
    use chrono::NaiveDate;
    use std::io::Write;

    #[test]
    fn volume_names_are_stable_and_sortable() {
        let date = NaiveDate::from_ymd_opt(2026, 7, 24).unwrap();
        let root = std::path::Path::new("logs");

        assert_eq!(
            volume_path(root, date, 0),
            root.join("stacker-2026-07-24.log")
        );
        assert_eq!(
            volume_path(root, date, 3),
            root.join("stacker-2026-07-24.3.log")
        );
        assert_eq!(
            parse_volume("stacker-2026-07-24.log", "stacker-2026-07-24"),
            Some(0)
        );
        assert_eq!(
            parse_volume("stacker-2026-07-24.12.log", "stacker-2026-07-24"),
            Some(12)
        );
    }

    #[test]
    fn writer_rolls_to_the_next_volume_at_the_limit() {
        let dir = tempfile::tempdir().unwrap();
        let mut writer = DailyLogWriter::new(dir.path().to_path_buf(), 10).unwrap();

        writer.write_all(b"12345678").unwrap();
        writer.write_all(b"abcd").unwrap();
        writer.flush().unwrap();

        let date = chrono::Local::now().date_naive();
        assert_eq!(
            std::fs::read(volume_path(dir.path(), date, 0)).unwrap(),
            b"12345678"
        );
        assert_eq!(
            std::fs::read(volume_path(dir.path(), date, 1)).unwrap(),
            b"abcd"
        );
        assert_eq!(
            latest_volume_for_date(dir.path(), date).map(|(volume, _, _)| volume),
            Some(1)
        );
    }
}
