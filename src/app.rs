//! egui application shell for the Diablo II helper overlay.
//!
//! This module owns UI layout only. Packet capture and `GameState` conversion
//! live in separate modules so the first debug overlay can grow into a richer
//! tool without coupling rendering directly to packet parsing.

use std::{
    collections::HashMap,
    fs, io,
    path::{Path, PathBuf},
    time::Duration,
};

use directories::UserDirs;
use eframe::egui::{self, Color32, RichText};
use libd2::{PartyAffiliation, PartyId};
use tracing::{info, warn};

use crate::capture::CaptureHandle;
use crate::render::render_automap;
use crate::snapshot::{
    CharacterExportSnapshot, SharedOverlayState, count_by_area, empty_shared_state, read_snapshot,
};

/// Main egui application state.
pub struct D2HelperApp {
    shared: SharedOverlayState,
    capture: CaptureHandle,
    background_opacity: u8,
    decorations_enabled: bool,
    maximized: bool,
    download_status: Option<DownloadStatus>,
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
            maximized: true,
            download_status: None,
        }
    }
}

impl eframe::App for D2HelperApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let context = ui.ctx().clone();
        context.request_repaint_after(Duration::from_millis(16));

        let snapshot = read_snapshot(&self.shared);
        let panel_fill = Color32::from_black_alpha(self.background_opacity);
        if let Some(maximized) = context.input(|input| input.viewport().maximized) {
            self.maximized = maximized;
        }

        egui::Panel::top("toolbar")
            .frame(egui::Frame::NONE.fill(panel_fill))
            .show_inside(ui, |ui| {
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
                    if let Some(status) = &self.download_status {
                        ui.separator();
                        ui.label(RichText::new(&status.message).color(status.color));
                    }
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

        egui::Panel::left("characters")
            .resizable(true)
            .default_size(560.0)
            .min_size(360.0)
            .frame(egui::Frame::NONE.fill(panel_fill))
            .show_inside(ui, |ui| {
                if let Some(status) = draw_character_panel(ui, &snapshot) {
                    self.download_status = Some(status);
                }
            });

        egui::Panel::bottom("status")
            .frame(egui::Frame::NONE.fill(panel_fill))
            .show_inside(ui, |ui| {
                draw_status_bar(ui, &snapshot);
            });

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(panel_fill))
            .show_inside(ui, |ui| {
                draw_map_panel(ui, &snapshot);
            });
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }
}

#[derive(Debug, Clone)]
struct DownloadStatus {
    message: String,
    color: Color32,
}

fn draw_character_panel(
    ui: &mut egui::Ui,
    snapshot: &crate::snapshot::OverlaySnapshot,
) -> Option<DownloadStatus> {
    let mut download_status = None;
    ui.heading("Characters");
    ui.add_space(6.0);

    if snapshot.players.is_empty() {
        ui.label(RichText::new("waiting for player packets").color(Color32::from_gray(160)));
        return None;
    }

    let party_colors = PartyColorMap::from_players(&snapshot.players);
    for player in &snapshot.players {
        egui::Frame::group(ui.style())
            .fill(party_colors.row_fill(player.party_affiliation))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let label = if player.is_local {
                        format!("{}  local", player.name)
                    } else {
                        player.name.clone()
                    };
                    let name_text = if player.is_local {
                        RichText::new(label)
                            .strong()
                            .color(Color32::from_rgb(210, 112, 32))
                    } else {
                        RichText::new(label).strong()
                    };
                    let name_width = (ui.available_width() - 160.0).max(80.0);
                    ui.add_sized([name_width, 20.0], egui::Label::new(name_text).truncate());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_enabled(false, egui::Button::new("Inspect"));
                        if player.is_local {
                            let can_download = snapshot.character_export.is_some();
                            if ui
                                .add_enabled(can_download, egui::Button::new("Download"))
                                .clicked()
                            {
                                let export = snapshot
                                    .character_export
                                    .as_ref()
                                    .expect("enabled download requires export payload");
                                download_status = Some(download_character(export));
                            }
                        }
                    });
                });
                ui.horizontal(|ui| {
                    ui.label(&player.class_name);
                    ui.separator();
                    ui.label(level_label(player.level));
                    ui.separator();
                    ui.label(party_label(&party_colors, player.party_affiliation));
                    ui.separator();
                    ui.label(area_label(player));
                    ui.separator();
                    ui.label(format!("id {}", player.id));
                    ui.separator();
                    ui.label(position_label(player));
                });
                draw_resource_values(ui, player);
                for mercenary in snapshot
                    .mercenaries
                    .iter()
                    .filter(|mercenary| mercenary.owner_id == player.id)
                {
                    draw_mercenary_row(ui, mercenary);
                }
            });
        ui.add_space(4.0);
    }

    download_status
}

