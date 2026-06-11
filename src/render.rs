//! Debug automap renderer.
//!
//! Diablo II uses an isometric projection for world/map coordinates. The first
//! renderer keeps the art intentionally simple and projects revealed cells plus
//! unit markers into diamond-shaped map space. Later MPQ-backed tile rendering
//! can reuse the same snapshot and projection boundary.

use eframe::egui::{self, Color32, Painter, Pos2, Rect, Shape, Stroke, Vec2};
use libd2::{GeneratedMap, MapObjectKind};

use crate::snapshot::{MapFocus, OverlaySnapshot};

const TILE_WIDTH: f32 = 16.0;
const TILE_HEIGHT: f32 = 8.0;
const AUTOMAP_TILE_WORLD_OFFSET: u16 = 4096;

/// Draws the current map snapshot into the provided egui allocation.
pub fn render_automap(ui: &mut egui::Ui, snapshot: &OverlaySnapshot) {
    let available = ui.available_size_before_wrap();
    let desired = Vec2::new(available.x.max(360.0), available.y.max(360.0));
    let (rect, _) = ui.allocate_exact_size(desired, egui::Sense::hover());
    let painter = ui.painter_at(rect);

    painter.rect_stroke(
        rect,
        0.0,
        Stroke::new(1.0_f32, Color32::from_gray(70)),
        egui::StrokeKind::Inside,
    );

    let Some(focus) = snapshot.map_focus() else {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "waiting for map or unit packets",
            egui::FontId::proportional(16.0),
            Color32::from_gray(170),
        );
        return;
    };

    let projector = IsoProjector::new(rect, focus);
    draw_generated_map(&painter, &projector, snapshot);
    draw_revealed_tiles(&painter, &projector, snapshot);
    draw_objects(&painter, &projector, snapshot);
    draw_items(&painter, &projector, snapshot);
    draw_npcs(&painter, &projector, snapshot);
    draw_players(&painter, &projector, snapshot);
    draw_focus_label(&painter, rect, focus);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::RevealedTileSnapshot;

    #[test]
    fn iso_projection_keeps_center_on_rect_center() {
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(200.0, 100.0));
        let mut snapshot = OverlaySnapshot::default();
        snapshot.game.revealed_tiles.push(RevealedTileSnapshot {
            x: 10,
            y: 20,
            area_id: 1,
        });
        snapshot.game.revealed_tiles.push(RevealedTileSnapshot {
            x: 30,
            y: 40,
            area_id: 1,
        });
        let focus = snapshot.map_focus().expect("focus");
        let projector = IsoProjector::new(rect, focus);

        assert_eq!(projector.project(20, 30), rect.center());
    }

    #[test]
    fn iso_projection_matches_diablo_style_axes() {
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(200.0, 100.0));
        let mut snapshot = OverlaySnapshot::default();
        snapshot.game.revealed_tiles.push(RevealedTileSnapshot {
            x: 10,
            y: 10,
            area_id: 1,
        });
        let focus = snapshot.map_focus().expect("focus");
        let projector = IsoProjector::new(rect, focus);

        assert_eq!(
            projector.project(11, 10),
            rect.center() + Vec2::new(TILE_WIDTH * 0.5, TILE_HEIGHT * 0.5)
        );
        assert_eq!(
            projector.project(10, 11),
            rect.center() + Vec2::new(-TILE_WIDTH * 0.5, TILE_HEIGHT * 0.5)
        );
    }

    #[test]
    fn revealed_tile_normalization_prefers_lod_world_offset_when_nearer_focus() {
        assert_eq!(
            normalize_revealed_tile(1100, 1000, 5200.0, 5100.0),
            (5196, 5096)
        );
        assert_eq!(
            normalize_revealed_tile(5200, 5100, 5200.0, 5100.0),
            (5200, 5100)
        );
    }

    #[test]
    fn local_axis_range_clamps_visible_world_to_generated_map_bounds() {
        assert_eq!(local_axis_range(5200, 5250, 5190, 40), Some((10, 39)));
        assert_eq!(local_axis_range(5100, 5110, 5190, 40), None);
    }
}

