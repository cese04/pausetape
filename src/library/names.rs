//! Name normalisation: artist keys for clustering, and FAT/exFAT-safe path components.

use deunicode::deunicode;

/// A comparison key that is identical for trivial spelling variants:
/// "The Beatles" / "Beatles, The" / "beatles", "AC/DC" / "ACDC", "Beyoncé" / "Beyonce",
/// "Simon & Garfunkel" / "Simon and Garfunkel".
pub fn artist_key(name: &str) -> String {
    let folded = deunicode(name).to_lowercase().replace('&', " and ").replace('+', " and ");
    let cleaned: String = folded
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect();
    let mut words: Vec<&str> = cleaned.split_whitespace().collect();
    if words.len() > 1 && words[0] == "the" {
        words.remove(0);
    }
    if words.len() > 1 && words[words.len() - 1] == "the" {
        words.pop();
    }
    let key = words.concat();
    if key.is_empty() { name.trim().to_lowercase() } else { key }
}

/// Generic title/album key: like `artist_key` but keeps a leading "the".
pub fn title_key(name: &str) -> String {
    let folded = deunicode(name).to_lowercase().replace('&', " and ");
    let key: String = folded.chars().filter(|c| c.is_alphanumeric()).collect();
    if key.is_empty() { name.trim().to_lowercase() } else { key }
}

/// Strip featured artists so "Daft Punk feat. Pharrell" files under "Daft Punk".
pub fn primary_artist(raw: &str) -> String {
    let raw = raw.split(['\0', ';']).next().unwrap_or(raw);
    // ASCII lowercasing keeps byte offsets aligned with `raw`.
    let lower = raw.to_ascii_lowercase();
    let cut = [" feat. ", " feat ", " ft. ", " ft ", " featuring ", " (feat", " [feat", " (ft", " [ft"]
        .iter()
        .filter_map(|sep| lower.find(sep))
        .min();
    match cut {
        Some(i) => raw[..i].trim().to_string(),
        None => raw.trim().to_string(),
    }
}

/// Whether two artist keys are probably the same artist with a typo or small variation.
/// Exact key matches are merged automatically; this only produces suggestions.
pub fn similar_keys(a: &str, b: &str) -> Option<f64> {
    let (la, lb) = (a.chars().count(), b.chars().count());
    if la < 4 || lb < 4 || la.abs_diff(lb) > 3 {
        return None;
    }
    let jw = strsim::jaro_winkler(a, b);
    let lev = strsim::levenshtein(a, b);
    if jw >= 0.93 || (lev == 1 && la.min(lb) >= 6) { Some(jw) } else { None }
}

/// A folder or album name taken apart:
/// "1979. Unknown Pleasures {1989, US, Qwest Records} v.1" → "Unknown Pleasures", 1979,
/// extras ["1989, US, Qwest Records", "v.1"].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AlbumName {
    pub album: String,
    pub year: Option<u16>,
    /// Edition details that were removed: release year, country, label, format, "v.1".
    pub extras: Vec<String>,
}

/// The album itself, without year, edition brackets or a leading "<artist> - ".
/// Brackets that may belong to the title, like "(What's the Story) Morning Glory?", are kept.
pub fn clean_album(s: &str, artist: Option<&str>) -> AlbumName {
    let (stripped, extras) = strip_extras(s);
    let mut rest = stripped.as_str();
    let mut year = None;
    for _ in 0..2 {
        // "1979 - Album", then "Artist - 1972 - Album" once the artist is gone.
        if year.is_none()
            && let Some((y, after)) = year_prefix(rest)
        {
            year = Some(y);
            rest = after;
        }
        if let Some(a) = artist
            && let Some(after) = strip_artist_prefix(rest, a)
        {
            rest = after;
        }
    }
    let mut album = rest.to_string();
    // "Tilt - 1995"
    if let Some((before, y)) = album.rsplit_once(" - ").and_then(|(b, y)| Some((b, year_of(y.trim())?))) {
        year.get_or_insert(y);
        album = before.to_string();
    }
    if year.is_none() {
        year = extras.iter().filter(|e| !e.to_lowercase().contains("remaster")).find_map(|e| year_prefix(e).map(|(y, _)| y).or(year_of(e)));
    }
    let album = tidy(&album);
    if album.is_empty() {
        return AlbumName { album: tidy(s), year, extras: Vec::new() };
    }
    AlbumName { album, year, extras }
}

