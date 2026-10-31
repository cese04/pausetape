//! The players a card is written for, and what each plays and shows. Like calibre's device
//! drivers: the library stays as it is, and each card setup converts and warns for its player.
//!
//! RECHO's facts come from its site, the stock firmware's from reviews (docs/NEXT.md part E).
//! A test card (`testcard.rs`) checks them on the player itself: what the user saw is kept with
//! the card setup and wins over the built-in lists (`Player`).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::library::Track;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Device {
    /// The Snowsky Echo Mini running RECHO: folders sorted by name; FLAC, MP3 and WAV only; no
    /// covers, no playlists.
    #[default]
    EchoMiniRecho,
    /// The Echo Mini with Snowsky's own firmware: a library built from tags, more formats,
    /// covers on the play screen, at most 8,192 songs.
    EchoMiniStock,
}

impl Device {
    pub const ALL: [Device; 2] = [Device::EchoMiniRecho, Device::EchoMiniStock];

    /// For choosing one: "Echo Mini · RECHO".
    pub fn name(self) -> &'static str {
        match self {
            Device::EchoMiniRecho => "Echo Mini · RECHO",
            Device::EchoMiniStock => "Echo Mini · stock firmware",
        }
    }

    /// In sentences and badges: "won't play on RECHO".
    pub fn short(self) -> &'static str {
        match self {
            Device::EchoMiniRecho => "RECHO",
            Device::EchoMiniStock => "the Echo Mini",
        }
    }

    /// "RECHO plays FLAC, MP3 and WAV only."
    pub fn plays_text(self) -> &'static str {
        match self {
            Device::EchoMiniRecho => "RECHO plays FLAC, MP3 and WAV only.",
            Device::EchoMiniStock => "The Echo Mini's stock firmware plays FLAC, MP3, WAV, APE, AAC (M4A), Ogg Vorbis and WMA.",
        }
    }

    /// Under the choice in the card tab.
    pub fn about(self) -> &'static str {
        match self {
            Device::EchoMiniRecho => "Browses folders sorted by name, so album folders carry the year. Plays FLAC, MP3 and WAV; shows no covers.",
            Device::EchoMiniStock => {
                "Lists artists and albums from the tags, so corrected tags matter most. Plays more formats and shows embedded covers; lists at most 8,192 songs. From reviews, not yet checked on a player."
            }
        }
    }

    /// The player can play `t` as it is, or (for a song in an album file) the FLAC it's cut into.
    pub fn plays(self, t: &Track) -> bool {
        match self {
            Device::EchoMiniRecho => t.plays_anywhere(),
            Device::EchoMiniStock => {
                t.plays_anywhere()
                    || t.cue.is_none()
                        && match t.ext.as_str() {
                            "ape" | "wma" | "ogg" | "oga" | "aac" => true,
                            // AAC only: ALAC inside M4A isn't known to play.
                            "m4a" | "m4b" | "mp4" => t.codec.as_deref() == Some("mp4a"),
                            _ => false,
                        }
            }
        }
    }

    /// Why a song stays off the card when the setup leaves out what the player can't play.
    pub fn unplayable_reason(self) -> &'static str {
        match self {
            Device::EchoMiniRecho => "format not playable on RECHO",
            Device::EchoMiniStock => "format not playable on the Echo Mini",
        }
    }

    /// The player shows covers embedded in the songs.
    pub fn shows_covers(self) -> bool {
        self == Device::EchoMiniStock
    }

    /// The player builds its library from tags (so a playlist's copies show up as songs too).
    pub fn browses_by_tags(self) -> bool {
        self == Device::EchoMiniStock
    }

    /// The most songs the player lists.
    pub fn max_songs(self) -> Option<usize> {
        match self {
            Device::EchoMiniRecho => None,
            Device::EchoMiniStock => Some(8192),
        }
    }
}

/// One thing the test card checks. Saved by name, so the order can change.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Sample {
    Flac,
    Mp3,
    Wav,
    Flac96,
    Flac192,
    Flac384,
    Aac,
    Alac,
    Aiff,
    Opus,
    Vorbis,
    WavPack,
    Wma,
    Ape,
    /// A FLAC with an embedded cover: does the player show it?
    Cover,
    /// MP3s titled with accents and Japanese, in ID3v2.3 and ID3v2.4 tags.
    Id3v23,
    Id3v24,
    /// An .m3u8 at the card's top level listing the samples.
    Playlist,
}

