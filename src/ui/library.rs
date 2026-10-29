//! The library tab: artists, albums and tracks, with letter headings, an A–Z rail, album
//! folders, format badges and MusicBrainz suggestions.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use egui::{Align, Align2, FontId, Layout, RichText, ScrollArea, Sense, Stroke, Ui, vec2};

use super::actions::{Action, apply};
use super::text::{self, plural};
use super::theme::{self, ink};
use super::widgets::{self, caps, link, muted, truncated};
use super::ROW_H;
use crate::app::{App, Lookup};
use crate::library::Track;

/// Longest folder path shown before it's shortened with "…" in the middle.
const FOLDER_CHARS: usize = 60;

/// A track row painted directly so columns line up like a table without any grid lines.
fn track_row(ui: &mut Ui, t: &Track, number: &str, show_artist: bool, playing: bool, can_add: bool) -> (egui::Response, bool) {
    let ink = ink(ui);
    let width = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(width, ROW_H), Sense::click());
    let p = ui.painter_at(rect);
    if resp.hovered() {
        p.rect_filled(rect, 0.0, ink.hover);
    }
    let y = rect.center().y;
    let font = FontId::proportional(14.5);
    let num_color = if playing { ink.accent } else { ink.muted };
    p.text(egui::pos2(rect.left() + 30.0, y), Align2::RIGHT_CENTER, if playing { "▸" } else { number }, font.clone(), num_color);

    let mut title = t.display_title();
    if show_artist && let Some(a) = &t.artist {
        title = format!("{title}  ·  {a}");
    }
    let right_cols = if t.on_recho() { 130.0 } else { 250.0 };
    let galley = truncated(ui, title, font.clone(), if t.plays_here() { ink.text } else { ink.muted }, width - 44.0 - right_cols);
    p.galley(egui::pos2(rect.left() + 44.0, y - galley.size().y / 2.0), galley, ink.text);

    let mut x = rect.right() - 28.0;
    p.text(egui::pos2(x, y), Align2::RIGHT_CENTER, text::duration(t.duration), font.clone(), ink.muted);
    x -= 56.0;
    if !t.on_recho() {
        // A small outlined badge: the format, and what it means for the player.
        let label = text::format_badge(t);
        let g = ui.painter().layout_no_wrap(label, FontId::proportional(11.5), ink.accent);
        let badge = egui::Rect::from_min_size(egui::pos2(x - g.size().x - 8.0, y - g.size().y / 2.0 - 2.0), g.size() + vec2(8.0, 4.0));
        p.rect_stroke(badge, 3.0, Stroke::new(0.8, ink.accent), egui::StrokeKind::Inside);
        p.galley(badge.min + vec2(4.0, 2.0), g, ink.accent);
    } else if t.dirty {
        p.text(egui::pos2(x, y), Align2::RIGHT_CENTER, "edited", FontId::proportional(12.0), ink.muted);
    }
    let mut added = false;
    if can_add {
        let plus = egui::Rect::from_center_size(egui::pos2(rect.right() - 8.0, y), vec2(16.0, ROW_H));
        let r = ui.interact(plus, resp.id.with("add"), Sense::click()).on_hover_text("add to playlist");
        if resp.hovered() || r.hovered() {
            p.text(plus.center(), Align2::CENTER_CENTER, "+", font, if r.hovered() { ink.text } else { ink.muted });
        }
        added = r.clicked();
    }
    let resp = match text::format_note(t) {
        Some(note) => resp.on_hover_text(note),
        None => resp,
    };
    (resp, added)
}

fn album_minutes(tracks: &[Track], idxs: &[usize]) -> String {
    let total: Duration = idxs.iter().map(|&i| tracks[i].duration).sum();
    format!("{} min", total.as_secs().div_ceil(60))
}

