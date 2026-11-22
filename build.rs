//! The app icon at the sizes each system wants, from the one drawing `scripts/make-icon.py`
//! makes (`assets/icon/icon-1024.png`): a PNG for the window, and on Windows an icon built into
//! the .exe, which Explorer, the taskbar and the Start menu show. (The Mac app gets its own from
//! `scripts/package-macos.sh`.)

use std::fs::File;
use std::path::PathBuf;

use image::codecs::ico::{IcoEncoder, IcoFrame};
use image::imageops::{self, FilterType};
use image::{ExtendedColorType, RgbaImage};

const MASTER: &str = "assets/icon/icon-1024.png";

fn main() {
    println!("cargo:rerun-if-changed={MASTER}");
    println!("cargo:rerun-if-changed=build.rs");
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("cargo sets OUT_DIR"));
    let master = image::open(MASTER).expect("the app icon").into_rgba8();
    // The drawing leaves room round the paper for the Mac's shadow; small icons need it filled.
    let paper = imageops::crop_imm(&master, 92, 92, 840, 840).to_image();
    let at = |side: u32| -> RgbaImage { imageops::resize(&paper, side, side, FilterType::Lanczos3) };

    at(256).save(out.join("icon-256.png")).expect("writing the window icon");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let frames: Vec<IcoFrame> = [16, 20, 24, 32, 40, 48, 64, 128, 256]
            .into_iter()
            .map(|side| IcoFrame::as_png(at(side).as_raw(), side, side, ExtendedColorType::Rgba8).expect("an icon size"))
            .collect();
        let ico = out.join("icon.ico");
        IcoEncoder::new(File::create(&ico).expect("writing the icon")).encode_images(&frames).expect("writing the icon");
        winresource::WindowsResource::new()
            .set_icon(ico.to_str().expect("a UTF-8 build folder"))
            .set("ProductName", "Pausetape")
            .set("FileDescription", "Pausetape")
            .compile()
            .expect("building the icon into the .exe (needs the Windows SDK's rc.exe, part of the C++ build tools)");
    }
}
