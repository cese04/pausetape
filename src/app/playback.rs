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
        let path = self.tracks[i].path.clone();
        self.audio.load(&path);
        if self.cover.as_ref().is_none_or(|(p, _)| *p != path) {
            let p = path.clone();
            self.spawn(move |tx| {
                let img = crate::library::cover::load(&p)
                    .map(|rgba| egui::ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw()));
                let _ = tx.send(Msg::Cover(p.clone(), img));
            });
        }
        if self.envelope.as_ref().is_none_or(|(p, _)| *p != path) {
            self.envelope = None;
            self.spawn(move |tx| {
                if let Some(env) = audio::envelope(&path) {
                    let _ = tx.send(Msg::Envelope(path, env));
                }
            });
        }
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
