//! egui application shell for the Diablo II helper overlay.
//!
//! This module owns UI layout only. Packet capture and `GameState` conversion
//! live in separate modules so the first debug overlay can grow into a richer
//! tool without coupling rendering directly to packet parsing.

use std::time::Duration;

use eframe::egui::{self, Color32, RichText};

use crate::capture::CaptureHandle;
use crate::render::render_automap;
use crate::snapshot::{count_by_area, empty_shared_state, read_snapshot, SharedOverlayState};

/// Main egui application state.
pub struct D2HelperApp {
    shared: SharedOverlayState,
    capture: CaptureHandle,
    background_opacity: u8,
    decorations_enabled: bool,
    maximized: bool,
}

impl D2HelperApp {
    /// Creates the overlay with dark visuals and an empty snapshot.
    pub fn new(creation_context: &eframe::CreationContext<'_>) -> Self {
        creation_context.egui_ctx.set_visuals(egui::Visuals::dark());
        let shared = empty_shared_state();
        let mut capture = CaptureHandle::new();
        capture.start(shared.clone());
        Self {
            shared,
            capture,
            background_opacity: 255,
            decorations_enabled: true,
            maximized: false,
        }
    }
}

impl eframe::App for D2HelperApp {
    fn update(&mut self, context: &egui::Context, _frame: &mut eframe::Frame) {
        context.request_repaint_after(Duration::from_millis(33));

        let snapshot = read_snapshot(&self.shared);
        let panel_fill = Color32::from_black_alpha(self.background_opacity);
        if let Some(maximized) = context.input(|input| input.viewport().maximized) {
            self.maximized = maximized;
        }

        egui::TopBottomPanel::top("toolbar")
            .frame(egui::Frame::NONE.fill(panel_fill))
            .show(context, |ui| {
                ui.horizontal(|ui| {
                    let capture_enabled = self.capture.enabled();
                    let (capture_label, capture_fill) = if capture_enabled {
                        ("Capture started", Color32::from_rgb(25, 120, 55))
                    } else {
                        ("Capture stopped", Color32::from_rgb(140, 40, 35))
                    };
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new(capture_label)
                                    .strong()
                                    .color(Color32::from_rgb(245, 245, 245)),
                            )
                            .fill(capture_fill),
                        )
                        .clicked()
                    {
                        self.capture.toggle_enabled(&self.shared);
                    }

                    ui.separator();
                    ui.label(if snapshot.capture.running {
                        RichText::new("capture running").color(Color32::from_rgb(90, 220, 120))
                    } else {
                        RichText::new("capture idle").color(Color32::from_gray(180))
                    });
                    if !snapshot.capture.status.is_empty() {
                        ui.label(
                            RichText::new(&snapshot.capture.status).color(Color32::from_gray(190)),
                        );
                    }
                    ui.separator();
                    ui.label("Opacity");
                    ui.add(egui::Slider::new(&mut self.background_opacity, 0..=255));
                    ui.separator();
                    ui.label(format!(
                        "events {} applied {} errors {} tcp {}",
                        snapshot.capture.total_events,
                        snapshot.capture.applied_messages,
                        snapshot.capture.parse_errors,
                        snapshot.capture.transport_warnings
                    ));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add_sized([24.0, 20.0], egui::Button::new(RichText::new("X").strong()))
                            .clicked()
                        {
                            context.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                        if ui
                            .add_sized(
                                [64.0, 20.0],
                                egui::Button::new(if self.maximized { "Restore" } else { "Max" }),
                            )
                            .clicked()
                        {
                            self.maximized = !self.maximized;
                            context.send_viewport_cmd(egui::ViewportCommand::Maximized(
                                self.maximized,
                            ));
                        }
                        if ui
                            .add_sized(
                                [68.0, 20.0],
                                egui::Button::new(if self.decorations_enabled {
                                    "Hide Bar"
                                } else {
                                    "Show Bar"
                                }),
                            )
                            .clicked()
                        {
                            self.decorations_enabled = !self.decorations_enabled;
                            context.send_viewport_cmd(egui::ViewportCommand::Decorations(
                                self.decorations_enabled,
                            ));
                        }
                    });
                });
            });

        egui::SidePanel::left("characters")
            .resizable(true)
            .default_width(560.0)
            .min_width(360.0)
            .frame(egui::Frame::NONE.fill(panel_fill))
            .show(context, |ui| {
                draw_character_panel(ui, &snapshot);
            });

        egui::TopBottomPanel::bottom("status")
            .frame(egui::Frame::NONE.fill(panel_fill))
            .show(context, |ui| {
                draw_status_bar(ui, &snapshot);
            });

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(panel_fill))
            .show(context, |ui| {
                draw_map_panel(ui, &snapshot);
            });
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }
}