impl Sample {
    pub const ALL: [Sample; 18] = [
        Sample::Flac,
        Sample::Mp3,
        Sample::Wav,
        Sample::Flac96,
        Sample::Flac192,
        Sample::Flac384,
        Sample::Aac,
        Sample::Alac,
        Sample::Aiff,
        Sample::Opus,
        Sample::Vorbis,
        Sample::WavPack,
        Sample::Wma,
        Sample::Ape,
        Sample::Cover,
        Sample::Id3v23,
        Sample::Id3v24,
        Sample::Playlist,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Sample::Flac => "FLAC 16-bit 44.1 kHz",
            Sample::Mp3 => "MP3 320",
            Sample::Wav => "WAV 16-bit 44.1 kHz",
            Sample::Flac96 => "FLAC 24-bit 96 kHz",
            Sample::Flac192 => "FLAC 24-bit 192 kHz",
            Sample::Flac384 => "FLAC 24-bit 384 kHz",
            Sample::Aac => "AAC in M4A",
            Sample::Alac => "ALAC in M4A",
            Sample::Aiff => "AIFF",
            Sample::Opus => "Opus",
            Sample::Vorbis => "Ogg Vorbis",
            Sample::WavPack => "WavPack",
            Sample::Wma => "WMA",
            Sample::Ape => "APE",
            Sample::Cover => "Embedded cover",
            Sample::Id3v23 => "ID3v2.3 · Café ü 日本",
            Sample::Id3v24 => "ID3v2.4 · Café ü 日本",
            Sample::Playlist => "Playlist (.m3u8)",
        }
    }

    /// What to look for on the player.
    pub fn question(self) -> &'static str {
        match self {
            Sample::Cover => "shows the cover (orange and blue)",
            Sample::Id3v23 | Sample::Id3v24 => "shows the title with é, ü and 日本",
            Sample::Playlist => "lists the playlist “NeatLibrary format test”",
            _ => "plays",
        }
    }

    /// The format a song is in, as the test card checks it (rates aside).
    pub fn of(t: &Track) -> Option<Sample> {
        Some(match t.ext.as_str() {
            "flac" => Sample::Flac,
            "mp3" => Sample::Mp3,
            "wav" => Sample::Wav,
            "aiff" | "aif" | "aifc" => Sample::Aiff,
            "m4a" | "m4b" | "mp4" => match t.codec.as_deref() {
                Some("mp4a") => Sample::Aac,
                Some("alac") => Sample::Alac,
                _ => return None,
            },
            "aac" => Sample::Aac,
            "opus" => Sample::Opus,
            "ogg" | "oga" => Sample::Vorbis,
            "wv" => Sample::WavPack,
            "wma" => Sample::Wma,
            "ape" => Sample::Ape,
            _ => return None,
        })
    }
}

/// What the user saw on the player, per sample: true it worked, false it didn't.
pub type Results = BTreeMap<Sample, bool>;

/// The test results as two bit sets, so `Rules` can stay `Copy`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tested {
    works: u32,
    fails: u32,
}

impl Tested {
    pub fn from_results(r: &Results) -> Tested {
        let bit = |s: &Sample| 1u32 << Sample::ALL.iter().position(|x| x == s).unwrap_or(31);
        let (works, fails) = r.iter().fold((0, 0), |(w, f), (s, ok)| if *ok { (w | bit(s), f) } else { (w, f | bit(s)) });
        Tested { works, fails }
    }

    pub fn answer(self, s: Sample) -> Option<bool> {
        let bit = 1u32 << Sample::ALL.iter().position(|x| *x == s)?;
        if self.works & bit != 0 {
            Some(true)
        } else if self.fails & bit != 0 {
            Some(false)
        } else {
            None
        }
    }

    pub fn is_empty(self) -> bool {
        self.works | self.fails == 0
    }
}

/// A player as a card setup knows it: the built-in profile, corrected by the test card.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Player {
    pub device: Device,
    pub tested: Tested,
}

impl From<Device> for Player {
    fn from(device: Device) -> Player {
        Player { device, tested: Tested::default() }
    }
}

