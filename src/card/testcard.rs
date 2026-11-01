//! The test card: a folder of short samples, one per format (and a cover, accented titles, a
//! playlist), written to the card so the user can see on the player what works. What they see
//! is kept with the card setup (`Options::tested`) and corrects the player's built-in profile.
//!
//! Samples are 6-second tones made with ffmpeg, a different pitch each. APE has no encoder in
//! ffmpeg, so its sample is the smallest APE song in the library, copied as it is.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use super::Sample;
use crate::library::Track;

/// The folder on the card, and the playlist beside it (at the top level, where players look).
pub const TEST_FOLDER: &str = "NeatLibrary format test";
pub const TEST_PLAYLIST: &str = "NeatLibrary format test.m3u8";

/// The file a sample is written to, inside `TEST_FOLDER`.
pub fn file_name(s: Sample) -> String {
    let n = Sample::ALL.iter().position(|&x| x == s).unwrap_or(0) + 1;
    let ext = match s {
        Sample::Flac | Sample::Flac96 | Sample::Flac192 | Sample::Flac384 | Sample::Cover => "flac",
        Sample::Mp3 | Sample::Id3v23 | Sample::Id3v24 => "mp3",
        Sample::Wav => "wav",
        Sample::Aac | Sample::Alac => "m4a",
        Sample::Aiff => "aiff",
        Sample::Opus => "opus",
        Sample::Vorbis => "ogg",
        Sample::WavPack => "wv",
        Sample::Wma => "wma",
        Sample::Ape => "ape",
        Sample::Playlist => return TEST_PLAYLIST.into(),
    };
    // File names stay ASCII: the accents are for the tags to show.
    let label = s.label().split(" · ").next().unwrap_or_default();
    format!("{n:02} - {label}.{ext}")
}

/// Write every sample to `dest` (the card): what each became, or why it couldn't be made.
pub fn write(dest: &Path, tracks: &[Track], mut progress: impl FnMut(usize, usize)) -> Vec<(Sample, Result<(), String>)> {
    let folder = dest.join(TEST_FOLDER);
    let mut out = Vec::new();
    if let Err(e) = std::fs::create_dir_all(&folder) {
        return Sample::ALL.iter().map(|&s| (s, Err(format!("couldn't make the folder on the card: {e}")))).collect();
    }
    let total = Sample::ALL.len();
    for (n, &s) in Sample::ALL.iter().enumerate() {
        let made = if s == Sample::Playlist {
            playlist(dest, &out)
        } else {
            sample(&folder, s, n, tracks)
        };
        out.push((s, made.map_err(|e| format!("{e:#}"))));
        progress(n + 1, total);
    }
    // macOS leaves "._" companions on FAT cards, named like the samples: a player lists them as
    // songs that don't play.
    super::clean_mac_files(&folder);
    remove_companions(dest);
    out
}

/// Take the test folder and playlist off the card: only the files the test card writes.
pub fn remove(dest: &Path) -> Result<()> {
    let folder = dest.join(TEST_FOLDER);
    for &s in &Sample::ALL[..Sample::ALL.len() - 1] {
        let f = folder.join(file_name(s));
        if f.exists() {
            std::fs::remove_file(&f).with_context(|| format!("removing {}", f.display()))?;
        }
        // macOS leaves "._" companions on FAT cards.
        let _ = std::fs::remove_file(folder.join(format!("._{}", file_name(s))));
    }
    // Only if nothing else was put in it.
    let _ = std::fs::remove_dir(&folder);
    let list = dest.join(TEST_PLAYLIST);
    if list.exists() {
        std::fs::remove_file(&list).with_context(|| format!("removing {}", list.display()))?;
    }
    remove_companions(dest);
    Ok(())
}

/// The "._" companions macOS gives the test folder and playlist on a FAT card.
fn remove_companions(dest: &Path) {
    for name in [TEST_FOLDER, TEST_PLAYLIST] {
        let _ = std::fs::remove_file(dest.join(format!("._{name}")));
    }
}

