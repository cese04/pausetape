//! Writing corrected tags back into audio files.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use lofty::config::WriteOptions;
use lofty::file::FileType;
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
    let audio = if file.file_type() == FileType::Mpeg { untidy_mp3_head(path).with_context(|| format!("reading {}", path.display()))? } else { None };
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
            let jpeg = super::cover::for_embedding(image).with_context(|| format!("making the cover for {}", path.display()))?;
            tag.remove_picture_type(PictureType::CoverFront);
            tag.push_picture(Picture::unchecked(jpeg).pic_type(PictureType::CoverFront).mime_type(MimeType::Jpeg).build());
        }
    }
    match audio {
        Some(start) => rewrite_from(path, start, |tmp| Ok(file.save_to_path(tmp, WriteOptions::default())?)),
        None => {
            can_write(path)?;
            Ok(file.save_to_path(path, WriteOptions::default())?)
        }
    }
    .with_context(|| format!("writing {}", path.display()))
}

/// Where an MP3's audio starts, when what comes before it would keep lofty from writing: more
/// than one ID3v2 tag (taggers that add a tag rather than replace the one there), or zeros
/// between the tag and the first frame (lofty looks for the audio no further than 1 KB past the
/// tag). Reading, lofty merges the stacked tags, the later one's frames winning, as players do.
/// `None` when the file is tidy, or when what follows isn't audio (that's left alone).
fn untidy_mp3_head(path: &Path) -> std::io::Result<Option<u64>> {
    let mut f = File::open(path)?;
    let (mut pos, mut tags, mut zeros) = (0u64, 0, 0u64);
    loop {
        let blank = zeros_at(&mut f, pos)?;
        (pos, zeros) = (pos + blank, zeros + blank);
        let mut head = [0u8; 10];
        f.seek(SeekFrom::Start(pos))?;
        if f.read_exact(&mut head).is_err() {
            return Ok(None);
        }
        if &head[..3] == b"ID3" {
            let size = head[6..10].iter().fold(0u64, |n, b| (n << 7) | u64::from(b & 0x7f));
            let footer = if head[5] & 0x10 != 0 { 10 } else { 0 };
            (pos, tags) = (pos + 10 + size + footer, tags + 1);
            continue;
        }
        let frame = head[0] == 0xff && head[1] & 0xe0 == 0xe0;
        return Ok((frame && (tags > 1 || zeros > 0)).then_some(pos));
    }
}

/// How many zero bytes `f` holds from `pos` on.
fn zeros_at(f: &mut File, pos: u64) -> std::io::Result<u64> {
    f.seek(SeekFrom::Start(pos))?;
    let (mut buf, mut n) = ([0u8; 4096], 0u64);
    loop {
        let got = f.read(&mut buf)?;
        let blank = buf[..got].iter().take_while(|&&b| b == 0).count();
        n += blank as u64;
        if got == 0 || blank < got {
            return Ok(n);
        }
    }
}

/// Rewrite `path` without its first `start` bytes, then `write` the tags into it: on a copy
/// beside it that takes its place only once all is written, so a failure leaves it as it was.
fn rewrite_from(path: &Path, start: u64, write: impl FnOnce(&Path) -> Result<()>) -> Result<()> {
    let tmp = beside(path);
    let done = (|| {
        let mut src = File::open(path)?;
        src.seek(SeekFrom::Start(start))?;
        std::io::copy(&mut src, &mut File::create(&tmp)?)?;
        write(&tmp)?;
        std::fs::set_permissions(&tmp, std::fs::metadata(path)?.permissions())?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    })();
    if done.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    done
}

fn beside(path: &Path) -> PathBuf {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    path.with_file_name(format!(".{name}.pausetape"))
}

