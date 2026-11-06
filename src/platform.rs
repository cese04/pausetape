//! Things that differ per operating system and aren't about music: where the app keeps its
//! data, opening a folder in the system file manager, and the Trash.

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
fn without_recycle_bin(path: &Path) -> bool {
    use std::os::windows::ffi::OsStrExt;
    const DRIVE_REMOVABLE: u32 = 2;
    const DRIVE_REMOTE: u32 = 4;
    let root: Vec<u16> = path.ancestors().last().unwrap_or(path).as_os_str().encode_wide().chain([0]).collect();
    // SAFETY: `root` is a NUL-terminated drive root such as "E:\".
    let kind = unsafe { windows_sys::Win32::Storage::FileSystem::GetDriveTypeW(root.as_ptr()) };
    matches!(kind, DRIVE_REMOVABLE | DRIVE_REMOTE)
}