pub fn show(app: &mut App, ui: &mut Ui) {
    let ink = ink(ui);
    ui.horizontal(|ui| {
        caps(ui, "music folder");
        let name = app.saved.source.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "not chosen".into());
        ui.label(RichText::new(name).strong()).on_hover_text(app.saved.source.as_ref().map(|p| p.display().to_string()).unwrap_or_default());
        if link(ui, "change").clicked() {
            app.picker = Some(crate::ui::folders::Picker::Source(crate::ui::navigator::Navigator::new(app.saved.source.as_deref())));
        }
    });
    ui.horizontal(|ui| {
        caps(ui, "filter");
        ui.add(egui::TextEdit::singleline(&mut app.filter).hint_text("artist, album or title").desired_width(260.0));
        if link(ui, "rescan").clicked() {
            app.scan();
        }
        if link(ui, "collapse all").clicked() {
            app.expanded.clear();
        }
        let dirty = app.dirty_in_files();
        if dirty > 0 && link(ui, format!("write {dirty} corrected tags to source files")).clicked() {
            app.save_tags_to_source();
        }
    });
    if let Some((n, formats)) = crate::library::non_recho_summary(&app.tracks) {
        ui.add_space(6.0);
        let collapsed = app.saved.format_note_collapsed;
        theme::warning_frame(ui).show(ui, |ui| {
            ui.set_max_width(ui.available_width() - RAIL_W - 40.0);
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(format!("{} in formats the Echo Mini can't play: {formats}", plural(n, "song"))).color(ink.accent));
                if link(ui, if collapsed { "why?" } else { "hide" }).clicked() {
                    app.saved.format_note_collapsed = !collapsed;
                }
            });
            if !collapsed {
                muted(
                    ui,
                    if crate::audio::ffmpeg().is_some() {
                        "RECHO plays FLAC, MP3 and WAV only. Here, AAC, ALAC, AIFF and Ogg directly; Dolby (E-AC-3, AC-3), Opus, APE, WavPack and WMA by converting with ffmpeg first, in stereo; Dolby AC-4 has no decoder yet. On the card they're converted (lossless to FLAC, the rest to MP3 320) unless you choose otherwise in the card tab."
                    } else {
                        "RECHO plays FLAC, MP3 and WAV only. AAC, ALAC, AIFF and Ogg still play here; Dolby (E-AC-3, AC-3), Opus, APE, WavPack and WMA need ffmpeg (brew install ffmpeg). On the card they're converted (lossless to FLAC, the rest to MP3 320) unless you choose otherwise in the card tab."
                    },
                );
            }
        });
    }
    ui.add_space(10.0);
    if app.tracks.is_empty() {
        muted(ui, "Choose the folder that holds your music in the folders tab. It can be as messy as it likes; nothing in it changes unless you ask.");
        return;
    }

    if app.duplicates_only {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Showing albums with a copy that another copy covers in the same or higher quality.").color(ink.text));
            if link(ui, "show all").clicked() {
                app.duplicates_only = false;
            }
        });
        ui.add_space(4.0);
    }
    // Albums (by base id) where one copy covers another, for the duplicates view.
    let duplicated: std::collections::HashSet<String> = app
        .index
        .superseded()
        .into_iter()
        .map(|(a, b)| app.index.artists[a].albums[b].base_id(&app.index.clusters[app.index.artists[a].cluster].key))
        .collect();
    let needle = app.filter.to_lowercase();
    let mut actions = Vec::new();
    let current = app.current();
    let can_add = app.active_playlist.is_some();
    let present: std::collections::BTreeSet<char> = app.index.artists.iter().map(|a| initial(&a.name)).collect();
    let jump = app.jump_to.take();
    let source_root = app.saved.source.clone().unwrap_or_default();
    // The list fills the window up to the rail, which sits at the right edge.
    const RAIL_W: f32 = 28.0;
    let height = ui.available_height();
    let list_w = (ui.available_width() - RAIL_W - 12.0).max(240.0);
    let mut rail_click = None;
    ui.horizontal_top(|ui| {
        // Cells of a horizontal row inherit its left-to-right layout unless told otherwise.
        ui.allocate_ui_with_layout(vec2(list_w, height), Layout::top_down(Align::Min), |ui| {
            ui.set_width(list_w);
            ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                let mut last_initial = None;
                for (ai, artist) in app.index.artists.iter().enumerate() {
                    let akey = crate::library::names::artist_key(&artist.name);
                    let hit = |s: &str| s.to_lowercase().contains(&needle);
                    let ck = &app.index.clusters[artist.cluster].key;
                    let matching: Vec<usize> = if app.duplicates_only {
                        (0..artist.albums.len()).filter(|&i| duplicated.contains(&artist.albums[i].base_id(ck))).collect()
                    } else if needle.is_empty() {
                        (0..artist.albums.len()).collect()
                    } else if hit(&artist.name) {
                        (0..artist.albums.len()).collect()
                    } else {
                        (0..artist.albums.len())
                            .filter(|&i| {
                                let al = &artist.albums[i];
                                al.title.as_deref().is_some_and(hit) || al.tracks.iter().any(|&t| hit(&app.tracks[t].display_title()))
                            })
                            .collect()
                    };
                    if matching.is_empty() {
                        continue;
                    }
                    let open = app.expanded.contains(&akey) || app.duplicates_only || (!needle.is_empty() && matching.len() < 6);
                    let n_tracks: usize = artist.albums.iter().map(|a| a.tracks.len()).sum();
                    // Letter headings, which the A–Z rail scrolls to.
                    let letter = initial(&artist.name);
                    if last_initial != Some(letter) {
                        last_initial = Some(letter);
                        ui.add_space(10.0);
                        let r = ui.label(RichText::new(letter.to_string()).size(13.0).color(ink.muted).strong());
                        ui.painter().hline(r.rect.right() + 8.0..=ui.max_rect().right(), r.rect.center().y, Stroke::new(0.5, ink.faint));
                        if jump == Some(letter) {
                            r.scroll_to_me(Some(Align::TOP));
                        }
                    }
                    ui.add_space(6.0);
                    let r = ui
                        .horizontal(|ui| {
                            let meta = format!("{} · {}", plural(artist.albums.len(), "album"), plural(n_tracks, "track"));
                            let name_w = ui.available_width() - widgets::text_width(ui, &meta, 15.5) - 16.0;
                            let name = RichText::new(&artist.name).size(20.0).color(ink.text);
                            let r = widgets::fitted(ui, name, name_w, Sense::click()).on_hover_text(&artist.name);
                            muted(ui, meta);
                            r
                        })
                        .inner;
                    if r.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                        actions.push(Action::ToggleArtist(akey.clone()));
                    }
                    if !open {
                        continue;
                    }
                    for &bi in &matching {
                        let al = &artist.albums[bi];
                        let id = al.id(&app.index.clusters[artist.cluster].key);
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            ui.add_sized([44.0, 20.0], egui::Label::new(RichText::new(al.year.map(|y| y.to_string()).unwrap_or_default()).color(ink.muted)));
                            let title = al.title.as_deref().unwrap_or("Loose tracks");
                            let mut meta = format!("{} · {}", plural(al.tracks.len(), "track"), album_minutes(&app.tracks, &al.tracks));
                            if let Some(e) = &al.edition {
                                meta = format!("edition {} of {} · {} · {meta}", e.rank + 1, e.count, e.format);
                            }
                            let off = al.tracks.iter().filter(|&&i| !app.tracks[i].on_recho()).count();
                            let off_text = match off {
                                0 => String::new(),
                                n if n == al.tracks.len() => "won't play on RECHO".to_string(),
                                n => format!("{n} won't play on RECHO"),
                            };
                            // The title gets whatever the counts, warning and "play · look up" leave.
                            let reserve = widgets::text_width(ui, &meta, 15.5) + widgets::text_width(ui, &off_text, 12.5) + 150.0;
                            let shown = RichText::new(title).italics().size(17.0);
                            widgets::fitted(ui, shown, ui.available_width() - reserve, Sense::hover()).on_hover_text(title);
                            let r = ui.label(RichText::new(meta).color(ink.muted));
                            if let Some(e) = &al.edition {
                                r.on_hover_text(format!("{}\nThe card gets one edition; choose which in the card tab.", e.label));
                            }
                            if off > 0 {
                                ui.label(RichText::new(off_text).size(12.5).color(ink.accent));
                            }
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if link(ui, "look up").on_hover_text("suggest tags from MusicBrainz").clicked() {
                                    actions.push(Action::LookupAlbum(ai, bi));
                                }
                                if link(ui, "play").clicked() {
                                    actions.push(Action::Play(al.tracks.clone(), 0));
                                }
                            });
                        });
                        album_folders(ui, &app.tracks, &al.tracks, &source_root);
                        if let Some(s) = al.edition.as_ref().and_then(|e| e.superseded.as_ref()) {
                            superseded_note(ui, &app.tracks, &al.tracks, s);
                        }
                        if let Some(l) = app.lookups.get_mut(&id) {
                            lookup_block(ui, &app.tracks, &id, l, &mut actions);
                        }
                        // The duplicates view compares copies; their song lists would bury that.
                        if app.duplicates_only {
                            continue;
                        }
                        for (pos, &ti) in al.tracks.iter().enumerate() {
                            let t = &app.tracks[ti];
                            if !needle.is_empty() && !hit(&artist.name) && !al.title.as_deref().is_some_and(hit) && !hit(&t.display_title()) {
                                continue;
                            }
                            let num = match (t.track, al.multi_disc) {
                                (Some(n), true) => format!("{}-{n}", t.disc.unwrap_or(1)),
                                (Some(n), false) => n.to_string(),
                                (None, _) => String::new(),
                            };
                            let (r, add) = track_row(ui, t, &num, al.various || al.title.is_none(), current == Some(ti), can_add);
                            if r.double_clicked() || r.clicked() {
                                actions.push(Action::Play(al.tracks.clone(), pos));
                            }
                            if add {
                                actions.push(Action::Add(ti));
                            }
                        }
                    }
                }
            });
        });
        ui.add_space(12.0);
        ui.allocate_ui_with_layout(vec2(RAIL_W, height), Layout::top_down(Align::Min), |ui| {
            rail_click = az_rail(ui, &present, height);
        });
    });
    if let Some(c) = rail_click {
        app.jump_to = Some(c);
    }
    apply(app, actions);
}

