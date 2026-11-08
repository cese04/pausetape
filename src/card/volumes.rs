//! Mounted drives that could be the player's card, and their free space and filesystem.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};
#[cfg(not(windows))]
use std::fs;

/// Drives being ejected, or ejected and still in a reader that can't push the card out: the
/// mount point, and the medium's change count once ejected. On Windows any read mounts such a
/// drive again, so nothing here reads it until that medium has left.
static EJECTED: Mutex<Vec<(PathBuf, Option<u32>)>> = Mutex::new(Vec::new());

fn ejected_drives() -> MutexGuard<'static, Vec<(PathBuf, Option<u32>)>> {
    EJECTED.lock().unwrap_or_else(PoisonError::into_inner)
}

/// An Eject of `volume` starts: from now on nothing should read it.
pub fn ejecting(volume: &Path) {
    ejected_drives().push((volume.to_path_buf(), None));
}

/// The Eject ended. A drive still holding its medium after a good one stays out of sight.
pub fn eject_ended(volume: &Path, worked: bool) {
    let medium = if worked { crate::platform::medium(volume) } else { None };
    let mut drives = ejected_drives();
    drives.retain(|(v, _)| v != volume);
    if let Some(m) = medium {
        drives.push((volume.to_path_buf(), Some(m)));
    }
}

/// Whether `path` is on a drive that shouldn't be read for now.
pub fn held(path: &Path) -> bool {
    ejected_drives().iter().any(|(v, _)| path.starts_with(v))
}

/// Whether the folder `path` can be reached. Reads the drive, unless it's just been ejected.
pub fn connected(path: &Path) -> bool {
    !held(path) && path.is_dir()
}

/// See which ejected drives have lost their medium (taken out, or another put in), and go back
/// to reading those. Asks the drive, not what's on it.
pub fn let_go_of_ejected() {
    let drives = ejected_drives().clone();
    for (volume, medium) in drives {
        if medium.is_some() && crate::platform::medium(&volume) != medium {
            ejected_drives().retain(|(v, _)| *v != volume);
        }
    }
}

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
pub use windows::{drives, space, volumes};

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
    const DRIVE_REMOTE: u32 = 4;

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

    /// Mounted drives of these kinds, by letter, each with whether it's Windows' own.
    fn mounted(kinds: &[u32]) -> Vec<(Volume, bool)> {
        let mut buf = [0u16; 512];
        // SAFETY: the buffer is as long as said; the call fills it with NUL-separated roots.
        let n = unsafe { GetLogicalDriveStringsW(buf.len() as u32, buf.as_mut_ptr()) } as usize;
        let system = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into()).to_uppercase();
        let mut out = Vec::new();
        for root in buf[..n.min(buf.len())].split(|&c| c == 0).filter(|r| !r.is_empty()) {
            let path = PathBuf::from(OsString::from_wide(root));
            if super::held(&path) {
                continue;
            }
            let letter = path.to_string_lossy().trim_end_matches('\\').to_uppercase();
            // SAFETY: a NUL-terminated root such as "E:\".
            let kind = unsafe { GetDriveTypeW(wide(&path).as_ptr()) };
            if !kinds.contains(&kind) {
                continue;
            }
            // Unreadable: no disc or card in it (an empty reader).
            let Some((label, _)) = label_and_fs(&path) else { continue };
            let name = if label.is_empty() { format!("Drive ({letter})") } else { format!("{label} ({letter})") };
            out.push((Volume { name, info: space(&path), path }, letter == system));
        }
        out
    }

    /// Drives that could be a card: removable ones, and fixed ones other than Windows' own
    /// (card readers and USB drives can show up as either); not network or CD drives.
    pub fn volumes() -> Vec<Volume> {
        mounted(&[DRIVE_REMOVABLE, DRIVE_FIXED]).into_iter().filter(|(_, system)| !system).map(|(v, _)| v).collect()
    }

    /// Every drive a music folder could be on, Windows' own and network ones too, for the
    /// in-app folder browser.
    pub fn drives() -> Vec<Volume> {
        mounted(&[DRIVE_REMOVABLE, DRIVE_FIXED, DRIVE_REMOTE]).into_iter().map(|(v, _)| v).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drive_being_ejected_is_left_alone() {
        // A folder that exists, so only being held can make it unreachable.
        let volume = std::env::temp_dir().join(format!("pausetape-test-ejecting-{}", std::process::id()));
        std::fs::create_dir_all(volume.join("Music")).unwrap();
        assert!(connected(&volume.join("Music")));
        ejecting(&volume);
        assert!(held(&volume.join("Music")) && !connected(&volume.join("Music")));
        // Refused: it's there as before.
        eject_ended(&volume, false);
        assert!(!held(&volume) && connected(&volume.join("Music")));
        let _ = std::fs::remove_dir_all(&volume);
        // Done, with no medium left to wait for (no such drive): let go at once.
        let gone = Path::new("0-no-such-drive");
        ejecting(gone);
        eject_ended(gone, true);
        assert!(!held(gone));
    }

    #[test]
    fn an_ejected_drive_is_let_go_of_when_its_medium_changes() {
        // A count no drive here has, as when the card in it has changed.
        let volume = std::env::temp_dir().join(format!("pausetape-test-ejected-{}", std::process::id()));
        ejected_drives().push((volume.clone(), Some(0xDEAD_BEEF)));
        assert!(held(&volume));
        let_go_of_ejected();
        assert!(!held(&volume));
        // While the Eject runs there's nothing to compare yet.
        ejecting(&volume);
        let_go_of_ejected();
        assert!(held(&volume));
        eject_ended(&volume, false);
    }
}

