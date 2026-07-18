use super::model::{ScanMode, ScanRequest, VolumeInfo};
use std::error::Error;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ValidatedTarget {
    Directory(PathBuf),
    Drive(PathBuf),
}

impl ValidatedTarget {
    pub fn path(&self) -> &Path {
        match self {
            Self::Directory(path) | Self::Drive(path) => path,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TargetError {
    UnexpectedTargets,
    MissingTargets,
    NotAbsolute(String),
    NotFound(String),
    NotDirectory(String),
    LinkedTarget(String),
    CannotCanonicalize(String),
    NotFixedVolume(String),
}

impl fmt::Display for TargetError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedTargets => {
                write!(formatter, "quick scans do not accept manual targets")
            }
            Self::MissingTargets => write!(formatter, "select at least one scan target"),
            Self::NotAbsolute(path) => write!(formatter, "scan target is not absolute: {path}"),
            Self::NotFound(path) => write!(formatter, "scan target does not exist: {path}"),
            Self::NotDirectory(path) => write!(formatter, "scan target is not a directory: {path}"),
            Self::LinkedTarget(path) => {
                write!(
                    formatter,
                    "scan target is a symbolic link or reparse point: {path}"
                )
            }
            Self::CannotCanonicalize(path) => {
                write!(formatter, "scan target cannot be canonicalized: {path}")
            }
            Self::NotFixedVolume(path) => {
                write!(formatter, "scan target is not on a fixed volume: {path}")
            }
        }
    }
}

impl Error for TargetError {}

pub fn validate_targets(request: &ScanRequest) -> Result<Vec<ValidatedTarget>, TargetError> {
    match request.mode {
        ScanMode::Quick => {
            if request.targets.is_empty() {
                Ok(Vec::new())
            } else {
                Err(TargetError::UnexpectedTargets)
            }
        }
        ScanMode::Directories => validate_directories(&request.targets),
        ScanMode::Drives => validate_drives(&request.targets),
    }
}

fn validate_directories(targets: &[String]) -> Result<Vec<ValidatedTarget>, TargetError> {
    require_targets(targets)?;
    let fixed_volumes = list_fixed_volumes();

    targets
        .iter()
        .map(|target| {
            let path = Path::new(target);
            if !path.is_absolute() {
                return Err(TargetError::NotAbsolute(target.clone()));
            }

            let metadata = fs::symlink_metadata(path).map_err(|error| {
                if error.kind() == std::io::ErrorKind::NotFound {
                    TargetError::NotFound(target.clone())
                } else {
                    TargetError::CannotCanonicalize(target.clone())
                }
            })?;
            if is_link_or_reparse_point(&metadata) {
                return Err(TargetError::LinkedTarget(target.clone()));
            }
            if !metadata.is_dir() {
                return Err(TargetError::NotDirectory(target.clone()));
            }

            let canonical = fs::canonicalize(path)
                .map_err(|_| TargetError::CannotCanonicalize(target.clone()))?;
            if !is_on_fixed_volume(&canonical, &fixed_volumes) {
                return Err(TargetError::NotFixedVolume(target.clone()));
            }

            Ok(ValidatedTarget::Directory(canonical))
        })
        .collect()
}

fn validate_drives(targets: &[String]) -> Result<Vec<ValidatedTarget>, TargetError> {
    require_targets(targets)?;
    let fixed_volumes = list_fixed_volumes();

    targets
        .iter()
        .map(|target| {
            if !Path::new(target).is_absolute() {
                return Err(TargetError::NotAbsolute(target.clone()));
            }

            fixed_volumes
                .iter()
                .find(|volume| roots_equal(target, &volume.root))
                .map(|volume| ValidatedTarget::Drive(PathBuf::from(&volume.root)))
                .ok_or_else(|| TargetError::NotFixedVolume(target.clone()))
        })
        .collect()
}

fn require_targets(targets: &[String]) -> Result<(), TargetError> {
    if targets.is_empty() {
        Err(TargetError::MissingTargets)
    } else {
        Ok(())
    }
}

