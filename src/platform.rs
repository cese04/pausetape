//! Things that differ per operating system and aren't about music: where the app keeps its
//! data, opening a folder in the system file manager, the Trash, and ejecting a card.

use std::path::{Path, PathBuf};

/// The app's data folder: settings (`app.ron`, written by eframe), the library database and
/// undo journals. macOS `~/Library/Application Support/<APP_ID>`, Windows
/// `%APPDATA%\<APP_ID>\data`, Linux `~/.local/share/<app_id>`.
pub fn data_dir() -> PathBuf {
    eframe::storage_dir(crate::APP_ID).unwrap_or_else(std::env::temp_dir)
}

/// Open a folder in Finder, Explorer, or the desktop's file manager (a file in its usual app).
pub fn reveal(path: &Path) {
    let cmd = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(windows) {
        "explorer"
    } else {
        "xdg-open"
    };
    let _ = std::process::Command::new(cmd).arg(path).spawn();
}

/// Windows release builds have no console of their own (so none opens behind the app). Started
/// from a terminal, they print `--version`, `--plan` and errors there instead; started from
/// Explorer, there's nothing to attach to and output goes nowhere, as before.
pub fn attach_console() {
    #[cfg(all(windows, not(debug_assertions)))]
    {
        use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
        // SAFETY: takes a process id and touches no memory of ours; failure is harmless.
        unsafe { AttachConsole(ATTACH_PARENT_PROCESS) };
    }
}

/// Whether the system asks apps for less motion: macOS's *Reduce motion*, Windows' *Animation
/// effects* turned off, GNOME's animations turned off.
pub fn reduce_motion() -> bool {
    #[cfg(target_os = "macos")]
    {
        let out = std::process::Command::new("defaults").args(["read", "com.apple.universalaccess", "reduceMotion"]).output();
        out.is_ok_and(|o| String::from_utf8_lossy(&o.stdout).trim() == "1")
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{SPI_GETCLIENTAREAANIMATION, SystemParametersInfoW};
        let mut on: i32 = 1;
        // SAFETY: writes one BOOL into `on`, which outlives the call.
        let ok = unsafe { SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION, 0, (&raw mut on).cast(), 0) };
        ok != 0 && on == 0
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        let out = std::process::Command::new("gsettings").args(["get", "org.gnome.desktop.interface", "enable-animations"]).output();
        out.is_ok_and(|o| String::from_utf8_lossy(&o.stdout).trim() == "false")
    }
}

/// Clear a file's lock, for good: macOS's "locked" flag (a FAT/exFAT file's read-only attribute
/// shows as it), Windows' read-only attribute. False if it wasn't locked or couldn't be unlocked.
#[cfg(target_os = "macos")]
pub fn unlock(path: &Path) -> bool {
    use std::os::macos::fs::MetadataExt;
    use std::os::unix::ffi::OsStrExt;
    let Ok(flags) = std::fs::symlink_metadata(path).map(|m| m.st_flags()) else { return false };
    let Ok(c) = std::ffi::CString::new(path.as_os_str().as_bytes()) else { return false };
    if flags & libc::UF_IMMUTABLE == 0 {
        return false;
    }
    // SAFETY: `c` is a valid NUL-terminated path; the call only reads it.
    unsafe { libc::chflags(c.as_ptr(), (flags & !libc::UF_IMMUTABLE) as _) == 0 }
}

#[cfg(windows)]
pub fn unlock(path: &Path) -> bool {
    let Ok(meta) = std::fs::symlink_metadata(path) else { return false };
    let mut perms = meta.permissions();
    if !perms.readonly() {
        return false;
    }
    // On Windows this clears the read-only attribute and nothing else.
    #[allow(clippy::permissions_set_readonly_false)]
    perms.set_readonly(false);
    std::fs::set_permissions(path, perms).is_ok()
}

#[cfg(not(any(target_os = "macos", windows)))]
pub fn unlock(_: &Path) -> bool {
    false
}

/// How to install ffmpeg here, for messages that ask for it.
pub const FFMPEG_INSTALL: &str = if cfg!(target_os = "macos") {
    "brew install ffmpeg"
} else if cfg!(windows) {
    "winget install Gyan.FFmpeg"
} else {
    "sudo apt install ffmpeg, or your system's package manager"
};

