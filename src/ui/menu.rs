//! The menu a right-click opens on music, wherever it is: an artist, an album (its title, or its
//! row in the table), songs (one, or the selection it's in), a playlist's songs. The same
//! commands in the same order, so what's learnt in one place works in the others. (Play next and
//! Add to the queue show once something's playing; before that, Play does it.)

use egui::Ui;

use super::actions::Action;
use super::icons::Icon;
use super::text::plural;
use super::widgets;
use crate::app::App;

/// What was right-clicked.
pub(super) enum Of {
    /// Songs; one plays on through its album, as a double click does.
    Songs(Vec<usize>),
    Album(usize, usize),
    /// An artist: the card command chooses the whole artist, as its own link does.
    Artist(usize),
}

/// The commands for `of`; `extra` adds the place's own after them (a playlist's "remove").
pub(super) fn music(app: &App, ui: &mut Ui, of: Of, actions: &mut Vec<Action>, extra: impl FnOnce(&mut Ui, &mut Vec<Action>)) {
    let songs: Vec<usize> = match &of {
        Of::Songs(s) => s.clone(),
        Of::Album(a, b) => app.index.artists[*a].albums[*b].tracks.clone(),
        Of::Artist(a) => app.index.artists[*a].albums.iter().flat_map(|al| al.tracks.iter().copied()).collect(),
    };
    if songs.is_empty() {
        return;
    }
    let albums: Vec<(usize, usize)> = match &of {
        Of::Album(a, b) => vec![(*a, *b)],
        _ => app.albums_of(&songs.iter().copied().collect()),
    };
    let them = match &of {
        Of::Songs(s) if s.len() > 1 => format!(" {}", plural(s.len(), "song")),
        _ => String::new(),
    };
    let item = |ui: &mut Ui, actions: &mut Vec<Action>, icon, text: String, action: Action| {
        if widgets::icon_link(ui, icon, text).clicked() {
            actions.push(action);
            ui.close();
        }
    };

    // Play: a single song from where it is in its album.
    let play = match (&of, &albums[..]) {
        (Of::Songs(s), [(a, b)]) if s.len() == 1 => {
            let tracks = &app.index.artists[*a].albums[*b].tracks;
            Action::Play(tracks.clone(), tracks.iter().position(|&t| t == s[0]).unwrap_or(0))
        }
        _ => Action::Play(songs.clone(), 0),
    };
    item(ui, actions, Icon::Play, format!("Play{them}"), play);
    if !app.queue.is_empty() {
        item(ui, actions, Icon::Forward, format!("Play{them} next"), Action::PlayNext(songs.clone()));
        item(ui, actions, Icon::Playlists, format!("Add{them} to the queue"), Action::AddToQueue(songs.clone()));
    }

    ui.menu_button(format!("Add{them} to playlist"), |ui| {
        for (i, p) in app.saved.playlists.iter().enumerate() {
            if widgets::icon_link(ui, Icon::Playlists, &p.name).clicked() {
                actions.push(Action::AddToPlaylist(i, songs.clone()));
                ui.close();
            }
        }
        if !app.saved.playlists.is_empty() {
            ui.separator();
        }
        if widgets::icon_link(ui, Icon::Add, "New playlist").clicked() {
            actions.push(Action::NewPlaylistWith(songs.clone()));
            ui.close();
        }
    });

    match &of {
        Of::Artist(a) => {
            let ck = &app.index.clusters[app.index.artists[*a].cluster].key;
            let whole = app.write().custom.whole_artist(ck);
            let text = if whole { "Take all off the card" } else { "Send all to the card" };
            item(ui, actions, if whole { Icon::Remove } else { Icon::Add }, text.into(), Action::CardArtist(*a, !whole));
        }
        _ => {
            let on = app.all_on_card(&albums);
            let what = if albums.len() > 1 { format!(" {}", plural(albums.len(), "album")) } else { String::new() };
            let text = if on { format!("Take{what} off the card") } else { format!("Send{what} to the card") };
            item(ui, actions, if on { Icon::Remove } else { Icon::Add }, text, Action::ToggleCard(albums.clone()));
        }
    }

    item(ui, actions, Icon::Rename, format!("Edit tags{}", if them.is_empty() { String::new() } else { format!(" of{them}") }), Action::EditTags(songs.clone()));

    let coverless: Vec<(usize, usize)> = albums.iter().copied().filter(|&(a, b)| app.lacks_cover(&app.index.artists[a].albums[b])).collect();
    if !coverless.is_empty() && app.busy.is_none() {
        let text = if coverless.len() == 1 { "Find cover".to_string() } else { format!("Find covers ({})", coverless.len()) };
        item(ui, actions, Icon::Image, text, Action::FindCovers(coverless));
    }
    if let [(a, b)] = albums[..] {
        item(ui, actions, Icon::Search, "Look up on MusicBrainz".into(), Action::LookupAlbum(a, b));
    }
    if app.busy.is_none() && widgets::icon_link(ui, Icon::Search, format!("Find lyrics{}", if them.is_empty() { String::new() } else { format!(" for{them}") })).on_hover_text("from the songs' own, else LRCLIB; kept to read offline").clicked() {
        actions.push(Action::FindLyrics(songs.clone()));
        ui.close();
    }
    if let [(a, b)] = albums[..]
        && !matches!(of, Of::Artist(_))
        && widgets::icon_link(ui, Icon::Info, "Details").clicked()
    {
        actions.push(Action::Details(a, b));
        ui.close();
    }
    if let Some(dir) = app.tracks[songs[0]].path.parent()
        && widgets::icon_link(ui, Icon::Folder, "Open folder").clicked()
    {
        crate::platform::reveal(dir);
        ui.close();
    }
    extra(ui, actions);
}
