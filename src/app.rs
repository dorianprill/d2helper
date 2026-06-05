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
        Self {
            shared: empty_shared_state(),
            capture: CaptureHandle::new(),
            background_opacity: 190,
            decorations_enabled: true,
            maximized: false,
        }
    }
}

impl eframe::App for D2HelperApp {
    fn update(&mut self, context: &egui::Context, _frame: &mut eframe::Frame) {
        context.request_repaint_after(Duration::from_millis(100));

        let snapshot = read_snapshot(&self.shared);
        let panel_fill = Color32::from_black_alpha(self.background_opacity);
        if let Some(maximized) = context.input(|input| input.viewport().maximized) {
            self.maximized = maximized;
        }

        egui::TopBottomPanel::top("toolbar")
            .frame(egui::Frame::NONE.fill(panel_fill))
            .show(context, |ui| {
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            !self.capture.started(),
                            egui::Button::new("Start LoD capture"),
                        )
                        .clicked()
                    {
                        self.capture.start(self.shared.clone());
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
                        "events {} applied {} errors {}",
                        snapshot.capture.total_events,
                        snapshot.capture.applied_messages,
                        snapshot.capture.parse_errors
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
            .default_width(280.0)
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
            ui.horizontal(|ui| {
                let label = if player.is_local {
                    format!("{}  local", player.name)
                } else {
                    player.name.clone()
                };
                ui.label(RichText::new(label).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_enabled(false, egui::Button::new("Inspect"));
                    ui.add_enabled(false, egui::Button::new("Download"));
                });
            });
            ui.label(format!(
                "{} level {}  id {}  ({}, {})",
                player.class_name, player.level, player.id, player.x, player.y
            ));
            draw_resource_values(ui, player);
        });
        ui.add_space(4.0);
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
        ui.colored_label(
            Color32::from_rgb(230, 210, 105),
            format!("ST {}", raw_value_label(player.stamina)),
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
        ui.allocate_exact_size(egui::vec2(width, height * 3.0 + 4.0), egui::Sense::hover());
    draw_raw_resource_bar(
        ui,
        egui::Rect::from_min_size(rect.min, egui::vec2(width, height)),
        player.life,
        Color32::from_rgb(120, 30, 35),
    );
    draw_raw_resource_bar(
        ui,
        egui::Rect::from_min_size(
            rect.min + egui::vec2(0.0, height + 2.0),
            egui::vec2(width, height),
        ),
        player.mana,
        Color32::from_rgb(35, 55, 135),
    );
    draw_raw_resource_bar(
        ui,
        egui::Rect::from_min_size(
            rect.min + egui::vec2(0.0, (height + 2.0) * 2.0),
            egui::vec2(width, height),
        ),
        player.stamina,
        Color32::from_rgb(120, 105, 35),
    );
}

fn draw_raw_resource_bar(ui: &mut egui::Ui, rect: egui::Rect, value: Option<u16>, color: Color32) {
    ui.painter()
        .rect_filled(rect, 2.0, Color32::from_rgba_unmultiplied(80, 80, 80, 80));
    let Some(value) = value else {
        return;
    };
    let fraction = (value as f32 / 0x7fff as f32).clamp(0.0, 1.0);
    let filled =
        egui::Rect::from_min_size(rect.min, egui::vec2(rect.width() * fraction, rect.height()));
    ui.painter().rect_filled(filled, 2.0, color);
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
