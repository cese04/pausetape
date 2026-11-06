//! The play queue on top of the audio thread. The thread owns the queue and moves from song to
//! song by itself; the app follows its position (`follow_audio`) to show the right song, cover
//! and waveform whenever the window is drawn.

use super::*;

impl App {
    /// A song as the audio thread plays it. Songs nothing here can decode (Opus, APE… without
    /// ffmpeg) stay in the queue as `None`, so positions match, and the thread steps over them.
    fn queue_item(&self, i: usize) -> crate::audio::Item {
        let t = &self.tracks[i];
        t.plays_here().then(|| (t.path.clone(), t.cue.as_ref().map(|c| (c.start_time(), c.end_time()))))
    }

    pub fn play(&mut self, queue: Vec<usize>, start: usize) {
        let items = queue.iter().map(|&i| self.queue_item(i)).collect();
        self.queue = queue;
        self.qpos = start;
        self.audio.play_queue(items, start);
        self.show_current();
    }

    pub fn current(&self) -> Option<usize> {
        self.queue.get(self.qpos).copied()
    }

    /// Catch up with the audio thread: the song it's on, and songs it stepped over.
    pub(super) fn follow_audio(&mut self) {
        let (skipped, why) = self.audio.take_skipped();
        if skipped > 0 {
            let why = why.map(|w| format!(" (last: {w})")).unwrap_or_default();
            self.notice(format!("skipped {} Pausetape can't play{why}", plural_files(skipped)));
        }
        if let Some(i) = self.audio.status().index
            && i != self.qpos
            && i < self.queue.len()
        {
            self.qpos = i;
            // The lyrics follow the song that's begun, not one chosen before it.
            self.lyrics_song = None;
            self.show_current();
        }
    }

    /// Load the cover and waveform of the song at `qpos`, if not already shown.
    fn show_current(&mut self) {
        let Some(i) = self.current() else { return };
        let t = &self.tracks[i];
        let (path, id) = (t.path.clone(), t.id());
        let span = t.cue.as_ref().map(|c| (c.start_time(), c.end_time()));
        if self.cover.as_ref().is_none_or(|(p, _, _)| *p != path) {
            self.load_cover(path.clone());
        }
        if self.envelope.as_ref().is_none_or(|(p, _)| *p != id) {
            self.envelope = None;
            self.spawn(move |tx| {
                if let Some(env) = audio::envelope(&path, span) {
                    let _ = tx.send(Msg::Envelope(id, env));
                }
            });
        }
    }

