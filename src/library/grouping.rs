//! Turns a flat list of tracks into Artist → Album → Track, merging artist spelling variants.
//!
//! Variants with the same `artist_key` merge automatically. Near-misses ("Radiohaed") become
//! suggestions the user accepts or rejects; both decisions persist in `ArtistPrefs`.
//!
//! Tracks with the same album name are split by folder: copies that share their songs are
//! *editions* of one album (two rips, a reissue), copies that don't are different albums with
//! the same name (a band's self-titled first and second albums).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::library::Track;
use crate::library::names::{artist_key, clean_album, disc_folder, primary_artist, similar_keys, title_key};

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
    /// Tells apart different albums with the same name: their year (or folder, if that's the
    /// same too). `None` when the name is unique for the artist.
    pub variant: Option<String>,
    /// One of several copies of the same album.
    pub edition: Option<Edition>,
    /// Songs stored twice in this album, as (extra copy, the copy that stays); see
    /// `repeated_songs`. The card gets each song once.
    pub repeats: Vec<(usize, usize)>,
}

/// A copy of an album when there are several: different releases or rips of the same songs.
#[derive(Clone, Debug)]
pub struct Edition {
    /// Stable within the album: from the copy's folder name.
    pub key: String,
    /// What tells it apart, for people: the folder's edition details ("1986, UK, Factory
    /// Records, FACD 10"), else the folder name.
    pub label: String,
    /// 0 = the edition a write uses unless told otherwise (the most complete one in formats every player plays).
    pub rank: usize,
    pub count: usize,
    /// "FLAC 16/44.1", "MP3 320".
    pub format: String,
    /// Another edition has all of this one's songs in the same or higher quality, so this copy
    /// could go.
    pub superseded: Option<Superseded>,
}

#[derive(Clone, Debug)]
pub struct Superseded {
    /// The better (or equal) edition: its rank and label.
    pub rank: usize,
    pub label: String,
    pub format: String,
    /// Higher quality, rather than the same.
    pub higher: bool,
}

/// What an edition's audio is: lossless with its resolution, or lossy with its bitrate.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Quality {
    pub lossless: bool,
    pub bit_depth: u8,
    pub sample_rate: u32,
    /// Median over the songs (lossy files are often variable bitrate).
    pub kbps: u32,
}

impl Quality {
    fn of(tracks: &[Track], idxs: &[usize]) -> Quality {
        let songs = || idxs.iter().map(|&i| &tracks[i]);
        let mut kbps: Vec<u32> = songs().filter_map(|t| t.kbps).collect();
        kbps.sort();
        Quality {
            // A copy is only as good as its weakest file.
            lossless: songs().all(Track::lossless),
            bit_depth: songs().map(|t| t.bit_depth.unwrap_or(0)).min().unwrap_or(0),
            sample_rate: songs().map(|t| t.sample_rate.unwrap_or(0)).min().unwrap_or(0),
            kbps: kbps.get(kbps.len() / 2).copied().unwrap_or(0),
        }
    }

    /// Lossless beats lossy; lossless copies compare by bit depth, then sample rate; lossy ones
    /// by bitrate, within 10% counting as the same.
    pub fn compare(&self, other: &Quality) -> std::cmp::Ordering {
        use std::cmp::Ordering::*;
        match (self.lossless, other.lossless) {
            (true, false) => Greater,
            (false, true) => Less,
            (true, true) => (self.bit_depth, self.sample_rate).cmp(&(other.bit_depth, other.sample_rate)),
            (false, false) if self.kbps.abs_diff(other.kbps) * 10 <= self.kbps.max(other.kbps) => Equal,
            (false, false) => self.kbps.cmp(&other.kbps),
        }
    }

    /// "FLAC 16/44.1" or "MP3 320", given the format's name.
    pub fn label(&self, format: &str) -> String {
        let khz = |hz: u32| if hz % 1000 == 0 { (hz / 1000).to_string() } else { format!("{:.1}", hz as f32 / 1000.0) };
        match (self.lossless, self.bit_depth, self.sample_rate, self.kbps) {
            (true, b, r, _) if b > 0 && r > 0 => format!("{format} {b}/{}", khz(r)),
            (false, _, _, k) if k > 0 => format!("{format} {k}"),
            _ => format.to_string(),
        }
    }
}

