//! Editing tags by hand: one song, an album, or any songs chosen in the library. Edits are
//! corrections like MusicBrainz's: kept in the app (and the library database), copied into card
//! copies, and written into the music files only with "write corrected tags".

use super::*;

/// One field the songs being edited share. The box starts with their value, or empty when they
/// differ; whatever is typed goes to all of them, and an untouched or empty box changes nothing.
pub struct Shared {
    pub text: String,
    /// The songs' value when they all agree.
    pub was: Option<String>,
}

impl Shared {
    fn of(values: impl Iterator<Item = Option<String>>) -> Shared {
        let mut values = values.map(Option::unwrap_or_default);
        let first = values.next().unwrap_or_default();
        let was = values.all(|v| v == first).then_some(first);
        Shared { text: was.clone().unwrap_or_default(), was }
    }

    pub fn mixed(&self) -> bool {
        self.was.is_none()
    }

    /// What to set, if anything.
    fn new_value(&self) -> Option<&str> {
        let t = self.text.trim();
        (!t.is_empty() && Some(t) != self.was.as_deref().map(str::trim)).then_some(t)
    }
}

pub struct TagEdit {
    pub tracks: Vec<usize>,
    pub album: Shared,
    pub album_artist: Shared,
    pub artist: Shared,
    pub year: Shared,
    pub disc: Shared,
    /// Number and title of each song, as typed.
    pub songs: Vec<(String, String)>,
}

impl TagEdit {
    /// What's typed that can't be saved, for the dialog to say (and to keep Save off).
    pub fn problem(&self) -> Option<String> {
        if self.year.new_value().is_some_and(|y| y.parse::<u16>().is_err()) {
            return Some("The year has to be a number, like 1997.".into());
        }
        if self.disc.new_value().is_some_and(|d| d.parse::<u32>().is_err()) {
            return Some("The disc has to be a number.".into());
        }
        if self.songs.iter().any(|(n, _)| !n.trim().is_empty() && n.trim().parse::<u32>().is_err()) {
            return Some("Song numbers have to be numbers.".into());
        }
        None
    }
}

impl App {
    /// Open the tag editor on these songs (in the order given).
    pub fn edit_tags(&mut self, tracks: Vec<usize>) {
        let tracks: Vec<usize> = tracks.into_iter().filter(|&i| i < self.tracks.len()).collect();
        if tracks.is_empty() {
            return;
        }
        let ts: Vec<&Track> = tracks.iter().map(|&i| &self.tracks[i]).collect();
        self.tag_edit = Some(TagEdit {
            album: Shared::of(ts.iter().map(|t| t.album.clone())),
            album_artist: Shared::of(ts.iter().map(|t| t.album_artist.clone())),
            artist: Shared::of(ts.iter().map(|t| t.artist.clone())),
            year: Shared::of(ts.iter().map(|t| t.year.map(|y| y.to_string()))),
            disc: Shared::of(ts.iter().map(|t| t.disc.map(|d| d.to_string()))),
            songs: ts.iter().map(|t| (t.track.map(|n| n.to_string()).unwrap_or_default(), t.title.clone().unwrap_or_default())).collect(),
            tracks,
        });
    }

    /// Apply the open editor's changes as corrections, and close it.
    pub fn save_tag_edit(&mut self) {
        let Some(e) = self.tag_edit.take() else { return };
        if e.problem().is_some() {
            self.tag_edit = Some(e);
            return;
        }
        let mut touched = Vec::new();
        for (&i, (number, title)) in e.tracks.iter().zip(&e.songs) {
            let t = &mut self.tracks[i];
            let before = (t.album.clone(), t.album_artist.clone(), t.artist.clone(), t.year, t.disc, t.track, t.title.clone());
            if let Some(v) = e.album.new_value() {
                t.album = Some(v.into());
            }
            if let Some(v) = e.album_artist.new_value() {
                t.album_artist = Some(v.into());
            }
            if let Some(v) = e.artist.new_value() {
                t.artist = Some(v.into());
            }
            if let Some(v) = e.year.new_value().and_then(|v| v.parse().ok()) {
                t.year = Some(v);
            }
            if let Some(v) = e.disc.new_value().and_then(|v| v.parse().ok()) {
                t.disc = Some(v);
            }
            if let Ok(n) = number.trim().parse() {
                t.track = Some(n);
            }
            if !title.trim().is_empty() {
                t.title = Some(title.trim().into());
            }
            if before != (t.album.clone(), t.album_artist.clone(), t.artist.clone(), t.year, t.disc, t.track, t.title.clone()) {
                t.dirty = true;
                touched.push(i);
            }
        }
        if touched.is_empty() {
            return;
        }
        if let Some(db) = &mut self.db
            && let Err(e) = db.save_edits(touched.iter().map(|&i| &self.tracks[i]))
        {
            self.notices.push(format!("couldn't save corrections to the library database: {e:#}"));
        }
        self.selected.clear();
        self.rebuild();
        let in_files = touched.iter().any(|&i| self.tracks[i].cue.is_none());
        self.notice(format!(
            "tags changed on {}: card copies get them at the next Sync{}",
            plural_songs(touched.len()),
            if in_files { "; the music files only with \"write corrected tags\"" } else { "" }
        ));
    }

    /// ⌘-click: add a song to the selection, or take it out.
    pub fn toggle_selected(&mut self, track: usize) {
        if !self.selected.remove(&track) {
            self.selected.insert(track);
        }
        self.select_anchor = Some(track);
    }

    /// Shift-click: add a run of songs (from the last one clicked) to the selection.
    pub fn select_run(&mut self, tracks: Vec<usize>) {
        self.select_anchor = tracks.last().copied().or(self.select_anchor);
        self.selected.extend(tracks);
    }
}

fn plural_songs(n: usize) -> String {
    if n == 1 { "1 song".into() } else { format!("{n} songs") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_fields_change_only_what_was_typed() {
        let same = Shared::of([Some("OK Computer".to_string()), Some("OK Computer".to_string())].into_iter());
        assert_eq!((same.text.as_str(), same.new_value()), ("OK Computer", None), "untouched: nothing to set");
        let mixed = Shared::of([Some("1997".to_string()), None].into_iter());
        assert!(mixed.mixed() && mixed.text.is_empty() && mixed.new_value().is_none(), "differing songs start empty and stay as they are");
        let typed = Shared { text: " 1998 ".into(), ..mixed };
        assert_eq!(typed.new_value(), Some("1998"));
        let cleared = Shared { text: String::new(), ..same };
        assert_eq!(cleared.new_value(), None, "an empty box never clears a tag");
    }

    #[test]
    fn numbers_must_be_numbers() {
        let blank = || Shared { text: String::new(), was: None };
        let mut e = TagEdit { tracks: vec![0], album: blank(), album_artist: blank(), artist: blank(), year: blank(), disc: blank(), songs: vec![("3".into(), "Airbag".into())] };
        assert!(e.problem().is_none());
        e.year.text = "nineties".into();
        assert!(e.problem().is_some());
        e.year.text = "1997".into();
        e.songs[0].0 = "3a".into();
        assert!(e.problem().is_some());
    }
}
