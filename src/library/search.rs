//! The library's search box: words match artists, albums and song titles; `field:value` terms
//! narrow albums by what they are ("format:m4a", "quality:lossy", "card:on", "cover:no",
//! "year:1979..1985", "edited:yes"); a leading "-" turns a term around ("-format:mp3").

use super::Track;

#[derive(Clone, Debug, PartialEq)]
pub enum Filter {
    /// Any song in this format: extension or codec ("flac", "m4a", "alac", "e-ac-3").
    Format(String),
    Quality(Quality),
    Card(OnCard),
    /// Has a cover the Echo Mini shows (embedded, or waiting to be).
    Cover(bool),
    /// Released in this range, both ends included.
    Year(Option<u16>, Option<u16>),
    /// Has songs with corrections not yet in the files.
    Edited(bool),
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
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Query {
    /// Lowercased; each must appear in the artist, the album or a song title.
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
}

pub const HELP: &str = "Words find artists, albums and song titles. Narrow by what albums are:\n\
    format:flac   format:m4a   format:alac\n\
    quality:lossless   quality:lossy   quality:hires\n\
    card:on   card:off   card:sync (changes at the next Sync)   card:better (a better copy exists)\n\
    cover:no   cover:yes\n\
    year:1979   year:1979..1985   year:..1990\n\
    edited:yes\n\
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
                q.words.push(term.to_lowercase());
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
                    _ => None,
                },
                "cover" => yes_no(&value).map(Filter::Cover),
                "edited" => yes_no(&value).map(Filter::Edited),
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
        let (artist, album) = (artist.to_lowercase(), album.to_lowercase());
        self.words.iter().all(|w| artist.contains(w) || album.contains(w))
    }

    /// Every word is in the artist, the album or this song's title.
    pub fn song_matches(&self, artist: &str, album: &str, title: &str) -> bool {
        let (artist, album, title) = (artist.to_lowercase(), album.to_lowercase(), title.to_lowercase());
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
            Filter::Cover(has) => a.cover == *has,
            Filter::Year(from, to) => a.year.is_some_and(|y| from.is_none_or(|f| y >= f) && to.is_none_or(|t| y <= t)),
            Filter::Edited(e) => a.tracks.iter().any(|t| t.dirty) == *e,
        }
    }
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
        let album = |tracks| AlbumFacts { artist: "Joy Division", album: "Unknown Pleasures", year: Some(1979), tracks, card: CardFacts::default(), cover: false };
        let mixed = album(vec![&a, &b]);
        let flac_only = album(vec![&a]);
        let m = |q: &str, al: &AlbumFacts| Query::parse(q).album_matches(al);
        assert!(m("joy pleasures", &mixed) && m("disorder", &mixed) && !m("closer", &mixed));
        assert!(m("format:m4a", &mixed) && !m("format:m4a", &flac_only) && m("-format:m4a", &flac_only));
        assert!(m("quality:lossless", &flac_only) && !m("quality:lossless", &mixed) && m("quality:lossy", &mixed));
        assert!(m("year:1979", &mixed) && m("year:1970..1980", &mixed) && !m("year:1980..", &mixed));
        assert!(m("cover:no card:off", &mixed) && !m("cover:yes", &mixed) && !m("card:on", &mixed));
        assert!(!m("edited:yes", &mixed));
        let q = Query::parse("joy candidate");
        assert!(!q.names_match("Joy Division", "Unknown Pleasures") && q.song_matches("Joy Division", "Unknown Pleasures", "Candidate"));
    }
}
