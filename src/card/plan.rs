//! The layout plan: which file goes where on the card (or in a tidied music folder).
//!
//! RECHO sorts entries by name, so ordering is encoded in names: albums get a year prefix
//! ("1997 - OK Computer") or a custom position ("02 - OK Computer"), and tracks a zero-padded
//! number ("1-03 - Title.flac" on multi-disc sets).

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

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

/// What a layout does with songs in formats RECHO can't play (AAC, ALAC, Dolby, Opus…).
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
    /// Encoded to FLAC with ffmpeg: lossless sources RECHO can't read (ALAC, AIFF, APE…), and
    /// songs cut out of a lossless album file.
    Flac,
    /// Encoded to FLAC at CD quality: 16-bit, 44.1 or 48 kHz (`cd_rate`), dithered. For hi-res
    /// lossless songs when the write asks to save space.
    FlacCd,
    /// Encoded to MP3 at 320 kbps, stereo: lossy sources RECHO can't read (AAC, Dolby, Opus…).
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
#[derive(Clone, Copy, Debug)]
pub struct Rules {
    pub unplayable: Unplayable,
    /// Write hi-res lossless songs at CD quality to save space.
    pub cd_quality: bool,
}

/// More than CD quality: over 16 bits or 48 kHz.
pub fn hires(t: &Track) -> bool {
    t.bit_depth.is_some_and(|b| b > 16) || t.sample_rate.is_some_and(|r| r > 48_000)
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
/// conversion. CD quality is a preference: without ffmpeg, hi-res songs go as they are.
pub fn target(t: &Track, rules: Rules) -> Result<Target, &'static str> {
    let to = base_target(t, rules.unplayable)?;
    let can_cd = crate::audio::ffmpeg().is_some() && t.ffmpeg_can_decode();
    Ok(match to {
        Target::Copy | Target::Flac if rules.cd_quality && t.lossless() && hires(t) && can_cd => Target::FlacCd,
        to => to,
    })
}

fn base_target(t: &Track, unplayable: Unplayable) -> Result<Target, &'static str> {
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
    if t.on_recho() {
        return Ok(Target::Copy);
    }
    match unplayable {
        Unplayable::Leave => Err("format not playable on RECHO"),
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
    pub album_order: AlbumOrder,
    pub unplayable: Unplayable,
    /// Write hi-res lossless songs at CD quality (16-bit, 44.1/48 kHz) to save space.
    pub cd_quality: bool,
    pub write_tags: bool,
    pub clean_mac_files: bool,
    pub singles_folder: String,
}

impl Options {
    pub fn rules(&self) -> Rules {
        Rules { unplayable: self.unplayable, cd_quality: self.cd_quality }
    }
}

impl Default for Options {
    fn default() -> Self {
        Options {
            album_order: AlbumOrder::Year,
            unplayable: Unplayable::default(),
            cd_quality: false,
            write_tags: true,
            clean_mac_files: true,
            singles_folder: "Singles".into(),
        }
    }
}

/// Per-layout choices layered over the library: what to leave out, what to call it, in what order.
/// Artists are keyed by cluster key, albums by `AlbumNode::id`.
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Custom {
    pub excluded_artists: BTreeSet<String>,
    pub excluded_albums: BTreeSet<String>,
    pub artist_names: BTreeMap<String, String>,
    pub album_names: BTreeMap<String, String>,
    /// Cluster key → album ids in the user's order (for `AlbumOrder::Custom`).
    pub album_order: BTreeMap<String, Vec<String>>,
    /// Album (`AlbumNode::base_id`) → the edition to put on the card, when there are several.
    pub editions: BTreeMap<String, String>,
}

