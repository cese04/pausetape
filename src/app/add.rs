//! Adding new music from another folder (Downloads, a friend's drive…): check it against the
//! library, then copy what the library doesn't have into the music folder, under "Added".

use super::*;
use crate::card::adopt::{ADDED_FOLDER, Survey};

/// A folder checked against the library, waiting for the user to say what to copy.
pub struct Adding {
    pub from: PathBuf,
    pub survey: Survey,
    /// Also copy other versions of songs the library has (an MP3 where it has FLAC…).
    pub versions: bool,
}

impl App {
    /// Check a folder's music against the library in the background (`Msg::FolderSurveyed`).
    pub fn check_folder(&mut self, from: PathBuf) {
        if self.busy.is_some() {
            return;
        }
        // Against an empty library everything would look new.
        if let Some(why) = self.library_unavailable() {
            self.notice(why);
            return;
        }
        if self.saved.source.as_ref().is_some_and(|s| from.starts_with(s) || s.starts_with(&from)) {
            self.notice("that folder is your music folder, or holds it: choose one outside it to add from");
            return;
        }
        let tracks = self.tracks.clone();
        let ctx = self.ctx.clone();
        let name = from.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        self.busy = Some(Busy::new(format!("checking “{name}” against the library"), 0));
        self.spawn(move |tx| {
            // No plan: every exact copy of a library song counts as already there.
            let s = card::adopt::survey(&tracks, &[], &[], &from, |done, total| {
                if done % 25 == 0 {
                    let _ = tx.send(Msg::WriteProgress(done, total));
                    ctx.request_repaint();
                }
            });
            let _ = tx.send(Msg::FolderSurveyed(from, Box::new(s)));
        });
    }

    /// Copy the new music of the folder checked last into the music folder, then rescan.
    pub fn add_checked_folder(&mut self) {
        let Some(music) = self.saved.source.clone() else { return };
        if self.busy.is_some() {
            return;
        }
        // Copying into a music folder whose drive is gone would make the folder on this disk.
        if let Some(why) = self.library_unavailable() {
            self.notice(why);
            return;
        }
        let Some(mut a) = self.adding.take() else { return };
        if a.versions {
            a.survey.unknown.append(&mut a.survey.versions);
        }
        let ctx = self.ctx.clone();
        let (busy, stop) = Busy::stoppable(format!("adding music to “{ADDED_FOLDER}”"), a.survey.unknown.len());
        self.busy = Some(busy);
        self.problems = None;
        self.spawn(move |tx| {
            let out = card::adopt::import(&a.survey, &a.from, &music, ADDED_FOLDER, &stop, |done, total| {
                if done % 10 == 0 {
                    let _ = tx.send(Msg::WriteProgress(done, total));
                    ctx.request_repaint();
                }
            });
            let _ = tx.send(Msg::Added(Box::new(out)));
        });
    }

    /// Bytes that adding would copy (songs only; covers and booklets are small).
    pub fn adding_bytes(&self) -> u64 {
        self.adding.as_ref().map_or(0, |a| Survey::bytes(&a.survey.unknown) + if a.versions { Survey::bytes(&a.survey.versions) } else { 0 })
    }
}