#[derive(Debug, Default)]
struct PartyColorMap {
    indices: HashMap<PartyColorKey, usize>,
}

impl PartyColorMap {
    fn from_players(players: &[crate::snapshot::PlayerSnapshot]) -> Self {
        let mut indices = HashMap::new();
        for player in players {
            let Some(key) = PartyColorKey::from_affiliation(player.party_affiliation) else {
                continue;
            };
            let next_index = indices.len();
            indices.entry(key).or_insert(next_index);
        }

        Self { indices }
    }

    fn row_fill(&self, party_affiliation: PartyAffiliation) -> Color32 {
        match party_affiliation {
            PartyAffiliation::Unknown => Color32::from_rgb(14, 14, 16),
            PartyAffiliation::Unpartied => Color32::from_rgb(0, 0, 0),
            PartyAffiliation::LocalParty => self
                .indices
                .get(&PartyColorKey::Local)
                .map(|index| party_row_color(*index))
                .unwrap_or_else(|| Color32::from_rgb(14, 14, 16)),
            PartyAffiliation::Party(party_id) => self
                .indices
                .get(&PartyColorKey::Id(party_id))
                .map(|index| party_row_color(*index))
                .unwrap_or_else(|| Color32::from_rgb(14, 14, 16)),
        }
    }

    fn party_number(&self, key: PartyColorKey) -> Option<usize> {
        self.indices.get(&key).map(|index| index + 1)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum PartyColorKey {
    Local,
    Id(PartyId),
}

impl PartyColorKey {
    fn from_affiliation(party_affiliation: PartyAffiliation) -> Option<Self> {
        match party_affiliation {
            PartyAffiliation::LocalParty => Some(Self::Local),
            PartyAffiliation::Party(party_id) => Some(Self::Id(party_id)),
            PartyAffiliation::Unknown | PartyAffiliation::Unpartied => None,
        }
    }
}

fn party_row_color(index: usize) -> Color32 {
    match index % 6 {
        0 => Color32::from_rgb(2, 28, 13),
        1 => Color32::from_rgb(5, 15, 38),
        2 => Color32::from_rgb(40, 5, 8),
        3 => Color32::from_rgb(29, 15, 43),
        4 => Color32::from_rgb(3, 31, 31),
        _ => Color32::from_rgb(39, 28, 7),
    }
}

fn party_label(colors: &PartyColorMap, party_affiliation: PartyAffiliation) -> String {
    match party_affiliation {
        PartyAffiliation::Unknown => "party --".to_owned(),
        PartyAffiliation::Unpartied => "unpartied".to_owned(),
        PartyAffiliation::LocalParty => colors
            .party_number(PartyColorKey::Local)
            .map(|number| format!("party {number}"))
            .unwrap_or_else(|| "party --".to_owned()),
        PartyAffiliation::Party(party_id) => colors
            .party_number(PartyColorKey::Id(party_id))
            .map(|number| format!("party {number}"))
            .unwrap_or_else(|| "party --".to_owned()),
    }
}

fn download_character(export: &CharacterExportSnapshot) -> DownloadStatus {
    match save_character_export(export) {
        Ok(path) => {
            info!(path = %path.display(), "wrote exported local-player D2S");
            DownloadStatus {
                message: format!("saved {}", path.display()),
                color: Color32::from_rgb(90, 220, 120),
            }
        }
        Err(error) => {
            warn!(error = %error, file = %export.file_name, "failed to write exported local-player D2S");
            DownloadStatus {
                message: format!("download failed: {error}"),
                color: Color32::from_rgb(255, 120, 90),
            }
        }
    }
}

fn save_character_export(export: &CharacterExportSnapshot) -> io::Result<PathBuf> {
    let directory = export_directory()?;
    fs::create_dir_all(&directory)?;
    let path = next_available_export_path(&directory, &export.file_name);
    fs::write(&path, export.bytes.as_ref())?;
    Ok(path)
}

fn export_directory() -> io::Result<PathBuf> {
    let user_dirs = UserDirs::new().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "could not resolve the current user's home directory",
        )
    })?;
    default_download_directory(user_dirs.download_dir(), Some(user_dirs.home_dir())).ok_or_else(
        || {
            io::Error::new(
                io::ErrorKind::NotFound,
                "could not resolve the default Downloads directory",
            )
        },
    )
}

