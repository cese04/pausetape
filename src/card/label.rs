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
}

impl Label {
    pub fn of(w: &WriteProfile) -> Label {
        Label { id: w.id.clone(), name: w.name.clone(), device: w.options.device, written: super::now() }
    }

    /// The label at the top of `card`, if Pausetape wrote one there.
    pub fn read(card: &Path) -> Option<Label> {
        let bytes = std::fs::read(card.join(LABEL_FILE)).or_else(|_| std::fs::read(card.join(former_label_file()))).ok()?;
        let l: Label = serde_json::from_slice(&bytes).ok()?;
        (!l.id.is_empty()).then_some(l)
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

/// A new setup's id: random, 16 hex digits.
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
