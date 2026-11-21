//! Small monochrome line icons drawn with the painter (no emoji, no image files).

use eframe::egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, Vec2, epaint::CubicBezierShape, pos2, vec2};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Icon {
    Search,
    Fetch,
    Pull,
    Push,
    Stash,
    Pop,
    Branch,
    Merge,
    Commit,
    Folder,
    Terminal,
    Activity,
    Settings,
    Plus,
    Close,
    Changes,
    History,
    Tag,
    Cloud,
    Cube,
    Worktree,
    Check,
    Warning,
    Info,
    Refresh,
    ChevronDown,
    ChevronRight,
    Lock,
    Sidebar,
    Theme,
    Pin,
    Eye,
    Filter,
    Chart,
}

fn line(p: &Painter, a: Pos2, b: Pos2, s: Stroke) {
    p.line_segment([a, b], s);
}

fn arrow_head(p: &Painter, tip: Pos2, dir: Vec2, size: f32, s: Stroke) {
    let d = dir.normalized();
    let n = vec2(-d.y, d.x);
    line(p, tip, tip - d * size + n * size * 0.8, s);
    line(p, tip, tip - d * size - n * size * 0.8, s);
}

/// Draws `icon` centred in `rect` (designed on a 16×16 grid, scaled to the rect).
pub fn draw(p: &Painter, rect: Rect, icon: Icon, color: Color32) {
    let sz = rect.width().min(rect.height());
    let k = sz / 16.0;
    let o = rect.center() - vec2(8.0, 8.0) * k;
    let at = |x: f32, y: f32| pos2(o.x + x * k, o.y + y * k);
    let s = Stroke::new((1.35 * k).max(1.0), color);
    match icon {
        Icon::Sidebar => {
            p.rect_stroke(Rect::from_min_max(at(1.5, 2.5), at(14.5, 13.5)), 1.5 * k, s, eframe::egui::StrokeKind::Middle);
            line(p, at(6.0, 2.5), at(6.0, 13.5), s);
            line(p, at(3.0, 5.5), at(4.6, 5.5), s);
            line(p, at(3.0, 8.0), at(4.6, 8.0), s);
        }
        Icon::Theme => {
            p.circle_stroke(at(8.0, 8.0), 5.5 * k, s);
            let pts: Vec<Pos2> = (0..=16).map(|i| { let a = -std::f32::consts::FRAC_PI_2 + std::f32::consts::PI * i as f32 / 16.0; at(8.0 + 5.5 * a.cos(), 8.0 + 5.5 * a.sin()) }).collect();
            let mut poly = pts.clone();
            poly.push(at(8.0, 8.0));
            p.add(Shape::convex_polygon(poly, color, Stroke::NONE));
        }
        Icon::Pin => {
            line(p, at(8.0, 9.5), at(8.0, 14.5), s);
            p.rect_stroke(Rect::from_min_max(at(5.0, 2.0), at(11.0, 6.5)), 1.0 * k, s, eframe::egui::StrokeKind::Middle);
            line(p, at(3.5, 9.5), at(12.5, 9.5), s);
            line(p, at(5.5, 6.5), at(4.5, 9.5), s);
            line(p, at(10.5, 6.5), at(11.5, 9.5), s);
        }
        Icon::Eye => {
            let up: Vec<Pos2> = (0..=16).map(|i| { let t = i as f32 / 16.0; at(1.5 + 13.0 * t, 8.0 - 4.5 * (std::f32::consts::PI * t).sin()) }).collect();
            let dn: Vec<Pos2> = (0..=16).map(|i| { let t = i as f32 / 16.0; at(1.5 + 13.0 * t, 8.0 + 4.5 * (std::f32::consts::PI * t).sin()) }).collect();
            p.add(Shape::line(up, s));
            p.add(Shape::line(dn, s));
            p.circle_stroke(at(8.0, 8.0), 2.0 * k, s);
        }
        Icon::Filter => {
            p.add(Shape::line(vec![at(2.0, 3.0), at(14.0, 3.0), at(9.5, 8.5), at(9.5, 13.5), at(6.5, 12.0), at(6.5, 8.5), at(2.0, 3.0)], s));
        }
        Icon::Chart => {
            line(p, at(2.0, 14.0), at(14.0, 14.0), s);
            line(p, at(4.0, 14.0), at(4.0, 9.0), Stroke::new(s.width * 1.6, color));
            line(p, at(8.0, 14.0), at(8.0, 4.0), Stroke::new(s.width * 1.6, color));
            line(p, at(12.0, 14.0), at(12.0, 7.0), Stroke::new(s.width * 1.6, color));
        }
        Icon::Search => {
            p.circle_stroke(at(6.8, 6.8), 4.3 * k, s);
            line(p, at(10.0, 10.0), at(14.0, 14.0), Stroke::new(s.width * 1.3, color));
        }
        Icon::Fetch => {
            // circular arrow
            let c = at(8.0, 8.5);
            let r = 5.2 * k;
            let pts: Vec<Pos2> = (0..=24)
                .map(|i| {
                    let a = (-30.0f32 + i as f32 * 12.5).to_radians();
                    c + vec2(a.cos(), a.sin()) * r
                })
                .collect();
            p.add(Shape::line(pts.clone(), s));
            let end = *pts.last().unwrap();
            arrow_head(p, end, vec2(0.6, -1.0), 2.6 * k, s);
        }
        Icon::Pull => {
            line(p, at(8.0, 2.0), at(8.0, 11.5), s);
            arrow_head(p, at(8.0, 11.5), vec2(0.0, 1.0), 3.2 * k, s);
            line(p, at(3.0, 14.0), at(13.0, 14.0), s);
        }
        Icon::Push => {
            line(p, at(8.0, 14.0), at(8.0, 4.5), s);
            arrow_head(p, at(8.0, 4.5), vec2(0.0, -1.0), 3.2 * k, s);
            line(p, at(3.0, 2.0), at(13.0, 2.0), s);
        }
        Icon::Stash | Icon::Pop => {
            p.rect_stroke(Rect::from_min_max(at(2.0, 7.0), at(14.0, 14.0)), 1.5 * k, s, eframe::egui::StrokeKind::Middle);
            line(p, at(2.0, 9.5), at(14.0, 9.5), s);
            if icon == Icon::Stash {
                line(p, at(8.0, 1.0), at(8.0, 6.0), s);
                arrow_head(p, at(8.0, 6.0), vec2(0.0, 1.0), 2.4 * k, s);
            } else {
                line(p, at(8.0, 6.0), at(8.0, 1.0), s);
                arrow_head(p, at(8.0, 1.0), vec2(0.0, -1.0), 2.4 * k, s);
            }
        }
        Icon::Branch => {
            p.circle_stroke(at(4.5, 3.0), 1.7 * k, s);
            p.circle_stroke(at(4.5, 13.0), 1.7 * k, s);
            p.circle_stroke(at(11.5, 5.0), 1.7 * k, s);
            line(p, at(4.5, 4.7), at(4.5, 11.3), s);
            p.add(CubicBezierShape::from_points_stroke([at(11.5, 6.7), at(11.5, 10.0), at(4.5, 8.5), at(4.5, 11.0)], false, Color32::TRANSPARENT, s));
        }
        Icon::Merge => {
            p.circle_stroke(at(4.5, 3.0), 1.7 * k, s);
            p.circle_stroke(at(4.5, 13.0), 1.7 * k, s);
            p.circle_stroke(at(11.5, 11.0), 1.7 * k, s);
            line(p, at(4.5, 4.7), at(4.5, 11.3), s);
            p.add(CubicBezierShape::from_points_stroke([at(4.5, 5.0), at(4.5, 8.0), at(11.5, 6.0), at(11.5, 9.3)], false, Color32::TRANSPARENT, s));
        }
        Icon::Commit => {
            p.circle_stroke(at(8.0, 8.0), 3.0 * k, s);
            line(p, at(1.0, 8.0), at(5.0, 8.0), s);
            line(p, at(11.0, 8.0), at(15.0, 8.0), s);
        }
        Icon::Folder => {
            p.add(Shape::closed_line(vec![at(1.5, 4.0), at(6.0, 4.0), at(7.5, 5.5), at(14.5, 5.5), at(14.5, 13.0), at(1.5, 13.0)], s));
        }
        Icon::Terminal => {
            p.rect_stroke(Rect::from_min_max(at(1.5, 2.5), at(14.5, 13.5)), 1.5 * k, s, eframe::egui::StrokeKind::Middle);
            line(p, at(4.0, 6.0), at(6.5, 8.0), s);
            line(p, at(6.5, 8.0), at(4.0, 10.0), s);
            line(p, at(8.0, 10.5), at(11.5, 10.5), s);
        }
        Icon::Activity => {
            p.add(Shape::line(vec![at(1.0, 9.0), at(4.5, 9.0), at(6.5, 3.5), at(9.5, 13.0), at(11.5, 8.0), at(15.0, 8.0)], s));
        }
        Icon::Settings => {
            p.circle_stroke(at(8.0, 8.0), 2.3 * k, s);
            for i in 0..8 {
                let a = (i as f32 * 45.0).to_radians();
                let d = vec2(a.cos(), a.sin());
                line(p, at(8.0, 8.0) + d * 4.3 * k, at(8.0, 8.0) + d * 6.3 * k, Stroke::new(s.width * 1.6, color));
            }
            p.circle_stroke(at(8.0, 8.0), 4.6 * k, s);
        }
        Icon::Plus => {
            line(p, at(8.0, 3.0), at(8.0, 13.0), s);
            line(p, at(3.0, 8.0), at(13.0, 8.0), s);
        }
        Icon::Close => {
            line(p, at(4.0, 4.0), at(12.0, 12.0), s);
            line(p, at(12.0, 4.0), at(4.0, 12.0), s);
        }
        Icon::Changes => {
            p.add(Shape::closed_line(vec![at(3.0, 1.5), at(10.0, 1.5), at(13.0, 4.5), at(13.0, 14.5), at(3.0, 14.5)], s));
            line(p, at(5.5, 8.0), at(10.5, 8.0), s);
            line(p, at(8.0, 5.5), at(8.0, 10.5), s);
        }
        Icon::History => {
            p.circle_stroke(at(8.0, 8.0), 6.0 * k, s);
            line(p, at(8.0, 4.5), at(8.0, 8.0), s);
            line(p, at(8.0, 8.0), at(10.5, 9.5), s);
        }
        Icon::Tag => {
            p.add(Shape::closed_line(vec![at(1.5, 2.0), at(8.0, 2.0), at(14.5, 8.5), at(8.5, 14.5), at(1.5, 8.0)], s));
            p.circle_filled(at(5.0, 5.5), 1.1 * k, color);
        }
        Icon::Cloud => {
            p.add(Shape::line(
                vec![at(4.5, 12.5), at(2.5, 11.5), at(2.0, 9.5), at(3.2, 7.5), at(5.5, 7.0), at(6.5, 4.8), at(9.0, 4.0), at(11.5, 5.2), at(12.3, 7.3), at(14.0, 8.5), at(14.0, 11.0), at(12.0, 12.5)],
                s,
            ));
            line(p, at(4.5, 12.5), at(12.0, 12.5), s);
        }
        Icon::Cube => {
            p.add(Shape::closed_line(vec![at(8.0, 1.5), at(14.0, 4.5), at(14.0, 11.5), at(8.0, 14.5), at(2.0, 11.5), at(2.0, 4.5)], s));
            line(p, at(2.0, 4.5), at(8.0, 7.5), s);
            line(p, at(14.0, 4.5), at(8.0, 7.5), s);
            line(p, at(8.0, 7.5), at(8.0, 14.5), s);
        }
        Icon::Worktree => {
            p.add(Shape::closed_line(vec![at(1.5, 5.0), at(5.5, 5.0), at(7.0, 6.5), at(14.5, 6.5), at(14.5, 14.0), at(1.5, 14.0)], s));
            line(p, at(4.0, 2.0), at(12.0, 2.0), s);
        }
        Icon::Check => {
            p.add(Shape::line(vec![at(3.0, 8.5), at(6.5, 12.0), at(13.0, 4.5)], Stroke::new(s.width * 1.2, color)));
        }
        Icon::Warning => {
            p.add(Shape::closed_line(vec![at(8.0, 1.5), at(15.0, 14.0), at(1.0, 14.0)], s));
            line(p, at(8.0, 6.0), at(8.0, 10.0), s);
            p.circle_filled(at(8.0, 12.0), 0.9 * k, color);
        }
        Icon::Info => {
            p.circle_stroke(at(8.0, 8.0), 6.5 * k, s);
            line(p, at(8.0, 7.0), at(8.0, 11.5), s);
            p.circle_filled(at(8.0, 4.8), 0.9 * k, color);
        }
        Icon::Refresh => {
            let c = at(8.0, 8.0);
            let r = 5.5 * k;
            let pts: Vec<Pos2> = (0..=22).map(|i| {
                let a = (20.0f32 + i as f32 * 13.0).to_radians();
                c + vec2(a.cos(), a.sin()) * r
            }).collect();
            p.add(Shape::line(pts.clone(), s));
            arrow_head(p, *pts.last().unwrap(), vec2(0.9, -0.2), 2.6 * k, s);
        }
        Icon::ChevronDown => {
            p.add(Shape::line(vec![at(4.0, 6.0), at(8.0, 10.0), at(12.0, 6.0)], s));
        }
        Icon::ChevronRight => {
            p.add(Shape::line(vec![at(6.0, 4.0), at(10.0, 8.0), at(6.0, 12.0)], s));
        }
        Icon::Lock => {
            p.rect_stroke(Rect::from_min_max(at(3.0, 7.0), at(13.0, 14.5)), 1.5 * k, s, eframe::egui::StrokeKind::Middle);
            p.add(Shape::line(vec![at(5.5, 7.0), at(5.5, 4.5), at(8.0, 2.0), at(10.5, 4.5), at(10.5, 7.0)], s));
        }
    }
}
