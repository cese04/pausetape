//! Reading and understanding the music folder: scanning, tags, the library database, cover
//! art, and grouping tracks into artists and albums.

pub mod cover;
pub mod cue;
pub mod db;
pub mod dsd;
pub mod grouping;
pub mod mp4;
pub mod names;
pub mod search;
mod scan;
mod tags;
mod track;

pub use scan::{AUDIO, ScanStats, load, mtime_of, read_file, scan};
pub use tags::{carry_cover, write_tags};
pub use track::{Track, unplayable_summary};
