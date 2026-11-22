//! The library's search box: words match artists, albums and song titles; `field:value` terms
//! narrow albums by what they are ("format:m4a", "quality:lossy", "card:on", "cover:no",
//! "year:1979..1985", "edited:yes", "damaged:yes"); a leading "-" turns a term around ("-format:mp3").

use deunicode::deunicode_char;

use super::Track;

#[derive(Clone, Debug, PartialEq)]
pub enum Filter {
    /// Any song in this format: extension or codec ("flac", "m4a", "alac", "e-ac-3").
    Format(String),
    Quality(Quality),
    Card(OnCard),
    /// Has a cover players show (embedded, or waiting to be).
    Cover(bool),
    /// Released in this range, both ends included.
    Year(Option<u16>, Option<u16>),
    /// Has songs with corrections not yet in the files.
    Edited(bool),
    /// Has damaged files (`Track::harmed`).
    Damaged(bool),
    /// Has this genre (by `genres::key`); "" for albums with none.
    Genre(String),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Quality {
    /// Every song lossless.
    Lossless,
    /// Some song lossy.
    Lossy,
    /// Some song above CD quality.
    Hires,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OnCard {
    /// Some of it is on the card (as of the last write).
    On,
    /// Nothing of it is on the card, nor chosen.
    Off,
    /// The next Sync adds or takes off some of it.
    Changing,
    /// Chosen, while another copy has its songs in higher quality.
    Better,
    /// On the card, or chosen for it: what the card view lists (including what Sync takes off).
    Listed,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Query {
    /// Folded (`fold`); each must appear in the artist, the album or a song title.
    pub words: Vec<String>,
    /// (filter, wanted): false for a "-" term.
    pub filters: Vec<(Filter, bool)>,
    /// Terms that didn't make sense, to say so.
    pub unknown: Vec<String>,
}

/// Where an album stands with the card, as the library marks it.
#[derive(Clone, Copy, Default)]
pub struct CardFacts {
    pub on: bool,
    pub changing: bool,
    pub better: bool,
    pub chosen: bool,
}

/// What a query looks at in an album.
pub struct AlbumFacts<'a> {
    pub artist: &'a str,
    pub album: &'a str,
    pub year: Option<u16>,
    pub tracks: Vec<&'a Track>,
    pub card: CardFacts,
    pub cover: bool,
    /// Its genres, by `genres::key` (its artist's, when it has none of its own).
    pub genres: Vec<String>,
}

pub const HELP: &str = "Words find artists, albums and song titles. Narrow by what albums are:\n\
    format:flac   format:m4a   format:alac\n\
    quality:lossless   quality:lossy   quality:hires\n\
    card:on   card:off   card:sync (changes at the next Sync)   card:all (on it, going on or off)   card:better (a better copy exists)\n\
    cover:no   cover:yes\n\
    year:1979   year:1979..1985   year:..1990\n\
    edited:yes   damaged:yes   genre:trip-hop   genre:none\n\
    A \"-\" in front turns a term around: -format:mp3";

impl Query {
    pub fn parse(text: &str) -> Query {
        let mut q = Query::default();
        for term in text.split_whitespace() {
            let (wanted, body) = match term.strip_prefix('-') {
                Some(rest) if rest.contains(':') => (false, rest),
                _ => (true, term),
            };
            let Some((field, value)) = body.split_once(':') else {
                q.words.push(fold(term));
                continue;
            };
            let value = value.to_lowercase();
            let yes_no = |v: &str| match v {
                "yes" | "y" | "true" | "1" => Some(true),
                "no" | "n" | "false" | "0" | "none" => Some(false),
                _ => None,
            };
            let filter = match field.to_lowercase().as_str() {
                "format" | "f" if !value.is_empty() => Some(Filter::Format(value.clone())),
                "quality" | "q" => match value.as_str() {
                    "lossless" => Some(Filter::Quality(Quality::Lossless)),
                    "lossy" => Some(Filter::Quality(Quality::Lossy)),
                    "hires" | "hi-res" => Some(Filter::Quality(Quality::Hires)),
                    _ => None,
                },
                "card" | "on" => match value.as_str() {
                    "on" | "yes" | "card" => Some(Filter::Card(OnCard::On)),
                    "off" | "no" => Some(Filter::Card(OnCard::Off)),
                    "sync" | "changing" | "pending" => Some(Filter::Card(OnCard::Changing)),
                    "better" => Some(Filter::Card(OnCard::Better)),
                    "all" | "any" => Some(Filter::Card(OnCard::Listed)),
                    _ => None,
                },
                "cover" => yes_no(&value).map(Filter::Cover),
                "edited" => yes_no(&value).map(Filter::Edited),
                "damaged" => yes_no(&value).map(Filter::Damaged),
                "genre" | "g" if !value.is_empty() => Some(Filter::Genre(if value == "none" { String::new() } else { super::genres::key(&value) })),
                "year" | "y" => year_range(&value).map(|(a, b)| Filter::Year(a, b)),
                _ => None,
            };
            match filter {
                Some(f) => q.filters.push((f, wanted)),
                None => q.unknown.push(term.to_string()),
            }
        }
        q
    }

    pub fn is_empty(&self) -> bool {
        self.words.is_empty() && self.filters.is_empty()
    }

    /// The album passes every filter (words aside).
    pub fn filters_pass(&self, a: &AlbumFacts) -> bool {
        self.filters.iter().all(|(f, wanted)| f.holds(a) == *wanted)
    }

    /// Every word is in the artist or album name: then all its songs are shown.
    pub fn names_match(&self, artist: &str, album: &str) -> bool {
        let (artist, album) = (fold(artist), fold(album));
        self.words.iter().all(|w| artist.contains(w) || album.contains(w))
    }

    /// Every word is in the artist, the album or this song's title.
    pub fn song_matches(&self, artist: &str, album: &str, title: &str) -> bool {
        let (artist, album, title) = (fold(artist), fold(album), fold(title));
        self.words.iter().all(|w| artist.contains(w) || album.contains(w) || title.contains(w))
    }

    /// The album passes the filters and each word is found somewhere in it.
    pub fn album_matches(&self, a: &AlbumFacts) -> bool {
        self.filters_pass(a) && (self.names_match(a.artist, a.album) || a.tracks.iter().any(|t| self.song_matches(a.artist, a.album, &t.display_title())))
    }

    pub fn wants(&self, f: &Filter) -> bool {
        self.filters.iter().any(|(g, wanted)| g == f && *wanted)
    }
}

impl Filter {
    fn holds(&self, a: &AlbumFacts) -> bool {
        match self {
            Filter::Format(f) => a.tracks.iter().any(|t| t.ext == *f || t.codec.as_deref().is_some_and(|c| c == f) || t.format_label().to_lowercase() == *f),
            Filter::Quality(Quality::Lossless) => !a.tracks.is_empty() && a.tracks.iter().all(|t| t.lossless()),
            Filter::Quality(Quality::Lossy) => a.tracks.iter().any(|t| !t.lossless()),
            Filter::Quality(Quality::Hires) => a.tracks.iter().any(|t| t.lossless() && t.hires()),
            Filter::Card(OnCard::On) => a.card.on,
            Filter::Card(OnCard::Off) => !a.card.on && !a.card.chosen,
            Filter::Card(OnCard::Changing) => a.card.changing,
            Filter::Card(OnCard::Better) => a.card.better,
            Filter::Card(OnCard::Listed) => a.card.on || a.card.chosen,
            Filter::Cover(has) => a.cover == *has,
            Filter::Year(from, to) => a.year.is_some_and(|y| from.is_none_or(|f| y >= f) && to.is_none_or(|t| y <= t)),
            Filter::Edited(e) => a.tracks.iter().any(|t| t.dirty) == *e,
            Filter::Damaged(d) => a.tracks.iter().any(|t| t.harmed()) == *d,
            Filter::Genre(k) if k.is_empty() => a.genres.is_empty(),
            Filter::Genre(k) => a.genres.contains(k),
        }
    }
}

/// Lowercased, with Latin letters' accents dropped and curly quotes made straight, so that
/// "bjork" finds "Björk", "strasse" "Straße" and "don't" "Don’t". Other scripts are kept as they
/// are: spelt in Latin letters they'd match too much ("lin" in "林檎", read "Lin Qin").
/// The card tab's filter folds the same way.
pub fn fold(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        let latin = matches!(c, '\u{c0}'..='\u{24f}' | '\u{1e00}'..='\u{1eff}' | '\u{2010}'..='\u{201f}');
        match deunicode_char(c).filter(|_| latin) {
            Some(plain) => out.push_str(plain),
            None => out.push(c),
        }
    }
    out.to_lowercase()
}

/// "1979" → 1979..=1979, "1979..1985", "..1990", "2000..".
fn year_range(v: &str) -> Option<(Option<u16>, Option<u16>)> {
    let num = |s: &str| if s.is_empty() { Ok(None) } else { s.parse::<u16>().map(Some) };
    match v.split_once("..") {
        Some((a, b)) => {
            let (a, b) = (num(a).ok()?, num(b).ok()?);
            (a.is_some() || b.is_some()).then_some((a, b))
        }
        None => v.parse::<u16>().ok().map(|y| (Some(y), Some(y))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_words_and_fields() {
        let q = Query::parse("Joy  format:FLAC -format:mp3 year:1979..1985 cover:no card:sync colour:red");
        assert_eq!(q.words, vec!["joy"]);
        assert_eq!(
            q.filters,
            vec![
                (Filter::Format("flac".into()), true),
                (Filter::Format("mp3".into()), false),
                (Filter::Year(Some(1979), Some(1985)), true),
                (Filter::Cover(false), true),
                (Filter::Card(OnCard::Changing), true),
            ]
        );
        assert_eq!(q.unknown, vec!["colour:red"]);
        assert_eq!(Query::parse("year:..1990").filters[0].0, Filter::Year(None, Some(1990)));
        assert!(Query::parse("-word").words == vec!["-word"], "a dash alone is part of a word");
    }

    #[test]
    fn matches_albums_by_what_they_are() {
        let song = |ext: &str, title: &str| Track { ext: ext.into(), title: Some(title.into()), ..Default::default() };
        let (a, b) = (song("flac", "Disorder"), song("m4a", "Candidate"));
        let album = |tracks| AlbumFacts { artist: "Joy Division", album: "Unknown Pleasures", year: Some(1979), tracks, card: CardFacts::default(), cover: false, genres: vec!["postpunk".into()] };
        let mixed = album(vec![&a, &b]);
        let flac_only = album(vec![&a]);
        let m = |q: &str, al: &AlbumFacts| Query::parse(q).album_matches(al);
        assert!(m("joy pleasures", &mixed) && m("disorder", &mixed) && !m("closer", &mixed));
        assert!(m("format:m4a", &mixed) && !m("format:m4a", &flac_only) && m("-format:m4a", &flac_only));
        assert!(m("quality:lossless", &flac_only) && !m("quality:lossless", &mixed) && m("quality:lossy", &mixed));
        assert!(m("year:1979", &mixed) && m("year:1970..1980", &mixed) && !m("year:1980..", &mixed));
        assert!(m("cover:no card:off", &mixed) && !m("cover:yes", &mixed) && !m("card:on", &mixed));
        assert!(!m("card:all", &mixed), "neither on the card nor chosen");
        assert!(!m("edited:yes", &mixed));
        assert!(m("genre:Post-Punk", &mixed) && m("genre:postpunk", &mixed), "any spelling of the genre");
        assert!(!m("genre:none", &mixed) && !m("genre:rock", &mixed));
        let q = Query::parse("joy candidate");
        assert!(!q.names_match("Joy Division", "Unknown Pleasures") && q.song_matches("Joy Division", "Unknown Pleasures", "Candidate"));
    }

    #[test]
    fn ignores_accents_and_curly_quotes() {
        let names = |q: &str, artist: &str| Query::parse(q).names_match(artist, "");
        assert!(names("bjork", "Björk") && names("björk", "Bjork") && names("BJÖRK", "björk"));
        assert!(names("sigur ros", "Sigur Rós") && names("motorhead", "Motörhead") && names("strasse", "Straße"));
        assert!(Query::parse("don't").song_matches("Oasis", "", "Don’t Look Back in Anger"));
        assert!(names("кино", "Кино") && names("林檎", "椎名林檎") && !names("lin", "椎名林檎"));
    }
}