/// A command for a console program (ffmpeg) that, on Windows, runs without opening a console
/// window of its own: the app has none to lend it, so each run would flash one up.
pub fn quiet_command(program: impl AsRef<std::ffi::OsStr>) -> std::process::Command {
    #[allow(unused_mut)]
    let mut cmd = std::process::Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// Move a file to the Trash (the Recycle Bin on Windows), where it can be restored from.
pub fn trash(path: &Path) -> Result<(), String> {
    // Windows keeps no Recycle Bin on removable drives (USB sticks, SD cards) or network ones:
    // "moving" a file there deletes it for good, so it's left where it is.
    #[cfg(windows)]
    if without_recycle_bin(path) {
        return Err(format!("{}: this drive has no Recycle Bin, so it would be deleted for good; left where it is", path.display()));
    }
    #[allow(unused_mut)]
    let mut ctx = trash::TrashContext::default();
    // macOS: ask the file manager service directly; the Finder way would make the app ask for
    // permission to control Finder, and play its sound for every file.
    #[cfg(target_os = "macos")]
    trash::macos::TrashContextExtMacos::set_delete_method(&mut ctx, trash::macos::DeleteMethod::NsFileManager);
    ctx.delete(path).map_err(|e| e.to_string())
}

#[cfg(windows)]
const DRIVE_REMOVABLE: u32 = 2;

/// The kind of drive `path` is on, as `GetDriveTypeW` names them.
#[cfg(windows)]
fn drive_type(path: &Path) -> u32 {
    use std::os::windows::ffi::OsStrExt;
    let root: Vec<u16> = path.ancestors().last().unwrap_or(path).as_os_str().encode_wide().chain([0]).collect();
    // SAFETY: `root` is a NUL-terminated drive root such as "E:\".
    unsafe { windows_sys::Win32::Storage::FileSystem::GetDriveTypeW(root.as_ptr()) }
}

#[cfg(windows)]
fn without_recycle_bin(path: &Path) -> bool {
    const DRIVE_REMOTE: u32 = 4;
    matches!(drive_type(path), DRIVE_REMOVABLE | DRIVE_REMOTE)
}

/// Whether the drive mounted at `volume` is one to offer ejecting. On Windows, sticks and cards
/// only: internal drives count as fixed (and so, as a rule, do USB hard disks). Elsewhere what's
/// listed as a volume is already external.
pub fn ejectable(volume: &Path) -> bool {
    #[cfg(windows)]
    {
        drive_type(volume) == DRIVE_REMOVABLE
    }
    #[cfg(not(windows))]
    {
        let _ = volume;
        true
    }
}

/// Eject the card or drive mounted at `volume`, so it can be taken out: macOS's `diskutil
/// eject`; on Windows, what *Eject* in Explorer does (lock, dismount, then eject the medium);
/// elsewhere `gio mount --eject`. The error says why not, e.g. a file on it still open.
pub fn eject(volume: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        windows_eject::eject(volume)
    }
    #[cfg(not(windows))]
    {
        let mut cmd = if cfg!(target_os = "macos") { std::process::Command::new("diskutil") } else { std::process::Command::new("gio") };
        if cfg!(target_os = "macos") {
            cmd.arg("eject");
        } else {
            cmd.args(["mount", "--eject"]);
        }
        let out = cmd.arg(volume).output().map_err(|e| e.to_string())?;
        if out.status.success() {
            return Ok(());
        }
        let said = String::from_utf8_lossy(if out.stderr.is_empty() { &out.stdout } else { &out.stderr }).trim().to_string();
        Err(why_not_ejected(&said))
    }
}

/// diskutil's answer, in a line: "Unmount of disk10 failed: at least one volume could not be
/// unmounted\nUnmount was dissented by PID 20475 (/bin/sleep)…" says who holds it.
#[cfg(not(windows))]
fn why_not_ejected(said: &str) -> String {
    let holder = said.split_once("dissented by PID").and_then(|(_, rest)| rest.split_once('(')).and_then(|(_, rest)| rest.split_once(')')).map(|(path, _)| path);
    if let Some(name) = holder.and_then(|p| Path::new(p).file_name()) {
        return format!("{} is using it; close what it has open there and eject again", name.to_string_lossy());
    }
    said.lines().next().filter(|l| !l.is_empty()).unwrap_or("the system said no").to_string()
}

#[cfg(windows)]
mod windows_eject {
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    use windows_sys::Win32::Foundation::{CloseHandle, GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING};
    use windows_sys::Win32::System::IO::DeviceIoControl;
    use windows_sys::Win32::System::Ioctl::{FSCTL_DISMOUNT_VOLUME, FSCTL_LOCK_VOLUME, IOCTL_STORAGE_EJECT_MEDIA, IOCTL_STORAGE_MEDIA_REMOVAL, PREVENT_MEDIA_REMOVAL};

