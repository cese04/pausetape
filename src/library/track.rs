//! A track as the app knows it, and which formats the Echo Mini and this app can play.

use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::cue::CueSpan;
use super::mp4::codec_name;

/// Formats the RECHO firmware plays (FLAC/WAV up to 24-bit/192 kHz, MP3 up to 320 kbps).
pub const RECHO_FORMATS: &[&str] = &["flac", "mp3", "wav"];

/// Formats NeatLibrary itself can decode and play (via Symphonia), beyond RECHO's.
pub const APP_FORMATS: &[&str] = &["flac", "mp3", "wav", "m4a", "m4b", "mp4", "aac", "alac", "aiff", "aif", "aifc", "ogg", "oga", "caf", "mka", "mp2"];

/// MP4 sample entries Symphonia decodes. Anything else in an .m4a (Dolby, Opus…) needs ffmpeg.
/// MP4 sample entries FFmpeg has no decoder for.
const NO_DECODER_CODECS: &[&str] = &["ac-4"];
const NATIVE_MP4_CODECS: &[&str] = &["mp4a", "alac"];

/// Lossless formats: songs cut from an album file in one of these become FLAC on the card.
const LOSSLESS: &[&str] = &["flac", "wav", "aiff", "aif", "aifc", "ape", "wv", "w64"];

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Track {
    pub path: PathBuf,
    pub ext: String,
    pub artist: Option<String>,
    pub album_artist: Option<String>,
    pub album: Option<String>,
    pub title: Option<String>,
    pub track: Option<u32>,
    pub disc: Option<u32>,
    pub year: Option<u16>,
    pub compilation: bool,
    pub duration: Duration,
    pub size: u64,
    /// Modification time (ns since the Unix epoch) when the tags were read.
    pub mtime: u64,
    /// Codec inside an MP4-family container, as its sample-entry code ("mp4a", "alac", "ec-3"…).
    /// `None` for other containers, whose codec the extension already implies.
    pub codec: Option<String>,
    /// Fields were changed in-app (e.g. from MusicBrainz) and differ from the file's tags.
    pub dirty: bool,
    /// One song of an album stored as a single file, as a CUE sheet describes it. `path` is
    /// then the album file, and `duration`/`size` are the song's (size estimated).
    pub cue: Option<CueSpan>,
}

impl Track {
    /// What identifies this track in playlists, corrections and caches: its path, plus
    /// "#<track>" for a song inside an album file ("…/Album.flac#03").
    pub fn id(&self) -> PathBuf {
        match &self.cue {
            Some(c) => {
                let mut s = self.path.clone().into_os_string();
                s.push(format!("#{:02}", c.number));
                s.into()
            }
            None => self.path.clone(),
        }
    }

    /// The Echo Mini running RECHO can play this file, or (for a song in an album file) the
    /// FLAC the song is cut into on the card.
    pub fn on_recho(&self) -> bool {
        match self.cue {
            Some(_) => self.lossless(),
            None => RECHO_FORMATS.contains(&self.ext.as_str()),
        }
    }

    /// The file extension on the card: songs cut from an album file become FLAC.
    pub fn card_ext(&self) -> &str {
        if self.cue.is_some() && self.lossless() { "flac" } else { &self.ext }
    }

    fn lossless(&self) -> bool {
        LOSSLESS.contains(&self.ext.as_str()) || self.codec.as_deref() == Some("alac")
    }

    /// The built-in decoder (Symphonia) handles this file without help.
    pub fn native(&self) -> bool {
        APP_FORMATS.contains(&self.ext.as_str()) && self.codec.as_deref().is_none_or(|c| NATIVE_MP4_CODECS.contains(&c))
    }

    /// NeatLibrary's player can play this file, natively or by converting it with ffmpeg.
    pub fn plays_here(&self) -> bool {
        self.native() || (self.ffmpeg_can_decode() && crate::audio::ffmpeg().is_some())
    }

    /// False for the few codecs not even ffmpeg decodes (Dolby AC-4, as of FFmpeg 9).
    pub fn ffmpeg_can_decode(&self) -> bool {
        !self.codec.as_deref().is_some_and(|c| NO_DECODER_CODECS.contains(&c))
    }

    /// What to call the format in warnings: the codec when it's what matters ("E-AC-3"), else
    /// the extension ("M4A").
    pub fn format_label(&self) -> String {
        match self.codec.as_deref() {
            Some(c) if !NATIVE_MP4_CODECS.contains(&c) => codec_name(c).to_string(),
            _ => self.ext.to_uppercase(),
        }
    }

    pub fn display_title(&self) -> String {
        self.title.clone().unwrap_or_else(|| {
            self.path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
        })
    }

    /// The artist whose folder this track belongs in, before clustering.
    pub fn folder_artist_raw(&self) -> Option<&str> {
        self.album_artist.as_deref().or(self.artist.as_deref()).filter(|s| !s.trim().is_empty())
    }
}

/// "Dolby E-AC-3 ×28, M4A ×8, OPUS ×4" for the files RECHO can't play, most common first.
pub fn non_recho_summary<'a>(tracks: impl IntoIterator<Item = &'a Track>) -> Option<(usize, String)> {
    let mut by_ext: std::collections::BTreeMap<String, usize> = Default::default();
    for t in tracks.into_iter().filter(|t| !t.on_recho()) {
        *by_ext.entry(t.format_label()).or_default() += 1;
    }
    let total: usize = by_ext.values().sum();
    if total == 0 {
        return None;
    }
    let mut v: Vec<_> = by_ext.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    Some((total, v.iter().map(|(e, n)| format!("{e} ×{n}")).collect::<Vec<_>>().join(", ")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m4a(codec: &str) -> Track {
        Track { ext: "m4a".into(), codec: Some(codec.into()), ..Default::default() }
    }

    #[test]
    fn what_plays_where() {
        let flac = Track { ext: "flac".into(), ..Default::default() };
        assert!(flac.on_recho() && flac.native());
        // AAC and ALAC in .m4a: not on RECHO, but the built-in decoder handles them.
        assert!(!m4a("mp4a").on_recho() && m4a("mp4a").native() && m4a("alac").native());
        // Dolby E-AC-3 (Atmos releases) needs ffmpeg; AC-4 has no decoder anywhere.
        assert!(!m4a("ec-3").native() && m4a("ec-3").ffmpeg_can_decode());
        assert!(!m4a("ac-4").native() && !m4a("ac-4").ffmpeg_can_decode() && !m4a("ac-4").plays_here());
        assert_eq!(m4a("ec-3").format_label(), "Dolby E-AC-3");
        assert_eq!(m4a("mp4a").format_label(), "M4A");
    }
}
