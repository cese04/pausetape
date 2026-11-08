//! Testing the card's player: writing the test card's samples, and taking them off again.

use super::*;
use crate::card::testcard;

impl App {
    /// Write the test folder to the active write's card, in the background.
    pub fn write_test_card(&mut self) {
        let Some(dest) = self.write().dest.clone().filter(|d| d.is_dir()) else {
            self.notice("choose the card first, and put it in");
            return;
        };
        if self.busy.is_some() {
            return;
        }
        let tracks = self.tracks.clone();
        self.busy = Some(Busy::new("writing the test folder", crate::card::Sample::ALL.len()));
        self.spawn(move |tx| {
            let made = testcard::write(&dest, &tracks, |n, total| {
                let _ = tx.send(Msg::WriteProgress(n, total));
            });
            let _ = tx.send(Msg::TestCardWritten(made));
        });
    }

    /// The test folder is on the active write's card.
    pub fn test_card_on_card(&self) -> bool {
        self.write().dest.as_ref().is_some_and(|d| d.join(testcard::TEST_FOLDER).is_dir() || d.join(testcard::TEST_PLAYLIST).is_file())
    }

    pub fn remove_test_card(&mut self) {
        let Some(dest) = self.write().dest.clone() else { return };
        if self.busy.is_some() || self.card_missing(&dest) {
            return;
        }
        self.busy = Some(Busy::new("taking the test folder off the card", 0));
        self.spawn(move |tx| {
            let _ = tx.send(Msg::TestCardRemoved(testcard::remove(&dest).map_err(|e| format!("{e:#}"))));
        });
    }

    /// Record what a sample did on the player (None: not tried).
    pub fn set_tested(&mut self, s: crate::card::Sample, answer: Option<bool>) {
        let options = &mut self.write_mut().options;
        let tested = options.tested.entry(options.device).or_default();
        match answer {
            Some(a) => tested.insert(s, a),
            None => tested.remove(&s),
        };
        self.refresh_on_card();
    }
}
