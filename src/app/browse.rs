//! Browsing the library: what the search box asks of each album, and the album table's rows,
//! order and selection.

use super::*;
use crate::library::grouping::{Quality, describe};
use crate::library::search::{AlbumFacts, CardFacts, Query};

/// The album table's columns, and what it can be sorted by.
#[derive(Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SortBy {
    #[default]
    Artist,
    Album,
    Year,
    Format,
    Songs,
    Size,
    Card,
    Cover,
}

/// One album in the table.
pub struct AlbumRow {
    pub artist: usize,
    pub album: usize,
    pub id: String,
    pub artist_name: String,
    pub title: String,
    pub year: Option<u16>,
    pub format: String,
    quality: Quality,
    pub songs: usize,
    pub bytes: u64,
    pub card: CardFacts,
    pub cover: bool,
    pub edited: bool,
    /// Every song plays on the card's player as it is.
    pub plays_on_device: bool,
}

impl App {
    /// Where an album stands with the card.
    pub fn card_facts(&self, ck: &str, artist: &grouping::ArtistNode, al: &AlbumNode) -> CardFacts {
        let cs = &self.card_state;
        let on = al.tracks.iter().any(|i| cs.on_card.contains(i));
        let planned = al.tracks.iter().any(|i| cs.planned.contains(i));
        let changing = al.tracks.iter().any(|i| cs.on_card.contains(i) != cs.planned.contains(i));
        let chosen = self.write().custom.includes(ck, al, &artist.albums);
        let better = chosen && al.edition.as_ref().and_then(|e| e.superseded.as_ref()).is_some_and(|s| s.higher);
        CardFacts { on, changing, better, chosen: chosen || planned }
    }

    /// What the search box looks at in album `bi` of artist `ai`.
    /// The albums (artist, album) these songs belong to, in library order.
    pub fn albums_of(&self, songs: &BTreeSet<usize>) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        for (ai, a) in self.index.artists.iter().enumerate() {
            for (bi, al) in a.albums.iter().enumerate() {
                if al.tracks.iter().any(|t| songs.contains(t)) {
                    out.push((ai, bi));
                }
            }
        }
        out
    }

    /// Whether all these albums are chosen for the active setup's card.
    pub fn all_on_card(&self, albums: &[(usize, usize)]) -> bool {
        albums.iter().all(|&(ai, bi)| {
            let a = &self.index.artists[ai];
            self.write().custom.includes(&self.index.clusters[a.cluster].key, &a.albums[bi], &a.albums)
        })
    }

    /// Send these albums to the card, or take them off if they're all there already (⌘D).
    pub fn toggle_on_card(&mut self, albums: Vec<(usize, usize)>) {
        if albums.is_empty() {
            return;
        }
        let on = !self.all_on_card(&albums);
        self.card_albums(&albums, on);
    }

    pub fn album_facts(&self, ai: usize, bi: usize) -> AlbumFacts<'_> {
        let a = &self.index.artists[ai];
        let al = &a.albums[bi];
        let ck = &self.index.clusters[a.cluster].key;
        AlbumFacts {
            artist: &a.name,
            album: al.title.as_deref().unwrap_or(""),
            year: al.year,
            tracks: al.tracks.iter().map(|&i| &self.tracks[i]).collect(),
            card: self.card_facts(ck, a, al),
            cover: !self.lacks_cover(al),
        }
    }

    /// The search box, parsed (cheap enough to do every frame).
    pub fn query(&self) -> Query {
        Query::parse(&self.filter)
    }

    /// The albums the query lets through, in the table's order.
    pub fn album_rows(&self, q: &Query) -> Vec<AlbumRow> {
        let mut rows = Vec::new();
        for (ai, a) in self.index.artists.iter().enumerate() {
            let ck = &self.index.clusters[a.cluster].key;
            for (bi, al) in a.albums.iter().enumerate() {
                let facts = self.album_facts(ai, bi);
                if !q.album_matches(&facts) {
                    continue;
                }
                let (format, quality) = describe(&self.tracks, &al.tracks);
                let device = self.player();
                rows.push(AlbumRow {
                    artist: ai,
                    album: bi,
                    id: al.id(ck),
                    artist_name: a.name.clone(),
                    title: al.title.clone().unwrap_or_else(|| "Loose tracks".into()),
                    year: al.year,
                    format,
                    quality,
                    songs: al.tracks.len(),
                    bytes: al.tracks.iter().map(|&i| self.tracks[i].size).sum(),
                    card: facts.card,
                    cover: facts.cover,
                    edited: facts.tracks.iter().any(|t| t.dirty),
                    plays_on_device: facts.tracks.iter().all(|t| device.plays(t)),
                });
            }
        }
        let (by, descending) = self.album_sort;
        let key = |r: &AlbumRow| (crate::library::names::artist_key(&r.artist_name), r.year, r.title.to_lowercase());
        rows.sort_by(|x, y| {
            let o = match by {
                SortBy::Artist => std::cmp::Ordering::Equal,
                SortBy::Album => x.title.to_lowercase().cmp(&y.title.to_lowercase()),
                SortBy::Year => x.year.cmp(&y.year),
                SortBy::Format => x.quality.compare(&y.quality).then_with(|| x.format.cmp(&y.format)),
                SortBy::Songs => x.songs.cmp(&y.songs),
                SortBy::Size => x.bytes.cmp(&y.bytes),
                SortBy::Card => (x.card.on, x.card.chosen, x.card.changing).cmp(&(y.card.on, y.card.chosen, y.card.changing)),
                SortBy::Cover => x.cover.cmp(&y.cover),
            };
            let o = if by == SortBy::Artist { key(x).cmp(&key(y)) } else { o.then_with(|| key(x).cmp(&key(y))) };
            if descending { o.reverse() } else { o }
        });
        rows
    }

    /// Click a column heading: sort by it, or turn the order around.
    pub fn sort_albums(&mut self, by: SortBy) {
        self.album_sort = match self.album_sort {
            (b, d) if b == by => (by, !d),
            // Numbers read best largest first.
            _ => (by, matches!(by, SortBy::Songs | SortBy::Size)),
        };
    }

    /// The songs of the selected albums, in table order.
    pub fn selected_album_songs(&self, rows: &[AlbumRow]) -> Vec<usize> {
        rows.iter().filter(|r| self.selected_albums.contains(&r.id)).flat_map(|r| self.index.artists[r.artist].albums[r.album].tracks.iter().copied()).collect()
    }

    /// Put the selected albums on the card, or take them off.
    pub fn card_albums(&mut self, albums: &[(usize, usize)], on: bool) {
        let n = albums.len();
        let what = if n == 1 { "1 album".to_string() } else { format!("{n} albums") };
        self.remember_chosen(if on { format!("sending {what} to the card") } else { format!("taking {what} off the card") });
        for &(ai, bi) in albums {
            let a = &self.index.artists[ai];
            let ck = self.index.clusters[a.cluster].key.clone();
            self.saved.writes[self.saved.active_write].custom.set_album(&ck, &a.albums[bi], on);
        }
        self.chosen_changed();
    }
}
