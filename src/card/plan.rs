//! The layout plan: which file goes where on the card (or in a tidied music folder).
//!
//! RECHO (the Echo Mini firmware this began with) sorts entries by name, so ordering is encoded in names: albums get a year prefix
//! ("1997 - OK Computer") or a custom position ("02 - OK Computer"), and tracks a zero-padded
//! number ("1-03 - Title.flac" on multi-disc sets). Names come from a pattern (`card::pattern`):
//! the album order's, or the user's own.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::pattern::{Field, Pattern, default_for};
use super::{Device, Player, Results};
use crate::library::Track;
use crate::library::grouping::{AlbumNode, Index};
use crate::library::names::sanitize_component;

/// How album folders are ordered inside an artist folder.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AlbumOrder {
    /// "1997 - OK Computer": chronological.
    Year,
    /// "OK Computer": alphabetical.
    Title,
    /// "02 - OK Computer": the order the user arranged.
    Custom,
}

/// What a layout does with songs in formats its player can't play (on RECHO: AAC, ALAC, Dolby, Opus…).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Unplayable {
    /// Leave them off the card.
    Leave,
    /// Copy them as they are (for another player, or a future firmware).
    Copy,
    /// Convert them: lossless to FLAC, the rest to MP3 320.
    #[default]
    Convert,
}

/// How a song becomes a file on the card.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// The file as it is.
    Copy,
    /// Encoded to FLAC with ffmpeg: lossless sources the player can't read (ALAC, AIFF, APE…), and
    /// songs cut out of a lossless album file.
    Flac,
    /// Encoded to FLAC at CD quality: 16-bit, 44.1 or 48 kHz (`cd_rate`), dithered. For hi-res
    /// lossless songs when the write asks to save space.
    FlacCd,
    /// Encoded to MP3 at 320 kbps, stereo: lossy sources the player can't read (AAC, Dolby, Opus…).
    Mp3,
}

impl Target {
    /// The file extension on the card.
    pub fn ext(self, t: &Track) -> &str {
        match self {
            Target::Copy => &t.ext,
            Target::Flac | Target::FlacCd => "flac",
            Target::Mp3 => "mp3",
        }
    }
}

/// What decides how songs reach the card, from a write's options.
#[derive(Clone, Copy, Debug, Default)]
pub struct Rules {
    /// The player: what it plays goes as it is.
    pub player: Player,
    pub unplayable: Unplayable,
    /// Write hi-res lossless songs at CD quality to save space.
    pub cd_quality: bool,
    /// Write every lossless song as MP3 320, to save more space still.
    pub lossless_mp3: bool,
}

/// More than CD quality: over 16 bits or 48 kHz.
pub fn hires(t: &Track) -> bool {
    t.hires()
}

/// The CD-quality rate for a hi-res rate: 48 kHz for its family (96, 192…), 44.1 for its own
/// (88.2, 176.4…), so resampling is by a whole ratio where possible.
pub fn cd_rate(rate: u32) -> u32 {
    if rate <= 48_000 {
        rate
    } else if rate % 44_100 == 0 {
        44_100
    } else {
        48_000
    }
}

/// How `t` reaches the card under `rules`, or why it can't. Lossy audio is never encoded to
/// FLAC (it would only take more space), and never re-encoded unless the user asked for
/// conversion. CD quality and MP3 for lossless are preferences: without ffmpeg (or LAME), songs
/// go as they otherwise would.
pub fn target(t: &Track, rules: Rules) -> Result<Target, &'static str> {
    let to = base_target(t, rules.player, rules.unplayable)?;
    let can_encode = crate::audio::ffmpeg().is_some() && t.ffmpeg_can_decode();
    Ok(match to {
        Target::Copy | Target::Flac if rules.lossless_mp3 && t.lossless() && can_encode && crate::audio::ffmpeg_encodes("libmp3lame") => Target::Mp3,
        Target::Copy | Target::Flac if rules.cd_quality && t.lossless() && hires(t) && can_encode => Target::FlacCd,
        // Above the highest rate the test card played.
        Target::Copy | Target::Flac if t.lossless() && can_encode && rules.player.max_rate().is_some_and(|m| t.sample_rate.is_some_and(|r| r > m)) => Target::FlacCd,
        to => to,
    })
}