/// A lone folder that names both: "Artist - Album (1997) [FLAC]", "Artist - 1972 - Album",
/// "Artist.2005.Album [extras]". Only when the name also has a year or edition details, so
/// "Bits - Pieces" isn't mistaken for an artist and an album.
pub fn split_artist_album(s: &str) -> Option<(String, AlbumName)> {
    let (stripped, _) = strip_extras(s);
    let (artist, rest) = match split_dash(&stripped) {
        Some((a, _)) => (a.to_string(), s),
        None => {
            let (a, y, b) = dotted_year(&stripped)?;
            let name = clean_album(b, None);
            return Some((tidy(a), AlbumName { year: Some(y), ..name }));
        }
    };
    let name = clean_album(rest, Some(&artist));
    let evidence = name.year.is_some() || !name.extras.is_empty();
    (evidence && !name.album.is_empty() && name.album != tidy(s)).then(|| (tidy(&artist), name))
}

/// A folder that holds music without naming an artist: "Music", iTunes' "Música", a downloads
/// folder, or the one NeatLibrary copies a card's music into.
pub fn container_folder(s: &str) -> bool {
    const NAMES: &[&str] = &[
        "music", "música", "musica", "musique", "musik", "muziek", "itunes", "itunes media", "itunes music", "media", "downloads", "download", "descargas",
        "new music", "unsorted",
    ];
    let s = s.trim().to_lowercase();
    NAMES.contains(&s.as_str()) || s == crate::card::adopt::IMPORT_FOLDER.to_lowercase()
}

/// "Guns N' Roses-Appetite For Destruction": a folder that starts with an artist the library
/// knows (`known` takes an `artist_key`), joined to the album by a dash with or without spaces.
/// Tries the shortest artist first, so "Jay-Z-The Blueprint" needs "Jay-Z" known, not "Jay".
pub fn split_known_artist(s: &str, known: impl Fn(&str) -> bool) -> Option<(String, AlbumName)> {
    s.char_indices().filter(|(_, c)| matches!(c, '-' | '–' | '—')).find_map(|(i, c)| {
        let (artist, rest) = (s[..i].trim(), s[i + c.len_utf8()..].trim());
        let name = clean_album(rest, Some(artist));
        (!artist.is_empty() && !name.album.is_empty() && known(&artist_key(artist))).then(|| (tidy(artist), name))
    })
}

/// An artist folder without collection wording: "Joy Division - коллекция" → "Joy Division",
/// "Haken [restored]" → "Haken", "American Football FLAC" → "American Football".
pub fn clean_artist_folder(s: &str) -> String {
    const COLLECTION: &[&str] = &[
        "коллекция", "дискография", "discography", "complete discography", "collection", "the collection", "complete", "anthology", "albums",
    ];
    const FORMAT: &[&str] = &["flac", "mp3", "lossless", "hi-res", "hires", "24bit", "16bit"];
    let (stripped, _) = strip_extras(s);
    let mut name = stripped.trim().to_string();
    if let Some((a, suffix)) = split_dash_last(&name)
        && COLLECTION.contains(&suffix.trim().to_lowercase().as_str())
    {
        name = a.to_string();
    }
    while let Some((a, word)) = name.rsplit_once(' ')
        && FORMAT.contains(&word.to_lowercase().as_str())
    {
        name = a.to_string();
    }
    let name = tidy(&name);
    if name.is_empty() { tidy(s) } else { name }
}