/// Lofty tells a file's kind from its contents before writing (from its extension too when
/// reading): say so plainly when it can't, rather than "no format could be determined".
fn can_write(path: &Path) -> Result<()> {
    let probe = lofty::probe::Probe::new(File::open(path)?).guess_file_type()?;
    if probe.file_type().is_none() {
        bail!("the audio isn't where its tags say it starts, so the tags weren't written");
    }
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

    /// An ID3v2 tag of version 2.`ver` holding text frames (a "COMM" one is a comment).
    fn id3(ver: u8, frames: &[(&str, &str)]) -> Vec<u8> {
        let mut body = Vec::new();
        for (id, text) in frames {
            let mut data = vec![0];
            if *id == "COMM" {
                data.extend_from_slice(b"eng\0");
            }
            data.extend_from_slice(text.as_bytes());
            body.extend_from_slice(id.as_bytes());
            body.extend_from_slice(&(data.len() as u32).to_be_bytes()); // under 128: syncsafe too
            body.extend_from_slice(&[0, 0]);
            body.extend(data);
        }
        let n = body.len() as u32;
        let mut tag = vec![b'I', b'D', b'3', ver, 0, 0];
        tag.extend([n >> 21, n >> 14, n >> 7, n].map(|b| (b & 0x7f) as u8));
        tag.extend(body);
        tag
    }

    /// Forty silent MPEG-1 layer III frames, 128 kbps at 44.1 kHz.
    fn frames() -> Vec<u8> {
        let mut frame = vec![0xff, 0xfb, 0x90, 0xc0];
        frame.resize(417, 0);
        frame.repeat(40)
    }

    /// The tags at the start of `bytes`, then what follows them.
    fn head(bytes: &[u8]) -> (usize, &[u8]) {
        let (mut pos, mut tags) = (0, 0);
        while bytes[pos..].starts_with(b"ID3") {
            pos += 10 + bytes[pos + 6..pos + 10].iter().fold(0, |n, b| (n << 7) | usize::from(b & 0x7f));
            tags += 1;
        }
        (tags, &bytes[pos..pos + 2])
    }

    #[test]
    fn untidy_mp3s_get_one_tag_before_their_audio() {
        let dir = std::env::temp_dir().join(format!("pausetape-test-untidy-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let t = Track { artist: Some("Cocteau Twins".into()), title: Some("Musette and Drums".into()), ..Default::default() };
        // Two tags, as some taggers leave them, and 1044 zeros after the tag, as others do.
        let stacked = [id3(3, &[("TIT2", "Musette"), ("COMM", "kept")]), id3(4, &[("TIT2", "Musette"), ("TPE1", "Cocteau")]), frames()].concat();
        let zeros = [id3(3, &[("TIT2", "Musette"), ("COMM", "kept")]), vec![0; 1044], frames()].concat();
        for (name, bytes) in [("stacked.mp3", stacked), ("zeros.mp3", zeros)] {
            let path = dir.join(name);
            std::fs::write(&path, bytes).unwrap();
            write_tags(&t, &path).unwrap();
            assert_eq!(head(&std::fs::read(&path).unwrap()), (1, &[0xff, 0xfb][..]), "{name}");
            let file = lofty::read_from_path(&path).unwrap();
            let tag = file.primary_tag().unwrap();
            assert_eq!((tag.title().as_deref(), tag.artist().as_deref()), (Some("Musette and Drums"), Some("Cocteau Twins")), "{name}");
            assert_eq!(tag.comment().as_deref(), Some("kept"), "{name}: the frames not edited stay");
            assert!(!beside(&path).exists(), "{name}: no copy left behind");
        }
        // What follows the tag isn't audio: nothing is written, and the file stays as it was.
        let path = dir.join("junk.mp3");
        let junk = [id3(3, &[("TIT2", "Musette")]), vec![0x55; 2000], frames()].concat();
        std::fs::write(&path, &junk).unwrap();
        let e = write_tags(&t, &path).unwrap_err();
        assert!(format!("{e:#}").contains("the audio isn't where its tags say it starts"), "{e:#}");
        assert_eq!(std::fs::read(&path).unwrap(), junk);
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