/// Index letter for an artist: "The Beatles" → B, "Beyoncé" → B, "2Pac" → #.
fn initial(name: &str) -> char {
    match crate::library::names::artist_key(name).chars().next() {
        Some(c) if c.is_ascii_alphabetic() => c.to_ascii_uppercase(),
        _ => '#',
    }
}

/// The A–Z rail; letters without artists are faint. Returns the clicked letter.
fn az_rail(ui: &mut Ui, present: &std::collections::BTreeSet<char>, height: f32) -> Option<char> {
    let ink = ink(ui);
    let mut letters: Vec<char> = std::iter::once('#').chain('A'..='Z').collect();
    // Too short for all 27 at a readable size: keep only the letters that lead somewhere.
    if height / (letters.len() as f32) < 13.0 {
        letters.retain(|c| present.contains(c));
    }
    let row = (height / letters.len().max(1) as f32).clamp(9.0, 20.0);
    ui.spacing_mut().item_spacing.y = 0.0;
    let mut clicked = None;
    for c in letters {
        let has = present.contains(&c);
        let sense = if has { Sense::click() } else { Sense::hover() };
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), row), sense);
        if resp.hovered() && has {
            ui.painter().circle_filled(rect.center(), row * 0.48, ink.faint);
        }
        let color = if has { ink.text } else { ink.faint };
        ui.painter().text(rect.center(), Align2::CENTER_CENTER, c, FontId::proportional((row * 0.72).min(13.0)), color);
        if resp.on_hover_cursor(if has { egui::CursorIcon::PointingHand } else { egui::CursorIcon::Default }).clicked() {
            clicked = Some(c);
        }
    }
    clicked
}