impl Custom {
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

#[derive(Clone)]
pub struct PlanItem {
    pub track: usize,
    pub rel: PathBuf,
    pub skip: Option<&'static str>,
    pub target: Target,
}

/// `position` is the album's 0-based place among the artist's titled albums.
pub fn album_folder(album: &AlbumNode, title: Option<&str>, position: usize, opts: &Options) -> String {
    let Some(t) = title.or(album.title.as_deref()) else {
        return sanitize_component(&opts.singles_folder, 100);
    };
    // Different albums with the same name need different folders: by year, unless the year
    // prefix already does it.
    let same_name = album.variant.as_ref().filter(|v| !(opts.album_order == AlbumOrder::Year && album.year.map(|y| y.to_string()).as_ref() == Some(v)));
    let with_variant;
    let t = match same_name {
        Some(v) => {
            with_variant = format!("{t} ({v})");
            with_variant.as_str()
        }
        None => t,
    };
    let name = match (opts.album_order, album.year) {
        (AlbumOrder::Year, Some(y)) => format!("{y} - {t}"),
        (AlbumOrder::Custom, _) => format!("{:02} - {t}", position + 1),
        _ => t.to_string(),
    };
    sanitize_component(&name, 100)
}

pub fn file_name(t: &Track, album: &AlbumNode, ext: &str) -> String {
    let mut name = String::new();
    if let Some(n) = t.track {
        if album.multi_disc {
            name.push_str(&format!("{}-{n:02} - ", t.disc.unwrap_or(1)));
        } else {
            name.push_str(&format!("{n:02} - "));
        }
    }
    if album.various || album.title.is_none() {
        if let Some(a) = &t.artist {
            name.push_str(&format!("{a} - "));
        }
    }
    name.push_str(&t.display_title());
    format!("{}.{ext}", sanitize_component(&name, 120))
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
    for artist in &index.artists {
        let ck = &index.clusters[artist.cluster].key;
        if custom.excluded_artists.contains(ck) {
            continue;
        }
        let artist_dir = sanitize_component(custom.artist_names.get(ck).unwrap_or(&artist.name), 80);
        let ordered = custom.ordered_albums(ck, &artist.albums);
        let positions = custom.positions(ck, &ordered);
        for (album, position) in ordered.into_iter().zip(positions) {
            if !custom.on_card(ck, album, &artist.albums) {
                continue;
            }
            let album_dir = album_folder(album, custom.album_name(ck, album), position, opts);
            for &i in &album.tracks {
                let t = &tracks[i];
                let (target, skip) = match target(t, opts.rules()) {
                    Ok(to) => (to, None),
                    Err(why) => (Target::Copy, Some(why)),
                };
                let ext = target.ext(t);
                let base = PathBuf::from(&artist_dir).join(&album_dir);
                let mut rel = base.join(file_name(t, album, ext));
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
        let r = |unplayable| Rules { unplayable, cd_quality: false };
        let m4a = |codec: &str| Track { ext: "m4a".into(), codec: Some(codec.into()), ..Default::default() };
        let flac = Track { ext: "flac".into(), ..Default::default() };
        // RECHO's own formats are copied whatever the setting.
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
        let cd = Rules { unplayable: Unplayable::Convert, cd_quality: true };
        let ff = crate::audio::ffmpeg().is_some();
        assert_eq!(target(&flac(24, 192_000), cd), Ok(if ff { Target::FlacCd } else { Target::Copy }), "only with ffmpeg; else as it is");
        assert_eq!(target(&flac(16, 44_100), cd), Ok(Target::Copy), "already CD quality");
        assert_eq!(target(&flac(24, 192_000), Rules { cd_quality: false, ..cd }), Ok(Target::Copy));
        assert_eq!((cd_rate(192_000), cd_rate(96_000), cd_rate(88_200), cd_rate(44_100)), (48_000, 48_000, 44_100, 44_100));
        assert_eq!(card_size(&flac(24, 192_000), Target::FlacCd), 16_666_666);
        assert_eq!(card_size(&flac(24, 96_000), Target::FlacCd), 33_333_333);
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
}
