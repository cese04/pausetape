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

/// Move a file to the Trash (the Recycle Bin on Windows), where it can be restored from.
pub fn trash(path: &Path) -> Result<(), String> {
    #[allow(unused_mut)]
    let mut ctx = trash::TrashContext::default();
    // macOS: ask the file manager service directly; the Finder way would make the app ask for
    // permission to control Finder, and play its sound for every file.
    #[cfg(target_os = "macos")]
    trash::macos::TrashContextExtMacos::set_delete_method(&mut ctx, trash::macos::DeleteMethod::NsFileManager);
    ctx.delete(path).map_err(|e| e.to_string())
}