/// "CD 1", "CD2 - Chet Plays", "Disc 2", "disk_3" → the disc number.
pub fn disc_folder(s: &str) -> Option<u32> {
    let lower = s.trim().to_lowercase();
    let rest = ["disc", "disk", "cd"].iter().find_map(|p| lower.strip_prefix(p))?;
    let rest = rest.trim_start_matches([' ', '.', '_', '-']);
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    let after = &rest[digits.len()..];
    let ok = !digits.is_empty() && digits.len() <= 2 && (after.is_empty() || after.starts_with([' ', '-', '.', '_', '(', '[']));
    ok.then(|| digits.parse().ok()).flatten()
}

/// What a file name says. Tags win; this only fills gaps.
#[derive(Debug, Default, PartialEq)]
pub struct StemParts {
    pub disc: Option<u32>,
    pub track: Option<u32>,
    pub year: Option<u16>,
    pub artist: Option<String>,
    pub title: Option<String>,
}

/// "03 - Title", "1-03 - Title", "03 Artist - Title", "Artist - Title", "1979 - Album" (a year,
/// not an artist or a track), "Artist.2005.Album".
pub fn parse_file_stem(stem: &str) -> StemParts {
    let mut p = StemParts::default();
    let mut rest = stem.trim();
    if let Some((y, after)) = year_prefix(rest) {
        p.year = Some(y);
        rest = after;
    } else if let Some((disc, track, after)) = track_prefix(rest) {
        (p.disc, p.track) = (disc, Some(track));
        rest = after;
    }
    if let Some((a, t)) = split_dash(rest) {
        p.artist = Some(a.trim().to_string());
        p.title = Some(t.trim().to_string());
    } else if let Some((a, y, t)) = dotted_year(rest) {
        p.artist = Some(a.trim().to_string());
        p.year.get_or_insert(y);
        p.title = Some(t.trim().to_string());
    } else if !rest.is_empty() {
        p.title = Some(rest.to_string());
    }
    p
}

/// A song title for comparing with another source: without a leading track number or a
/// leading "<artist> - " ("01 The Beatles - Something" → "Something").
pub fn clean_title(s: &str, artists: &[&str]) -> String {
    let mut rest = s.trim();
    if let Some((_, _, after)) = track_prefix(rest) {
        rest = after;
    }
    for a in artists.iter().filter(|a| !a.trim().is_empty()) {
        if let Some(after) = strip_artist_prefix(rest, a) {
            rest = after;
            break;
        }
    }
    tidy(rest)
}

/// "1979 - X", "1979. X", "1979 X" → (1979, "X"). The year must be followed by more text.
fn year_prefix(s: &str) -> Option<(u16, &str)> {
    let y = year_of(s.get(..4)?)?;
    let after = &s[4..];
    if !after.starts_with([' ', '.', '-', '_', ',']) {
        return None;
    }
    let after = after.trim_start_matches([' ', '.', '-', '_', ',']);
    // "1989 (Taylor's Version)" is a title, not a year and a title.
    let bracket = s[4..].starts_with(' ') && after.starts_with('(');
    (!after.is_empty() && !bracket).then_some((y, after))
}

/// A whole string that is a plausible release year.
fn year_of(s: &str) -> Option<u16> {
    let y: u16 = s.parse().ok().filter(|_| s.len() == 4)?;
    (1900..=2099).contains(&y).then_some(y)
}

/// "03 - X", "03. X", "1-03 X", "2.05_X" → (disc, track, "X"). Up to 3 digits, then a separator.
fn track_prefix(s: &str) -> Option<(Option<u32>, u32, &str)> {
    let digits = |s: &str| s.chars().take_while(char::is_ascii_digit).count();
    let n = digits(s);
    if n == 0 || n > 3 {
        return None;
    }
    let mut disc = None;
    let mut track: u32 = s[..n].parse().ok()?;
    let mut after = &s[n..];
    // "1-03": disc 1, track 3.
    if n <= 2
        && let Some(r) = after.strip_prefix(['-', '.'])
        && digits(r) == 2
        && r[2..].starts_with([' ', '.', '-', '_'])
    {
        disc = Some(track);
        track = r[..2].parse().ok()?;
        after = &r[2..];
    }
    if !after.starts_with([' ', '.', '-', '_', ')']) {
        return None;
    }
    let after = after.trim_start_matches([' ', '.', '-', '_', ')']);
    (!after.is_empty()).then_some((disc, track, after))
}