/// A copy whose songs another edition has in the same or higher quality: say so, and what
/// removing it would free. The app never deletes music itself.
fn superseded_note(ui: &mut Ui, tracks: &[Track], idxs: &[usize], s: &crate::library::grouping::Superseded) {
    let ink = ink(ui);
    let bytes: u64 = idxs.iter().map(|&i| tracks[i].size).sum();
    let how = if s.higher { format!("in higher quality ({})", s.format) } else { format!("in the same quality ({})", s.format) };
    ui.horizontal_wrapped(|ui| {
        ui.add_space(52.0);
        ui.label(RichText::new(format!("Edition {} has all these songs {how}; this copy could go ({}).", s.rank + 1, text::bytes(bytes))).size(12.5).italics().color(ink.text))
            .on_hover_text(format!(
                "Edition {}: {}.\nNeatLibrary never deletes music: to remove this copy, open its folder and delete it yourself. Until then, the card gets only one edition anyway.",
                s.rank + 1,
                s.label
            ));
    });
}

/// Where an album's files live on disk, relative to the music folder, with a way to open it.
fn album_folders(ui: &mut Ui, tracks: &[Track], idxs: &[usize], root: &Path) {
    let ink = ink(ui);
    let mut dirs: Vec<&Path> = idxs.iter().filter_map(|&i| tracks[i].path.parent()).collect();
    dirs.sort();
    dirs.dedup();
    let Some(first) = dirs.first().copied() else { return };
    let rel = |d: &Path| {
        let r = d.strip_prefix(root).unwrap_or(d).display().to_string();
        if r.is_empty() { "(top of the music folder)".to_string() } else { r }
    };
    ui.horizontal(|ui| {
        ui.add_space(52.0);
        ui.spacing_mut().item_spacing.x = 6.0;
        widgets::folder_icon(ui, 13.0, ink.muted);
        let text = match dirs.len() {
            1 => text::shorten(&rel(first), FOLDER_CHARS),
            n => format!("{} and {} more folders", text::shorten(&rel(first), FOLDER_CHARS - 20), n - 1),
        };
        // "open folder" goes first so a long path can never push it out of view.
        if link(ui, "open folder").clicked() {
            crate::platform::reveal(first);
        }
        if idxs.iter().any(|&i| tracks[i].cue.is_some()) {
            ui.label(RichText::new("one file + CUE sheet ·").size(12.5).color(ink.muted))
                .on_hover_text("This album is stored as a single file; its CUE sheet lists the songs. On the card each song becomes its own FLAC file.");
        }
        let r = ui.add(egui::Label::new(RichText::new(text).size(12.5).color(ink.muted)).truncate());
        r.on_hover_text(dirs.iter().map(|d| rel(d)).collect::<Vec<_>>().join("\n"));
    });
}

