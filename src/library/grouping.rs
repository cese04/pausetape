//! Turns a flat list of tracks into Artist → Album → Track, merging artist spelling variants.
//!
//! Variants with the same `artist_key` merge automatically. Near-misses ("Radiohaed") become
//! suggestions the user accepts or rejects; both decisions persist in `ArtistPrefs`.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde::{Deserialize, Serialize};

use crate::library::Track;
use crate::library::names::{artist_key, primary_artist, similar_keys, title_key};

pub const VARIOUS: &str = "Various Artists";
pub const UNKNOWN: &str = "Unknown Artist";

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct ArtistPrefs {
    /// Key pairs the user merged.
    pub merges: Vec<(String, String)>,
    /// Key pairs the user said are different artists.
    pub rejected: BTreeSet<(String, String)>,
    /// Display name chosen for a key (by hand or from MusicBrainz).
    pub names: BTreeMap<String, String>,
}

impl ArtistPrefs {
    pub fn reject(&mut self, a: &str, b: &str) {
        self.rejected.insert(ordered(a, b));
    }
}

fn ordered(a: &str, b: &str) -> (String, String) {
    if a <= b { (a.into(), b.into()) } else { (b.into(), a.into()) }
}

pub struct Cluster {
    /// Key of the most common spelling; used as the cluster's identity.
    pub key: String,
    pub keys: BTreeSet<String>,
    /// Spelling → number of tracks using it.
    pub variants: BTreeMap<String, usize>,
    pub name: String,
}

pub struct Suggestion {
    pub a: usize,
    pub b: usize,
    pub score: f64,
}

pub struct AlbumNode {
    pub title: Option<String>,
    pub year: Option<u16>,
    pub tracks: Vec<usize>,
    pub multi_disc: bool,
    /// Tracks by several artists (compilation): file names carry the track artist.
    pub various: bool,
}

impl AlbumNode {
    /// Stable id for saved layouts, UI state and MusicBrainz replies. Uses the artist cluster's
    /// key rather than its display name, so renaming an artist keeps its albums' ids.
    pub fn id(&self, cluster_key: &str) -> String {
        format!("{cluster_key}|{}", self.title.as_deref().map(title_key).unwrap_or_default())
    }
}

pub struct ArtistNode {
    pub cluster: usize,
    pub name: String,
    pub albums: Vec<AlbumNode>,
}

#[derive(Default)]
pub struct Index {
    pub clusters: Vec<Cluster>,
    pub suggestions: Vec<Suggestion>,
    pub artists: Vec<ArtistNode>,
}

/// Raw artist used for a track's folder, before clustering. `None` means unknown.
fn folder_artist(t: &Track, various: bool) -> Option<String> {
    if various || t.compilation {
        return Some(VARIOUS.to_string());
    }
    t.folder_artist_raw().map(primary_artist).filter(|s| !s.is_empty())
}

/// Albums without an album-artist tag whose tracks span many artists in one folder.
fn detect_untagged_compilations(tracks: &[Track]) -> Vec<bool> {
    let mut groups: HashMap<(Option<&std::path::Path>, String), Vec<usize>> = HashMap::new();
    for (i, t) in tracks.iter().enumerate() {
        if t.album_artist.is_none()
            && let Some(album) = &t.album
        {
            groups.entry((t.path.parent(), title_key(album))).or_default().push(i);
        }
    }
    let mut out = vec![false; tracks.len()];
    for idxs in groups.values() {
        let artists: BTreeSet<String> = idxs
            .iter()
            .filter_map(|&i| tracks[i].artist.as_deref())
            .map(|a| artist_key(&primary_artist(a)))
            .collect();
        if idxs.len() >= 4 && artists.len() >= 3 {
            for &i in idxs {
                out[i] = true;
            }
        }
    }
    out
}

struct UnionFind(Vec<usize>);

impl UnionFind {
    fn find(&mut self, x: usize) -> usize {
        let p = self.0[x];
        if p == x {
            return x;
        }
        let r = self.find(p);
        self.0[x] = r;
        r
    }
    fn union(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        self.0[ra] = rb;
    }
}

