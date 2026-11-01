//! Mounted drives that could be the player's card, and their free space and filesystem.

use std::path::PathBuf;
#[cfg(not(windows))]
use std::{fs, path::Path};

#[derive(Clone)]
pub struct Volume {
    pub name: String,
    pub path: PathBuf,
    pub info: Option<SpaceInfo>,
}

#[derive(Clone, Copy)]
pub struct SpaceInfo {
    pub total: u64,
    pub free: u64,
    /// "msdos" / "exfat" / "apfs"…, where the OS tells us.
    pub fs: Option<[u8; 16]>,
}

impl SpaceInfo {
    pub fn fs_name(&self) -> Option<String> {
        self.fs.map(|b| String::from_utf8_lossy(&b).trim_end_matches('\0').to_string())
    }

    /// FAT32/exFAT, i.e. plausibly a player's SD card. (Windows says "FAT32", "exFAT".)
    pub fn is_fat(&self) -> Option<bool> {
        self.fs_name().map(|f| matches!(f.to_lowercase().as_str(), "msdos" | "exfat" | "vfat" | "fat32" | "fat"))
    }
}

/// Capacity, free space and filesystem of the volume holding `path`.
#[cfg(unix)]
pub fn space(path: &Path) -> Option<SpaceInfo> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let c = CString::new(path.as_os_str().as_bytes()).ok()?;
    // SAFETY: `statvfs` is a plain C struct, for which all zeroes is a valid value.
    let mut v: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: `c` is a valid NUL-terminated path and `v` a struct of the type the call fills.
    if unsafe { libc::statvfs(c.as_ptr(), &mut v) } != 0 {
        return None;
    }
    let frsize = v.f_frsize as u64;
    #[cfg(target_os = "macos")]
    let fs = {
        // SAFETY: as above, for `statfs`.
        let mut st: libc::statfs = unsafe { std::mem::zeroed() };
        // SAFETY: as above.
        (unsafe { libc::statfs(c.as_ptr(), &mut st) } == 0).then(|| {
            let mut name = [0u8; 16];
            for (d, s) in name.iter_mut().zip(st.f_fstypename.iter()) {
                *d = *s as u8;
            }
            name
        })
    };
    #[cfg(not(target_os = "macos"))]
    let fs = None;
    Some(SpaceInfo { total: v.f_blocks as u64 * frsize, free: v.f_bavail as u64 * frsize, fs })
}

#[cfg(windows)]
pub use windows::{space, volumes};

#[cfg(not(any(unix, windows)))]
pub fn space(_path: &Path) -> Option<SpaceInfo> {
    None
}

/// Mounted removable-looking volumes: /Volumes on macOS, /media and /run/media on Linux.
#[cfg(not(windows))]
pub fn volumes() -> Vec<Volume> {
    let mut roots = vec![PathBuf::from("/Volumes")];
    if let Ok(user) = std::env::var("USER") {
        roots.push(PathBuf::from("/media").join(&user));
        roots.push(PathBuf::from("/run/media").join(&user));
    }
    let mut out = Vec::new();
    for root in roots {
        let Ok(entries) = fs::read_dir(&root) else { continue };
        for e in entries.flatten() {
            let path = e.path();
            // macOS lists the boot disk here as a symlink to "/".
            if fs::read_link(&path).is_ok() || !path.is_dir() {
                continue;
            }
            out.push(Volume { name: e.file_name().to_string_lossy().into_owned(), info: space(&path), path });
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Windows: drive letters instead of mount points.
#[cfg(windows)]
mod windows {
    use std::ffi::OsString;
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use std::path::{Path, PathBuf};

    use windows_sys::Win32::Storage::FileSystem::{GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDriveStringsW, GetVolumeInformationW};

    use super::{SpaceInfo, Volume};

    const DRIVE_REMOVABLE: u32 = 2;
    const DRIVE_FIXED: u32 = 3;

    fn wide(p: &Path) -> Vec<u16> {
        p.as_os_str().encode_wide().chain([0]).collect()
    }

    fn text(buf: &[u16]) -> String {
        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        OsString::from_wide(&buf[..end]).to_string_lossy().into_owned()
    }

    /// The drive's label ("" when it has none) and filesystem ("FAT32", "exFAT", "NTFS").
    fn label_and_fs(root: &Path) -> Option<(String, String)> {
        let (mut label, mut fs) = ([0u16; 261], [0u16; 261]);
        // SAFETY: `root` is NUL-terminated; the buffers are as long as said.
        let ok = unsafe {
            GetVolumeInformationW(wide(root).as_ptr(), label.as_mut_ptr(), label.len() as u32, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), fs.as_mut_ptr(), fs.len() as u32)
        };
        (ok != 0).then(|| (text(&label), text(&fs)))
    }

    /// Capacity, free space and filesystem of the drive holding `path`.
    pub fn space(path: &Path) -> Option<SpaceInfo> {
        let (mut free, mut total) = (0u64, 0u64);
        // SAFETY: the path is NUL-terminated; the outputs are plain integers.
        if unsafe { GetDiskFreeSpaceExW(wide(path).as_ptr(), &mut free, &mut total, std::ptr::null_mut()) } == 0 {
            return None;
        }
        let root = path.ancestors().last().unwrap_or(path);
        let fs = label_and_fs(root).map(|(_, fs)| {
            let mut name = [0u8; 16];
            for (d, s) in name.iter_mut().zip(fs.bytes()) {
                *d = s;
            }
            name
        });
        Some(SpaceInfo { total, free, fs })
    }

    /// Drives that could be a card: removable ones, and fixed ones other than Windows' own
    /// (card readers and USB drives can show up as either); not network or CD drives.
    pub fn volumes() -> Vec<Volume> {
        let mut buf = [0u16; 512];
        // SAFETY: the buffer is as long as said; the call fills it with NUL-separated roots.
        let n = unsafe { GetLogicalDriveStringsW(buf.len() as u32, buf.as_mut_ptr()) } as usize;
        let system = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into()).to_uppercase();
        let mut out = Vec::new();
        for root in buf[..n.min(buf.len())].split(|&c| c == 0).filter(|r| !r.is_empty()) {
            let path = PathBuf::from(OsString::from_wide(root));
            let letter = path.to_string_lossy().trim_end_matches('\\').to_uppercase();
            // SAFETY: a NUL-terminated root such as "E:\".
            let kind = unsafe { GetDriveTypeW(wide(&path).as_ptr()) };
            if letter == system || !matches!(kind, DRIVE_REMOVABLE | DRIVE_FIXED) {
                continue;
            }
            let Some((label, _)) = label_and_fs(&path) else { continue };
            let name = if label.is_empty() { format!("Drive ({letter})") } else { format!("{label} ({letter})") };
            out.push(Volume { name, info: space(&path), path });
        }
        out
    }
}

