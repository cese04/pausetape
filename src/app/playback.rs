//! The play queue on top of the audio thread. The thread owns the queue and moves from song to
//! song by itself; the app follows its position (`follow_audio`) to show the right song, cover
//! and waveform whenever the window is drawn.

use super::*;

impl App {
    pub fn play(&mut self, queue: Vec<usize>, start: usize) {
        // Entries nothing here can decode (Opus, APE… without ffmpeg) stay in the queue, so
        // positions match, but the audio thread steps over them.
        let items = queue
            .iter()
            .map(|&i| {
                let t = &self.tracks[i];
                t.plays_here().then(|| (t.path.clone(), t.cue.as_ref().map(|c| (c.start_time(), c.end_time()))))
            })
            .collect();
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