fn default_download_directory(
    download_dir: Option<&Path>,
    home_dir: Option<&Path>,
) -> Option<PathBuf> {
    download_dir
        .map(Path::to_path_buf)
        .or_else(|| home_dir.map(|home| home.join("Downloads")))
}

fn next_available_export_path(directory: &Path, file_name: &str) -> PathBuf {
    let candidate = directory.join(file_name);
    if !candidate.exists() {
        return candidate;
    }

    let file_path = Path::new(file_name);
    let stem = file_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
        .unwrap_or("character");
    let extension = file_path
        .extension()
        .and_then(|extension| extension.to_str());

    for suffix in 1u32.. {
        let numbered_name = match extension {
            Some(extension) if !extension.is_empty() => format!("{stem}-{suffix}.{extension}"),
            _ => format!("{stem}-{suffix}"),
        };
        let numbered_path = directory.join(numbered_name);
        if !numbered_path.exists() {
            return numbered_path;
        }
    }

    unreachable!("u32 suffix iteration should always find a path");
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
            format!("HP {}", player_life_label(player)),
        );
        if player.is_local {
            ui.colored_label(
                Color32::from_rgb(90, 130, 240),
                format!("MP {}", resource_value_label(player.mana, player.mana_max)),
            );
            if let (Some(life_regen), Some(mana_regen)) = (player.life_regen, player.mana_regen) {
                ui.label(format!("regen {life_regen}/{mana_regen}"));
            }
            if let (Some(dx), Some(dy)) = (player.movement_dx, player.movement_dy) {
                ui.label(format!("move {dx},{dy}"));
            }
        }
    });

    let width = ui.available_width();
    let height = 8.0;
    let bar_count = if player.is_local { 2.0 } else { 1.0 };
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(width, height * bar_count + 2.0 * (bar_count - 1.0)),
        egui::Sense::hover(),
    );
    let (life, life_max) = player_life_bar_values(player);
    draw_resource_bar(
        ui,
        egui::Rect::from_min_size(rect.min, egui::vec2(width, height)),
        life,
        life_max,
        Color32::from_rgb(120, 30, 35),
    );
    if player.is_local {
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
}