/// Why a damaged file is left out (`Track::damaged`).
pub const DAMAGED: &str = "the file is damaged: it starts blank, so there's no audio to copy or convert";

fn base_target(t: &Track, device: Player, unplayable: Unplayable) -> Result<Target, &'static str> {
    if t.damaged {
        return Err(DAMAGED);
    }
    let encode = |to: Target| {
        if crate::audio::ffmpeg().is_none() {
            Err(if t.cue.is_some() { "needs ffmpeg to cut the album file into songs" } else { "needs ffmpeg to convert" })
        } else if !t.ffmpeg_can_decode() {
            Err("no decoder for this format")
        } else if to == Target::Mp3 && !crate::audio::ffmpeg_encodes("libmp3lame") {
            Err("this ffmpeg can't make MP3s")
        } else {
            Ok(to)
        }
    };
    if t.cue.is_some() {
        return match (t.lossless(), unplayable) {
            (true, _) => encode(Target::Flac),
            (false, Unplayable::Convert) => encode(Target::Mp3),
            (false, _) => Err("inside a lossy album file; can't be cut out without re-encoding"),
        };
    }
    if device.plays(t) {
        return Ok(Target::Copy);
    }
    match unplayable {
        Unplayable::Leave => Err(device.unplayable_reason()),
        Unplayable::Copy => Ok(Target::Copy),
        Unplayable::Convert => encode(if t.lossless() { Target::Flac } else { Target::Mp3 }),
    }
}

/// Bytes `t` takes on the card as `target`. MP3 320 is 40 KB a second; a FLAC made from another
/// lossless format is about as large as its source; at CD quality it shrinks with bits × rate
/// (measured: 24/192 → 17%, 24/96 → 32%).
pub fn card_size(t: &Track, target: Target) -> u64 {
    match target {
        Target::Mp3 => (t.duration.as_secs_f64() * 40_000.0) as u64,
        Target::FlacCd => {
            let (bits, rate) = (t.bit_depth.unwrap_or(24).max(16) as u64, t.sample_rate.unwrap_or(96_000).max(1) as u64);
            t.size * 16 * cd_rate(rate as u32) as u64 / (bits * rate)
        }
        _ => t.size,
    }
}

/// Naming and filtering rules for one layout (a card write, or tidying the source).
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Options {
    /// The player the card is for.
    pub device: Device,
    /// What the test card showed, per player (a firmware change is a different player).
    pub tested: BTreeMap<Device, Results>,
    pub album_order: AlbumOrder,
    pub unplayable: Unplayable,
    /// Write hi-res lossless songs at CD quality (16-bit, 44.1/48 kHz) to save space.
    pub cd_quality: bool,
    /// Write all lossless songs as MP3 320: about a quarter of the space of CD-quality FLAC.
    pub lossless_mp3: bool,
    pub write_tags: bool,
    pub clean_mac_files: bool,
    pub singles_folder: String,
    /// The user's naming pattern; `None` names as `album_order`'s pattern does.
    pub pattern: Option<String>,
}

impl Options {
    pub fn player(&self) -> Player {
        Player { device: self.device, tested: self.tested.get(&self.device).map(super::Tested::from_results).unwrap_or_default() }
    }

    /// The pattern names on the card come from: the user's, if it's valid, else the album order's.
    pub fn pattern(&self) -> Pattern {
        self.pattern.as_deref().and_then(|p| Pattern::parse(p).ok()).unwrap_or_else(|| Pattern::parse(default_for(self.album_order)).expect("built-in patterns parse"))
    }

    pub fn pattern_text(&self) -> &str {
        self.pattern.as_deref().unwrap_or(default_for(self.album_order))
    }

    pub fn rules(&self) -> Rules {
        Rules { player: self.player(), unplayable: self.unplayable, cd_quality: self.cd_quality, lossless_mp3: self.lossless_mp3 }
    }
}

impl Default for Options {
    fn default() -> Self {
        Options {
            device: Device::default(),
            tested: BTreeMap::new(),
            album_order: AlbumOrder::Year,
            unplayable: Unplayable::default(),
            cd_quality: false,
            lossless_mp3: false,
            write_tags: true,
            clean_mac_files: true,
            singles_folder: "Singles".into(),
            pattern: None,
        }
    }
}

