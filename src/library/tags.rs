//! Writing corrected tags back into audio files.

use std::path::Path;

use anyhow::{Context, Result};
use lofty::config::WriteOptions;
use lofty::prelude::*;
use lofty::tag::Tag;
use lofty::tag::items::Timestamp;

use super::track::Track;

/// Write the in-app metadata of `t` into the file at `path` (which may be a copy of `t.path`).
pub fn write_tags(t: &Track, path: &Path) -> Result<()> {
    let mut file = lofty::read_from_path(path).with_context(|| format!("reading {}", path.display()))?;
    if file.primary_tag().is_none() {
        file.insert_tag(Tag::new(file.primary_tag_type()));
    }
    let tag = file.primary_tag_mut().expect("tag inserted above");
    if let Some(v) = &t.artist {
        tag.set_artist(v.clone());
    }
    if let Some(v) = &t.album_artist {
        tag.insert_text(ItemKey::AlbumArtist, v.clone());
    }
    if let Some(v) = &t.album {
        tag.set_album(v.clone());
    }
    if let Some(v) = &t.title {
        tag.set_title(v.clone());
    }
    if let Some(v) = t.track {
        tag.set_track(v);
    }
    if let Some(v) = t.disc {
        tag.set_disk(v);
    }
    if let Some(y) = t.year
        && let Ok(ts) = y.to_string().parse::<Timestamp>()
    {
        tag.set_date(ts);
    }
    tag.save_to_path(path, WriteOptions::default()).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}
