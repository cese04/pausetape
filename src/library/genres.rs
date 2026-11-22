//! Genres: read from tags that hold several ("Electronic; Trip Hop"), grouped across spellings
//! ("Trip Hop", "trip-hop", "TripHop" are one), and written back in one style.

use deunicode::deunicode;

/// The genres in a tag's value or values: split where taggers separate several (";", "|", a
/// null, " / "), trimmed, without repeats (by `key`). Commas and bare slashes stay, as genre
/// names use them ("Folk, World, & Country", "AC/DC" in a genre would be odd, but "Hip-Hop/Rap"
/// is one genre in some stores).
pub fn split<'a>(values: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for v in values {
        for part in v.split([';', '|', '\0']).flat_map(|p| p.split(" / ")) {
            let g = part.trim();
            if !g.is_empty() && !out.iter().any(|o| key(o) == key(g)) {
                out.push(g.to_string());
            }
        }
    }
    out
}

/// What a genre is grouped by: letters and digits only, lower case, accents dropped, "&" and
/// "and" alike. "Trip Hop", "trip-hop" and "TripHop" give "triphop"; "R&B" and "R and B", "randb".
pub fn key(genre: &str) -> String {
    let folded = deunicode(genre).to_lowercase().replace('&', " and ");
    folded.split(|c: char| !c.is_alphanumeric()).collect::<String>()
}

/// A genre as MusicBrainz gives it ("trip hop", "r&b") in the style tags use: each word
/// capitalised, after spaces, hyphens and "&" too ("Trip Hop", "R&B", "Lo-Fi").
pub fn tag_style(genre: &str) -> String {
    let mut out = String::with_capacity(genre.len());
    let mut start = true;
    for c in genre.chars() {
        out.extend(if start { c.to_uppercase().collect::<Vec<_>>() } else { vec![c] });
        start = matches!(c, ' ' | '-' | '&' | '/');
    }
    out
}

/// The spelling to show for a genre spelt several ways: the one most songs use, then the
/// first in order.
pub fn usual_spelling<'a>(spellings: impl IntoIterator<Item = &'a str>) -> Option<String> {
    let mut counts: Vec<(&str, usize)> = Vec::new();
    for s in spellings {
        match counts.iter_mut().find(|(t, _)| *t == s) {
            Some((_, n)) => *n += 1,
            None => counts.push((s, 1)),
        }
    }
    let best = counts.iter().map(|(_, n)| *n).max()?;
    counts.into_iter().find(|(_, n)| *n == best).map(|(s, _)| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn several_genres_in_one_tag_come_apart() {
        assert_eq!(split(["Electronic; Trip Hop"]), ["Electronic", "Trip Hop"]);
        assert_eq!(split(["Rock\0Pop", "rock"]), ["Rock", "Pop"], "a null separates ID3v2.4 values; repeats go");
        assert_eq!(split(["Ambient / Downtempo | Chill"]), ["Ambient", "Downtempo", "Chill"]);
        assert_eq!(split(["Folk, World, & Country", "Hip-Hop/Rap"]), ["Folk, World, & Country", "Hip-Hop/Rap"], "commas and bare slashes stay");
        assert!(split(["  ", ";"]).is_empty());
    }

    #[test]
    fn spellings_of_one_genre_share_a_key() {
        assert_eq!(key("Trip Hop"), key("trip-hop"));
        assert_eq!(key("TripHop"), "triphop");
        assert_eq!(key("R&B"), key("R and B"));
        assert_eq!(key("Électronique"), "electronique");
        assert_ne!(key("Rock"), key("Pop"));
    }

    #[test]
    fn musicbrainz_genres_in_tag_style() {
        assert_eq!(tag_style("trip hop"), "Trip Hop");
        assert_eq!(tag_style("r&b"), "R&B");
        assert_eq!(tag_style("lo-fi hip hop"), "Lo-Fi Hip Hop");
    }

    #[test]
    fn the_usual_spelling_wins() {
        assert_eq!(usual_spelling(["trip-hop", "Trip Hop", "Trip Hop"]).as_deref(), Some("Trip Hop"));
        assert_eq!(usual_spelling(["Rock", "rock"]).as_deref(), Some("Rock"), "a tie goes to the first");
        assert_eq!(usual_spelling([]), None);
    }
}
