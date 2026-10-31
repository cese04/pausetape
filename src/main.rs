//! NeatLibrary: organise a music collection, write it to SD cards in the Artist / Album / Track
//! folders a portable player browses (the Snowsky Echo Mini, with RECHO or its own firmware),
//! and listen to it on the way.

mod app;
mod audio;
mod card;
mod library;
mod musicbrainz;
mod platform;
mod ui;

/// Name shown to the user. A placeholder: renaming the app means changing this, `APP_NAME` in
/// `scripts/package-macos.sh`, and the bundle identifier there.
pub const APP_NAME: &str = "NeatLibrary";
/// Folder name for settings, the library database and undo journals. Keep it stable across a
/// rename (or migrate the folder), or users lose their settings.
pub const APP_ID: &str = "NeatLibrary";

fn main() -> eframe::Result {
    // Headless dry run: `neatlibrary --plan <music folder>` prints the layout without touching files.
    let args: Vec<String> = std::env::args().collect();
    // `--version`: what's running and which ffmpeg it uses (handy for bug reports).
    if args.get(1).is_some_and(|a| a == "--version") {
        let ffmpeg = match audio::ffmpeg() {
            Some(p) if audio::ffmpeg_is_bundled() => format!("bundled ({})", p.display()),
            Some(p) => p.display().to_string(),
            None => "not found".into(),
        };
        println!("{APP_NAME} {} · ffmpeg: {ffmpeg}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if let [_, flag, dir] = args.as_slice()
        && flag == "--plan"
    {
        print_plan(std::path::Path::new(dir));
        return Ok(());
    }
    audio::clear_converted();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title(APP_NAME).with_inner_size([1040.0, 760.0]).with_min_inner_size([720.0, 480.0]),
        ..Default::default()
    };
    eframe::run_native(APP_ID, options, Box::new(|cc| Ok(Box::new(app::App::new(cc)))))
}

fn print_plan(root: &std::path::Path) {
    let (tracks, _) = library::scan(root, None, false, |_| {});
    let index = library::grouping::build(&tracks, &library::grouping::ArtistPrefs::default());
    for s in &index.suggestions {
        println!("possible duplicate: {} ≈ {} ({:.2})", index.clusters[s.a].name, index.clusters[s.b].name, s.score);
    }
    for p in card::build_plan(&tracks, &index, &card::Options::default(), &card::Custom::default()) {
        let t = &tracks[p.track];
        let id = t.id();
        let src = id.strip_prefix(root).unwrap_or(&id);
        println!("{:<60} <- {}{}", p.rel.display(), src.display(), p.skip.map(|s| format!("  [{s}]")).unwrap_or_default());
    }
    // Albums in several copies: which goes on the card, and which another copy covers.
    for a in &index.artists {
        for al in a.albums.iter().filter(|al| al.edition.is_some()) {
            let e = al.edition.as_ref().unwrap();
            let note = match &e.superseded {
                Some(s) => format!("covered by edition {} ({} quality)", s.rank + 1, if s.higher { "higher" } else { "same" }),
                None if e.rank == 0 => "on the card".into(),
                None => "keep: has songs or quality the others lack".into(),
            };
            let title = al.title.as_deref().unwrap_or_default();
            println!("edition {}/{} {} / {title}: {} · {} · {} songs · {note}", e.rank + 1, e.count, a.name, e.label, e.format, al.tracks.len());
        }
    }
}
