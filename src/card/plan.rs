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

/// Naming and filtering rules for one layout (a card write, or tidying the source).
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Options {
    pub album_order: AlbumOrder,
    pub skip_unplayable: bool,
    pub write_tags: bool,
    pub clean_mac_files: bool,
    pub singles_folder: String,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            album_order: AlbumOrder::Year,
            skip_unplayable: true,
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

pub fn file_name(t: &Track, album: &AlbumNode) -> String {
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
    format!("{}.{}", sanitize_component(&name, 120), t.card_ext())
}

/// Why a track can't go on the card at all, not even with "copy anyway". Only songs inside an
/// album file (CUE): they're cut out with ffmpeg, losslessly, so a lossy album file is left out.
pub fn cannot_write(t: &Track) -> Option<&'static str> {
    t.cue.as_ref()?;
    if !t.on_recho() {
        Some("inside a lossy album file; can't be cut out without re-encoding")
    } else if crate::audio::ffmpeg().is_none() {
        Some("needs ffmpeg to cut the album file into songs")
    } else {
        None
    }
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
                let base = PathBuf::from(&artist_dir).join(&album_dir);
                let mut rel = base.join(file_name(t, album));
                let stem = rel.file_stem().unwrap().to_string_lossy().into_owned();
                let mut n = 2;
                while !taken.insert(rel.to_string_lossy().to_lowercase()) {
                    rel = base.join(format!("{stem} ({n}).{}", t.card_ext()));
                    n += 1;
                }
                let skip = cannot_write(t).or((opts.skip_unplayable && !t.on_recho()).then_some("format not playable on RECHO"));
                plan.push(PlanItem { track: i, rel, skip });
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