    /// The steps of Microsoft's "how to eject removable media" (KB165721).
    pub fn eject(volume: &Path) -> Result<(), String> {
        let letter = volume.to_string_lossy().chars().next().filter(char::is_ascii_alphabetic).ok_or("not a drive letter")?;
        let device: Vec<u16> = std::ffi::OsStr::new(&format!("\\\\.\\{letter}:")).encode_wide().chain([0]).collect();
        // SAFETY: `device` is NUL-terminated; no security attributes or template.
        let h = unsafe { CreateFileW(device.as_ptr(), GENERIC_READ | GENERIC_WRITE, FILE_SHARE_READ | FILE_SHARE_WRITE, std::ptr::null(), OPEN_EXISTING, 0, std::ptr::null_mut()) };
        if h == INVALID_HANDLE_VALUE {
            return Err(format!("couldn't open drive {letter}: ({})", std::io::Error::last_os_error()));
        }
        let steps = || {
            // A file still open on it (a player, Explorer's preview) keeps the lock away for a
            // moment: try for about two seconds.
            let mut locked = false;
            for _ in 0..20 {
                if control(h, FSCTL_LOCK_VOLUME, None) {
                    locked = true;
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            if !locked {
                return Err("a file on it is still open somewhere; close it and eject again".to_string());
            }
            if !control(h, FSCTL_DISMOUNT_VOLUME, None) {
                return Err(format!("couldn't dismount it ({})", std::io::Error::last_os_error()));
            }
            let allow = PREVENT_MEDIA_REMOVAL { PreventMediaRemoval: false };
            control(h, IOCTL_STORAGE_MEDIA_REMOVAL, Some(&allow));
            // Some card readers can't eject the card itself; dismounted, it's safe to take out.
            control(h, IOCTL_STORAGE_EJECT_MEDIA, None);
            Ok(())
        };
        let done = steps();
        // SAFETY: `h` was opened above and is closed once.
        unsafe { CloseHandle(h) };
        done
    }

    fn control(h: HANDLE, code: u32, input: Option<&PREVENT_MEDIA_REMOVAL>) -> bool {
        let mut returned = 0u32;
        let (ptr, len) = match input {
            Some(i) => ((i as *const PREVENT_MEDIA_REMOVAL).cast(), size_of::<PREVENT_MEDIA_REMOVAL>() as u32),
            None => (std::ptr::null(), 0),
        };
        // SAFETY: `h` is an open volume; the input, if any, is `len` bytes that outlive the call;
        // no output buffer; synchronous (no OVERLAPPED).
        unsafe { DeviceIoControl(h, code, ptr, len, std::ptr::null_mut(), 0, &mut returned, std::ptr::null_mut()) != 0 }
    }
}

#[cfg(test)]
mod tests {
    /// `PAUSETAPE_EJECT=<mount point> cargo test -- --ignored ejects`: ejects that drive (try a
    /// disk image: `hdiutil create -size 20m -fs MS-DOS -volname PTEST t.dmg; hdiutil attach t.dmg`).
    #[test]
    #[ignore]
    fn ejects() {
        let Ok(v) = std::env::var("PAUSETAPE_EJECT") else { return };
        let v = std::path::PathBuf::from(v);
        assert!(v.exists());
        super::eject(&v).unwrap();
        assert!(!v.exists(), "gone once ejected");
    }

    #[test]
    #[cfg(windows)]
    fn the_system_drive_is_not_offered_for_ejecting() {
        let system = std::env::var("SystemDrive").unwrap();
        assert!(!super::ejectable(std::path::Path::new(&format!("{system}\\"))));
    }

    #[test]
    #[cfg(not(windows))]
    fn says_who_keeps_a_card_from_ejecting() {
        let said = "Unmount of disk10 failed: at least one volume could not be unmounted\nUnmount was dissented by PID 20475 (/Applications/Music.app/Contents/MacOS/Music)\nDissenter parent PPID 1 (launchd)";
        assert_eq!(super::why_not_ejected(said), "Music is using it; close what it has open there and eject again");
        assert_eq!(super::why_not_ejected("Error unmounting: target is busy\nmore"), "Error unmounting: target is busy");
        assert_eq!(super::why_not_ejected(""), "the system said no");
    }
}
