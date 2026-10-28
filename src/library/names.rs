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
    fn sanitize_for_fat() {
        assert_eq!(sanitize_component("Album: Part 1/2?", 100), "Album - Part 1-2");
        assert_eq!(sanitize_component("...hidden. ", 100), "hidden");
        assert_eq!(sanitize_component("???", 100), "_");
    }
}