/// Per-layout choices layered over the library: what goes on the card, what to call it, in what
/// order. Artists are keyed by cluster key, albums by `AlbumNode::id`.
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Custom {
    /// What the user chose to put on the card. None in layouts saved before choosing existed:
    /// then everything goes but `excluded_artists` (see `choose_from_exclusions`).
    pub chosen: Option<Chosen>,
    pub excluded_artists: BTreeSet<String>,
    /// Albums left out of an artist chosen whole (or of everything, in old layouts).
    pub excluded_albums: BTreeSet<String>,
    pub artist_names: BTreeMap<String, String>,
    pub album_names: BTreeMap<String, String>,
    /// Cluster key → album ids in the user's order (for `AlbumOrder::Custom`).
    pub album_order: BTreeMap<String, Vec<String>>,
    /// Album (`AlbumNode::base_id`) → the edition to put on the card, when there are several.
    pub editions: BTreeMap<String, String>,
}

/// The music a write puts on its card, as Calibre sends books to a device: artists taken whole
/// (albums added to the library later too) and single albums, by `AlbumNode::base_id` (of
/// several editions, the one `Custom::editions` picks).
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Chosen {
    pub artists: BTreeSet<String>,
    pub albums: BTreeSet<String>,
}

impl Custom {
    /// Turn an old "everything but these artists" layout into a chosen one that puts the same
    /// music on the card: every other artist, whole. `artists` are the library's cluster keys.
    pub fn choose_from_exclusions<'a>(&mut self, artists: impl IntoIterator<Item = &'a String>) {
        if self.chosen.is_some() {
            return;
        }
        let whole = artists.into_iter().filter(|k| !self.excluded_artists.contains(*k)).cloned().collect();
        self.chosen = Some(Chosen { artists: whole, albums: BTreeSet::new() });
        self.excluded_artists.clear();
    }

    /// Whether the artist is chosen whole.
    pub fn whole_artist(&self, cluster_key: &str) -> bool {
        match &self.chosen {
            Some(c) => c.artists.contains(cluster_key),
            None => !self.excluded_artists.contains(cluster_key),
        }
    }

    /// Whether this write puts the album on the card.
    pub fn includes(&self, cluster_key: &str, album: &AlbumNode, siblings: &[AlbumNode]) -> bool {
        let chosen = self.whole_artist(cluster_key) || self.chosen.as_ref().is_some_and(|c| c.albums.contains(&album.base_id(cluster_key)));
        chosen && self.on_card(cluster_key, album, siblings)
    }

    /// Put an album on the card or take it off. For one of several editions, putting it on
    /// picks that edition.
    pub fn set_album(&mut self, cluster_key: &str, album: &AlbumNode, on: bool) {
        let (id, base) = (album.id(cluster_key), album.base_id(cluster_key));
        let whole = self.whole_artist(cluster_key);
        let c = self.chosen.get_or_insert_with(Default::default);
        if on {
            self.excluded_albums.remove(&id);
            self.excluded_albums.remove(&base);
            if !whole {
                c.albums.insert(base.clone());
            }
            if album.edition.is_some() {
                self.editions.insert(base, id);
            }
        } else {
            c.albums.remove(&base);
            if whole {
                self.excluded_albums.insert(id);
            }
        }
    }

    /// Put an artist on the card whole, all albums, or take all of it off.
    pub fn set_artist(&mut self, cluster_key: &str, albums: &[AlbumNode], on: bool) {
        let c = self.chosen.get_or_insert_with(Default::default);
        for a in albums {
            c.albums.remove(&a.base_id(cluster_key));
            self.excluded_albums.remove(&a.id(cluster_key));
            self.excluded_albums.remove(&a.base_id(cluster_key));
        }
        if on {
            c.artists.insert(cluster_key.to_string());
        } else {
            c.artists.remove(cluster_key);
        }
    }

    /// Whether this write puts the album on the card (its artist aside). Of several editions,
    /// only one goes: the one chosen for this write, else the first-ranked. Exclusions and
    /// names saved before the album had editions (under its base id) still apply.
    pub fn on_card(&self, cluster_key: &str, album: &AlbumNode, siblings: &[AlbumNode]) -> bool {
        let (id, base) = (album.id(cluster_key), album.base_id(cluster_key));
        if self.excluded_albums.contains(&id) || self.excluded_albums.contains(&base) {
            return false;
        }
        let Some(e) = &album.edition else { return true };
        match self.editions.get(&base) {
            Some(chosen) if siblings.iter().any(|s| s.id(cluster_key) == *chosen) => *chosen == id,
            _ => e.rank == 0,
        }
    }

    /// The card name for an album in this write, if renamed.
    pub fn album_name(&self, cluster_key: &str, album: &AlbumNode) -> Option<&str> {
        self.album_names.get(&album.id(cluster_key)).or_else(|| self.album_names.get(&album.base_id(cluster_key))).map(String::as_str)
    }

    /// The number each album's folder gets with `AlbumOrder::Custom` ("02 - …"), 0-based:
    /// titled albums count up, and all editions of an album share one number.
    pub fn positions(&self, cluster_key: &str, ordered: &[&AlbumNode]) -> Vec<usize> {
        let mut next = 0;
        let mut seen: BTreeMap<String, usize> = BTreeMap::new();
        ordered
            .iter()
            .map(|a| {
                if let Some(&p) = seen.get(&a.base_id(cluster_key)) {
                    return p;
                }
                let p = next;
                if a.title.is_some() {
                    seen.insert(a.base_id(cluster_key), p);
                    next += 1;
                }
                p
            })
            .collect()
    }

    /// Albums of an artist in layout order: the user's order first, then the rest as grouped.
    pub fn ordered_albums<'a>(&self, cluster_key: &str, albums: &'a [AlbumNode]) -> Vec<&'a AlbumNode> {
        let mut out: Vec<&AlbumNode> = Vec::with_capacity(albums.len());
        if let Some(order) = self.album_order.get(cluster_key) {
            for id in order {
                if let Some(a) = albums.iter().find(|a| a.id(cluster_key) == *id) {
                    out.push(a);
                }
            }
        }
        for a in albums {
            if !out.iter().any(|o| std::ptr::eq(*o, a)) {
                out.push(a);
            }
        }
        out
    }
}

