//! The card's label: a small file at the top of the card naming the card setup it belongs to,
//! as calibre's `metadata.calibre` does, so a card plugged in brings up its own setup and player,
//! and one setup can't sync over another's card. Players skip it: it isn't music.

use std::hash::{BuildHasher, Hasher};
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use super::{Device, WriteProfile};

pub const LABEL_FILE: &str = ".pausetape";

/// The label as the app wrote it under its former name: read, and replaced at the next write.
fn former_label_file() -> String {
    format!(".{}", crate::FORMER_NAME.to_lowercase())
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Label {
    /// `WriteProfile::id` of the setup that last wrote the card.
    pub id: String,
    /// Its name and player then, for a card whose setup isn't on this Mac.
    pub name: String,
    pub device: Device,
    /// Unix seconds.
    pub written: u64,
    /// The card's own id, kept whichever setup labels it: what tells two cards apart. Empty on
    /// labels written before cards had one.
    pub card: String,
}

/// What's at the top of a card, label-wise.
pub enum Found {
    Label(Label),
    /// No label: the files aren't there.
    NotThere,
    /// A label that can't be read, or isn't one. Says nothing about which card this is.
    CantTell,
}

impl Label {
    pub fn of(w: &WriteProfile) -> Label {
        Label { id: w.id.clone(), name: w.name.clone(), device: w.options.device, written: super::now(), card: w.card_id.clone().unwrap_or_default() }
    }

    /// This setup's label for `card`, with the id the card already has, or a new one.
    pub fn for_card(w: &WriteProfile, card: &Path) -> Label {
        let id = Label::read(card).map(|l| l.card).filter(|c| !c.is_empty()).unwrap_or_else(new_id);
        Label { card: id, ..Label::of(w) }
    }

    pub fn find(card: &Path) -> Found {
        use std::io::ErrorKind::NotFound;
        let bytes = match std::fs::read(card.join(LABEL_FILE)) {
            Err(e) if e.kind() == NotFound => std::fs::read(card.join(former_label_file())),
            read => read,
        };
        match bytes {
            Ok(bytes) => match serde_json::from_slice::<Label>(&bytes) {
                Ok(l) if !l.id.is_empty() => Found::Label(l),
                _ => Found::CantTell,
            },
            Err(e) if e.kind() == NotFound => Found::NotThere,
            Err(_) => Found::CantTell,
        }
    }

    /// The label at the top of `card`, if Pausetape wrote one there.
    pub fn read(card: &Path) -> Option<Label> {
        match Label::find(card) {
            Found::Label(l) => Some(l),
            _ => None,
        }
    }

    pub fn save(&self, card: &Path) -> Result<()> {
        let path = card.join(LABEL_FILE);
        write_hidden(&path, &serde_json::to_vec_pretty(self)?).with_context(|| format!("labelling the card ({})", path.display()))?;
        // macOS adds a "._" companion on FAT cards.
        let _ = std::fs::remove_file(card.join(format!("._{LABEL_FILE}")));
        let former = former_label_file();
        for name in [former.clone(), format!("._{former}")] {
            let _ = std::fs::remove_file(card.join(name));
        }
        Ok(())
    }
}

/// Write a file the player's owner needn't see. The leading dot hides it on macOS and Linux;
/// Windows needs its hidden attribute, which must also be given to write over it once set.
fn write_hidden(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut open = std::fs::OpenOptions::new();
    open.write(true).create(true).truncate(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
        open.attributes(FILE_ATTRIBUTE_HIDDEN);
    }
    std::io::Write::write_all(&mut open.open(path)?, bytes)
}

/// Whether the card found is the one `w`'s record describes. Only a clear answer says no: a
/// label that can't be read must never cost a setup its record.
pub fn same_card(w: &WriteProfile, found: &Found) -> bool {
    match (&w.card_id, found) {
        (_, Found::CantTell) => true,
        (Some(id), Found::Label(l)) if !l.card.is_empty() => *id == l.card,
        // No ids to compare (from before cards had one): whose label it is will do.
        (_, Found::Label(l)) => l.id == w.id,
        (Some(_), Found::NotThere) => false,
        (None, Found::NotThere) => true,
    }
}

/// A new setup's or card's id: random, 16 hex digits.
pub fn new_id() -> String {
    // Each `RandomState` has fresh random keys; the time makes ids differ within a run too.
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0));
    format!("{:016x}", h.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_name_their_setup() {
        let card = std::env::temp_dir().join(format!("pausetape-test-label-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&card);
        std::fs::create_dir_all(&card).unwrap();
        assert_eq!(Label::read(&card), None);
        let w = WriteProfile { name: "Road trip".into(), ..Default::default() };
        assert_eq!(w.id.len(), 16);
        assert_ne!(w.id, WriteProfile::default().id, "each setup gets its own id");
        // Every write labels the card, even one with nothing to copy.
        let out = super::super::write(&w, &[], &[], &[], &card, &card, &std::sync::atomic::AtomicBool::new(false), |_| {});
        assert!(out.errors.is_empty() && out.manifest.is_empty(), "the label isn't one of the write's files");
        let l = Label::read(&card).unwrap();
        assert_eq!((l.id.as_str(), l.name.as_str(), l.device), (w.id.as_str(), "Road trip", Device::EchoMiniRecho));
        // Saved setups from before ids get one when they load.
        let old: WriteProfile = serde_json::from_str(r#"{"name":"Echo Mini"}"#).unwrap();
        assert_eq!(old.id.len(), 16);
        // Not Pausetape's, or damaged: no label.
        std::fs::write(card.join(LABEL_FILE), "{}").unwrap();
        assert_eq!(Label::read(&card), None);
        let _ = std::fs::remove_dir_all(&card);
    }

    #[test]
    fn a_card_is_known_by_its_own_id() {
        let w = WriteProfile { card_id: Some("card a".into()), ..Default::default() };
        let label = |id: &str, card: &str| Found::Label(Label { id: id.into(), card: card.into(), ..Default::default() });
        assert!(same_card(&w, &label(&w.id, "card a")));
        assert!(same_card(&w, &label("another setup", "card a")), "still this card, relabelled by another setup");
        assert!(!same_card(&w, &label(&w.id, "card b")), "a second card synced by the same setup");
        assert!(!same_card(&w, &Found::NotThere), "no label where this setup put one");
        assert!(same_card(&w, &Found::CantTell), "a failed read says nothing");
        // Labels and setups from before cards had ids.
        assert!(same_card(&w, &label(&w.id, "")));
        assert!(!same_card(&w, &label("another setup", "")));
        let old = WriteProfile { manifest: vec![super::super::Written { rel: "a.mp3".into(), size: 1, source: None }], ..Default::default() };
        assert!(same_card(&old, &Found::NotThere), "written before labels: nothing says it's another card");
        assert!(same_card(&old, &label(&old.id, "card a")));
        assert!(!same_card(&old, &label("another setup", "card a")));
    }

    #[test]
    fn a_card_keeps_its_id_whoever_labels_it() {
        let card = std::env::temp_dir().join(format!("pausetape-test-card-id-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&card);
        std::fs::create_dir_all(&card).unwrap();
        assert!(matches!(Label::find(&card), Found::NotThere));
        let (a, b) = (WriteProfile::default(), WriteProfile::default());
        let first = Label::for_card(&a, &card);
        assert_eq!(first.card.len(), 16);
        first.save(&card).unwrap();
        let second = Label::for_card(&b, &card);
        assert_eq!((second.card.as_str(), second.id.as_str()), (first.card.as_str(), b.id.as_str()));
        std::fs::write(card.join(LABEL_FILE), "not a label").unwrap();
        assert!(matches!(Label::find(&card), Found::CantTell));
        let _ = std::fs::remove_dir_all(&card);
    }

    #[test]
    fn a_label_from_the_former_name_is_read_then_replaced() {
        let card = std::env::temp_dir().join(format!("pausetape-test-former-label-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&card);
        std::fs::create_dir_all(&card).unwrap();
        let l = Label { id: "0123456789abcdef".into(), name: "Echo Mini".into(), ..Default::default() };
        std::fs::write(card.join(former_label_file()), serde_json::to_vec(&l).unwrap()).unwrap();
        assert_eq!(Label::read(&card).as_ref(), Some(&l));
        l.save(&card).unwrap();
        assert!(card.join(LABEL_FILE).is_file() && !card.join(former_label_file()).exists());
        assert_eq!(Label::read(&card), Some(l));
        let _ = std::fs::remove_dir_all(&card);
    }
}
