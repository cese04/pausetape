//! Names on the card as a pattern, like calibre's save-to-disk template:
//! `{albumartist}/[{year} - ]{album}/[[{disc}-]{track} - ][{trackartist} - ]{title}`.
//!
//! `/` separates folders, the last part names the file. A part in [brackets] is left out when a
//! field directly inside it is empty (brackets nest). Each folder and file name is made safe for
//! FAT as a whole, after the fields are filled in.

use std::path::PathBuf;

use super::AlbumOrder;
use crate::library::names::{safe_chars, sanitize_component};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    AlbumArtist,
    TrackArtist,
    Album,
    Year,
    Position,
    Disc,
    Track,
    Title,
}

impl Field {
    pub const ALL: [Field; 8] = [Field::AlbumArtist, Field::Album, Field::Year, Field::Position, Field::Disc, Field::Track, Field::TrackArtist, Field::Title];

    pub fn name(self) -> &'static str {
        match self {
            Field::AlbumArtist => "albumartist",
            Field::TrackArtist => "trackartist",
            Field::Album => "album",
            Field::Year => "year",
            Field::Position => "position",
            Field::Disc => "disc",
            Field::Track => "track",
            Field::Title => "title",
        }
    }

    pub fn about(self) -> &'static str {
        match self {
            Field::AlbumArtist => "the artist the album is under in the library (or as renamed for this write)",
            Field::TrackArtist => "the song's own artist, on compilations and songs without an album; empty otherwise",
            Field::Album => "the album title (or as renamed); songs without an album go under the folder named below",
            Field::Year => "the album's year",
            Field::Position => "the album's place in your order, 01, 02… (the arrows in the list above)",
            Field::Disc => "the disc number, on albums of several discs; empty otherwise",
            Field::Track => "the track number, 01, 02…",
            Field::Title => "the song title",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Piece {
    Text(String),
    Field(Field),
    Optional(Vec<Piece>),
}

/// A parsed pattern: the folders, then the file name (without its extension).
#[derive(Clone, Debug, PartialEq)]
pub struct Pattern {
    parts: Vec<Vec<Piece>>,
}

/// The pattern each album order stands for. They name things exactly as layouts did before
/// patterns, so existing cards keep their names.
pub fn default_for(order: AlbumOrder) -> &'static str {
    match order {
        AlbumOrder::Year => "{albumartist}/[{year} - ]{album}/[[{disc}-]{track} - ][{trackartist} - ]{title}",
        AlbumOrder::Title => "{albumartist}/{album}/[[{disc}-]{track} - ][{trackartist} - ]{title}",
        AlbumOrder::Custom => "{albumartist}/[{position} - ]{album}/[[{disc}-]{track} - ][{trackartist} - ]{title}",
    }
}

impl Pattern {
    pub fn parse(s: &str) -> Result<Pattern, String> {
        let s = s.trim();
        if s.is_empty() {
            return Err("the pattern is empty".into());
        }
        let mut parts = Vec::new();
        for (i, part) in split_folders(s)?.into_iter().enumerate() {
            if part.trim().is_empty() {
                return Err(if i == 0 { "it can't start with /".into() } else { "there's an empty folder (// or a / at the end)".into() });
            }
            let mut chars = part.chars().peekable();
            let pieces = parse_pieces(&mut chars, false)?;
            if pieces.iter().all(|p| matches!(p, Piece::Text(t) if t.trim().trim_matches('.').is_empty())) {
                return Err(format!("“{part}” isn't a folder name"));
            }
            parts.push(pieces);
        }
        let p = Pattern { parts };
        if !p.parts.last().is_some_and(|f| contains(f, Field::Title)) {
            return Err("the file name (after the last /) needs {title}".into());
        }
        Ok(p)
    }

    pub fn uses(&self, f: Field) -> bool {
        self.parts.iter().any(|p| contains(p, f))
    }

    /// The path for a song: fields from `value`, the file ending in `.ext`.
    pub fn render(&self, value: impl Fn(Field) -> Option<String>, ext: &str) -> PathBuf {
        let last = self.parts.len() - 1;
        let mut path = PathBuf::new();
        for (i, part) in self.parts.iter().enumerate() {
            let mut s = String::new();
            render_into(part, &value, &mut s);
            if i == last {
                path.push(format!("{}.{ext}", sanitize_component(&s, 120)));
            } else {
                path.push(sanitize_component(&s, 100));
            }
        }
        path
    }
}

