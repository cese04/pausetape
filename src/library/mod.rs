//! Reading and understanding the music folder: scanning, tags, the library database, cover
//! art, and grouping tracks into artists and albums.

pub mod cover;
pub mod cue;
pub mod db;
pub mod grouping;
pub mod mp4;
pub mod names;
mod scan;
mod tags;
mod track;

pub use scan::{ScanStats, mtime_of, scan};
pub use tags::write_tags;
pub use track::{Track, non_recho_summary};
