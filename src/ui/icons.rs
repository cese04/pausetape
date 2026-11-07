//! Small line icons drawn with the painter, so they look the same whatever fonts the system has
//! (few serif faces carry symbols). Each is drawn in a square, in one colour.

use std::f32::consts::PI;

use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, vec2};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Icon {
    // Tabs
    Folder,
    Library,
    Artists,
    Card,
    Playlists,
    Settings,
    // Actions
    Play,
    Search,
    Add,
    Remove,
    Rescan,
    Collapse,
    Rename,
    Trash,
    Sync,
    Compare,
    Back,
    /// What a value becomes (MusicBrainz suggestions).
    Forward,
    Close,
    Info,
    Up,
    Down,
    Copy,
    Check,
    /// A picture: covers.
    Image,
    /// A triangle over a bar: eject the card.
    Eject,
    // The artist panel
    /// A shopping bag: where to buy.
    Buy,
    /// Headphones: where to listen.
    Listen,
    /// A compass: where to find out more.
    Discover,
    /// An arrow out of a box: opens in the browser.
    External,
}

/// Paint `icon` inside `rect` (meant to be square) in `color`.
pub fn paint(p: &Painter, rect: Rect, icon: Icon, color: Color32) {
    let s = rect.width().min(rect.height());
    let o = rect.center() - vec2(s, s) / 2.0;
    // Coordinates are fractions of the square, (0, 0) top left.
    let at = |x: f32, y: f32| o + vec2(x * s, y * s);
    let stroke = Stroke::new((s / 11.0).clamp(1.1, 1.8), color);
    let line = |pts: &[(f32, f32)]| Shape::line(pts.iter().map(|&(x, y)| at(x, y)).collect(), stroke);
    let closed = |pts: &[(f32, f32)]| Shape::closed_line(pts.iter().map(|&(x, y)| at(x, y)).collect(), stroke);
    let arc = |cx: f32, cy: f32, r: f32, from: f32, to: f32| -> Vec<Pos2> {
        let n = 20;
        (0..=n).map(|i| from + (to - from) * i as f32 / n as f32).map(|a| at(cx + r * a.cos(), cy + r * a.sin())).collect()
    };
    // An arrowhead at the end of a path, pointing along its last step.
    let head = |pts: &[Pos2]| {
        let (a, b) = (pts[pts.len() - 2], pts[pts.len() - 1]);
        let d = (b - a).normalized() * s * 0.2;
        let n = vec2(-d.y, d.x);
        Shape::line(vec![b - d + n, b, b - d - n], stroke)
    };
    match icon {
        Icon::Folder => {
            p.add(closed(&[(0.08, 0.22), (0.4, 0.22), (0.5, 0.33), (0.92, 0.33), (0.92, 0.82), (0.08, 0.82)]));
        }
        Icon::Library => {
            // Two beamed notes.
            p.circle_filled(at(0.3, 0.76), s * 0.12, color);
            p.circle_filled(at(0.74, 0.66), s * 0.12, color);
            p.add(line(&[(0.41, 0.76), (0.41, 0.22), (0.85, 0.12), (0.85, 0.66)]));
        }
        Icon::Artists => {
            p.circle_stroke(at(0.5, 0.32), s * 0.17, stroke);
            p.add(Shape::line(arc(0.5, 0.92, 0.33, PI, 2.0 * PI), stroke));
        }
        Icon::Card => {
            p.add(closed(&[(0.22, 0.08), (0.62, 0.08), (0.8, 0.26), (0.8, 0.92), (0.22, 0.92)]));
            for x in [0.36, 0.48, 0.6] {
                p.add(line(&[(x, 0.2), (x, 0.34)]));
            }
        }
        Icon::Playlists => {
            p.add(line(&[(0.1, 0.25), (0.66, 0.25)]));
            p.add(line(&[(0.1, 0.48), (0.66, 0.48)]));
            p.add(line(&[(0.1, 0.71), (0.42, 0.71)]));
            p.add(Shape::convex_polygon(vec![at(0.58, 0.58), at(0.92, 0.75), at(0.58, 0.92)], color, Stroke::NONE));
        }
        Icon::Settings => {
            // Three sliders.
            for (y, x) in [(0.22, 0.32), (0.5, 0.68), (0.78, 0.44)] {
                p.add(line(&[(0.08, y), (0.92, y)]));
                p.circle_filled(at(x, y), s * 0.1, color);
            }
        }
        Icon::Play => {
            p.add(Shape::convex_polygon(vec![at(0.24, 0.14), at(0.86, 0.5), at(0.24, 0.86)], color, Stroke::NONE));
        }
        Icon::Search => {
            p.circle_stroke(at(0.42, 0.42), s * 0.27, stroke);
            p.add(line(&[(0.62, 0.62), (0.88, 0.88)]));
        }
        Icon::Add | Icon::Remove => {
            p.circle_stroke(at(0.5, 0.5), s * 0.42, stroke);
            p.add(line(&[(0.3, 0.5), (0.7, 0.5)]));
            if icon == Icon::Add {
                p.add(line(&[(0.5, 0.3), (0.5, 0.7)]));
            }
        }
        Icon::Rescan => {
            let pts = arc(0.5, 0.52, 0.32, -0.35 * PI, 1.25 * PI);
            p.add(Shape::line(pts.clone(), stroke));
            p.add(head(&pts));
        }
        Icon::Collapse => {
            p.add(line(&[(0.25, 0.12), (0.5, 0.36), (0.75, 0.12)]));
            p.add(line(&[(0.25, 0.88), (0.5, 0.64), (0.75, 0.88)]));
        }
        Icon::Rename => {
            p.add(closed(&[(0.14, 0.86), (0.18, 0.66), (0.68, 0.16), (0.84, 0.32), (0.34, 0.82)]));
            p.add(line(&[(0.58, 0.26), (0.74, 0.42)]));
        }
        Icon::Trash => {
            p.add(line(&[(0.12, 0.24), (0.88, 0.24)]));
            p.add(line(&[(0.38, 0.24), (0.38, 0.12), (0.62, 0.12), (0.62, 0.24)]));
            p.add(line(&[(0.22, 0.24), (0.28, 0.9), (0.72, 0.9), (0.78, 0.24)]));
            p.add(line(&[(0.43, 0.4), (0.43, 0.74)]));
            p.add(line(&[(0.57, 0.4), (0.57, 0.74)]));
        }
        Icon::Sync => {
            let top = arc(0.5, 0.5, 0.34, 1.05 * PI, 1.9 * PI);
            let bottom = arc(0.5, 0.5, 0.34, 0.05 * PI, 0.9 * PI);
            p.add(Shape::line(top.clone(), stroke));
            p.add(head(&top));
            p.add(Shape::line(bottom.clone(), stroke));
            p.add(head(&bottom));
        }
        Icon::Compare => {
            // An eye: look, change nothing.
            let n = 16;
            let upper: Vec<(f32, f32)> = (0..=n).map(|i| i as f32 / n as f32).map(|t| (0.06 + 0.88 * t, 0.5 - 0.3 * (PI * t).sin())).collect();
            let lower: Vec<(f32, f32)> = (0..=n).map(|i| i as f32 / n as f32).map(|t| (0.06 + 0.88 * t, 0.5 + 0.3 * (PI * t).sin())).collect();
            p.add(line(&upper));
            p.add(line(&lower));
            p.circle_filled(at(0.5, 0.5), s * 0.13, color);
        }
        Icon::Back => {
            p.add(line(&[(0.88, 0.5), (0.14, 0.5)]));
            p.add(line(&[(0.4, 0.24), (0.14, 0.5), (0.4, 0.76)]));
        }
        Icon::Forward => {
            p.add(line(&[(0.12, 0.5), (0.86, 0.5)]));
            p.add(line(&[(0.6, 0.24), (0.86, 0.5), (0.6, 0.76)]));
        }
        Icon::Close => {
            p.add(line(&[(0.22, 0.22), (0.78, 0.78)]));
            p.add(line(&[(0.78, 0.22), (0.22, 0.78)]));
        }
        Icon::Info => {
            p.circle_stroke(at(0.5, 0.5), s * 0.42, stroke);
            p.circle_filled(at(0.5, 0.3), s * 0.06, color);
            p.add(line(&[(0.5, 0.44), (0.5, 0.74)]));
        }
        Icon::Up => {
            p.add(line(&[(0.22, 0.64), (0.5, 0.34), (0.78, 0.64)]));
        }
        Icon::Down => {
            p.add(line(&[(0.22, 0.36), (0.5, 0.66), (0.78, 0.36)]));
        }
        Icon::Eject => {
            p.add(closed(&[(0.2, 0.6), (0.5, 0.24), (0.8, 0.6)]));
            p.add(line(&[(0.2, 0.78), (0.8, 0.78)]));
        }
        Icon::Copy => {
            p.add(closed(&[(0.3, 0.3), (0.88, 0.3), (0.88, 0.88), (0.3, 0.88)]));
            p.add(line(&[(0.12, 0.66), (0.12, 0.12), (0.66, 0.12)]));
        }
        Icon::Check => {
            p.add(line(&[(0.16, 0.52), (0.4, 0.76), (0.86, 0.24)]));
        }
        Icon::Image => {
            // A frame with a hill and the sun.
            p.add(closed(&[(0.1, 0.18), (0.9, 0.18), (0.9, 0.82), (0.1, 0.82)]));
            p.add(line(&[(0.1, 0.72), (0.38, 0.44), (0.6, 0.66), (0.72, 0.54), (0.9, 0.72)]));
            p.circle_filled(at(0.68, 0.36), s * 0.07, color);
        }
        Icon::Buy => {
            // A bag with a handle.
            p.add(closed(&[(0.18, 0.36), (0.82, 0.36), (0.88, 0.9), (0.12, 0.9)]));
            p.add(Shape::line(arc(0.5, 0.36, 0.17, PI, 2.0 * PI), stroke));
        }
        Icon::Listen => {
            // A headband over two ear cups.
            p.add(Shape::line(arc(0.5, 0.56, 0.36, PI, 2.0 * PI), stroke));
            p.add(closed(&[(0.14, 0.56), (0.3, 0.56), (0.3, 0.86), (0.14, 0.86)]));
            p.add(closed(&[(0.7, 0.56), (0.86, 0.56), (0.86, 0.86), (0.7, 0.86)]));
        }
        Icon::Discover => {
            // A compass: a ring and its needle.
            p.circle_stroke(at(0.5, 0.5), s * 0.42, stroke);
            p.add(Shape::convex_polygon(vec![at(0.66, 0.34), at(0.56, 0.56), at(0.44, 0.44)], color, Stroke::NONE));
            p.add(Shape::closed_line(vec![at(0.34, 0.66), at(0.44, 0.44), at(0.56, 0.56)], stroke));
        }
        Icon::External => {
            p.add(line(&[(0.62, 0.14), (0.14, 0.14), (0.14, 0.86), (0.86, 0.86), (0.86, 0.38)]));
            let pts = vec![at(0.44, 0.56), at(0.86, 0.14)];
            p.add(Shape::line(pts.clone(), stroke));
            p.add(head(&pts));
        }
    }
}