impl AlbumNode {
    /// An extra copy of a song this album already has.
    pub fn is_repeat(&self, track: usize) -> bool {
        self.repeats.iter().any(|&(c, _)| c == track)
    }

    /// Stable id for saved layouts, UI state and MusicBrainz replies. Uses the artist cluster's
    /// key rather than its display name, so renaming an artist keeps its albums' ids.
    pub fn id(&self, cluster_key: &str) -> String {
        match &self.edition {
            Some(e) => format!("{}#{}", self.base_id(cluster_key), e.key),
            None => self.base_id(cluster_key),
        }
    }

    /// The id shared by all editions of the album (equal to `id` when there's one copy).
    pub fn base_id(&self, cluster_key: &str) -> String {
        let mut id = format!("{cluster_key}|{}", self.title.as_deref().map(title_key).unwrap_or_default());
        if let Some(v) = &self.variant {
            id.push('|');
            id.push_str(v);
        }
        id
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

impl Index {
    /// Editions that another edition covers in the same or higher quality, as (artist, album)
    /// positions: copies that could go.
    pub fn superseded(&self) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        for (ai, a) in self.artists.iter().enumerate() {
            for (bi, al) in a.albums.iter().enumerate() {
                if al.edition.as_ref().is_some_and(|e| e.superseded.is_some()) {
                    out.push((ai, bi));
                }
            }
        }
        out
    }

    /// Songs stored twice, over all albums: (extra copy, the copy that stays).
    pub fn repeats(&self) -> Vec<(usize, usize)> {
        self.artists.iter().flat_map(|a| &a.albums).flat_map(|al| al.repeats.iter().copied()).collect()
    }
}

/// Raw artist used for a track's folder, before clustering. `None` means unknown.
fn folder_artist(t: &Track, various: bool) -> Option<String> {
    if various || t.compilation {
        return Some(VARIOUS.to_string());
    }
    t.folder_artist_raw().map(primary_artist).filter(|s| !s.is_empty())
}

/// Keep each album in one artist's folder. Songs of one album (same name, same folder) can
/// point at different artists: a guest on one song without an album-artist tag, a compilation
/// flag on some songs only, a split release. They all go under the album artist when the songs
/// that have one agree on it and are at least half the album; else under an artist that holds
/// three quarters of the songs; else under Various Artists. `merges` are the user's: artists
/// merged by hand count as one.
fn keep_albums_together(tracks: &[Track], spellings: &mut [String], merges: &[(String, String)]) {
    let mut root: HashMap<String, String> = HashMap::new();
    fn find(root: &HashMap<String, String>, k: &str) -> String {
        let mut k = k.to_string();
        while let Some(p) = root.get(&k) {
            k = p.clone();
        }
        k
    }
    for (a, b) in merges {
        let (ra, rb) = (find(&root, a), find(&root, b));
        if ra != rb {
            root.insert(ra, rb);
        }
    }
    let who = |spelling: &str| find(&root, &artist_key(spelling));
    let mut albums: HashMap<(PathBuf, String), Vec<usize>> = HashMap::new();
    for (i, t) in tracks.iter().enumerate() {
        if let Some(album) = t.album.as_deref().map(title_key).filter(|k| !k.is_empty()) {
            albums.entry((album_dir(&t.path), album)).or_default().push(i);
        }
    }
    for idxs in albums.values() {
        let mut folders: HashMap<String, (usize, &str)> = HashMap::new();
        for &i in idxs {
            folders.entry(who(&spellings[i])).or_insert((0, &spellings[i])).0 += 1;
        }
        if folders.len() < 2 {
            continue;
        }
        let mut tagged: HashMap<String, (usize, String)> = HashMap::new();
        for &i in idxs {
            if let Some(a) = tracks[i].album_artist.as_deref().map(primary_artist).filter(|a| !a.is_empty()) {
                tagged.entry(who(&a)).or_insert((0, a)).0 += 1;
            }
        }
        let to = match (tagged.len(), tagged.values().next()) {
            (1, Some((n, a))) if n * 2 >= idxs.len() => a.clone(),
            _ => match folders.values().max_by_key(|(n, _)| *n) {
                Some((n, a)) if n * 4 >= idxs.len() * 3 => a.to_string(),
                _ => VARIOUS.to_string(),
            },
        };
        for &i in idxs {
            spellings[i] = to.clone();
        }
    }
}

/// Songs of an album that repeat another of its songs: the same disc and number (or title),
/// size and length, as when a backup tool kept two copies ("Song (2015_05_12 05_18_04
/// UTC).mp3"), as (extra copy, the copy that stays). Of each set the shortest file name stays.
fn repeated_songs(tracks: &[Track], idxs: &[usize]) -> Vec<(usize, usize)> {
    // Disc, number, title (when unnumbered), size, length in ms.
    type Same = (u32, Option<u32>, String, u64, u128);
    let mut same: BTreeMap<Same, Vec<usize>> = BTreeMap::new();
    for &i in idxs.iter().filter(|&&i| tracks[i].cue.is_none()) {
        let t = &tracks[i];
        let title = if t.track.is_none() { title_key(&t.display_title()) } else { String::new() };
        same.entry((t.disc.unwrap_or(1), t.track, title, t.size, t.duration.as_millis())).or_default().push(i);
    }
    let mut out = Vec::new();
    for mut set in same.into_values().filter(|s| s.len() > 1) {
        set.sort_by_key(|&i| (tracks[i].path.file_name().map_or(0, |n| n.len()), tracks[i].path.clone()));
        out.extend(set[1..].iter().map(|&i| (i, set[0])));
    }
    out.sort();
    out
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
/// ("Radiohead" over "radiohead", "Joy Division" over a CUE sheet's "JOY DIVISION"), natural
/// order ("The Beatles" over "Beatles, The"), and punctuation ("AC/DC" over "ACDC").
fn spelling_quality(v: &str) -> (usize, bool, bool, bool, usize) {
    let non_ascii = v.chars().filter(|c| !c.is_ascii()).count();
    let capitalised = v.chars().next().is_some_and(char::is_uppercase);
    let not_shouting = v.chars().any(char::is_lowercase);
    let natural = !v.to_lowercase().ends_with(", the");
    (non_ascii, capitalised, not_shouting, natural, v.len())
}

/// One copy of an album: the folder(s) holding it and its tracks.
struct Copy {
    dirs: Vec<PathBuf>,
    tracks: Vec<usize>,
}

impl Copy {
    /// A key from the copy's folder name, stable across rescans.
    fn key(&self) -> String {
        self.dirs[0].file_name().map(|n| title_key(&n.to_string_lossy())).filter(|k| !k.is_empty()).unwrap_or_else(|| "top".into())
    }
}

/// A song title for telling whether two copies hold the same song: without bracketed notes or
/// anything after " - " or " / " ("The Need For Repetition / The Man Land (Hidden Track)" →
/// "the need for repetition"), as a `title_key`. Spelling slips are left to a fuzzy comparison.
fn song_key(title: &str) -> String {
    let cut = [" - ", " / "].iter().filter_map(|sep| title.find(sep)).min().unwrap_or(title.len());
    let main = Some(&title[..cut]).filter(|m| !m.trim().is_empty()).unwrap_or(title);
    let mut out = String::new();
    let mut depth = 0;
    for c in main.chars() {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = (depth - 1).max(0),
            c if depth == 0 => out.push(c),
            _ => {}
        }
    }
    let key = title_key(&out);
    if key.is_empty() { title_key(title) } else { key }
}

/// The folder an album lives in: the track's own, or the one above a "CD 1" folder.
fn album_dir(path: &Path) -> PathBuf {
    let Some(dir) = path.parent() else { return PathBuf::new() };
    match dir.file_name() {
        Some(n) if disc_folder(&n.to_string_lossy()).is_some() => dir.parent().unwrap_or(dir).to_path_buf(),
        _ => dir.to_path_buf(),
    }
}

/// Tracks sharing an album name, as albums → copies. Folders that complete each other (no
/// shared track positions or titles, like discs kept in separate folders) form one copy;
/// copies sharing at least half their songs are editions of one album.
fn split_copies(tracks: &[Track], idxs: Vec<usize>) -> Vec<Vec<Copy>> {
    let mut by_dir: BTreeMap<PathBuf, Vec<usize>> = BTreeMap::new();
    for i in idxs {
        by_dir.entry(album_dir(&tracks[i].path)).or_default().push(i);
    }
    if by_dir.len() == 1 {
        let (dir, tracks) = by_dir.into_iter().next().unwrap();
        return vec![vec![Copy { dirs: vec![dir], tracks }]];
    }
    let positions = |c: &[usize]| -> BTreeSet<(u32, u32)> { c.iter().filter_map(|&i| Some((tracks[i].disc.unwrap_or(1), tracks[i].track?))).collect() };
    let titles = |c: &[usize]| -> BTreeSet<String> { c.iter().map(|&i| title_key(&tracks[i].display_title())).collect() };
    let mut copies: Vec<Copy> = Vec::new();
    for (dir, t) in by_dir {
        let (p, ti) = (positions(&t), titles(&t));
        match copies.iter_mut().find(|c| positions(&c.tracks).is_disjoint(&p) && titles(&c.tracks).is_disjoint(&ti)) {
            Some(c) => {
                c.dirs.push(dir);
                c.tracks.extend(t);
            }
            None => copies.push(Copy { dirs: vec![dir], tracks: t }),
        }
    }
    let mut albums: Vec<Vec<Copy>> = Vec::new();
    for c in copies {
        let ct = titles(&c.tracks);
        let same = albums.iter_mut().find(|a| {
            a.iter().any(|o| {
                let ot = titles(&o.tracks);
                let shared = ot.intersection(&ct).count();
                shared > 0 && shared * 2 >= ot.len().min(ct.len())
            })
        });
        match same {
            Some(a) => a.push(c),
            None => albums.push(vec![c]),
        }
    }
    albums
}

/// "FLAC 16/44.1", "MP3 320": the songs' most common format, at the quality of the weakest.
pub fn describe(tracks: &[Track], idxs: &[usize]) -> (String, Quality) {
    let q = Quality::of(tracks, idxs);
    let mut names: BTreeMap<String, usize> = BTreeMap::new();
    for &i in idxs {
        *names.entry(if tracks[i].cue.is_some() { tracks[i].ext.to_uppercase() } else { tracks[i].format_label() }).or_default() += 1;
    }
    (q.label(&most_common(&names, |_| 0).unwrap_or_default()), q)
}

/// Editions in rank order. First: the copy with the most lossless songs in FLAC, MP3 or WAV (a
/// FLAC vinyl rip over an MP3 with bonus tracks), then the most songs in those, then the most
/// songs, then the highest quality, then the one not needing a CUE sheet cut, then by folder.
/// Each edition also notes whether another has all its songs, and no less music, in the same or
/// higher quality.
fn editions(tracks: &[Track], mut copies: Vec<Copy>) -> Vec<(Copy, Edition)> {
    copies.sort_by_key(|c| {
        let count = |f: &dyn Fn(&Track) -> bool| usize::MAX - c.tracks.iter().filter(|&&i| f(&tracks[i])).count();
        let cue = c.tracks.iter().filter(|&&i| tracks[i].cue.is_some()).count();
        let q = Quality::of(tracks, &c.tracks);
        let quality = (!q.lossless, u8::MAX - q.bit_depth, u32::MAX - q.sample_rate, u32::MAX - q.kbps);
        (count(&|t| t.plays_anywhere() && t.lossless()), count(&|t| t.plays_anywhere()), usize::MAX - c.tracks.len(), quality, cue, c.dirs[0].clone())
    });
    let quality: Vec<Quality> = copies.iter().map(|c| Quality::of(tracks, &c.tracks)).collect();
    let format: Vec<String> = copies.iter().map(|c| describe(tracks, &c.tracks).0).collect();
    let songs: Vec<Vec<String>> = copies.iter().map(|c| c.tracks.iter().map(|&i| song_key(&tracks[i].display_title())).collect()).collect();
    // Also by length: the same titles can hide more music (a hidden track at the end).
    let length: Vec<f64> = copies.iter().map(|c| c.tracks.iter().map(|&i| tracks[i].duration.as_secs_f64()).sum()).collect();
    let covers = |j: usize, i: usize| {
        songs[i].iter().all(|a| songs[j].iter().any(|b| a == b || strsim::jaro_winkler(a, b) >= 0.92))
            && length[i] <= length[j] + (length[j] * 0.03).max(30.0)
    };
    let name = |d: &Path| d.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mut labels: Vec<String> = copies
        .iter()
        .map(|c| {
            let n = name(&c.dirs[0]);
            let extras = clean_album(&n, None).extras;
            if extras.is_empty() { n } else { extras.join(", ") }
        })
        .collect();
    // Same label twice (e.g. two rips in folders of the same name): add the folder above.
    for i in 0..labels.len() {
        if labels.iter().filter(|l| **l == labels[i]).count() > 1 {
            let parent = copies[i].dirs[0].parent().map(name).unwrap_or_default();
            labels[i] = format!("{} · {parent}", labels[i]);
        }
    }
    // Superseded: another edition (the best-ranked such) has every song of this one, in higher
    // quality, or in the same quality and ranked ahead.
    let superseded: Vec<Option<Superseded>> = (0..copies.len())
        .map(|i| {
            (0..copies.len())
                .filter(|&j| j != i && covers(j, i))
                .find_map(|j| match quality[j].compare(&quality[i]) {
                    std::cmp::Ordering::Greater => Some((j, true)),
                    std::cmp::Ordering::Equal if j < i => Some((j, false)),
                    _ => None,
                })
                .map(|(j, higher)| Superseded { rank: j, label: labels[j].clone(), format: format[j].clone(), higher })
        })
        .collect();
    let count = copies.len();
    let mut keys: Vec<String> = Vec::new();
    copies
        .into_iter()
        .zip(labels.into_iter().zip(format.into_iter().zip(superseded)))
        .enumerate()
        .map(|(rank, (c, (label, (format, superseded))))| {
            let mut key = c.key();
            if keys.contains(&key) {
                key = format!("{key}-{}", keys.len() + 1);
            }
            keys.push(key.clone());
            (c, Edition { key, label, rank, count, format, superseded })
        })
        .collect()
}

pub fn build(tracks: &[Track], prefs: &ArtistPrefs) -> Index {
    let various = detect_untagged_compilations(tracks);
    let mut spellings: Vec<String> = tracks.iter().enumerate().map(|(i, t)| folder_artist(t, various[i]).unwrap_or_else(|| UNKNOWN.to_string())).collect();
    keep_albums_together(tracks, &mut spellings, &prefs.merges);

    // 1. Exact-key buckets.
    let mut key_ids: HashMap<String, usize> = HashMap::new();
    let mut key_list: Vec<String> = Vec::new();
    let mut key_variants: Vec<BTreeMap<String, usize>> = Vec::new();
    let mut track_key: Vec<usize> = Vec::with_capacity(tracks.len());
    for spelling in spellings {
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
            let node = |mut idxs: Vec<usize>| {
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
                    repeats: repeated_songs(tracks, &idxs),
                    tracks: idxs,
                    variant: None,
                    edition: None,
                }
            };
            let mut nodes = Vec::new();
            for (key, idxs) in albums {
                if key.is_empty() {
                    nodes.push(node(idxs));
                    continue;
                }
                let groups = split_copies(tracks, idxs);
                let several = groups.len() > 1;
                let mut variants = BTreeSet::new();
                for copies in groups {
                    let all: Vec<usize> = copies.iter().flat_map(|c| c.tracks.iter().copied()).collect();
                    // Same name, different albums: tell them apart by year, else by folder.
                    let variant = several.then(|| {
                        let year = node(all.clone()).year.map(|y| y.to_string());
                        let v = year.filter(|y| !variants.contains(y)).unwrap_or_else(|| copies[0].key());
                        let v = if variants.contains(&v) { format!("{v}-{}", variants.len() + 1) } else { v };
                        variants.insert(v.clone());
                        v
                    });
                    if copies.len() == 1 {
                        nodes.push(AlbumNode { variant, ..node(all) });
                        continue;
                    }
                    for (copy, edition) in editions(tracks, copies) {
                        nodes.push(AlbumNode { variant: variant.clone(), edition: Some(edition), ..node(copy.tracks) });
                    }
                }
            }
            // Albums chronologically, loose tracks ("Singles") last; editions together.
            nodes.sort_by_key(|a| {
                (a.title.is_none(), a.year.unwrap_or(u16::MAX), a.title.as_deref().map(title_key), a.variant.clone(), a.edition.as_ref().map(|e| e.rank))
            });
            ArtistNode { cluster: c, name: clusters[c].name.clone(), albums: nodes }
        })
        .collect();
    artists.sort_by_key(|a| artist_key(&a.name));

    Index { clusters, suggestions, artists }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(artist: &str, album: &str) -> Track {
        Track { path: format!("/m/{album}/{artist}.mp3").into(), artist: Some(artist.into()), album: Some(album.into()), ..Default::default() }
    }

    #[test]
    fn variants_merge_and_typos_are_suggested() {
        let tracks = vec![t("The Beatles", "Abbey Road"), t("Beatles", "Abbey Road"), t("Radiohead", "OK"), t("Radiohaed", "Kid A")];
        let idx = build(&tracks, &ArtistPrefs::default());
        assert_eq!(idx.clusters.len(), 3);
        assert_eq!(idx.suggestions.len(), 1);
        let beatles = idx.artists.iter().find(|a| a.name.contains("Beatles")).unwrap();
        assert_eq!(beatles.albums[0].tracks.len(), 2);
    }

    fn at(path: &str, album: &str, n: u32, title: &str, year: u16) -> Track {
        Track { path: path.into(), artist: Some("Band".into()), album: Some(album.into()), track: Some(n), title: Some(title.into()), year: Some(year), ext: "flac".into(), ..Default::default() }
    }

    #[test]
    fn editions_and_same_name_albums() {
        let songs = ["Disorder", "Day of the Lords", "Candidate"];
        let mut tracks = Vec::new();
        for (dir, n) in [("UP {1986, UK}", 3), ("UP {1989, US} v.1", 3), ("UP {1989, US} v.2", 2)] {
            for (i, s) in songs.iter().take(n).enumerate() {
                tracks.push(at(&format!("/m/Band/{dir}/{i}.flac"), "Unknown Pleasures", i as u32 + 1, s, 1979));
            }
        }
        // Self-titled albums: same name, different songs and years.
        tracks.push(at("/m/Band/1999 - Band/1.flac", "Band", 1, "Never Meant", 1999));
        tracks.push(at("/m/Band/2016 - Band (LP2)/1.flac", "Band", 1, "Where Are We Now", 2016));
        // One album split over disc folders, and over two folders without overlap.
        tracks.push(at("/m/Band/Live/CD 1/1.flac", "Live", 1, "Intro", 2000));
        tracks.push(Track { disc: Some(2), ..at("/m/Band/Live/CD 2/1.flac", "Live", 1, "Outro", 2000) });
        tracks.push(Track { disc: Some(1), ..at("/m/Band/Hits (Disc 1)/1.flac", "Hits", 1, "One", 2001) });
        tracks.push(Track { disc: Some(2), ..at("/m/Band/Hits (Disc 2)/1.flac", "Hits", 1, "Two", 2001) });

        let idx = build(&tracks, &ArtistPrefs::default());
        let albums = &idx.artists[0].albums;
        let titled = |t: &str| albums.iter().filter(|a| a.title.as_deref() == Some(t)).collect::<Vec<_>>();
        let up = titled("Unknown Pleasures");
        assert_eq!(up.len(), 3, "three editions");
        assert!(up.iter().all(|a| a.edition.as_ref().is_some_and(|e| e.count == 3)));
        assert_eq!(up[0].edition.as_ref().unwrap().label, "1986, UK");
        assert_eq!(up[2].tracks.len(), 2, "the incomplete copy ranks last");
        assert_eq!(up.iter().map(|a| a.id("band")).collect::<BTreeSet<_>>().len(), 3, "editions have their own ids");
        assert_eq!(up[0].base_id("band"), up[1].base_id("band"));

        let band = titled("Band");
        assert_eq!(band.len(), 2);
        assert!(band.iter().all(|a| a.edition.is_none()));
        assert_eq!(band.iter().map(|a| a.variant.as_deref().unwrap()).collect::<Vec<_>>(), ["1999", "2016"]);
        assert_ne!(band[0].id("band"), band[1].id("band"));

        assert_eq!(titled("Live").len(), 1);
        assert_eq!(titled("Live")[0].tracks.len(), 2);
        assert_eq!(titled("Hits").len(), 1, "discs in separate folders are one album");
        // A unique name keeps its old id.
        assert_eq!(titled("Live")[0].id("band"), "band|live");
    }

    #[test]
    fn copies_covered_by_a_same_or_better_copy() {
        let flac = |dir: &str, n: u32, title: &str, depth: u8, rate: u32| Track { bit_depth: Some(depth), sample_rate: Some(rate), kbps: Some(900), ..at(&format!("/m/Band/{dir}/{n}.flac"), "Alaska", n, title, 2005) };
        let mp3 = |dir: &str, n: u32, title: &str, kbps: u32| Track { ext: "mp3".into(), kbps: Some(kbps), sample_rate: Some(44100), ..at(&format!("/m/Band/{dir}/{n}.mp3"), "Alaska", n, title, 2005) };
        let songs = ["Alaska", "Croakies", "Breathe"];
        let mut tracks = Vec::new();
        for (i, s) in songs.iter().enumerate() {
            let n = i as u32 + 1;
            tracks.push(flac("A {CD}", n, s, 16, 44100));
            tracks.push(flac("B {CD, again}", n, s, 16, 44100));
            tracks.push(flac("C {24-96}", n, s, 24, 96000));
            tracks.push(mp3("D {320}", n, s, 320));
        }
        // A copy with a longer last song (a hidden track) isn't covered by a shorter one.
        let mut hidden = tracks.clone();
        for t in hidden.iter_mut().filter(|t| t.path.to_string_lossy().contains("B {CD, again}") && t.track == Some(3)) {
            t.duration = std::time::Duration::from_secs(600);
        }
        let idx = build(&hidden, &ArtistPrefs::default());
        let again = idx.artists[0].albums.iter().filter_map(|a| a.edition.as_ref()).find(|e| e.label == "CD, again").unwrap();
        assert!(again.superseded.is_none());
        // An MP3 copy with a bonus song: not covered by the others, so it stays unflagged.
        tracks.push(mp3("D {320}", 4, "Bonus", 320));
        let idx = build(&tracks, &ArtistPrefs::default());
        let eds: Vec<&Edition> = idx.artists[0].albums.iter().filter_map(|a| a.edition.as_ref()).collect();
        let by = |label: &str| eds.iter().find(|e| e.label == label).copied().unwrap();
        assert_eq!(by("24-96").rank, 0, "highest quality first among equally complete lossless copies");
        assert_eq!(by("24-96").format, "FLAC 24/96");
        assert!(by("24-96").superseded.is_none());
        let cd = by("CD").superseded.as_ref().expect("covered by the 24/96 copy");
        assert!(cd.higher && cd.format == "FLAC 24/96");
        assert!(by("CD, again").superseded.as_ref().is_some_and(|s| s.higher));
        assert_eq!(by("320").format, "MP3 320");
        assert!(by("320").superseded.is_none(), "it has a song the others lack");
        assert_eq!(idx.superseded().len(), 2);

        assert_eq!(song_key("The Need For Repetition / The Man Land (Hidden Track)"), song_key("The Need For Repetition"));
        assert!(strsim::jaro_winkler(&song_key("The Need For Repitition"), &song_key("The Need For Repetition - The Man Land")) >= 0.92);

        // Same quality: only the lower-ranked copy is flagged, pointing at the other.
        let q = |k| Quality { kbps: k, ..Default::default() };
        assert_eq!(q(320).compare(&q(300)), std::cmp::Ordering::Equal);
        assert_eq!(q(320).compare(&q(192)), std::cmp::Ordering::Greater);
    }

    #[test]
    fn an_album_stays_in_one_folder() {
        let song = |dir: &str, n: u32, artist: &str, album_artist: Option<&str>| Track {
            path: format!("/m/{dir}/{n}.mp3").into(),
            artist: Some(artist.into()),
            album_artist: album_artist.map(Into::into),
            album: Some(dir.into()),
            track: Some(n),
            ..Default::default()
        };
        let mut tracks = Vec::new();
        // Most songs name the duo as album artist; two have no album artist.
        for n in 1..=8 {
            tracks.push(song("Duality", n, "Lisa Gerrard/Pieter Bourke", Some("Lisa Gerrard & Pieter Bourke")));
        }
        tracks.extend([song("Duality", 9, "Lisa Gerrard", None), song("Duality", 10, "Lisa Gerrard", None)]);
        // A compilation flag on some songs only, with one album artist throughout.
        for n in 1..=4 {
            tracks.push(Track { compilation: n > 1, ..song("Suspended Animation", n, "John Petrucci", Some("John Petrucci")) });
        }
        // A guest on one song, no album-artist tags.
        for n in 1..=4 {
            tracks.push(song("Hybrid Theory", n, if n == 4 { "Linkin Park & Guest" } else { "Linkin Park" }, None));
        }
        // A split release: two bands, no main one.
        for n in 1..=4 {
            tracks.push(song("Split Tape", n, if n <= 2 { "Lebanon Hanover" } else { "La Fete Triste" }, None));
        }
        // Same name, different folders and bands: not one album.
        tracks.push(Track { path: "/m/a/1.mp3".into(), ..song("Greatest Hits", 1, "The Doors", None) });
        tracks.push(Track { path: "/m/b/1.mp3".into(), ..song("Greatest Hits", 1, "Led Zeppelin", None) });

        let idx = build(&tracks, &ArtistPrefs::default());
        let holder = |album: &str| {
            let v: Vec<&str> = idx.artists.iter().filter(|a| a.albums.iter().any(|al| al.title.as_deref() == Some(album))).map(|a| a.name.as_str()).collect();
            v
        };
        assert_eq!(holder("Duality"), ["Lisa Gerrard & Pieter Bourke"]);
        assert_eq!(holder("Suspended Animation"), ["John Petrucci"]);
        assert_eq!(holder("Hybrid Theory"), ["Linkin Park"]);
        assert_eq!(holder("Split Tape"), [VARIOUS]);
        assert_eq!(holder("Greatest Hits").len(), 2);

        // Artists the user merged count as one: nothing to keep together.
        let tracks = vec![song("Axis", 1, "Jimi Hendrix", None), song("Axis", 2, "The Jimi Hendrix Experience", None)];
        let prefs = ArtistPrefs { merges: vec![(artist_key("Jimi Hendrix"), artist_key("The Jimi Hendrix Experience"))], ..Default::default() };
        assert_eq!(build(&tracks, &prefs).artists.len(), 1);
        assert_eq!(build(&tracks, &ArtistPrefs::default()).artists[0].name, VARIOUS);
    }

    #[test]
    fn songs_stored_twice() {
        let song = |file: &str, n: u32, size: u64| Track {
            path: format!("/m/Band/Hits/{file}").into(),
            artist: Some("Band".into()),
            album: Some("Hits".into()),
            track: Some(n),
            title: Some(format!("Song {n}")),
            size,
            duration: std::time::Duration::from_secs(200),
            ..Default::default()
        };
        let tracks = vec![
            song("01 Song 1 (2015_05_12 05_18_04 UTC).mp3", 1, 100),
            song("01 Song 1.mp3", 1, 100),
            song("01 Song 1 (2015_06_05 05_22_01 UTC).mp3", 1, 100),
            song("02 Song 2.mp3", 2, 100),
            // Same number, different file: not a repeat.
            song("02 Song 2 (live).mp3", 2, 150),
        ];
        let al = &build(&tracks, &ArtistPrefs::default()).artists[0].albums[0];
        assert_eq!(al.repeats, [(0, 1), (2, 1)], "the shortest name stays");
        assert!(al.is_repeat(2) && !al.is_repeat(1) && !al.is_repeat(4));
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