fn most_common<T: Ord + Clone, K: Ord>(counts: &BTreeMap<T, usize>, tiebreak: impl Fn(&T) -> K) -> Option<T> {
    counts.iter().max_by_key(|(v, n)| (**n, tiebreak(v))).map(|(v, _)| v.clone())
}

/// Tiebreak between equally common spellings: keep diacritics ("Beyoncé"), capitalisation
/// ("Radiohead" over "radiohead"), natural order ("The Beatles" over "Beatles, The"), and
/// punctuation ("AC/DC" over "ACDC").
fn spelling_quality(v: &str) -> (usize, bool, bool, usize) {
    let non_ascii = v.chars().filter(|c| !c.is_ascii()).count();
    let capitalised = v.chars().next().is_some_and(char::is_uppercase);
    let natural = !v.to_lowercase().ends_with(", the");
    (non_ascii, capitalised, natural, v.len())
}

pub fn build(tracks: &[Track], prefs: &ArtistPrefs) -> Index {
    let various = detect_untagged_compilations(tracks);

    // 1. Exact-key buckets.
    let mut key_ids: HashMap<String, usize> = HashMap::new();
    let mut key_list: Vec<String> = Vec::new();
    let mut key_variants: Vec<BTreeMap<String, usize>> = Vec::new();
    let mut track_key: Vec<usize> = Vec::with_capacity(tracks.len());
    for (i, t) in tracks.iter().enumerate() {
        let spelling = folder_artist(t, various[i]).unwrap_or_else(|| UNKNOWN.to_string());
        let key = artist_key(&spelling);
        let id = *key_ids.entry(key.clone()).or_insert_with(|| {
            key_list.push(key);
            key_variants.push(BTreeMap::new());
            key_list.len() - 1
        });
        *key_variants[id].entry(spelling).or_default() += 1;
        track_key.push(id);
    }

    // 2. User merges.
    let mut uf = UnionFind((0..key_list.len()).collect());
    for (a, b) in &prefs.merges {
        if let (Some(&ia), Some(&ib)) = (key_ids.get(a), key_ids.get(b)) {
            uf.union(ia, ib);
        }
    }

    // 3. Clusters.
    let mut root_cluster: HashMap<usize, usize> = HashMap::new();
    let mut clusters: Vec<Cluster> = Vec::new();
    let mut key_cluster = vec![0; key_list.len()];
    for k in 0..key_list.len() {
        let r = uf.find(k);
        let c = *root_cluster.entry(r).or_insert_with(|| {
            clusters.push(Cluster { key: String::new(), keys: BTreeSet::new(), variants: BTreeMap::new(), name: String::new() });
            clusters.len() - 1
        });
        key_cluster[k] = c;
        clusters[c].keys.insert(key_list[k].clone());
        for (v, n) in &key_variants[k] {
            *clusters[c].variants.entry(v.clone()).or_default() += n;
        }
    }
    for c in &mut clusters {
        let best = most_common(&c.variants, |v| spelling_quality(v)).unwrap_or_default();
        c.key = artist_key(&best);
        c.name = c.keys.iter().find_map(|k| prefs.names.get(k).cloned()).unwrap_or(best);
    }

    // 4. Fuzzy suggestions between clusters (cheap prefilter: same first letter).
    let mut suggestions = Vec::new();
    let mut by_initial: BTreeMap<char, Vec<usize>> = BTreeMap::new();
    for (i, c) in clusters.iter().enumerate() {
        if c.key != artist_key(VARIOUS) && c.key != artist_key(UNKNOWN) {
            by_initial.entry(c.key.chars().next().unwrap_or(' ')).or_default().push(i);
        }
    }
    for group in by_initial.values() {
        for (n, &a) in group.iter().enumerate() {
            for &b in &group[n + 1..] {
                let (ka, kb) = (&clusters[a].key, &clusters[b].key);
                if prefs.rejected.contains(&ordered(ka, kb)) {
                    continue;
                }
                if let Some(score) = similar_keys(ka, kb) {
                    suggestions.push(Suggestion { a, b, score });
                }
            }
        }
    }
    suggestions.sort_by(|x, y| y.score.total_cmp(&x.score));

    // 5. Albums per cluster.
    let mut per_cluster: BTreeMap<usize, BTreeMap<String, Vec<usize>>> = BTreeMap::new();
    for (i, t) in tracks.iter().enumerate() {
        let c = key_cluster[track_key[i]];
        let album_key = t.album.as_deref().map(title_key).unwrap_or_default();
        per_cluster.entry(c).or_default().entry(album_key).or_default().push(i);
    }
    let mut artists: Vec<ArtistNode> = per_cluster
        .into_iter()
        .map(|(c, albums)| {
            let mut albums: Vec<AlbumNode> = albums
                .into_values()
                .map(|mut idxs| {
                    let mut titles = BTreeMap::new();
                    let mut years = BTreeMap::new();
                    let mut track_artists = BTreeSet::new();
                    for &i in &idxs {
                        if let Some(a) = &tracks[i].album {
                            *titles.entry(a.clone()).or_insert(0) += 1;
                        }
                        if let Some(y) = tracks[i].year {
                            *years.entry(y).or_insert(0) += 1;
                        }
                        if let Some(a) = &tracks[i].artist {
                            track_artists.insert(artist_key(&primary_artist(a)));
                        }
                    }
                    idxs.sort_by(|&a, &b| {
                        let (ta, tb) = (&tracks[a], &tracks[b]);
                        (ta.disc.unwrap_or(1), ta.track.unwrap_or(u32::MAX), ta.display_title())
                            .cmp(&(tb.disc.unwrap_or(1), tb.track.unwrap_or(u32::MAX), tb.display_title()))
                    });
                    AlbumNode {
                        title: most_common(&titles, |_| 0),
                        year: most_common(&years, |y| (u16::MAX - y) as usize),
                        multi_disc: idxs.iter().any(|&i| tracks[i].disc.unwrap_or(1) > 1),
                        various: clusters[c].key == artist_key(VARIOUS) || track_artists.len() > 1,
                        tracks: idxs,
                    }
                })
                .collect();
            // Albums chronologically, loose tracks ("Singles") last.
            albums.sort_by_key(|a| (a.title.is_none(), a.year.unwrap_or(u16::MAX), a.title.as_deref().map(title_key)));
            ArtistNode { cluster: c, name: clusters[c].name.clone(), albums }
        })
        .collect();
    artists.sort_by_key(|a| artist_key(&a.name));

    Index { clusters, suggestions, artists }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(artist: &str, album: &str) -> Track {
        Track { artist: Some(artist.into()), album: Some(album.into()), ..Default::default() }
    }

    #[test]
    fn variants_merge_and_typos_are_suggested() {
        let tracks = vec![t("The Beatles", "Abbey Road"), t("Beatles", "Abbey Road"), t("Radiohead", "OK"), t("Radiohaed", "OK")];
        let idx = build(&tracks, &ArtistPrefs::default());
        assert_eq!(idx.clusters.len(), 3);
        assert_eq!(idx.suggestions.len(), 1);
        let beatles = idx.artists.iter().find(|a| a.name.contains("Beatles")).unwrap();
        assert_eq!(beatles.albums[0].tracks.len(), 2);
    }

    #[test]
    fn accepted_merge_joins_clusters() {
        let tracks = vec![t("Radiohead", "OK"), t("Radiohead", "OK"), t("Radiohaed", "OK")];
        let prefs = ArtistPrefs { merges: vec![(artist_key("Radiohead"), artist_key("Radiohaed"))], ..Default::default() };
        let idx = build(&tracks, &prefs);
        assert_eq!(idx.artists.len(), 1);
        assert_eq!(idx.artists[0].name, "Radiohead");
        assert!(idx.suggestions.is_empty());
    }
}