/// Why a song stored twice in an album is left out (see `AlbumNode::repeats`).
pub const REPEAT: &str = "the same song again";

#[derive(Clone)]
pub struct PlanItem {
    pub track: usize,
    pub rel: PathBuf,
    pub skip: Option<&'static str>,
    pub target: Target,
}

/// Where a song goes on the card, by `pattern`. `artist` is the artist's folder name (as renamed
/// for the write), `title` the album's if renamed, `position` the album's 0-based place among the
/// artist's titled albums.
#[allow(clippy::too_many_arguments)]
pub fn song_path(pattern: &Pattern, t: &Track, album: &AlbumNode, artist: &str, title: Option<&str>, position: usize, opts: &Options, ext: &str) -> PathBuf {
    let titled = album.title.is_some();
    pattern.render(
        |f| match f {
            Field::AlbumArtist => Some(sanitize_component(artist, 80)),
            Field::Album => Some(match title.or(album.title.as_deref()) {
                // Different albums with the same name need different folders: by year, unless the
                // year in the name already does it.
                Some(t) => match &album.variant {
                    Some(v) if !(pattern.uses(Field::Year) && album.year.map(|y| y.to_string()).as_ref() == Some(v)) => format!("{t} ({v})"),
                    _ => t.to_string(),
                },
                None => opts.singles_folder.clone(),
            }),
            Field::Year => album.year.filter(|_| titled).map(|y| y.to_string()),
            Field::Position => titled.then(|| format!("{:02}", position + 1)),
            Field::Disc => album.multi_disc.then(|| t.disc.unwrap_or(1).to_string()),
            Field::Track => t.track.map(|n| format!("{n:02}")),
            Field::TrackArtist => (album.various || !titled).then(|| t.artist.clone()).flatten(),
            Field::Title => Some(t.display_title()),
        },
        ext,
    )
}

