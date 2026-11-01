//! Words and numbers as shown to the user: durations, sizes, plurals, shortened names, and
//! the notes about formats the card's player can't play.

use std::time::Duration;

use crate::card::Player;

pub fn duration(d: Duration) -> String {
    let s = d.as_secs();
    if s >= 3600 { format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60) } else { format!("{}:{:02}", s / 60, s % 60) }
}

/// "8,192".
pub fn count(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

pub fn bytes(n: u64) -> String {
    let n = n as f64;
    match n {
        n if n >= 1e9 => format!("{:.1} GB", n / 1e9),
        n if n >= 1e6 => format!("{:.0} MB", n / 1e6),
        n => format!("{:.0} KB", n / 1e3),
    }
}

/// Why a file is flagged for the card's player, for tooltips.
pub fn format_note(t: &crate::library::Track, device: Player) -> Option<String> {
    if device.plays(t) {
        return None;
    }
    let (label, on) = (t.format_label(), device.short());
    if !t.native() && !t.ffmpeg_can_decode() {
        return Some(format!("{label}: no decoder exists for this format yet, here or on {on}. It's still organised with the rest."));
    }
    Some(match (t.native(), crate::audio::ffmpeg().is_some()) {
        (true, _) => format!("{label}: plays here, but not on {on}, so a card write converts it. {}", device.plays_text()),
        (false, true) => format!("{label}: plays here by converting it with ffmpeg first (surround is mixed down to stereo). It won't play on {on} as it is, so a card write converts it."),
        (false, false) => format!("{label}: needs ffmpeg to play here (brew install ffmpeg). It won't play on {on} either."),
    })
}

/// Short badge text: "Dolby E-AC-3 · not on RECHO" / "OPUS · needs ffmpeg" / "Dolby AC-4 · can't play".
pub fn format_badge(t: &crate::library::Track, device: Player) -> String {
    let state = match (t.plays_here(), t.ffmpeg_can_decode()) {
        (true, _) => format!("not on {}", device.short()),
        (false, true) => "needs ffmpeg".into(),
        (false, false) => "can't play".into(),
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

/// Rounded for a job's remaining time: "less than a minute", "about 12 min", "about 1 h 20 min".
pub fn time_left(d: Duration) -> String {
    let min = (d.as_secs() + 30) / 60;
    match min {
        0 => "less than a minute".into(),
        m if m < 60 => format!("about {m} min"),
        m => format!("about {} h {:02} min", m / 60, m % 60),
    }
}

/// How long ago `t` (seconds since 1970) was: "just now", "5 min ago", "3 days ago".
pub fn ago(t: u64) -> String {
    let s = crate::card::now().saturating_sub(t);
    match s {
        0..60 => "just now".into(),
        60..3600 => format!("{} min ago", s / 60),
        3600..86400 => format!("{} h ago", s / 3600),
        _ => format!("{} days ago", s / 86400),
    }
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