fn draw_generated_map(painter: &Painter, projector: &IsoProjector, snapshot: &OverlaySnapshot) {
    let Some(map) = snapshot.generated_map.as_deref() else {
        return;
    };

    draw_generated_collision(painter, projector, map);
    draw_generated_exits(painter, projector, map);
}

fn draw_generated_collision(painter: &Painter, projector: &IsoProjector, map: &GeneratedMap) {
    let bounds = projector.visible_world_bounds(10.0);
    let Some((min_x, max_x)) =
        local_axis_range(bounds.min_x, bounds.max_x, map.offset.x, map.size.width)
    else {
        return;
    };
    let Some((min_y, max_y)) =
        local_axis_range(bounds.min_y, bounds.max_y, map.offset.y, map.size.height)
    else {
        return;
    };

    for y in min_y..=max_y {
        for x in min_x..=max_x {
            if !map.collision.is_blocked(x, y).unwrap_or(false) {
                continue;
            }
            let center = projector.project_i32(map.offset.x + x as i32, map.offset.y + y as i32);
            if !projector.rect.expand(8.0).contains(center) {
                continue;
            }
            painter.add(Shape::convex_polygon(
                diamond(center, TILE_WIDTH * 0.42, TILE_HEIGHT * 0.42),
                Color32::from_rgba_unmultiplied(115, 130, 135, 115),
                Stroke::NONE,
            ));
        }
    }
}

fn draw_generated_exits(painter: &Painter, projector: &IsoProjector, map: &GeneratedMap) {
    for object in &map.objects {
        if object.kind != MapObjectKind::Exit {
            continue;
        }
        let position = projector.project_i32(
            map.offset.x + object.position.x,
            map.offset.y + object.position.y,
        );
        if !projector.rect.expand(48.0).contains(position) {
            continue;
        }
        let color = if object.is_good_exit {
            Color32::from_rgb(255, 210, 90)
        } else {
            Color32::from_rgb(90, 220, 230)
        };
        painter.add(Shape::convex_polygon(
            diamond(position, 8.0, 8.0),
            color,
            Stroke::new(1.0_f32, Color32::from_rgb(20, 25, 28)),
        ));
        let label = object
            .name
            .as_deref()
            .filter(|name| !name.trim().is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| format!("exit {}", object.id));
        painter.text(
            position + Vec2::new(9.0, -12.0),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::monospace(10.0),
            color,
        );
    }
}

fn draw_revealed_tiles(painter: &Painter, projector: &IsoProjector, snapshot: &OverlaySnapshot) {
    for tile in &snapshot.game.revealed_tiles {
        let (x, y) =
            normalize_revealed_tile(tile.x, tile.y, projector.center_x, projector.center_y);
        let center = projector.project(x, y);
        if !projector.rect.expand(32.0).contains(center) {
            continue;
        }
        painter.add(Shape::convex_polygon(
            diamond(center, TILE_WIDTH * 0.72, TILE_HEIGHT * 0.72),
            Color32::from_rgba_unmultiplied(75, 115, 145, 96),
            Stroke::new(0.8_f32, Color32::from_rgba_unmultiplied(145, 190, 220, 150)),
        ));
    }
}

fn draw_players(painter: &Painter, projector: &IsoProjector, snapshot: &OverlaySnapshot) {
    for player in &snapshot.players {
        if !player.has_known_world_position() {
            continue;
        }
        let position = projector.project(player.x, player.y);
        if !projector.rect.expand(64.0).contains(position) {
            continue;
        }
        let color = if player.is_local {
            Color32::from_rgb(80, 220, 255)
        } else {
            Color32::from_rgb(100, 180, 255)
        };
        painter.circle_filled(position, if player.is_local { 6.0 } else { 4.5 }, color);
        painter.text(
            position + Vec2::new(8.0, -16.0),
            egui::Align2::LEFT_CENTER,
            &player.name,
            egui::FontId::proportional(12.0),
            Color32::from_gray(230),
        );
    }
}