fn roots_equal(left: &str, right: &str) -> bool {
    left.replace('/', "\\").eq_ignore_ascii_case(right)
}

fn is_link_or_reparse_point(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }

    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;

        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }

    #[cfg(not(windows))]
    false
}

#[cfg(windows)]
fn is_on_fixed_volume(path: &Path, volumes: &[VolumeInfo]) -> bool {
    use std::path::{Component, Prefix};

    let drive = match path.components().next() {
        Some(Component::Prefix(prefix)) => match prefix.kind() {
            Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => Some(letter),
            _ => None,
        },
        _ => None,
    };

    drive.is_some_and(|letter| {
        let root = format!("{}:\\", char::from(letter));
        volumes
            .iter()
            .any(|volume| roots_equal(&root, &volume.root))
    })
}

#[cfg(not(windows))]
fn is_on_fixed_volume(_path: &Path, _volumes: &[VolumeInfo]) -> bool {
    true
}

#[cfg(windows)]
pub fn list_fixed_volumes() -> Vec<VolumeInfo> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use std::ptr;
    use winapi::shared::ntdef::ULARGE_INTEGER;
    use winapi::um::fileapi::{
        GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW,
    };
    use winapi::um::winbase::DRIVE_FIXED;

    fn wide_string(buffer: &[u16]) -> String {
        let length = buffer
            .iter()
            .position(|character| *character == 0)
            .unwrap_or(buffer.len());
        OsString::from_wide(&buffer[..length])
            .to_string_lossy()
            .into_owned()
    }

    let drive_mask = unsafe { GetLogicalDrives() };
    let mut volumes = Vec::new();

    for index in 0..26u32 {
        if drive_mask & (1 << index) == 0 {
            continue;
        }

        let letter = char::from(b'A' + index as u8);
        let root = format!("{letter}:\\");
        let wide_root: Vec<u16> = root.encode_utf16().chain(std::iter::once(0)).collect();
        if unsafe { GetDriveTypeW(wide_root.as_ptr()) } != DRIVE_FIXED {
            continue;
        }

        let mut label = [0u16; 261];
        let mut file_system = [0u16; 261];
        let volume_ok = unsafe {
            GetVolumeInformationW(
                wide_root.as_ptr(),
                label.as_mut_ptr(),
                label.len() as u32,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                file_system.as_mut_ptr(),
                file_system.len() as u32,
            )
        } != 0;

        let mut total_bytes: ULARGE_INTEGER = unsafe { std::mem::zeroed() };
        let mut free_bytes: ULARGE_INTEGER = unsafe { std::mem::zeroed() };
        let space_ok = unsafe {
            GetDiskFreeSpaceExW(
                wide_root.as_ptr(),
                ptr::null_mut(),
                &mut total_bytes,
                &mut free_bytes,
            )
        } != 0;

        if !volume_ok || !space_ok {
            continue;
        }

        volumes.push(VolumeInfo {
            root,
            label: wide_string(&label),
            file_system: wide_string(&file_system),
            total_bytes: unsafe { *total_bytes.QuadPart() },
            free_bytes: unsafe { *free_bytes.QuadPart() },
            fixed: true,
        });
    }

    volumes
}