fn draw_character_panel(ui: &mut egui::Ui, snapshot: &crate::snapshot::OverlaySnapshot) {
    ui.heading("Characters");
    ui.add_space(6.0);

    if snapshot.players.is_empty() {
        ui.label(RichText::new("waiting for player packets").color(Color32::from_gray(160)));
        return;
    }

    for player in &snapshot.players {
        ui.group(|ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let label = if player.is_local {
                    format!("{}  local", player.name)
                } else {
                    player.name.clone()
                };
                let name_width = (ui.available_width() - 160.0).max(80.0);
                ui.add_sized(
                    [name_width, 20.0],
                    egui::Label::new(RichText::new(label).strong()).truncate(),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_enabled(false, egui::Button::new("Inspect"));
                    ui.add_enabled(false, egui::Button::new("Download"));
                });
            });
            ui.horizontal(|ui| {
                ui.label(&player.class_name);
                ui.separator();
                ui.label(level_label(player.level));
                ui.separator();
                ui.label(area_label(player));
                ui.separator();
                ui.label(format!("id {}", player.id));
                ui.separator();
                ui.label(position_label(player));
            });
            draw_resource_values(ui, player);
        });
        ui.add_space(4.0);
    }
}

fn level_label(level: u32) -> String {
    if level == 0 {
        "level --".to_owned()
    } else {
        format!("level {level}")
    }
}

fn area_label(player: &crate::snapshot::PlayerSnapshot) -> String {
    player
        .area_name
        .as_deref()
        .map(|name| format!("area {name}"))
        .unwrap_or_else(|| "area --".to_owned())
}

fn position_label(player: &crate::snapshot::PlayerSnapshot) -> String {
    if player.has_known_world_position() {
        format!("@ {},{}", player.x, player.y)
    } else {
        "@ --".to_owned()
    }
}

fn draw_resource_values(ui: &mut egui::Ui, player: &crate::snapshot::PlayerSnapshot) {
    ui.horizontal_wrapped(|ui| {
        ui.colored_label(
            Color32::from_rgb(220, 80, 85),
            format!("HP {}", raw_value_label(player.life)),
        );
        ui.colored_label(
            Color32::from_rgb(90, 130, 240),
            format!("MP {}", raw_value_label(player.mana)),
        );
        if let (Some(life_regen), Some(mana_regen)) = (player.life_regen, player.mana_regen) {
            ui.label(format!("regen {life_regen}/{mana_regen}"));
        }
        if let (Some(dx), Some(dy)) = (player.movement_dx, player.movement_dy) {
            ui.label(format!("move {dx},{dy}"));
        }
    });

    let width = ui.available_width();
    let height = 8.0;
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(width, height * 2.0 + 2.0), egui::Sense::hover());
    draw_resource_bar(
        ui,
        egui::Rect::from_min_size(rect.min, egui::vec2(width, height)),
        player.life,
        player.life_max,
        Color32::from_rgb(120, 30, 35),
    );
    draw_resource_bar(
        ui,
        egui::Rect::from_min_size(
            rect.min + egui::vec2(0.0, height + 2.0),
            egui::vec2(width, height),
        ),
        player.mana,
        player.mana_max,
        Color32::from_rgb(35, 55, 135),
    );
}

fn draw_resource_bar(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    value: Option<u16>,
    max_value: Option<u32>,
    color: Color32,
) {
    ui.painter()
        .rect_filled(rect, 2.0, Color32::from_rgba_unmultiplied(80, 80, 80, 80));
    let Some(fraction) = resource_bar_fraction(value, max_value) else {
        return;
    };
    let filled =
        egui::Rect::from_min_size(rect.min, egui::vec2(rect.width() * fraction, rect.height()));
    ui.painter().rect_filled(filled, 2.0, color);
}