fn normalize_revealed_tile(tile_x: u16, tile_y: u16, center_x: f32, center_y: f32) -> (u16, u16) {
    let shifted_x = tile_x.saturating_add(AUTOMAP_TILE_WORLD_OFFSET);
    let shifted_y = tile_y.saturating_add(AUTOMAP_TILE_WORLD_OFFSET);
    let raw_distance = projected_distance(tile_x, tile_y, center_x, center_y);
    let shifted_distance = projected_distance(shifted_x, shifted_y, center_x, center_y);

    if shifted_distance < raw_distance {
        (shifted_x, shifted_y)
    } else {
        (tile_x, tile_y)
    }
}

fn projected_distance(x: u16, y: u16, center_x: f32, center_y: f32) -> f32 {
    let dx = x as f32 - center_x;
    let dy = y as f32 - center_y;
    (dx - dy).abs() + (dx + dy).abs()
}

fn draw_npcs(painter: &Painter, projector: &IsoProjector, snapshot: &OverlaySnapshot) {
    for npc in &snapshot.npcs {
        let position = projector.project(npc.x, npc.y);
        if !projector.rect.expand(32.0).contains(position) {
            continue;
        }
        let life = npc.life_percent.unwrap_or(100);
        let color = if life < 35 {
            Color32::from_rgb(160, 45, 45)
        } else {
            Color32::from_rgb(235, 80, 80)
        };
        let radius = if npc.state.is_some() { 3.5 } else { 3.0 };
        painter.circle_filled(position, radius, color);
        let label = npc
            .name
            .as_deref()
            .map(str::to_owned)
            .or_else(|| npc.class_id.map(|class_id| format!("M{class_id}")));
        if let Some(label) = label {
            painter.text(
                position + Vec2::new(6.0, -10.0),
                egui::Align2::LEFT_CENTER,
                label,
                egui::FontId::proportional(9.0),
                Color32::from_rgb(255, 180, 175),
            );
        }

        let _ = npc.id;
    }
}

fn draw_items(painter: &Painter, projector: &IsoProjector, snapshot: &OverlaySnapshot) {
    for item in &snapshot.items {
        let _ = (
            item.id,
            item.action.as_str(),
            item.category.as_str(),
            item.state_flags
                .map(|flags| (flags.unit_type, flags.unit_id, flags.and_value)),
        );
        let (Some(x), Some(y)) = (item.x, item.y) else {
            continue;
        };
        let position = projector.project(x, y);
        let color = match item.quality.as_deref() {
            Some("Unique") => Color32::from_rgb(210, 165, 60),
            Some("Set") => Color32::from_rgb(70, 220, 100),
            _ if item
                .code
                .as_deref()
                .is_some_and(|code| code.starts_with('r')) =>
            {
                Color32::from_rgb(235, 60, 60)
            }
            _ => Color32::from_rgb(220, 220, 220),
        };
        let rect = Rect::from_center_size(position, Vec2::splat(5.0));
        painter.rect_filled(rect, 1.0, color);
        if let Some(flags) = item.state_flags {
            if flags.flags != 0 {
                painter.rect_stroke(
                    rect.expand(2.0),
                    1.0,
                    Stroke::new(1.0_f32, Color32::from_rgb(255, 230, 130)),
                    egui::StrokeKind::Inside,
                );
            }
        }

        if let Some(label) = item.name.as_deref().or(item.code.as_deref()) {
            painter.text(
                position + Vec2::new(6.0, 8.0),
                egui::Align2::LEFT_CENTER,
                label,
                egui::FontId::monospace(9.0),
                Color32::from_gray(210),
            );
        }
    }
}

