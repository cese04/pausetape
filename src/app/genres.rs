//! Genres in the library: an album's (its songs' tags, else its artist's from MusicBrainz,
//! marked as such), and every genre with its albums, for the genres view. An album with
//! several genres is under each.

use super::*;
use crate::library::genres::{key, tag_style, usual_spelling};

/// An album's genres, and whether they're its artist's (no song of it names one).
pub struct AlbumGenres {
    pub names: Vec<String>,
    pub from_artist: bool,
}

/// One genre: its name as most songs spell it, and its albums (artist, album, from the artist).
pub struct Genre {
    pub key: String,
    pub name: String,
    pub albums: Vec<(usize, usize, bool)>,
}

/// Every genre, worked out again when the library or the artists' genres change.
#[derive(Default)]
pub struct GenreIndex {
    /// (library generation, artists with genres known) it was made at.
    made_at: (u64, usize),
    pub genres: Vec<Genre>,
    /// Albums with no genre at all, not even their artist's.
    pub none: Vec<(usize, usize)>,
}

impl App {
    /// An album's genres: those its songs' tags name, most used first, each spelt as most of
    /// them spell it; else its artist's, as MusicBrainz has them, if they're known.
    pub fn album_genres(&self, ai: usize, bi: usize) -> AlbumGenres {
        let al = &self.index.artists[ai].albums[bi];
        let mut by_key: Vec<(String, Vec<&str>)> = Vec::new();
        for g in al.tracks.iter().flat_map(|&t| self.tracks[t].genres.iter().flatten()) {
            let k = key(g);
            match by_key.iter_mut().find(|(kk, _)| *kk == k) {
                Some((_, spellings)) => spellings.push(g),
                None => by_key.push((k, vec![g])),
            }
        }
        if !by_key.is_empty() {
            by_key.sort_by_key(|(_, spellings)| std::cmp::Reverse(spellings.len()));
            let names = by_key.into_iter().filter_map(|(_, s)| usual_spelling(s)).collect();
            return AlbumGenres { names, from_artist: false };
        }
        let ck = &self.index.clusters[self.index.artists[ai].cluster].key;
        let names = match self.artist_info.get(ck) {
            Some(ArtistState::Found { info, .. }) => info.genres.iter().take(3).map(|g| tag_style(g)).collect(),
            _ => Vec::new(),
        };
        AlbumGenres { names, from_artist: true }
    }

    /// What MusicBrainz said about every artist, as the library database keeps it: once, so
    /// albums without genres of their own can take their artist's.
    fn artists_from_db(&mut self) {
        if self.artists_loaded {
            return;
        }
        self.artists_loaded = true;
        let Some(db) = &self.db else { return };
        for (k, info, fetched) in db.all_artist_info().unwrap_or_default() {
            self.artist_info.entry(k).or_insert(ArtistState::Found { info, fetched });
        }
    }

    /// Every genre in the library, by name, with its albums.
    pub fn genre_index(&mut self) -> &GenreIndex {
        self.artists_from_db();
        let known = self.artist_info.values().filter(|s| matches!(s, ArtistState::Found { info, .. } if !info.genres.is_empty())).count();
        if self.genre_index.made_at != (self.library_generation, known) || self.genre_index.genres.is_empty() && self.genre_index.none.is_empty() {
            let mut genres: Vec<Genre> = Vec::new();
            let mut none = Vec::new();
            for (ai, a) in self.index.artists.iter().enumerate() {
                for bi in 0..a.albums.len() {
                    let g = self.album_genres(ai, bi);
                    if g.names.is_empty() {
                        none.push((ai, bi));
                    }
                    for name in g.names {
                        let k = key(&name);
                        match genres.iter_mut().find(|x| x.key == k) {
                            Some(x) => x.albums.push((ai, bi, g.from_artist)),
                            None => genres.push(Genre { key: k, name, albums: vec![(ai, bi, g.from_artist)] }),
                        }
                    }
                }
            }
            genres.sort_by_key(|g| g.name.to_lowercase());
            self.genre_index = GenreIndex { made_at: (self.library_generation, known), genres, none };
        }
        &self.genre_index
    }
}