/// Split at the first " - " (or en/em dash).
fn split_dash(s: &str) -> Option<(&str, &str)> {
    [" - ", " – ", " — "]
        .iter()
        .filter_map(|d| s.find(d).map(|i| (i, d.len())))
        .min()
        .map(|(i, n)| (&s[..i], &s[i + n..]))
        .filter(|(a, b)| !a.trim().is_empty() && !b.trim().is_empty())
}

fn split_dash_last(s: &str) -> Option<(&str, &str)> {
    [" - ", " – ", " — "]
        .iter()
        .filter_map(|d| s.rfind(d).map(|i| (i, d.len())))
        .max()
        .map(|(i, n)| (&s[..i], &s[i + n..]))
        .filter(|(a, b)| !a.trim().is_empty() && !b.trim().is_empty())
}

/// "Dream Theater.2005.Octavarium" → ("Dream Theater", 2005, "Octavarium").
fn dotted_year(s: &str) -> Option<(&str, u16, &str)> {
    s.match_indices('.').find_map(|(i, _)| {
        let y = year_of(s.get(i + 1..i + 5)?)?;
        let after = s[i + 5..].strip_prefix('.')?;
        let before = &s[..i];
        (!before.trim().is_empty() && !after.trim().is_empty()).then_some((before, y, after))
    })
}

/// The rest after "<artist> - ", when the prefix is that artist under `artist_key` rules.
fn strip_artist_prefix<'a>(s: &'a str, artist: &str) -> Option<&'a str> {
    let (a, rest) = split_dash(s)?;
    let key = artist_key(primary_artist(artist).as_str());
    (artist_key(a) == key).then_some(rest)
}

/// Remove bracketed edition details. `[…]` and `{…}` always go; `(…)` only when it holds a year,
/// a catalogue number or edition words, so "(What's the Story)" stays. A trailing "v.1" goes too.
fn strip_extras(s: &str) -> (String, Vec<String>) {
    let mut out = String::with_capacity(s.len());
    let mut extras = Vec::new();
    let mut rest = s;
    while let Some(i) = rest.find(['(', '[', '{']) {
        let open = rest[i..].chars().next().unwrap();
        let close = match open {
            '(' => ')',
            '[' => ']',
            _ => '}',
        };
        let Some(len) = rest[i + 1..].find(close) else { break };
        let inner = &rest[i + 1..i + 1 + len];
        out.push_str(&rest[..i]);
        if open != '(' || is_edition_detail(inner) {
            if !inner.trim().is_empty() {
                extras.push(inner.trim().to_string());
            }
            out.push(' ');
        } else {
            out.push_str(&rest[i..i + len + 2]);
        }
        rest = &rest[i + len + 2..];
    }
    // A bare number after the brackets ("[FLAC] 88") is a rip detail, not part of the name.
    if !extras.is_empty() && !rest.trim().is_empty() && rest.trim().chars().all(|c| c.is_ascii_digit()) {
        extras.push(rest.trim().to_string());
        rest = "";
    }
    out.push_str(rest);
    if let Some((before, last)) = out.trim_end().rsplit_once(' ') {
        let l = last.to_lowercase();
        if let Some(n) = l.strip_prefix("v.").or_else(|| l.strip_prefix('v'))
            && !n.is_empty()
            && n.chars().all(|c| c.is_ascii_digit())
        {
            extras.push(last.to_string());
            out = before.to_string();
        }
    }
    (out, extras)
}

