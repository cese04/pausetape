//! Writing corrected tags back into audio files.

use std::path::Path;

use anyhow::{Context, Result};
use lofty::config::WriteOptions;
use lofty::picture::{MimeType, Picture, PictureType};
use lofty::prelude::*;
use lofty::tag::{Tag, TagType};
use lofty::tag::items::Timestamp;

use super::track::Track;

/// Write the in-app metadata of `t` into the file at `path` (which may be a copy of `t.path`):
/// every field, an emptied one removed, into each tag the file has (an MP3's ID3v1 beside its
/// ID3v2, an APE tag beside another), so players reading any of them see the same.
pub fn write_tags(t: &Track, path: &Path) -> Result<()> {
    let mut file = lofty::read_from_path(path).with_context(|| format!("reading {}", path.display()))?;
    if file.primary_tag().is_none() {
        file.insert_tag(Tag::new(file.primary_tag_type()));
    }
    let primary = file.primary_tag_type();
    let types: Vec<TagType> = file.tags().iter().map(Tag::tag_type).collect();
    for ty in types {
        let tag = file.tag_mut(ty).expect("listed above");
        set_fields(tag, t);
        if ty == primary
            && let Some(image) = &t.cover
        {
            let jpeg = super::cover::for_embedding(image)?;
            tag.remove_picture_type(PictureType::CoverFront);
            tag.push_picture(Picture::unchecked(jpeg).pic_type(PictureType::CoverFront).mime_type(MimeType::Jpeg).build());
        }
    }
    file.save_to_path(path, WriteOptions::default()).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

fn set_fields(tag: &mut Tag, t: &Track) {
    match &t.artist {
        Some(v) => tag.set_artist(v.clone()),
        None => tag.remove_artist(),
    }
    match &t.album_artist {
        Some(v) => {
            tag.insert_text(ItemKey::AlbumArtist, v.clone());
        }
        None => tag.remove_key(ItemKey::AlbumArtist),
    }
    match &t.album {
        Some(v) => tag.set_album(v.clone()),
        None => tag.remove_album(),
    }
    match &t.title {
        Some(v) => tag.set_title(v.clone()),
        None => tag.remove_title(),
    }
    match t.track {
        Some(v) => tag.set_track(v),
        None => tag.remove_track(),
    }
    match t.disc {
        Some(v) => tag.set_disk(v),
        None => tag.remove_disk(),
    }
    // The app keeps the year only: a full date ("1997-05-21") stays while its year is right.
    match t.year {
        Some(y) if tag.date().is_some_and(|d| d.year == y) => {}
        Some(y) => {
            if let Ok(ts) = y.to_string().parse::<Timestamp>() {
                tag.set_date(ts);
            }
        }
        None => tag.remove_date(),
    }
    // Genres only when known (`Track::genres`), several in one value as most players read them.
    match t.genres.as_deref() {
        Some([]) => {
            tag.remove_key(ItemKey::Genre);
        }
        Some(g) => {
            tag.insert_text(ItemKey::Genre, g.join("; "));
        }
        None => {}
    }
    if t.compilation {
        tag.insert_text(ItemKey::FlagCompilation, "1".into());
    } else {
        tag.remove_key(ItemKey::FlagCompilation);
    }
}

/// Give `dst` the front cover of `src` (embedded, else the image beside it) when `dst` has none:
/// files made by ffmpeg (converted, or cut from an album file) carry no pictures over, and the
/// players show embedded covers, not image files.
pub fn carry_cover(src: &Path, dst: &Path) -> Result<()> {
    let mut file = lofty::read_from_path(dst).with_context(|| format!("reading {}", dst.display()))?;
    if file.tags().iter().any(|t| t.pictures().iter().any(|p| p.pic_type() == PictureType::CoverFront)) {
        return Ok(());
    }
    let Some(jpeg) = super::cover::cover_for_embedding(src) else { return Ok(()) };
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
    use super::*;

    /// A WAV of a few silent samples: it carries two tags, ID3v2 and RIFF INFO.
    fn wav(path: &Path) {
        let mut b = b"RIFF".to_vec();
        b.extend_from_slice(&(4 + 24 + 8 + 4u32).to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&[1, 0, 1, 0, 0x44, 0xac, 0, 0, 0x88, 0x58, 1, 0, 2, 0, 16, 0]);
        b.extend_from_slice(b"data");
        b.extend_from_slice(&4u32.to_le_bytes());
        b.extend_from_slice(&[0; 4]);
        std::fs::write(path, b).unwrap();
    }

    #[test]
    fn fields_are_cleared_dates_kept_and_every_tag_written() {
        let dir = std::env::temp_dir().join(format!("pausetape-test-tags-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("song.wav");
        wav(&path);
        let mut file = lofty::read_from_path(&path).unwrap();
        for ty in [TagType::Id3v2, TagType::RiffInfo] {
            let mut tag = Tag::new(ty);
            tag.set_artist("Old".into());
            tag.set_album("Old Album".into());
            tag.set_date("1997-05-21".parse().unwrap());
            file.insert_tag(tag);
        }
        file.save_to_path(&path, WriteOptions::default()).unwrap();

        // The artist corrected, the album emptied, the year as it was, marked a compilation.
        let t = Track { artist: Some("New".into()), title: Some("Song".into()), year: Some(1997), compilation: true, ..Default::default() };
        write_tags(&t, &path).unwrap();
        let file = lofty::read_from_path(&path).unwrap();
        for ty in [TagType::Id3v2, TagType::RiffInfo] {
            let tag = file.tag(ty).unwrap();
            assert_eq!((tag.artist().as_deref(), tag.album()), (Some("New"), None), "{ty:?}");
        }
        let id3 = file.tag(TagType::Id3v2).unwrap();
        assert_eq!(id3.date().map(|d| (d.year, d.month, d.day)), Some((1997, Some(5), Some(21))), "the full date kept");
        assert_eq!(id3.get_string(ItemKey::FlagCompilation), Some("1"));
        // Another year replaces the date.
        write_tags(&Track { year: Some(1998), ..t.clone() }, &path).unwrap();
        let file = lofty::read_from_path(&path).unwrap();
        assert_eq!(file.tag(TagType::Id3v2).unwrap().date().map(|d| (d.year, d.month)), Some((1998, None)));
        // Genres written as one value, read back as several; unknown genres leave the tag alone.
        write_tags(&Track { genres: Some(vec!["Electronic".into(), "Trip Hop".into()]), ..t.clone() }, &path).unwrap();
        let read = crate::library::read_file(&path).unwrap();
        assert_eq!(read.genres.as_deref(), Some(&["Electronic".to_string(), "Trip Hop".to_string()][..]));
        write_tags(&Track { genres: None, ..t }, &path).unwrap();
        assert_eq!(crate::library::read_file(&path).unwrap().genres.map(|g| g.len()), Some(2), "kept when not known");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `PAUSETAPE_EMBED=<song>:<image> cargo test -- --ignored embeds_cover --nocapture`
    /// Embeds the image as the front cover of a temporary copy of the song and reads it back.
    #[test]
    #[ignore]
    fn embeds_cover() {
        use lofty::prelude::*;
        let Ok(spec) = std::env::var("PAUSETAPE_EMBED") else { return };
        let (song, image) = spec.rsplit_once(':').unwrap();
        let song = std::path::Path::new(song);
        let copy = std::env::temp_dir().join(format!("pausetape-embed-{}.{}", std::process::id(), song.extension().unwrap().to_string_lossy()));
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