/// MusicBrainz suggestions under an album. Identical changes across tracks collapse into one line.
fn lookup_block(ui: &mut Ui, tracks: &[Track], id: &str, l: &mut Lookup, actions: &mut Vec<Action>) {
    let ink = ink(ui);
    ui.indent(id, |ui| {
        match l {
            Lookup::Pending => {
                muted(ui, "asking MusicBrainz…");
            }
            Lookup::Failed(e) => {
                ui.horizontal_wrapped(|ui| {
                    muted(ui, format!("MusicBrainz: {e}"));
                    if link(ui, "dismiss").clicked() {
                        actions.push(Action::Dismiss(id.to_string()));
                    }
                });
            }
            Lookup::Ready { label, changes } => {
                ui.horizontal_wrapped(|ui| {
                    caps(ui, "musicbrainz");
                    ui.label(RichText::new(label.as_str()).color(ink.text));
                });
                if changes.is_empty() {
                    ui.horizontal(|ui| {
                        muted(ui, "tags already agree.");
                        if link(ui, "dismiss").clicked() {
                            actions.push(Action::Dismiss(id.to_string()));
                        }
                    });
                    return;
                }
                let mut groups: BTreeMap<(usize, String, String), Vec<usize>> = BTreeMap::new();
                let mut order = Vec::new();
                for (i, c) in changes.iter().enumerate() {
                    let key = (c.field as usize, c.old.clone(), c.new.clone());
                    if !groups.contains_key(&key) {
                        order.push(key.clone());
                    }
                    groups.entry(key).or_default().push(i);
                }
                for key in order {
                    let idxs = &groups[&key];
                    let first = changes[idxs[0]].clone();
                    let mut on = idxs.iter().all(|&i| changes[i].accept);
                    ui.horizontal(|ui| {
                        if ui.checkbox(&mut on, "").changed() {
                            for &i in idxs {
                                changes[i].accept = on;
                            }
                        }
                        let scope = if idxs.len() > 1 { format!("{} tracks", idxs.len()) } else { tracks[first.track].display_title() };
                        ui.add_sized([150.0, 18.0], egui::Label::new(RichText::new(format!("{} · {scope}", first.field.label())).size(12.5).color(ink.muted)).truncate());
                        if !first.old.is_empty() {
                            ui.label(RichText::new(&first.old).strikethrough().color(ink.muted));
                            muted(ui, "→");
                        }
                        ui.label(&first.new);
                    });
                }
                ui.horizontal(|ui| {
                    if link(ui, "apply selected").clicked() {
                        actions.push(Action::ApplyLookup(id.to_string()));
                    }
                    if link(ui, "dismiss").clicked() {
                        actions.push(Action::Dismiss(id.to_string()));
                    }
                });
            }
        }
    });
    ui.add_space(4.0);
}