#[cfg(not(windows))]
pub fn list_fixed_volumes() -> Vec<VolumeInfo> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::space_analysis::model::{ScanMode, ScanRequest};

    #[test]
    fn directories_require_existing_absolute_paths() {
        let request = ScanRequest {
            mode: ScanMode::Directories,
            targets: vec!["relative".into()],
        };

        assert!(matches!(
            validate_targets(&request),
            Err(TargetError::NotAbsolute(_))
        ));
    }

    #[test]
    fn quick_scan_rejects_manual_targets() {
        let request = ScanRequest {
            mode: ScanMode::Quick,
            targets: vec![r"C:\".into()],
        };

        assert!(matches!(
            validate_targets(&request),
            Err(TargetError::UnexpectedTargets)
        ));
    }

    #[test]
    fn manual_modes_require_a_selection() {
        for mode in [ScanMode::Directories, ScanMode::Drives] {
            let request = ScanRequest {
                mode,
                targets: Vec::new(),
            };
            assert!(matches!(
                validate_targets(&request),
                Err(TargetError::MissingTargets)
            ));
        }
    }

    #[test]
    fn directories_are_canonicalized() {
        let directory = tempfile::tempdir().unwrap();
        let request = ScanRequest {
            mode: ScanMode::Directories,
            targets: vec![directory.path().to_string_lossy().into_owned()],
        };

        let targets = validate_targets(&request).unwrap();
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].path(), directory.path().canonicalize().unwrap());
    }

    #[test]
    fn directories_reject_files() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("file.bin");
        std::fs::write(&file, b"data").unwrap();
        let request = ScanRequest {
            mode: ScanMode::Directories,
            targets: vec![file.to_string_lossy().into_owned()],
        };

        assert!(matches!(
            validate_targets(&request),
            Err(TargetError::NotDirectory(_))
        ));
    }

    #[cfg(windows)]
    #[test]
    fn directories_reject_symbolic_link_targets() {
        use std::os::windows::fs::symlink_dir;

        let directory = tempfile::tempdir().unwrap();
        let target = tempfile::tempdir().unwrap();
        let link = directory.path().join("directory-link");
        if symlink_dir(target.path(), &link).is_err() {
            return;
        }
        let request = ScanRequest {
            mode: ScanMode::Directories,
            targets: vec![link.to_string_lossy().into_owned()],
        };

        assert!(matches!(
            validate_targets(&request),
            Err(TargetError::LinkedTarget(_))
        ));
    }

    #[cfg(windows)]
    #[test]
    fn directories_reject_junction_targets() {
        use std::process::{Command, Stdio};

        let directory = tempfile::tempdir().unwrap();
        let target = tempfile::tempdir().unwrap();
        let junction = directory.path().join("directory-junction");
        let status = Command::new("cmd")
            .args(["/c", "mklink", "/J"])
            .arg(&junction)
            .arg(target.path())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap();
        assert!(status.success(), "failed to create test junction");
        let request = ScanRequest {
            mode: ScanMode::Directories,
            targets: vec![junction.to_string_lossy().into_owned()],
        };

        assert!(matches!(
            validate_targets(&request),
            Err(TargetError::LinkedTarget(_))
        ));
        std::fs::remove_dir(&junction).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn drive_targets_must_be_roots_from_the_fixed_volume_list() {
        let Some(volume) = list_fixed_volumes().into_iter().next() else {
            return;
        };
        let request = ScanRequest {
            mode: ScanMode::Drives,
            targets: vec![format!("{}Windows", volume.root)],
        };

        assert!(matches!(
            validate_targets(&request),
            Err(TargetError::NotFixedVolume(_))
        ));
    }

    #[cfg(windows)]
    #[test]
    fn drive_targets_resolve_to_roots_from_the_current_volume_list() {
        let Some(volume) = list_fixed_volumes().into_iter().next() else {
            return;
        };
        let request = ScanRequest {
            mode: ScanMode::Drives,
            targets: vec![volume.root.to_ascii_lowercase()],
        };

        let targets = validate_targets(&request).unwrap();
        assert_eq!(targets, vec![ValidatedTarget::Drive(volume.root.into())]);
    }

    #[cfg(windows)]
    #[test]
    fn fixed_volume_discovery_only_returns_fixed_roots() {
        use winapi::um::fileapi::GetDriveTypeW;
        use winapi::um::winbase::DRIVE_FIXED;

        let volumes = list_fixed_volumes();
        assert!(
            !volumes.is_empty(),
            "expected at least one discoverable fixed volume"
        );

        for volume in volumes {
            let root: Vec<u16> = volume
                .root
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            assert!(volume.fixed);
            assert_eq!(unsafe { GetDriveTypeW(root.as_ptr()) }, DRIVE_FIXED);
            assert!(volume.total_bytes >= volume.free_bytes);
        }
    }
}