/// The plan for reorganising the music folder in place. Songs inside an album file stay with
/// their file and its CUE sheet, so they're left out.
pub fn tidy_plan(tracks: &[Track], index: &Index, opts: &Options) -> Vec<PlanItem> {
    build_plan(tracks, index, opts, &Custom::default()).into_iter().filter(|p| tracks[p.track].cue.is_none()).collect()
}

pub fn build_plan(tracks: &[Track], index: &Index, opts: &Options, custom: &Custom) -> Vec<PlanItem> {
    let mut plan = Vec::with_capacity(tracks.len());
    // FAT/exFAT are case-insensitive: two names differing only in case collide.
    let mut taken: HashSet<String> = HashSet::new();
    let pattern = opts.pattern();
    for artist in &index.artists {
        let ck = &index.clusters[artist.cluster].key;
        let artist_name = custom.artist_names.get(ck).unwrap_or(&artist.name);
        let ordered = custom.ordered_albums(ck, &artist.albums);
        let positions = custom.positions(ck, &ordered);
        for (album, position) in ordered.into_iter().zip(positions) {
            if !custom.includes(ck, album, &artist.albums) {
                continue;
            }
            let title = custom.album_name(ck, album);
            // Songs stored twice go last, so the copy that stays gets the plain name.
            let once = album.tracks.iter().copied().filter(|&i| !album.is_repeat(i));
            for i in once.chain(album.repeats.iter().map(|&(c, _)| c)) {
                let t = &tracks[i];
                let (target, skip) = match target(t, opts.rules()) {
                    _ if album.is_repeat(i) => (Target::Copy, Some(REPEAT)),
                    Ok(to) => (to, None),
                    Err(why) => (Target::Copy, Some(why)),
                };
                let ext = target.ext(t);
                let mut rel = song_path(&pattern, t, album, artist_name, title, position, opts, ext);
                let base = rel.parent().map(PathBuf::from).unwrap_or_default();
                let stem = rel.file_stem().unwrap().to_string_lossy().into_owned();
                let mut n = 2;
                while !taken.insert(rel.to_string_lossy().to_lowercase()) {
                    rel = base.join(format!("{stem} ({n}).{ext}"));
                    n += 1;
                }
                plan.push(PlanItem { track: i, rel, skip, target });
            }
        }
    }
    plan
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::grouping::{ArtistPrefs, build};

    fn at(path: &str, album: &str, title: &str, year: u16) -> Track {
        Track { path: path.into(), artist: Some("Band".into()), album: Some(album.into()), track: Some(1), title: Some(title.into()), year: Some(year), ext: "flac".into(), size: 1, ..Default::default() }
    }

    #[test]
    fn what_unplayable_formats_become() {
        let r = |unplayable| Rules { unplayable, ..Default::default() };
        let m4a = |codec: &str| Track { ext: "m4a".into(), codec: Some(codec.into()), ..Default::default() };
        let flac = Track { ext: "flac".into(), ..Default::default() };
        // What the player plays is copied whatever the setting.
        for u in [Unplayable::Leave, Unplayable::Copy, Unplayable::Convert] {
            assert_eq!(target(&flac, r(u)), Ok(Target::Copy));
        }
        assert!(target(&m4a("mp4a"), r(Unplayable::Leave)).is_err());
        assert_eq!(target(&m4a("mp4a"), r(Unplayable::Copy)), Ok(Target::Copy));
        // A lossy song inside an album file can't be copied as a song of its own.
        let lossy_cue = Track { cue: Some(Default::default()), ..Track { ext: "mp3".into(), ..Default::default() } };
        assert!(target(&lossy_cue, r(Unplayable::Copy)).is_err());
        // Converting needs ffmpeg (and LAME for MP3); without them the reason says so.
        let ff = crate::audio::ffmpeg().is_some();
        assert_eq!(target(&m4a("alac"), r(Unplayable::Convert)).is_ok(), ff);
        if ff {
            assert_eq!(target(&m4a("alac"), r(Unplayable::Convert)), Ok(Target::Flac), "lossless becomes FLAC");
            assert_eq!(target(&m4a("ac-4"), r(Unplayable::Convert)), Err("no decoder for this format"));
            if crate::audio::ffmpeg_encodes("libmp3lame") {
                assert_eq!(target(&m4a("ec-3"), r(Unplayable::Convert)), Ok(Target::Mp3), "lossy becomes MP3, never FLAC");
            }
        }
        assert_eq!(Target::Mp3.ext(&m4a("mp4a")), "mp3");
        let song = Track { duration: std::time::Duration::from_secs(100), size: 9_000_000, ..m4a("mp4a") };
        assert_eq!(card_size(&song, Target::Mp3), 4_000_000);
    }

    #[test]
    fn cd_quality_for_hires() {
        let flac = |bits, rate| Track { ext: "flac".into(), bit_depth: Some(bits), sample_rate: Some(rate), size: 100_000_000, ..Default::default() };
        let cd = Rules { unplayable: Unplayable::Convert, cd_quality: true, ..Default::default() };
        let ff = crate::audio::ffmpeg().is_some();
        assert_eq!(target(&flac(24, 192_000), cd), Ok(if ff { Target::FlacCd } else { Target::Copy }), "only with ffmpeg; else as it is");
        assert_eq!(target(&flac(16, 44_100), cd), Ok(Target::Copy), "already CD quality");
        assert_eq!(target(&flac(24, 192_000), Rules { cd_quality: false, ..cd }), Ok(Target::Copy));
        assert_eq!((cd_rate(192_000), cd_rate(96_000), cd_rate(88_200), cd_rate(44_100)), (48_000, 48_000, 44_100, 44_100));
        assert_eq!(card_size(&flac(24, 192_000), Target::FlacCd), 16_666_666);
        assert_eq!(card_size(&flac(24, 96_000), Target::FlacCd), 33_333_333);
    }

    #[test]
    fn lossless_as_mp3_to_save_space() {
        let song = |ext: &str| Track { ext: ext.into(), bit_depth: Some(24), sample_rate: Some(96_000), duration: std::time::Duration::from_secs(100), size: 100_000_000, ..Default::default() };
        let mp3 = Rules { unplayable: Unplayable::Convert, cd_quality: true, lossless_mp3: true, ..Default::default() };
        let can = crate::audio::ffmpeg().is_some() && crate::audio::ffmpeg_encodes("libmp3lame");
        assert_eq!(target(&song("flac"), mp3), Ok(if can { Target::Mp3 } else { Target::Copy }), "MP3 wins over CD quality; without LAME, as it is");
        assert_eq!(target(&song("mp3"), mp3), Ok(Target::Copy), "lossy songs are never re-encoded");
        assert_eq!(card_size(&song("flac"), Target::Mp3), 4_000_000);
    }

    #[test]
    fn one_edition_goes_on_the_card() {
        let tracks = vec![
            at("/m/Band/UP {1986}/1.flac", "Unknown Pleasures", "Disorder", 1979),
            at("/m/Band/UP {1989}/1.flac", "Unknown Pleasures", "Disorder", 1979),
            at("/m/Band/1999 - Band/1.flac", "Band", "Never Meant", 1999),
            at("/m/Band/2016 - Band/1.flac", "Band", "Where Are We Now", 2016),
        ];
        let index = build(&tracks, &ArtistPrefs::default());
        let opts = Options { album_order: AlbumOrder::Title, ..Default::default() };
        let mut custom = Custom::default();
        let sources = |plan: &[PlanItem]| plan.iter().map(|p| tracks[p.track].path.to_string_lossy().into_owned()).collect::<Vec<_>>();
        let plan = build_plan(&tracks, &index, &opts, &custom);
        assert_eq!(plan.len(), 3);
        assert!(sources(&plan).contains(&"/m/Band/UP {1986}/1.flac".to_string()));
        let rels: Vec<PathBuf> = plan.iter().map(|p| p.rel.clone()).collect();
        assert!(rels.contains(&["Band", "Band (2016)", "01 - Where Are We Now.flac"].iter().collect()), "{rels:?}");

        // Choose the other edition for this write.
        let albums = &index.artists[0].albums;
        let other = albums.iter().find(|a| a.edition.as_ref().is_some_and(|e| e.rank == 1)).unwrap();
        custom.editions.insert(other.base_id("band"), other.id("band"));
        let plan = build_plan(&tracks, &index, &opts, &custom);
        assert!(sources(&plan).contains(&"/m/Band/UP {1989}/1.flac".to_string()));
        assert!(!sources(&plan).contains(&"/m/Band/UP {1986}/1.flac".to_string()));

        // Excluding the album as it was known before it had editions still works.
        custom.excluded_albums.insert(other.base_id("band"));
        assert_eq!(build_plan(&tracks, &index, &opts, &custom).len(), 2);

        // Custom numbering: both editions share one number.
        let ordered: Vec<&AlbumNode> = albums.iter().collect();
        let positions = Custom::default().positions("band", &ordered);
        let up: Vec<usize> = albums.iter().zip(&positions).filter(|(a, _)| a.edition.is_some()).map(|(_, p)| *p).collect();
        assert_eq!(up[0], up[1]);
    }

    #[test]
    fn the_card_holds_what_was_chosen() {
        let tracks = vec![
            at("/m/Band/UP {1986}/1.flac", "Unknown Pleasures", "Disorder", 1979),
            at("/m/Band/UP {1989}/1.flac", "Unknown Pleasures", "Disorder", 1979),
            at("/m/Band/1999 - Band/1.flac", "Band", "Never Meant", 1999),
            Track { artist: Some("Other".into()), ..at("/m/Other/2000 - Solo/1.flac", "Solo", "Alone", 2000) },
        ];
        let index = build(&tracks, &ArtistPrefs::default());
        let opts = Options::default();
        let band = index.artists.iter().find(|a| a.name == "Band").unwrap();
        let keys: Vec<String> = index.clusters.iter().map(|c| c.key.clone()).collect();
        let sources = |c: &Custom| {
            let mut v: Vec<String> = build_plan(&tracks, &index, &opts, c).iter().map(|p| tracks[p.track].path.to_string_lossy().into_owned()).collect();
            v.sort();
            v
        };

        // An old layout that left out "Other" becomes "Band, whole", with the same plan.
        let mut old = Custom::default();
        old.excluded_artists.insert("other".into());
        let before = sources(&old);
        old.choose_from_exclusions(&keys);
        assert_eq!(sources(&old), before);
        assert!(old.whole_artist("band") && !old.whole_artist("other"));

        // A new layout starts empty; one album at a time, then a whole artist.
        let mut c = Custom { chosen: Some(Chosen::default()), ..Default::default() };
        assert!(sources(&c).is_empty());
        let later = band.albums.iter().find(|a| a.edition.as_ref().is_some_and(|e| e.rank == 1)).unwrap();
        c.set_album("band", later, true);
        assert_eq!(sources(&c), ["/m/Band/UP {1989}/1.flac"], "that edition, not the first-ranked");
        c.set_album("band", later, false);
        assert!(sources(&c).is_empty());
        c.set_artist("band", &band.albums, true);
        assert_eq!(sources(&c).len(), 2, "one edition of each album");
        let solo = band.albums.iter().find(|a| a.title.as_deref() == Some("Band")).unwrap();
        c.set_album("band", solo, false);
        assert_eq!(sources(&c).len(), 1, "an album left out of a whole artist");
        c.set_album("band", solo, true);
        assert_eq!(sources(&c).len(), 2);
        c.set_artist("band", &band.albums, false);
        assert!(sources(&c).is_empty());
    }

    #[test]
    fn a_song_stored_twice_goes_once() {
        let copy = |file: &str| Track { size: 100, ..at(&format!("/m/Band/Hits/{file}"), "Hits", "Song", 2000) };
        let tracks = vec![copy("01 Song (2015_05_12 UTC).flac"), copy("01 Song.flac")];
        let index = build(&tracks, &ArtistPrefs::default());
        let plan = build_plan(&tracks, &index, &Options::default(), &Custom::default());
        let rels: Vec<(String, Option<&str>)> = plan.iter().map(|p| (p.rel.file_name().unwrap().to_string_lossy().into_owned(), p.skip)).collect();
        assert_eq!(rels, [("01 - Song.flac".to_string(), None), ("01 - Song (2).flac".to_string(), Some(REPEAT))]);
        assert_eq!(plan[0].track, 1, "the copy with the plain name stays");
    }
}
