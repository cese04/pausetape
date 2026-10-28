//! Mounted drives that could be the player's card, and their free space and filesystem.

use std::fs;
use std::path::{Path, PathBuf};

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

    /// FAT32/exFAT, i.e. plausibly a player's SD card.
    pub fn is_fat(&self) -> Option<bool> {
        self.fs_name().map(|f| matches!(f.as_str(), "msdos" | "exfat" | "vfat" | "fat32"))
    }
}

/// Capacity, free space and filesystem of the volume holding `path`.
#[cfg(unix)]
pub fn space(path: &Path) -> Option<SpaceInfo> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let c = CString::new(path.as_os_str().as_bytes()).ok()?;
    // SAFETY: `c` is a valid NUL-terminated path; the structs are plain data filled by the call.
    unsafe {
        let mut v: libc::statvfs = std::mem::zeroed();
        if libc::statvfs(c.as_ptr(), &mut v) != 0 {
            return None;
        }
        let frsize = v.f_frsize as u64;
        #[cfg(target_os = "macos")]
        let fs = {
            let mut st: libc::statfs = std::mem::zeroed();
            (libc::statfs(c.as_ptr(), &mut st) == 0).then(|| {
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
}

#[cfg(not(unix))]
pub fn space(_path: &Path) -> Option<SpaceInfo> {
    None
}

/// Mounted removable-looking volumes: /Volumes on macOS, /media and /run/media on Linux.
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

