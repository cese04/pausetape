//! The players a card is written for, and what each plays and shows. Like calibre's device
//! drivers: the library stays as it is, and each card setup converts and warns for its player.
//!
//! RECHO's facts come from its site, the stock firmware's from reviews (docs/NEXT.md part E);
//! neither is yet checked on a player by this app.

use serde::{Deserialize, Serialize};

use crate::library::Track;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
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
        let leave = |device| Rules { unplayable: Unplayable::Leave, device, ..Default::default() };
        assert_eq!(target(&aac, leave(stock)), Ok(Target::Copy));
        assert_eq!(target(&aac, leave(recho)), Err("format not playable on RECHO"));
        // Card setups saved before players existed are for RECHO.
        let old: Options = serde_json::from_str(r#"{"album_order":"Year"}"#).unwrap();
        assert_eq!(old.device, Device::EchoMiniRecho);
    }
}