fn draw_resource_bar(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    value: Option<u32>,
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

fn draw_mercenary_row(ui: &mut egui::Ui, mercenary: &crate::snapshot::MercenarySnapshot) {
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new("Merc").color(Color32::from_rgb(120, 230, 150)));
        ui.label(mercenary_label(mercenary));
        if let Some(level) = mercenary.level {
            ui.separator();
            ui.label(level_label(level));
        }
        ui.separator();
        ui.colored_label(
            Color32::from_rgb(220, 80, 85),
            format!(
                "HP {}",
                mercenary_life_label(mercenary.life, mercenary.life_max, mercenary.life_percent)
            ),
        );
        if let Some(revive_cost) = mercenary.revive_cost {
            ui.separator();
            ui.label(format!("revive {revive_cost}g"));
        }
        ui.separator();
        ui.label(mercenary_position_label(mercenary));
    });
    let (life, life_max) =
        mercenary_life_bar_values(mercenary.life, mercenary.life_max, mercenary.life_percent);
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 6.0), egui::Sense::hover());
    draw_resource_bar(ui, rect, life, life_max, Color32::from_rgb(120, 30, 35));
}

fn mercenary_label(mercenary: &crate::snapshot::MercenarySnapshot) -> String {
    mercenary
        .class_name
        .clone()
        .unwrap_or_else(|| format!("class {}", mercenary.class_id))
}

fn mercenary_position_label(mercenary: &crate::snapshot::MercenarySnapshot) -> String {
    if mercenary.world_location_known {
        format!("@ {},{}", mercenary.x, mercenary.y)
    } else {
        "@ unknown".to_owned()
    }
}

fn resource_bar_fraction(value: Option<u32>, max_value: Option<u32>) -> Option<f32> {
    let value = value?;
    let max_value = max_value?;
    if max_value == 0 {
        return None;
    }

    Some((value as f32 / max_value as f32).clamp(0.0, 1.0))
}

fn resource_value_label(value: Option<u32>, max_value: Option<u32>) -> String {
    let Some(value) = value else {
        return "--".to_owned();
    };
    let Some(max_value) = max_value else {
        return value.to_string();
    };
    if max_value == 0 {
        return value.to_string();
    }

    format!("{value}/{max_value}")
}

fn mercenary_life_label(
    life: Option<u32>,
    life_max: Option<u32>,
    life_percent: Option<u8>,
) -> String {
    if life.is_some() {
        resource_value_label(life, life_max)
    } else {
        life_percent
            .map(|life| format!("{life}%"))
            .unwrap_or_else(|| "--".to_owned())
    }
}

fn mercenary_life_bar_values(
    life: Option<u32>,
    life_max: Option<u32>,
    life_percent: Option<u8>,
) -> (Option<u32>, Option<u32>) {
    if life.is_some() {
        (life, life_max)
    } else {
        (life_percent.map(u32::from), life_percent.map(|_| 100))
    }
}

fn player_life_label(player: &crate::snapshot::PlayerSnapshot) -> String {
    if player.is_party_life_fraction() {
        return player
            .party_life
            .map(|life| party_life_label(u16::from(life)))
            .unwrap_or_else(|| "--".to_owned());
    }

    resource_value_label(player.life, player.life_max)
}

fn player_life_bar_values(player: &crate::snapshot::PlayerSnapshot) -> (Option<u32>, Option<u32>) {
    if let Some(party_life) = player.party_life {
        (Some(u32::from(party_life)), Some(128))
    } else {
        (player.life, player.life_max)
    }
}

