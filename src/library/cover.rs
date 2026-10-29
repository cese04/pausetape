//! Cover art for the player: the front cover embedded in the file, else an image beside it
//! ("cover.jpg", "folder.jpg", …). Decoding happens off the UI thread.

use std::path::{Path, PathBuf};

use lofty::picture::PictureType;
use lofty::prelude::*;

/// Longest side of the decoded image; the player shows it at about a third of this.
const MAX_SIDE: u32 = 320;
/// Longest side of a cover embedded into songs. Small players decode embedded art themselves;
/// a modest baseline JPEG displays everywhere and adds ~100 KB per song.
const EMBED_SIDE: u32 = 600;
const NAMES: &[&str] = &["cover", "folder", "front", "album", "albumart", "artwork", "front cover"];
const EXTS: &[&str] = &["jpg", "jpeg", "png"];

fn embedded(path: &Path) -> Option<Vec<u8>> {
    let file = lofty::read_from_path(path).ok()?;
    let pictures: Vec<_> = file.tags().iter().flat_map(|t| t.pictures()).collect();
    let front = pictures.iter().find(|p| p.pic_type() == PictureType::CoverFront).or(pictures.first())?;
    Some(front.data().to_vec())
}

/// Images that are never the cover: rip-quality reports and the back of the booklet.
const NOT_COVERS: &[&str] = &["spectrogram", "spectrum", "spectral", "back", "matrix", "inlay", "tray", "booklet", "cd"];
/// Subfolders rips keep their scans in ("Covers/Front.jpg").
const SCAN_DIRS: &[&str] = &["covers", "cover", "artwork", "scans", "art", "images"];

/// A cover beside the track: a conventional name ("folder.jpg"), else an image named after the
/// audio file (common for albums stored as one file), else "front"/"cover" in a scans
/// subfolder, else the first image that isn't a spectrogram or a back cover.
fn beside(path: &Path) -> Option<PathBuf> {
    let dir = path.parent()?;
    let images = images_in(dir);
    let stem = |p: &Path| p.file_stem().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
    let audio = stem(path);
    let words = |p: &Path| stem(p).split(|c: char| !c.is_alphanumeric()).map(String::from).collect::<Vec<_>>();
    let plausible = |p: &&PathBuf| !words(p).iter().any(|w| NOT_COVERS.contains(&w.as_str()));
    let named = |imgs: &[PathBuf]| imgs.iter().find(|p| NAMES.contains(&stem(p).as_str())).cloned();
    named(&images)
        .or_else(|| images.iter().find(|p| stem(p) == audio).cloned())
        .or_else(|| {
            let subdirs = std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).filter(|p| p.is_dir());
            subdirs.filter(|d| SCAN_DIRS.contains(&stem(d).as_str())).find_map(|d| named(&images_in(&d)))
        })
        .or_else(|| images.iter().find(plausible).cloned())
}

fn images_in(dir: &Path) -> Vec<PathBuf> {
    let mut images: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| EXTS.contains(&e.to_string_lossy().to_lowercase().as_str())))
        .filter(|p| !p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.')))
        .collect();
    images.sort();
    images
}

/// Decoded, downscaled cover for the track at `path`, if it has one, and the image file it came
/// from when it isn't embedded in the track (so it could be added to the track's tags).
pub fn load(path: &Path) -> Option<(image::RgbaImage, Option<PathBuf>)> {
    let (bytes, file) = match embedded(path) {
        Some(b) => (b, None),
        None => {
            let f = beside(path)?;
            (std::fs::read(&f).ok()?, Some(f))
        }
    };
    let img = image::load_from_memory(&bytes).ok()?;
    let img = if img.width() > MAX_SIDE || img.height() > MAX_SIDE { img.thumbnail(MAX_SIDE, MAX_SIDE) } else { img };
    Some((img.to_rgba8(), file))
}

/// An image file as a cover to embed: at most `EMBED_SIDE` pixels, baseline JPEG.
pub fn for_embedding(image: &Path) -> anyhow::Result<Vec<u8>> {
    use anyhow::Context;
    shrink(&std::fs::read(image).with_context(|| format!("reading {}", image.display()))?)
}

/// The front cover of a file, ready to embed in another (see `for_embedding`): the one embedded
/// in it, else the image beside it.
pub fn cover_for_embedding(path: &Path) -> Option<Vec<u8>> {
    let bytes = embedded(path).or_else(|| std::fs::read(beside(path)?).ok())?;
    shrink(&bytes).ok()
}

fn shrink(bytes: &[u8]) -> anyhow::Result<Vec<u8>> {
    let img = image::load_from_memory(bytes)?;
    let img = if img.width() > EMBED_SIDE || img.height() > EMBED_SIDE { img.thumbnail(EMBED_SIDE, EMBED_SIDE) } else { img };
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 90).encode_image(&img.to_rgb8())?;
    Ok(out)
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
        let (img, file) = load(&track).expect("cover found");
        assert_eq!(img.dimensions(), (MAX_SIDE, 240));
        assert_eq!(file, Some(dir.join("Folder.png")), "not embedded: it could be added to the tags");
        let jpeg = for_embedding(&dir.join("Folder.png")).unwrap();
        assert_eq!(image::load_from_memory(&jpeg).unwrap().width(), EMBED_SIDE);
        std::fs::remove_dir_all(&dir).unwrap();

        // An album file: its spectrogram sorts first, but the image named after it is the cover.
        std::fs::create_dir_all(&dir).unwrap();
        let album = dir.join("The Cure - Disintegration.flac");
        std::fs::write(&album, b"no tags").unwrap();
        image::RgbaImage::from_pixel(10, 10, image::Rgba([0, 0, 0, 255])).save(dir.join("The Cure - Disintegration.flac.Spectrogram.png")).unwrap();
        assert!(load(&album).is_none(), "a spectrogram is never the cover");
        image::RgbaImage::from_pixel(10, 20, image::Rgba([0, 0, 0, 255])).save(dir.join("The Cure - Disintegration.png")).unwrap();
        assert_eq!(beside(&album), Some(dir.join("The Cure - Disintegration.png")));
        std::fs::remove_dir_all(&dir).unwrap();

        // Scans kept in a subfolder.
        std::fs::create_dir_all(dir.join("Covers")).unwrap();
        image::RgbaImage::from_pixel(10, 10, image::Rgba([0, 0, 0, 255])).save(dir.join("Covers/Back.png")).unwrap();
        image::RgbaImage::from_pixel(10, 10, image::Rgba([0, 0, 0, 255])).save(dir.join("Covers/Front.png")).unwrap();
        assert_eq!(beside(&track), Some(dir.join("Covers/Front.png")));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
