//! The play queue on top of the audio thread.

use super::*;

impl App {
    pub fn play(&mut self, queue: Vec<usize>, start: usize) {
        self.queue = queue;
        self.qpos = start;
        self.play_current();
    }

    pub fn current(&self) -> Option<usize> {
        self.queue.get(self.qpos).copied()
    }

    fn play_current(&mut self) {
        // Step over files no decoder here can open (Opus, APE, WavPack, WMA).
        let start = self.qpos;
        while self.current().is_some_and(|i| !self.tracks[i].plays_here()) {
            self.qpos += 1;
        }
        if self.qpos != start {
            let n = self.qpos - start;
            self.notice(format!("skipped {} NeatLibrary can't play", plural_files(n)));
        }
        let Some(i) = self.current() else {
            self.qpos = start.min(self.queue.len().saturating_sub(1));
            self.audio.stop();
            return;
        };
        let t = &self.tracks[i];
        let (path, id) = (t.path.clone(), t.id());
        let span = t.cue.as_ref().map(|c| (c.start_time(), c.end_time()));
        self.audio.load(&path, span);
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

    /// Look up the cover for a track path in the background (`Msg::Cover`).
    pub fn load_cover(&mut self, p: std::path::PathBuf) {
        self.spawn(move |tx| {
            let found = crate::library::cover::load(&p);
            let file = found.as_ref().and_then(|(_, f)| f.clone());
            let img = found.map(|(rgba, _)| egui::ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw()));
            let _ = tx.send(Msg::Cover(p.clone(), img, file));
        });
    }

    pub fn next(&mut self) {
        if self.qpos + 1 < self.queue.len() {
            self.qpos += 1;
            self.play_current();
        } else {
            self.audio.stop();
        }
    }

    pub fn prev(&mut self) {
        // Like most players: restart the track unless we're near its start.
        if self.audio.status().pos.as_secs() > 3 || self.qpos == 0 {
            self.audio.seek(std::time::Duration::ZERO);
        } else {
            self.qpos -= 1;
            self.play_current();
        }
    }
}