fn sample(folder: &Path, s: Sample, n: usize, tracks: &[Track]) -> Result<()> {
    let path = folder.join(file_name(s));
    if s == Sample::Ape {
        // No APE encoder anywhere: borrow the library's smallest APE song.
        let Some(t) = tracks.iter().filter(|t| t.ext == "ape" && t.cue.is_none()).min_by_key(|t| t.size) else {
            bail!("no APE encoder exists, and the library has no APE song to copy");
        };
        std::fs::copy(&t.path, &path).with_context(|| format!("copying {}", t.path.display()))?;
        return Ok(());
    }
    let Some(ff) = crate::audio::ffmpeg() else { bail!("needs ffmpeg (brew install ffmpeg)") };
    let rate = match s {
        Sample::Flac96 => 96_000,
        Sample::Flac192 => 192_000,
        Sample::Flac384 => 384_000,
        Sample::Opus => 48_000,
        _ => 44_100,
    };
    // A different pitch for each, so a skipped song is noticed.
    let tone = format!("sine=frequency={}:sample_rate={rate}:duration=6", 330 + 30 * n);
    let encoder = |names: &[&'static str]| names.iter().copied().find(|e| crate::audio::ffmpeg_encodes(e));
    let mut codec: Vec<&str> = match s {
        Sample::Flac | Sample::Cover => vec!["-c:a", "flac", "-sample_fmt", "s16"],
        Sample::Flac96 | Sample::Flac192 | Sample::Flac384 => vec!["-c:a", "flac", "-sample_fmt", "s32", "-bits_per_raw_sample", "24"],
        Sample::Wav => vec!["-c:a", "pcm_s16le"],
        Sample::Aiff => vec!["-c:a", "pcm_s16be"],
        Sample::Alac => vec!["-c:a", "alac", "-sample_fmt", "s16p"],
        Sample::WavPack => vec!["-c:a", "wavpack"],
        Sample::Wma => vec!["-c:a", "wmav2", "-b:a", "192k"],
        Sample::Mp3 | Sample::Id3v23 | Sample::Id3v24 => vec!["-c:a", "libmp3lame", "-b:a", "320k"],
        Sample::Aac => vec!["-c:a", encoder(&["aac_at", "aac"]).unwrap_or("aac"), "-b:a", "256k"],
        Sample::Opus => vec!["-c:a", encoder(&["libopus", "opus"]).unwrap_or("libopus"), "-b:a", "160k", "-strict", "-2"],
        Sample::Vorbis => vec!["-c:a", encoder(&["libvorbis", "vorbis"]).unwrap_or("libvorbis"), "-q:a", "6", "-strict", "-2"],
        Sample::Ape | Sample::Playlist => unreachable!("not made with ffmpeg"),
    };
    if !crate::audio::ffmpeg_encodes(codec[1]) {
        bail!("this ffmpeg has no {} encoder", codec[1]);
    }
    match s {
        Sample::Id3v23 => codec.extend(["-id3v2_version", "3"]),
        Sample::Id3v24 => codec.extend(["-id3v2_version", "4"]),
        _ => {}
    }
    let title = format!("{:02} {}", n + 1, s.label());
    let track = (n + 1).to_string();
    let tags = ["title", &title, "artist", "NeatLibrary", "album_artist", "NeatLibrary", "album", "Format test", "track", &track];
    let mut cmd = crate::platform::quiet_command(ff);
    cmd.args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", &tone, "-ac", "2"]).args(&codec);
    for kv in tags.chunks(2) {
        cmd.arg("-metadata").arg(format!("{}={}", kv[0], kv[1]));
    }
    let done = cmd.arg(&path).output().context("running ffmpeg")?;
    if !done.status.success() {
        let _ = std::fs::remove_file(&path);
        bail!("ffmpeg: {}", String::from_utf8_lossy(&done.stderr).lines().last().unwrap_or("failed"));
    }
    if s == Sample::Cover {
        let image = std::env::temp_dir().join(format!("neatlibrary-test-cover-{}.png", std::process::id()));
        cover_image().save(&image).context("making the cover")?;
        let t = Track { cover: Some(image.clone()), ..Default::default() };
        let written = crate::library::write_tags(&t, &path);
        let _ = std::fs::remove_file(&image);
        written?;
    }
    Ok(())
}

/// An .m3u8 at the card's top level listing the samples that were made, by relative path.
fn playlist(dest: &Path, made: &[(Sample, Result<(), String>)]) -> Result<()> {
    let mut list = String::from("#EXTM3U\n");
    for (s, _) in made.iter().filter(|(_, r)| r.is_ok()) {
        list.push_str(&format!("#EXTINF:6,NeatLibrary - {}\n{TEST_FOLDER}/{}\n", s.label(), file_name(*s)));
    }
    let path: PathBuf = dest.join(TEST_PLAYLIST);
    std::fs::write(&path, list).with_context(|| format!("writing {}", path.display()))
}

/// Orange above, blue below, split on the diagonal: easy to recognise on a small screen.
fn cover_image() -> image::RgbImage {
    image::RgbImage::from_fn(600, 600, |x, y| if x + y < 600 { image::Rgb([240, 140, 40]) } else { image::Rgb([40, 90, 200]) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_card_files_and_removal() {
        assert_eq!(file_name(Sample::Flac), "01 - FLAC 16-bit 44.1 kHz.flac");
        assert_eq!(file_name(Sample::Id3v23), "16 - ID3v2.3.mp3");
        let card = std::env::temp_dir().join(format!("neatlibrary-test-testcard-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&card);
        // Without the encoders a sample says why; with them, it's made. Either way the playlist
        // lists only what was made, and removing leaves the card as it was.
        let made = write(&card, &[], |_, _| {});
        assert_eq!(made.len(), Sample::ALL.len());
        let ape = made.iter().find(|(s, _)| *s == Sample::Ape).unwrap();
        assert!(ape.1.as_ref().is_err_and(|e| e.contains("no APE")));
        let list = std::fs::read_to_string(card.join(TEST_PLAYLIST)).unwrap();
        assert!(!list.contains(".ape"));
        if crate::audio::ffmpeg().is_some() {
            assert!(card.join(TEST_FOLDER).join(file_name(Sample::Wav)).is_file());
            assert!(list.contains(&file_name(Sample::Wav)));
            let cover = lofty::read_from_path(card.join(TEST_FOLDER).join(file_name(Sample::Cover))).unwrap();
            assert!(lofty::file::TaggedFileExt::tags(&cover).iter().any(|t| t.picture_count() > 0), "the cover sample has a cover");
        }
        // Something the user put in the folder stays, and so does the folder.
        std::fs::write(card.join(TEST_FOLDER).join("mine.txt"), "x").unwrap();
        remove(&card).unwrap();
        assert!(!card.join(TEST_PLAYLIST).exists() && card.join(TEST_FOLDER).join("mine.txt").exists());
        assert_eq!(std::fs::read_dir(card.join(TEST_FOLDER)).unwrap().count(), 1);
        let _ = std::fs::remove_dir_all(&card);
    }

    /// `NEATLIBRARY_TESTCARD=<folder> cargo test -- --ignored writes_test_card --nocapture`
    /// Writes the test card's samples into the folder and says what each became.
    #[test]
    #[ignore]
    fn writes_test_card() {
        let Some(dest) = std::env::var_os("NEATLIBRARY_TESTCARD").map(PathBuf::from) else { return };
        for (s, r) in write(&dest, &[], |_, _| {}) {
            println!("{:<28} {}", file_name(s), r.err().unwrap_or_else(|| "ok".into()));
        }
    }
}