fn resource_bar_fraction(value: Option<u16>, max_value: Option<u32>) -> Option<f32> {
    let value = value? as f32;
    let max_value = normalized_resource_max(max_value?, value)?;
    Some((value / max_value).clamp(0.0, 1.0))
}

fn normalized_resource_max(max_value: u32, current_value: f32) -> Option<f32> {
    if max_value == 0 {
        return None;
    }

    let raw_max = max_value as f32;
    if current_value > raw_max {
        let fixed_point_max = raw_max * 256.0;
        if current_value <= fixed_point_max {
            return Some(fixed_point_max);
        }
    }
    Some(raw_max)
}

fn raw_value_label(value: Option<u16>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "--".to_owned())
}

fn draw_status_bar(ui: &mut egui::Ui, snapshot: &crate::snapshot::OverlaySnapshot) {
    ui.horizontal_wrapped(|ui| {
        ui.label(format!("difficulty {}", snapshot.game.difficulty));
        ui.separator();
        ui.label(format!("act {:?}", snapshot.game.act));
        ui.separator();
        ui.label(format!("area {:?}", snapshot.game.area_id));
        ui.separator();
        ui.label(format!("seed {:?}", snapshot.game.map_seed));
        ui.separator();
        ui.label(format!("automap {:?}", snapshot.game.automap));
        ui.separator();
        ui.label(format!("local {:?}", snapshot.game.local_player_id));
        ui.separator();
        ui.label(format!(
            "{}{}{}",
            if snapshot.game.is_expansion {
                "expansion"
            } else {
                "classic"
            },
            if snapshot.game.is_ladder {
                " ladder"
            } else {
                ""
            },
            if snapshot.game.is_hardcore {
                " hardcore"
            } else {
                ""
            }
        ));
        ui.separator();
        ui.label(format!("players {}", snapshot.players.len()));
        ui.separator();
        ui.label(format!("npcs {}", snapshot.npcs.len()));
        ui.separator();
        ui.label(format!("items {}", snapshot.items.len()));
        ui.separator();
        ui.label(format!("objects {}", snapshot.objects.len()));
        ui.separator();
        ui.label(if snapshot.generated_map.is_some() {
            "walls loaded"
        } else {
            "walls live-only"
        });
        ui.separator();
        ui.label(format!(
            "item stat streams {}",
            snapshot.game.item_stat_updates
        ));
        if let Some(packet_id) = snapshot.capture.last_packet_id {
            ui.separator();
            ui.label(format!("last packet 0x{packet_id:02X}"));
        }
        if let Some(error) = &snapshot.capture.last_error {
            ui.separator();
            ui.colored_label(Color32::from_rgb(255, 120, 90), error);
        }
    });
}

fn draw_map_panel(ui: &mut egui::Ui, snapshot: &crate::snapshot::OverlaySnapshot) {
    ui.horizontal(|ui| {
        ui.heading("Automap Debug View");
        ui.separator();
        ui.label(format!(
            "revealed tiles {}",
            snapshot.game.revealed_tiles.len()
        ));
        if let Some(map) = snapshot.generated_map.as_deref() {
            ui.label(format!(
                "generated {} {}x{}",
                map.name, map.size.width, map.size.height
            ));
        }
        for (area, count) in count_by_area(&snapshot.game.revealed_tiles)
            .into_iter()
            .take(4)
        {
            ui.label(format!("area {area}: {count}"));
        }
    });
    ui.add_space(6.0);
    render_automap(ui, snapshot);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_fraction_uses_fixed_point_max_when_needed() {
        assert_eq!(resource_bar_fraction(Some(1280), Some(10)), Some(0.5));
    }

    #[test]
    fn resource_fraction_accepts_raw_packet_unit_max() {
        assert_eq!(resource_bar_fraction(Some(1280), Some(2560)), Some(0.5));
    }

    #[test]
    fn resource_fraction_is_unknown_without_maximum() {
        assert_eq!(resource_bar_fraction(Some(1280), None), None);
    }
}
