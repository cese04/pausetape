//! Cover art for the player: the front cover embedded in the file, else an image beside it
//! ("cover.jpg", "folder.jpg", …). Decoding happens off the UI thread.

use std::path::{Path, PathBuf};

use lofty::picture::PictureType;
use lofty::prelude::*;

/// Longest side of the decoded image; the player shows it at about a third of this.
const MAX_SIDE: u32 = 320;
const NAMES: &[&str] = &["cover", "folder", "front", "album", "albumart", "artwork"];
const EXTS: &[&str] = &["jpg", "jpeg", "png"];

fn embedded(path: &Path) -> Option<Vec<u8>> {
    let file = lofty::read_from_path(path).ok()?;
    let pictures: Vec<_> = file.tags().iter().flat_map(|t| t.pictures()).collect();
    let front = pictures.iter().find(|p| p.pic_type() == PictureType::CoverFront).or(pictures.first())?;
    Some(front.data().to_vec())
}

/// A conventionally named image in the track's folder, else the only/first image there.
fn beside(path: &Path) -> Option<PathBuf> {
    let dir = path.parent()?;
    let mut images: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| EXTS.contains(&e.to_string_lossy().to_lowercase().as_str())))
        .filter(|p| !p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.')))
        .collect();
    images.sort();
    let stem = |p: &PathBuf| p.file_stem().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
    images.iter().find(|p| NAMES.contains(&stem(p).as_str())).or(images.first()).cloned()
}

/// Decoded, downscaled cover for the track at `path`, if it has one.
pub fn load(path: &Path) -> Option<image::RgbaImage> {
    let bytes = embedded(path).or_else(|| std::fs::read(beside(path)?).ok())?;
    let img = image::load_from_memory(&bytes).ok()?;
    let img = if img.width() > MAX_SIDE || img.height() > MAX_SIDE { img.thumbnail(MAX_SIDE, MAX_SIDE) } else { img };
    Some(img.to_rgba8())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn falls_back_to_an_image_beside_the_track() {
        let dir = std::env::temp_dir().join(format!("neatlibrary-test-cover-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let track = dir.join("01 - Song.mp3");
        std::fs::write(&track, b"no tags, no art").unwrap();
        assert!(load(&track).is_none());

        // A large "Folder.JPG"-style name is found case-insensitively and downscaled.
        image::RgbaImage::from_pixel(800, 600, image::Rgba([200, 30, 30, 255])).save(dir.join("Folder.png")).unwrap();
        image::RgbaImage::from_pixel(10, 10, image::Rgba([0, 0, 0, 255])).save(dir.join("back.png")).unwrap();
        let img = load(&track).expect("cover found");
        assert_eq!(img.dimensions(), (MAX_SIDE, 240));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
