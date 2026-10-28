//! Words and numbers as shown to the user: durations, sizes, plurals, shortened names, and
//! the notes about formats the Echo Mini can't play.

use std::time::Duration;

pub fn duration(d: Duration) -> String {
    let s = d.as_secs();
    if s >= 3600 { format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60) } else { format!("{}:{:02}", s / 60, s % 60) }
}

pub fn bytes(n: u64) -> String {
    let n = n as f64;
    match n {
        n if n >= 1e9 => format!("{:.1} GB", n / 1e9),
        n if n >= 1e6 => format!("{:.0} MB", n / 1e6),
        n => format!("{:.0} KB", n / 1e3),
    }
}

/// Why a file is flagged, for tooltips.
pub fn format_note(t: &crate::library::Track) -> Option<String> {
    if t.on_recho() {
        return None;
    }
    let label = t.format_label();
    if !t.native() && !t.ffmpeg_can_decode() {
        return Some(format!("{label}: no decoder exists for this format yet, here or on the Echo Mini. It's still organised with the rest."));
    }
    Some(match (t.native(), crate::audio::ffmpeg().is_some()) {
        (true, _) => format!("{label}: plays here, but the Echo Mini (RECHO) won't play it. RECHO plays FLAC, MP3 and WAV only."),
        (false, true) => format!(
            "{label}: plays here by converting it with ffmpeg first (surround is mixed down to stereo). The Echo Mini (RECHO) won't play it."
        ),
        (false, false) => format!("{label}: needs ffmpeg to play here (brew install ffmpeg). The Echo Mini (RECHO) won't play it either."),
    })
}

/// Short badge text: "Dolby E-AC-3 · not on RECHO" / "OPUS · needs ffmpeg" / "Dolby AC-4 · can't play".
pub fn format_badge(t: &crate::library::Track) -> String {
    let state = match (t.plays_here(), t.ffmpeg_can_decode()) {
        (true, _) => "not on RECHO",
        (false, true) => "needs ffmpeg",
        (false, false) => "can't play",
    };
    format!("{} · {state}", t.format_label())
}

/// Cap `s` at `max` characters with an ellipsis in the middle, keeping both ends: for paths and
/// long names the end ("…/2019 - Album") is as telling as the start.
pub fn shorten(s: &str, max: usize) -> String {
    let n = s.chars().count();
    if n <= max || max < 5 {
        return s.to_string();
    }
    let keep = max - 1;
    let head = keep / 2;
    let tail = keep - head;
    let mut out: String = s.chars().take(head).collect();
    out.push('…');
    out.extend(s.chars().skip(n - tail));
    out
}

pub fn plural(n: usize, word: &str) -> String {
    if n == 1 { format!("1 {word}") } else { format!("{n} {word}s") }
}

#[cfg(test)]
mod tests {
    #[test]
    fn shorten_keeps_both_ends() {
        assert_eq!(super::shorten("short", 10), "short");
        assert_eq!(super::shorten("Music/Very Long Artist Name/2019 - Album", 20), "Music/Ver…19 - Album");
        assert_eq!(super::shorten("Música/Ünïcödé", 9).chars().count(), 9);
    }
}