fn draw_objects(painter: &Painter, projector: &IsoProjector, snapshot: &OverlaySnapshot) {
    for object in &snapshot.objects {
        let position = projector.project(object.x, object.y);
        if !projector.rect.expand(32.0).contains(position) {
            continue;
        }
        let color = match object.is_targetable {
            Some(0) => Color32::from_rgb(120, 115, 145),
            Some(_) => Color32::from_rgb(200, 175, 255),
            None => Color32::from_rgb(180, 150, 255),
        };
        painter.add(Shape::convex_polygon(
            diamond(position, 5.0, 5.0),
            color,
            Stroke::NONE,
        ));
        let object_name = object
            .name
            .as_deref()
            .filter(|name| !name.trim().is_empty());
        let label = if let Some(name) = object_name {
            if object.state != 0 {
                format!("{name} S{}", object.state)
            } else {
                name.to_owned()
            }
        } else if object.state != 0 {
            format!("W{} S{}", object.class_id, object.state)
        } else if let Some(flags) = object.portal_flags {
            format!("W{} P{}", object.class_id, flags)
        } else {
            format!("W{}", object.class_id)
        };
        painter.text(
            position + Vec2::new(7.0, -8.0),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::monospace(9.0),
            Color32::from_rgb(215, 200, 255),
        );

        let _ = (object.id, object.class_id, object.object_type);
    }
}

fn draw_focus_label(painter: &Painter, rect: Rect, focus: MapFocus) {
    painter.text(
        rect.left_top() + Vec2::new(12.0, 12.0),
        egui::Align2::LEFT_TOP,
        format!("center {},{} ({:?})", focus.x, focus.y, focus.source),
        egui::FontId::monospace(12.0),
        Color32::from_gray(170),
    );
}

fn local_axis_range(
    min_world: i32,
    max_world: i32,
    map_offset: i32,
    map_size: u32,
) -> Option<(u32, u32)> {
    if map_size == 0 {
        return None;
    }
    let min = (min_world - map_offset).max(0);
    let max = (max_world - map_offset).min(map_size as i32 - 1);
    (min <= max).then_some((min as u32, max as u32))
}

fn diamond(center: Pos2, half_width: f32, half_height: f32) -> Vec<Pos2> {
    vec![
        center + Vec2::new(0.0, -half_height),
        center + Vec2::new(half_width, 0.0),
        center + Vec2::new(0.0, half_height),
        center + Vec2::new(-half_width, 0.0),
    ]
}

#[derive(Debug, Clone, Copy)]
struct WorldBounds {
    min_x: i32,
    min_y: i32,
    max_x: i32,
    max_y: i32,
}

struct IsoProjector {
    rect: Rect,
    center_x: f32,
    center_y: f32,
    origin: Pos2,
}

impl IsoProjector {
    fn new(rect: Rect, focus: MapFocus) -> Self {
        Self {
            rect,
            center_x: focus.x as f32,
            center_y: focus.y as f32,
            origin: rect.center(),
        }
    }

    fn project(&self, x: u16, y: u16) -> Pos2 {
        self.project_i32(x as i32, y as i32)
    }

    fn project_i32(&self, x: i32, y: i32) -> Pos2 {
        let dx = x as f32 - self.center_x;
        let dy = y as f32 - self.center_y;
        self.origin + Vec2::new((dx - dy) * TILE_WIDTH * 0.5, (dx + dy) * TILE_HEIGHT * 0.5)
    }

    fn visible_world_bounds(&self, margin_tiles: f32) -> WorldBounds {
        let rect = self.rect.expand(TILE_WIDTH.max(TILE_HEIGHT) * margin_tiles);
        let mut bounds = WorldBounds {
            min_x: i32::MAX,
            min_y: i32::MAX,
            max_x: i32::MIN,
            max_y: i32::MIN,
        };

        for corner in [
            rect.left_top(),
            rect.right_top(),
            rect.left_bottom(),
            rect.right_bottom(),
        ] {
            let (x, y) = self.unproject(corner);
            bounds.min_x = bounds.min_x.min(x.floor() as i32);
            bounds.min_y = bounds.min_y.min(y.floor() as i32);
            bounds.max_x = bounds.max_x.max(x.ceil() as i32);
            bounds.max_y = bounds.max_y.max(y.ceil() as i32);
        }

        bounds
    }

    fn unproject(&self, position: Pos2) -> (f32, f32) {
        let sx = (position.x - self.origin.x) / (TILE_WIDTH * 0.5);
        let sy = (position.y - self.origin.y) / (TILE_HEIGHT * 0.5);
        (
            self.center_x + (sx + sy) * 0.5,
            self.center_y + (sy - sx) * 0.5,
        )
    }
}
