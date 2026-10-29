//! Library and artist rows collect what the user clicked as `Action`s while drawing (when
//! the app state is borrowed); `apply` carries them out afterwards.

use crate::app::App;

pub(super) enum Action {
    Play(Vec<usize>, usize),
    ToggleArtist(String),
    LookupAlbum(usize, usize),
    ApplyLookup(String),
    Dismiss(String),
    Add(usize),
    Merge(usize, usize),
    Reject(usize, usize),
    LookupArtist(usize),
    Name(usize, String),
    /// Artist and album by index: on the card or off it, at the next Sync.
    CardAlbum(usize, usize, bool),
    /// The whole artist on the card or off it.
    CardArtist(usize, bool),
}

pub(super) fn apply(app: &mut App, actions: Vec<Action>) {
    for a in actions {
        match a {
            Action::Play(q, i) => app.play(q, i),
            Action::ToggleArtist(k) => {
                if !app.expanded.remove(&k) {
                    app.expanded.insert(k);
                }
            }
            Action::LookupAlbum(a, b) => app.lookup_album(a, b),
            Action::ApplyLookup(id) => app.apply_lookup(&id),
            Action::Dismiss(id) => {
                app.lookups.remove(&id);
            }
            Action::Add(t) => app.add_to_playlist(t),
            Action::Merge(a, b) => app.merge(a, b),
            Action::Reject(a, b) => app.reject(a, b),
            Action::LookupArtist(c) => app.lookup_artist(c),
            Action::CardAlbum(a, b, on) => app.card_album(a, b, on),
            Action::CardArtist(a, on) => app.card_artist(a, on),
            Action::Name(c, n) => {
                app.set_cluster_name(c, n);
                app.renaming = None;
            }
        }
    }
}

