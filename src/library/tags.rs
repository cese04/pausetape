//! Writing corrected tags back into audio files.

use std::path::Path;

use anyhow::{Context, Result};
use lofty::config::WriteOptions;
use lofty::picture::{MimeType, Picture, PictureType};
use lofty::prelude::*;
use lofty::tag::Tag;
use lofty::tag::items::Timestamp;

use super::track::Track;

/// Write the in-app metadata of `t` into the file at `path` (which may be a copy of `t.path`).
pub fn write_tags(t: &Track, path: &Path) -> Result<()> {
    let mut file = lofty::read_from_path(path).with_context(|| format!("reading {}", path.display()))?;
    if file.primary_tag().is_none() {
        file.insert_tag(Tag::new(file.primary_tag_type()));
    }
    let tag = file.primary_tag_mut().expect("tag inserted above");
    if let Some(v) = &t.artist {
        tag.set_artist(v.clone());
    }
    if let Some(v) = &t.album_artist {
        tag.insert_text(ItemKey::AlbumArtist, v.clone());
    }
    if let Some(v) = &t.album {
        tag.set_album(v.clone());
    }
    if let Some(v) = &t.title {
        tag.set_title(v.clone());
    }
    if let Some(v) = t.track {
        tag.set_track(v);
    }
    if let Some(v) = t.disc {
        tag.set_disk(v);
    }
    if let Some(y) = t.year
        && let Ok(ts) = y.to_string().parse::<Timestamp>()
    {
        tag.set_date(ts);
    }
    if let Some(image) = &t.cover {
        let jpeg = super::cover::for_embedding(image)?;
        tag.remove_picture_type(PictureType::CoverFront);
        tag.push_picture(Picture::unchecked(jpeg).pic_type(PictureType::CoverFront).mime_type(MimeType::Jpeg).build());
    }
    tag.save_to_path(path, WriteOptions::default()).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

/// Give `dst` the front cover embedded in `src` when `dst` has none: files made by ffmpeg
/// (converted, or cut from an album file) carry no pictures over.
pub fn carry_cover(src: &Path, dst: &Path) -> Result<()> {
    let mut file = lofty::read_from_path(dst).with_context(|| format!("reading {}", dst.display()))?;
    if file.tags().iter().any(|t| t.pictures().iter().any(|p| p.pic_type() == PictureType::CoverFront)) {
        return Ok(());
    }
    let Some(jpeg) = super::cover::embedded_for_embedding(src) else { return Ok(()) };
    if file.primary_tag().is_none() {
        file.insert_tag(Tag::new(file.primary_tag_type()));
    }
    let tag = file.primary_tag_mut().expect("tag inserted above");
    tag.push_picture(Picture::unchecked(jpeg).pic_type(PictureType::CoverFront).mime_type(MimeType::Jpeg).build());
    tag.save_to_path(dst, WriteOptions::default()).with_context(|| format!("writing {}", dst.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    /// `NEATLIBRARY_EMBED=<song>:<image> cargo test -- --ignored embeds_cover --nocapture`
    /// Embeds the image as the front cover of a temporary copy of the song and reads it back.
    #[test]
    #[ignore]
    fn embeds_cover() {
        use lofty::prelude::*;
        let Ok(spec) = std::env::var("NEATLIBRARY_EMBED") else { return };
        let (song, image) = spec.rsplit_once(':').unwrap();
        let song = std::path::Path::new(song);
        let copy = std::env::temp_dir().join(format!("neatlibrary-embed-{}.{}", std::process::id(), song.extension().unwrap().to_string_lossy()));
        std::fs::copy(song, &copy).unwrap();
        let t = super::Track { title: Some("Disorder".into()), cover: Some(image.into()), ..Default::default() };
        super::write_tags(&t, &copy).unwrap();
        let file = lofty::read_from_path(&copy).unwrap();
        let pics: Vec<_> = file.tags().iter().flat_map(|t| t.pictures()).collect();
        let front = pics.iter().find(|p| p.pic_type() == lofty::picture::PictureType::CoverFront).expect("front cover embedded");
        let img = image::load_from_memory(front.data()).unwrap();
        println!("{} pictures; front {}x{}, {} KB; song {} → {} KB", pics.len(), img.width(), img.height(), front.data().len() / 1024, std::fs::metadata(song).unwrap().len() / 1024, std::fs::metadata(&copy).unwrap().len() / 1024);
        assert!(img.width().max(img.height()) <= 600);
        let _ = std::fs::remove_file(copy);
    }
}