impl Player {
    pub fn name(self) -> &'static str {
        self.device.name()
    }

    pub fn short(self) -> &'static str {
        self.device.short()
    }

    pub fn unplayable_reason(self) -> &'static str {
        self.device.unplayable_reason()
    }

    pub fn browses_by_tags(self) -> bool {
        self.device.browses_by_tags()
    }

    /// The player plays `t` as it is: as tested, else as its profile says. A song in an album
    /// file is cut into FLAC (or MP3) on the card.
    pub fn plays(self, t: &Track) -> bool {
        match (&t.cue, Sample::of(t).and_then(|s| self.tested.answer(s))) {
            (None, Some(answer)) => answer,
            _ => self.device.plays(t),
        }
    }

    pub fn shows_covers(self) -> bool {
        self.tested.answer(Sample::Cover).unwrap_or(self.device.shows_covers())
    }

    /// The highest sample rate that played, when a higher one didn't: songs above it are
    /// written at CD quality.
    pub fn max_rate(self) -> Option<u32> {
        [(Sample::Flac96, 48_000), (Sample::Flac192, 96_000), (Sample::Flac384, 192_000)].into_iter().find(|&(s, _)| self.tested.answer(s) == Some(false)).map(|(_, rate)| rate)
    }

    /// "RECHO plays FLAC, MP3 and WAV only.", or what the test card showed.
    pub fn plays_text(self) -> String {
        if self.tested.is_empty() {
            return self.device.plays_text().to_string();
        }
        let formats = |ok: bool| -> Vec<&str> {
            Sample::ALL[..14].iter().filter(|&&s| self.tested.answer(s) == Some(ok)).map(|s| s.label()).collect()
        };
        let (works, fails) = (formats(true), formats(false));
        let mut s = format!("Tested on {}:", self.short());
        if !works.is_empty() {
            s.push_str(&format!(" plays {}", works.join(", ")));
        }
        if !fails.is_empty() {
            s.push_str(&format!("{} not {}", if works.is_empty() { "" } else { ";" }, fails.join(", ")));
        }
        s.push('.');
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::card::{Options, Rules, Target, Unplayable, target};

    #[test]
    fn what_each_player_plays() {
        let song = |ext: &str, codec: Option<&str>| Track { ext: ext.into(), codec: codec.map(Into::into), ..Default::default() };
        let (flac, aac, alac, ape) = (song("flac", None), song("m4a", Some("mp4a")), song("m4a", Some("alac")), song("ape", None));
        let (recho, stock) = (Device::EchoMiniRecho, Device::EchoMiniStock);
        assert!(recho.plays(&flac) && stock.plays(&flac));
        assert!(!recho.plays(&aac) && stock.plays(&aac));
        assert!(!recho.plays(&alac) && !stock.plays(&alac), "ALAC isn't known to play on either");
        assert!(!recho.plays(&ape) && stock.plays(&ape));
        // What the player plays goes as it is, whatever the setup does with the rest.
        let leave = |device: Device| Rules { unplayable: Unplayable::Leave, player: device.into(), ..Default::default() };
        assert_eq!(target(&aac, leave(stock)), Ok(Target::Copy));
        assert_eq!(target(&aac, leave(recho)), Err("format not playable on RECHO"));
        // Card setups saved before players existed are for RECHO.
        let old: Options = serde_json::from_str(r#"{"album_order":"Year"}"#).unwrap();
        assert_eq!(old.device, Device::EchoMiniRecho);
    }

    #[test]
    fn test_results_win_over_the_profile() {
        let song = |ext: &str, codec: Option<&str>, rate| Track { ext: ext.into(), codec: codec.map(Into::into), sample_rate: Some(rate), ..Default::default() };
        let (aac, alac) = (song("m4a", Some("mp4a"), 44_100), song("m4a", Some("alac"), 44_100));
        let results: Results = [(Sample::Aac, false), (Sample::Alac, true), (Sample::Flac192, false), (Sample::Cover, true)].into();
        let p = Player { device: Device::EchoMiniStock, tested: Tested::from_results(&results) };
        assert!(!p.plays(&aac) && p.plays(&alac), "what the user saw wins");
        assert!(p.plays(&song("ape", None, 44_100)), "untested formats follow the profile");
        assert_eq!(p.max_rate(), Some(96_000));
        let recho = Player { device: Device::EchoMiniRecho, tested: Tested::from_results(&results) };
        assert!(recho.shows_covers() && !Player::from(Device::EchoMiniRecho).shows_covers());
        assert!(Tested::default().is_empty());
        assert_eq!(p.plays_text(), "Tested on the Echo Mini: plays ALAC in M4A; not FLAC 24-bit 192 kHz, AAC in M4A.");
        // Saved by name.
        assert_eq!(serde_json::to_string(&results).unwrap(), r#"{"Flac192":false,"Aac":false,"Alac":true,"Cover":true}"#);
        // Kept per player: answers for the stock firmware say nothing of RECHO.
        let mut o = Options { device: Device::EchoMiniStock, ..Default::default() };
        o.tested.insert(Device::EchoMiniStock, results);
        assert!(!o.player().plays(&aac));
        o.device = Device::EchoMiniRecho;
        assert!(o.player().tested.is_empty());
        // A song above the highest rate that played is written at CD quality.
        o.device = Device::EchoMiniStock;
        let hires = Track { ext: "flac".into(), sample_rate: Some(192_000), bit_depth: Some(24), ..Default::default() };
        let ff = crate::audio::ffmpeg().is_some();
        assert_eq!(target(&hires, o.rules()), Ok(if ff { Target::FlacCd } else { Target::Copy }));
        assert_eq!(target(&song("flac", None, 96_000), o.rules()), Ok(Target::Copy));
    }
}