/// Split on `/` outside brackets.
fn split_folders(s: &str) -> Result<Vec<&str>, String> {
    let (mut out, mut depth, mut start) = (Vec::new(), 0i32, 0);
    for (i, c) in s.char_indices() {
        match c {
            '[' => depth += 1,
            ']' => depth -= 1,
            '/' if depth == 0 => {
                out.push(&s[start..i]);
                start = i + 1;
            }
            '/' => return Err("a / can't be inside [brackets]".into()),
            _ => {}
        }
        if depth < 0 {
            return Err("a ] has no [ before it".into());
        }
    }
    out.push(&s[start..]);
    Ok(out)
}

fn parse_pieces(chars: &mut std::iter::Peekable<std::str::Chars>, inside: bool) -> Result<Vec<Piece>, String> {
    let mut pieces = Vec::new();
    let mut text = String::new();
    while let Some(c) = chars.next() {
        match c {
            '{' => {
                let name: String = chars.by_ref().take_while(|&c| c != '}').collect();
                let field = Field::ALL.into_iter().find(|f| f.name() == name.trim().to_lowercase()).ok_or_else(|| format!("{{{name}}} isn't a field"))?;
                if !text.is_empty() {
                    pieces.push(Piece::Text(std::mem::take(&mut text)));
                }
                pieces.push(Piece::Field(field));
            }
            '}' => return Err("a } has no { before it".into()),
            '[' => {
                if !text.is_empty() {
                    pieces.push(Piece::Text(std::mem::take(&mut text)));
                }
                let inner = parse_pieces(chars, true)?;
                if !inner.iter().any(|p| matches!(p, Piece::Field(_))) {
                    return Err("a [part] needs a field in it, such as {year}".into());
                }
                pieces.push(Piece::Optional(inner));
            }
            ']' if inside => {
                if !text.is_empty() {
                    pieces.push(Piece::Text(text));
                }
                return Ok(pieces);
            }
            ']' => return Err("a ] has no [ before it".into()),
            c => text.push(c),
        }
    }
    if inside {
        return Err("a [ isn't closed".into());
    }
    if !text.is_empty() {
        pieces.push(Piece::Text(text));
    }
    Ok(pieces)
}

fn contains(pieces: &[Piece], f: Field) -> bool {
    pieces.iter().any(|p| match p {
        Piece::Field(x) => *x == f,
        Piece::Optional(inner) => contains(inner, f),
        Piece::Text(_) => false,
    })
}

fn render_into(pieces: &[Piece], value: &impl Fn(Field) -> Option<String>, out: &mut String) {
    for p in pieces {
        match p {
            Piece::Text(t) => out.push_str(t),
            Piece::Field(f) => out.push_str(&value(*f).map(|v| safe_chars(&v)).unwrap_or_default()),
            Piece::Optional(inner) => {
                let filled = inner.iter().all(|p| !matches!(p, Piece::Field(f) if value(*f).is_none_or(|v| v.is_empty())));
                if filled {
                    render_into(inner, value, out);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song(f: Field) -> Option<String> {
        match f {
            Field::AlbumArtist => Some("AC/DC".into()),
            Field::Album => Some("Back in Black".into()),
            Field::Year => Some("1980".into()),
            Field::Track => Some("06".into()),
            Field::Title => Some("Back in Black".into()),
            _ => None,
        }
    }

    #[test]
    fn patterns_fill_in_and_leave_out() {
        let p = Pattern::parse(default_for(AlbumOrder::Year)).unwrap();
        assert_eq!(p.render(song, "flac"), PathBuf::from("AC-DC/1980 - Back in Black/06 - Back in Black.flac"));
        let disc = |f| if f == Field::Disc { Some("2".into()) } else { song(f) };
        assert_eq!(p.render(disc, "flac"), PathBuf::from("AC-DC/1980 - Back in Black/2-06 - Back in Black.flac"));
        // No track number: the disc goes with it.
        let no_track = |f| if f == Field::Track { None } else { disc(f) };
        assert_eq!(p.render(no_track, "mp3"), PathBuf::from("AC-DC/1980 - Back in Black/Back in Black.mp3"));
        let flat = Pattern::parse("{albumartist} - {album} [({year})]/{track} {title}").unwrap();
        assert_eq!(flat.render(song, "mp3"), PathBuf::from("AC-DC - Back in Black (1980)/06 Back in Black.mp3"));
        assert!(flat.uses(Field::Year) && !flat.uses(Field::Position));
        // Fields can't make folders: a / in a value isn't one.
        let one = Pattern::parse("{title}").unwrap();
        assert_eq!(one.render(|f| (f == Field::Title).then(|| "Either/Or".into()), "flac"), PathBuf::from("Either-Or.flac"));
        for bad in ["", "/{title}", "{albumartist}//{title}", "{albumartist}/{album}", "{artist}/{title}", "[{year}/{title}]", "{album}/[{year}", "{album}/[ - ]{title}", "../{title}"] {
            assert!(Pattern::parse(bad).is_err(), "{bad:?} should be refused");
        }
    }
}