fn party_life_label(value: u16) -> String {
    format!(
        "{}%",
        ((value.min(128) as f32 / 128.0) * 100.0).round() as u32
    )
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
        ui.label(format!("mercs {}", snapshot.mercenaries.len()));
        ui.separator();
        ui.label(format!("missiles {}", snapshot.missiles.len()));
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
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn resource_fraction_uses_snapshot_resource_values() {
        assert_eq!(resource_bar_fraction(Some(5), Some(10)), Some(0.5));
    }

    #[test]
    fn resource_fraction_clamps_overfilled_values() {
        assert_eq!(resource_bar_fraction(Some(66), Some(62)), Some(1.0));
    }

    #[test]
    fn resource_fraction_is_unknown_without_maximum() {
        assert_eq!(resource_bar_fraction(Some(10), None), None);
    }

    #[test]
    fn resource_fraction_is_unknown_with_zero_maximum() {
        assert_eq!(resource_bar_fraction(Some(10), Some(0)), None);
    }

    #[test]
    fn resource_labels_use_snapshot_resource_values() {
        assert_eq!(resource_value_label(Some(5), Some(10)), "5/10");
        assert_eq!(resource_value_label(Some(66), Some(62)), "66/62");
    }

    #[test]
    fn party_life_label_converts_128_scale_to_percent() {
        assert_eq!(party_life_label(128), "100%");
        assert_eq!(party_life_label(64), "50%");
        assert_eq!(party_life_label(200), "100%");
    }

    #[test]
    fn player_life_bar_uses_party_life_fraction_for_remote_players() {
        let mut player = player_snapshot_with_party(1, PartyAffiliation::Unpartied);
        player.party_life = Some(64);

        assert_eq!(player_life_bar_values(&player), (Some(64), Some(128)));
    }

    #[test]
    fn party_color_map_assigns_colors_by_first_seen_party_id() {
        let first = PartyId::new(0x2200);
        let second = PartyId::new(0x1100);
        let players = vec![
            player_snapshot_with_party(1, PartyAffiliation::LocalParty),
            player_snapshot_with_party(2, PartyAffiliation::Party(first)),
            player_snapshot_with_party(3, PartyAffiliation::Party(second)),
        ];

        let colors = PartyColorMap::from_players(&players);

        assert_eq!(
            colors.row_fill(PartyAffiliation::LocalParty),
            party_row_color(0)
        );
        assert_eq!(
            colors.row_fill(PartyAffiliation::Party(first)),
            party_row_color(1)
        );
        assert_eq!(
            colors.row_fill(PartyAffiliation::Party(second)),
            party_row_color(2)
        );
        assert_eq!(
            colors.row_fill(PartyAffiliation::Unpartied),
            Color32::from_rgb(0, 0, 0)
        );
    }

    #[test]
    fn next_available_export_path_appends_numeric_suffix_after_existing_file() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after epoch")
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("d2helper-export-test-{unique}"));
        fs::create_dir_all(&directory).expect("temp export directory should be created");
        fs::write(directory.join("Hero.d2s"), b"first").expect("seed export should be written");

        let next = next_available_export_path(&directory, "Hero.d2s");

        assert_eq!(
            next.file_name().and_then(|name| name.to_str()),
            Some("Hero-1.d2s")
        );
        fs::remove_dir_all(&directory).expect("temp export directory should be removed");
    }

    #[test]
    fn default_download_directory_prefers_platform_user_dir() {
        let download_dir = Path::new("/tmp/My Downloads");
        let home_dir = Path::new("/tmp/home");

        assert_eq!(
            default_download_directory(Some(download_dir), Some(home_dir)),
            Some(download_dir.to_path_buf())
        );
    }

    #[test]
    fn default_download_directory_falls_back_to_home_downloads() {
        let home_dir = Path::new("/tmp/home");

        assert_eq!(
            default_download_directory(None, Some(home_dir)),
            Some(home_dir.join("Downloads"))
        );
    }

    fn player_snapshot_with_party(
        id: u32,
        party_affiliation: PartyAffiliation,
    ) -> crate::snapshot::PlayerSnapshot {
        crate::snapshot::PlayerSnapshot {
            id,
            name: format!("Player{id}"),
            class_name: "Paladin".to_owned(),
            level: 1,
            x: 0,
            y: 0,
            area_name: None,
            world_location_known: false,
            is_local: false,
            party_affiliation,
            party_life: None,
            life: None,
            life_max: None,
            mana: None,
            mana_max: None,
            life_regen: None,
            mana_regen: None,
            movement_dx: None,
            movement_dy: None,
        }
    }
}