fn is_edition_detail(s: &str) -> bool {
    const WORDS: &[&str] = &[
        "remaster", "remastered", "deluxe", "edition", "expanded", "anniversary", "bonus", "flac", "mp3", "aac", "alac", "lossless", "vinyl", "lp", "cd", "web", "hires",
        "atmos", "mono", "stereo", "japan", "japanese", "import", "reissue", "sacd", "dsd", "disc", "disk", "24bit", "16bit", "hdcd", "издание", "издания",
    ];
    let lower = deunicode(s).to_lowercase();
    let year = lower.split(|c: char| !c.is_ascii_digit()).any(|w| year_of(w).is_some());
    let digits = lower.chars().filter(char::is_ascii_digit).count();
    year || digits >= 4 || lower.split(|c: char| !c.is_alphanumeric()).any(|w| WORDS.contains(&w)) || s.to_lowercase().contains("издани")
}

/// Trim separators and quotes; collapse spaces. "'Disintegration' " → "Disintegration".
fn tidy(s: &str) -> String {
    let collapsed = s.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed.trim_matches([' ', '-', '–', '—', '.', '_', ',', '\'', '"', '«', '»', '“', '”', '‘', '’']).to_string()
}

/// Make a single path component safe for FAT32/exFAT and pleasant to read on a small screen.
pub fn sanitize_component(s: &str, max_chars: usize) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '/' | '\\' | '|' => out.push('-'),
            ':' => out.push_str(" -"),
            '"' => out.push('\''),
            '<' => out.push('('),
            '>' => out.push(')'),
            '?' | '*' => {}
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    let collapsed = out.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut s: String = collapsed.chars().take(max_chars).collect();
    // Trailing dots/spaces are illegal on FAT; a leading dot would hide the entry.
    while s.ends_with(['.', ' ']) {
        s.pop();
    }
    let s = s.trim_start_matches(['.', ' ']).to_string();
    if s.is_empty() { "_".to_string() } else { s }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_fold_trivial_variants() {
        assert_eq!(artist_key("The Beatles"), artist_key("Beatles, The"));
        assert_eq!(artist_key("AC/DC"), artist_key("ACDC"));
        assert_eq!(artist_key("Beyoncé"), artist_key("Beyonce"));
        assert_eq!(artist_key("Simon & Garfunkel"), artist_key("Simon and Garfunkel"));
        assert_eq!(artist_key("Guns N' Roses"), artist_key("Guns N Roses"));
        assert_ne!(artist_key("The The"), "");
    }

    #[test]
    fn featured_artists_are_stripped() {
        assert_eq!(primary_artist("Daft Punk feat. Pharrell Williams"), "Daft Punk");
        assert_eq!(primary_artist("Calvin Harris (feat. Rihanna)"), "Calvin Harris");
        assert_eq!(primary_artist("AC/DC"), "AC/DC");
    }

    #[test]
    fn fuzzy_suggestions() {
        assert!(similar_keys(&artist_key("Guns and Roses"), &artist_key("Guns N Roses")).is_some());
        assert!(similar_keys(&artist_key("Radiohead"), &artist_key("Radiohaed")).is_some());
        assert!(similar_keys(&artist_key("Tool"), &artist_key("Toto")).is_none());
    }

    #[test]
    fn album_folder_names() {
        let a = clean_album("1979. Unknown Pleasures {1989, US, Qwest Records, 9 25840-2} v.1", None);
        assert_eq!((a.album.as_str(), a.year), ("Unknown Pleasures", Some(1979)));
        assert_eq!(a.extras, ["1989, US, Qwest Records, 9 25840-2", "v.1"]);
        let a = clean_album("The Beatles - Love (2006) [Dolby Atmos] {Calderstone Productions Limited}", Some("Beatles"));
        assert_eq!((a.album.as_str(), a.year), ("Love", Some(2006)));
        assert_eq!(clean_album("1997 - OK Computer", None).album, "OK Computer");
        assert_eq!(clean_album("(What's the Story) Morning Glory?", None).album, "(What's the Story) Morning Glory?");
        assert_eq!(clean_album("Hiss Spun (Japan Edition)", None).album, "Hiss Spun");
        assert_eq!(clean_album("Substance (1977-1980)", None).album, "Substance");
        // A title that is only a year stays a title.
        assert_eq!(clean_album("1989 (Taylor's Version)", None).album, "1989 (Taylor's Version)");

        let (artist, a) = split_artist_album("Ritchie Kotzen & Greg Howe - Tilt - 1995 (Shrapnel SH-1085-2) [CD FLAC]").unwrap();
        assert_eq!((artist.as_str(), a.album.as_str(), a.year), ("Ritchie Kotzen & Greg Howe", "Tilt", Some(1995)));
        let (artist, a) = split_artist_album("Todd Rundgren - 1972 - Something - Anything").unwrap();
        assert_eq!((artist.as_str(), a.album.as_str(), a.year), ("Todd Rundgren", "Something - Anything", Some(1972)));
        let (artist, a) = split_artist_album("The Cure - 'Disintegration' [1989, USA, Elektra, 9 60855-2]").unwrap();
        assert_eq!((artist.as_str(), a.album.as_str(), a.year), ("The Cure", "Disintegration", Some(1989)));
        let (artist, a) = split_artist_album("Dream Theater.2005.Octavarium [2LP][24-196]").unwrap();
        assert_eq!((artist.as_str(), a.album.as_str(), a.year), ("Dream Theater", "Octavarium", Some(2005)));
        assert!(split_artist_album("Bits - Pieces").is_none());

        assert_eq!(clean_artist_folder("Joy Division - коллекция"), "Joy Division");
        assert_eq!(clean_artist_folder("Haken [restored]"), "Haken");
        assert_eq!(clean_artist_folder("American Football FLAC"), "American Football");
        assert_eq!(clean_artist_folder("Simon - Garfunkel"), "Simon - Garfunkel");
        assert_eq!(disc_folder("CD2 - Chet Plays"), Some(2));
        assert_eq!(disc_folder("CD 1"), Some(1));
        assert_eq!(disc_folder("Cdubs"), None);
    }

    #[test]
    fn file_names() {
        let p = parse_file_stem("1979 - Unknown Pleasures");
        assert_eq!((p.year, p.artist, p.title.as_deref()), (Some(1979), None, Some("Unknown Pleasures")));
        let p = parse_file_stem("01 - 1979");
        assert_eq!((p.track, p.title.as_deref()), (Some(1), Some("1979")));
        let p = parse_file_stem("1-03 - Title");
        assert_eq!((p.disc, p.track, p.artist, p.title.as_deref()), (Some(1), Some(3), None, Some("Title")));
        let p = parse_file_stem("01 The Beatles - Something");
        assert_eq!((p.track, p.artist.as_deref(), p.title.as_deref()), (Some(1), Some("The Beatles"), Some("Something")));
        let p = parse_file_stem("Dream Theater.2005.Octavarium (Side 1)");
        assert_eq!((p.year, p.artist.as_deref(), p.title.as_deref()), (Some(2005), Some("Dream Theater"), Some("Octavarium (Side 1)")));
        assert_eq!(parse_file_stem("3am").title.as_deref(), Some("3am"));

        assert_eq!(clean_title("01 The Beatles - Something", &["Beatles"]), "Something");
        assert_eq!(clean_title("The Beatles - Something", &["", "The Beatles"]), "Something");
        assert_eq!(clean_title("Ob-La-Di, Ob-La-Da", &["The Beatles"]), "Ob-La-Di, Ob-La-Da");
    }

    #[test]
    fn sanitize_for_fat() {
        assert_eq!(sanitize_component("Album: Part 1/2?", 100), "Album - Part 1-2");
        assert_eq!(sanitize_component("...hidden. ", 100), "hidden");
        assert_eq!(sanitize_component("???", 100), "_");
    }
}