    /// Look up the cover for a track path in the background (`Msg::Cover`): one waiting to be
    /// embedded, else the file's own.
    pub fn load_cover(&mut self, p: std::path::PathBuf) {
        let pending = self.tracks.iter().find(|t| t.path == p).and_then(|t| t.cover.clone());
        self.spawn(move |tx| {
            let found = match pending {
                Some(f) => crate::library::cover::load_image(&f).map(|img| (img, Some(f))),
                None => crate::library::cover::load(&p),
            };
            let file = found.as_ref().and_then(|(_, f)| f.clone());
            let img = found.map(|(rgba, _)| egui::ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw()));
            let _ = tx.send(Msg::Cover(p.clone(), img, file));
        });
    }

    /// Change the queue while it plays: `entries` is the new queue, each song with where it was
    /// (`None` for one just added). What's playing goes on.
    fn edit_queue(&mut self, entries: Vec<(usize, Option<usize>)>) {
        let moved = moved_to(self.queue.len(), &entries);
        let items = entries.iter().map(|&(t, _)| self.queue_item(t)).collect();
        self.queue = entries.iter().map(|&(t, _)| t).collect();
        self.qpos = moved.get(self.qpos).copied().flatten().unwrap_or(0);
        self.audio.edit_queue(items, moved);
    }

    /// The queue as it is, each song with its own position: the start of an edit.
    fn queue_entries(&self) -> Vec<(usize, Option<usize>)> {
        self.queue.iter().enumerate().map(|(i, &t)| (t, Some(i))).collect()
    }

    /// Play these songs after the one playing (or now, if nothing is queued).
    pub fn play_next(&mut self, songs: &[usize]) {
        if self.queue.is_empty() {
            return self.play(songs.to_vec(), 0);
        }
        let mut e = self.queue_entries();
        let at = (self.qpos + 1).min(e.len());
        e.splice(at..at, songs.iter().map(|&t| (t, None)));
        self.edit_queue(e);
        self.notice(format!("{} next", self.songs_named(songs, "plays", "play")));
    }

    /// Add these songs to the end of the queue (or play them, if nothing is queued).
    pub fn add_to_queue(&mut self, songs: &[usize]) {
        if self.queue.is_empty() {
            return self.play(songs.to_vec(), 0);
        }
        let mut e = self.queue_entries();
        e.extend(songs.iter().map(|&t| (t, None)));
        self.edit_queue(e);
        self.notice(format!("{} at the end of the queue", self.songs_named(songs, "plays", "play")));
    }

    /// Take a song out of the queue (not the one playing).
    pub fn remove_from_queue(&mut self, pos: usize) {
        if pos == self.qpos || pos >= self.queue.len() {
            return;
        }
        let mut e = self.queue_entries();
        e.remove(pos);
        self.edit_queue(e);
    }

    /// Move a song in the queue from `from` to `to`.
    pub fn move_in_queue(&mut self, from: usize, to: usize) {
        if from >= self.queue.len() || to >= self.queue.len() || from == to {
            return;
        }
        let mut e = self.queue_entries();
        let entry = e.remove(from);
        e.insert(to, entry);
        self.edit_queue(e);
    }

    /// Take out everything after the song playing.
    pub fn clear_upcoming(&mut self) {
        let mut e = self.queue_entries();
        e.truncate(self.qpos + 1);
        self.edit_queue(e);
    }

    /// "“Hunter” plays" or "3 songs play", for notices about the queue.
    fn songs_named(&self, songs: &[usize], one: &str, many: &str) -> String {
        match songs {
            [t] => format!("“{}” {one}", self.tracks[*t].display_title()),
            _ => format!("{} songs {many}", songs.len()),
        }
    }

    /// The cover of the album whose first song is `p`, for the details panel, unless it's the
    /// one shown or on its way.
    pub fn want_detail_cover(&mut self, p: &std::path::Path) {
        if self.detail_cover.as_ref().is_some_and(|(q, _)| q == p) {
            return;
        }
        self.detail_cover = Some((p.to_path_buf(), None));
        let p = p.to_path_buf();
        let pending = self.tracks.iter().find(|t| t.path == p).and_then(|t| t.cover.clone());
        self.spawn(move |tx| {
            let rgba = match pending {
                Some(f) => crate::library::cover::load_image(&f),
                None => crate::library::cover::load(&p).map(|(img, _)| img),
            };
            let img = rgba.map(|rgba| egui::ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw()));
            let _ = tx.send(Msg::DetailCover(p, img));
        });
    }

    pub fn next(&mut self) {
        if self.qpos + 1 < self.queue.len() {
            self.audio.jump(self.qpos + 1);
        } else {
            self.audio.stop();
        }
    }

    pub fn prev(&mut self) {
        // Like most players: restart the track unless we're near its start. Going back steps
        // over songs that can't play here, as going forward does.
        let before = (0..self.qpos).rev().find(|&q| self.tracks[self.queue[q]].plays_here());
        match before {
            Some(q) if self.audio.status().pos.as_secs() <= 3 => self.audio.jump(q),
            _ => self.audio.seek(std::time::Duration::ZERO),
        }
    }
}

/// Where each entry of a queue `len` long went in `entries` (each with the position it had,
/// `None` for one added). An entry taken out goes where the one before it went, so playback
/// carries on after it; `None` if nothing before it stayed.
fn moved_to(len: usize, entries: &[(usize, Option<usize>)]) -> Vec<Option<usize>> {
    let mut moved = vec![None; len];
    for (new, &(_, old)) in entries.iter().enumerate() {
        if let Some(old) = old.filter(|&o| o < len) {
            moved[old] = Some(new);
        }
    }
    let mut last = None;
    for m in &mut moved {
        match m {
            Some(_) => last = *m,
            None => *m = last,
        }
    }
    moved
}

#[cfg(test)]
mod tests {
    use super::moved_to;

    #[test]
    fn songs_inserted_after_the_one_playing_move_the_rest_along() {
        // Playing position 1 of [a, b, c]; two songs go in after it: [a, b, x, y, c].
        let e = [(10, Some(0)), (11, Some(1)), (20, None), (21, None), (12, Some(2))];
        assert_eq!(moved_to(3, &e), vec![Some(0), Some(1), Some(4)]);
    }

    #[test]
    fn a_song_taken_out_hands_its_place_to_the_one_before() {
        // [a, b, c, d] without c: playing c would go on with d, after b's new place.
        let e = [(10, Some(0)), (11, Some(1)), (13, Some(3))];
        assert_eq!(moved_to(4, &e), vec![Some(0), Some(1), Some(1), Some(2)]);
        // Without a, nothing before it stayed: from the start.
        let e = [(11, Some(1)), (12, Some(2))];
        assert_eq!(moved_to(3, &e), vec![None, Some(0), Some(1)]);
    }

    #[test]
    fn a_moved_song_is_followed() {
        // [a, b, c] with a moved to the end: [b, c, a].
        let e = [(11, Some(1)), (12, Some(2)), (10, Some(0))];
        assert_eq!(moved_to(3, &e), vec![Some(2), Some(0), Some(1)]);
    }
}
