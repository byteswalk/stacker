use std::fs::Metadata;
use std::io;
use std::path::Path;

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub(crate) struct FileIdentity(u64, u64);

#[cfg(windows)]
pub(crate) fn file_identity(path: &Path) -> io::Result<FileIdentity> {
    use std::fs::OpenOptions;
    use std::mem::MaybeUninit;
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    use winapi::um::fileapi::{GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION};
    use winapi::um::winbase::FILE_FLAG_BACKUP_SEMANTICS;
    use winapi::um::winnt::{
        FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };

    let file = OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)?;
    let mut information = MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::uninit();
    let succeeded = unsafe {
        GetFileInformationByHandle(file.as_raw_handle().cast(), information.as_mut_ptr())
    };
    if succeeded == 0 {
        return Err(io::Error::last_os_error());
    }

    let information = unsafe { information.assume_init() };
    let file_index =
        (u64::from(information.nFileIndexHigh) << 32) | u64::from(information.nFileIndexLow);
    Ok(FileIdentity(
        u64::from(information.dwVolumeSerialNumber),
        file_index,
    ))
}

#[cfg(unix)]
pub(crate) fn file_identity(path: &Path) -> io::Result<FileIdentity> {
    use std::os::unix::fs::MetadataExt;

    let metadata = path.metadata()?;
    Ok(FileIdentity(metadata.dev(), metadata.ino()))
}

#[cfg(not(any(windows, unix)))]
pub(crate) fn file_identity(path: &Path) -> io::Result<FileIdentity> {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    path.canonicalize()?.hash(&mut hasher);
    Ok(FileIdentity(0, hasher.finish()))
}

#[cfg(windows)]
pub(crate) fn allocated_size(path: &Path, metadata: &Metadata) -> u64 {
    use std::iter;
    use std::os::windows::ffi::OsStrExt;
    use winapi::shared::minwindef::DWORD;
    use winapi::um::errhandlingapi::SetLastError;
    use winapi::um::fileapi::{GetCompressedFileSizeW, INVALID_FILE_SIZE};

    let wide_path = path
        .as_os_str()
        .encode_wide()
        .chain(iter::once(0))
        .collect::<Vec<_>>();
    let mut high: DWORD = 0;
    unsafe { SetLastError(0) };
    let low = unsafe { GetCompressedFileSizeW(wide_path.as_ptr(), &mut high) };

    if low == INVALID_FILE_SIZE && io::Error::last_os_error().raw_os_error() != Some(0) {
        return metadata.len();
    }

    (u64::from(high) << 32) | u64::from(low)
}

#[cfg(not(windows))]
pub(crate) fn allocated_size(_path: &Path, metadata: &Metadata) -> u64 {
    metadata.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn hard_links_share_one_file_identity() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("first.bin");
        let second = dir.path().join("second.bin");
        std::fs::write(&first, vec![0u8; 4096]).unwrap();
        std::fs::hard_link(&first, &second).unwrap();

        assert_eq!(
            file_identity(&first).unwrap(),
            file_identity(&second).unwrap()
        );
    }

    #[cfg(windows)]
    #[test]
    fn allocated_size_handles_sparse_filesystem_behavior() {
        use std::fs::OpenOptions;
        use std::io::Write;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("allocation.bin");
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)
            .unwrap();
        file.write_all(&[1u8; 4096]).unwrap();
        file.set_len(1024 * 1024 + 1).unwrap();
        drop(file);

        let metadata = path.metadata().unwrap();
        let logical = metadata.len();
        let allocated = allocated_size(&path, &metadata);
        if allocated < logical {
            assert!(allocated < logical);
        } else {
            let cluster = cluster_size(dir.path()).unwrap();
            let rounded = logical
                .saturating_add(cluster - 1)
                .checked_div(cluster)
                .unwrap()
                .saturating_mul(cluster);
            assert!(allocated <= rounded.saturating_add(cluster));
        }
    }

    #[cfg(windows)]
    #[test]
    fn allocated_size_falls_back_to_logical_size_after_file_disappears() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("vanished.bin");
        std::fs::write(&path, vec![0u8; 123]).unwrap();
        let metadata = path.metadata().unwrap();
        std::fs::remove_file(&path).unwrap();

        assert_eq!(allocated_size(&path, &metadata), metadata.len());
    }

    #[cfg(windows)]
    fn cluster_size(path: &Path) -> io::Result<u64> {
        use std::iter;
        use std::os::windows::ffi::OsStrExt;
        use winapi::shared::minwindef::DWORD;
        use winapi::um::fileapi::GetDiskFreeSpaceW;

        let root = path.ancestors().last().unwrap_or(path);
        let wide_root = root
            .as_os_str()
            .encode_wide()
            .chain(iter::once(0))
            .collect::<Vec<_>>();
        let mut sectors_per_cluster: DWORD = 0;
        let mut bytes_per_sector: DWORD = 0;
        let succeeded = unsafe {
            GetDiskFreeSpaceW(
                wide_root.as_ptr(),
                &mut sectors_per_cluster,
                &mut bytes_per_sector,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        if succeeded == 0 {
            return Err(io::Error::last_os_error());
        }

        Ok(u64::from(sectors_per_cluster) * u64::from(bytes_per_sector))
    }
}
