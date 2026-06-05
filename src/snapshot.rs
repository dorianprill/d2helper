//! UI-owned snapshot types derived from `libd2r::GameState`.
//!
//! The capture thread receives a borrowed `GameState` from `libd2r` for every
//! decoded server event. The egui thread should not hold that borrow, so this
//! module copies only the fields needed by the overlay into small, cloneable
//! structs. This also keeps rendering insulated from packet-parser internals.

use std::{collections::HashMap, sync::Arc};

use libd2r::core::entity::Entity;
use libd2r::core::game_state::MapTile;
use libd2r::{
    ConnectionEvent, Difficulty, GameData, GameState, GeneratedMap, ItemPlacement,
    ServerMessageParseError,
};

/// Shared state boundary between the blocking packet-capture worker and egui.
pub type SharedOverlayState = std::sync::Arc<std::sync::RwLock<OverlaySnapshot>>;

/// Immutable, render-ready view of the latest known game and capture state.
#[derive(Debug, Clone)]
pub struct OverlaySnapshot {
    /// Packet-capture lifecycle and event counters.
    pub capture: CaptureSnapshot,
    /// Game metadata and revealed automap tiles.
    pub game: GameSnapshot,
    /// Known player units, including the local player when identified.
    pub players: Vec<PlayerSnapshot>,
    /// Known monster and NPC units.
    pub npcs: Vec<NpcSnapshot>,
    /// Known map objects such as portals, shrines, chests, and waypoints.
    pub objects: Vec<ObjectSnapshot>,
    /// Known items. Only ground items have world coordinates.
    pub items: Vec<ItemSnapshot>,
    /// Generated static collision map for the current seed/difficulty/area.
    pub generated_map: Option<Arc<GeneratedMap>>,
}

impl Default for OverlaySnapshot {
    fn default() -> Self {
        Self {
            capture: CaptureSnapshot::default(),
            game: GameSnapshot::default(),
            players: Vec::new(),
            npcs: Vec::new(),
            objects: Vec::new(),
            items: Vec::new(),
            generated_map: None,
        }
    }
}

impl OverlaySnapshot {
    /// Copies the currently decoded game state into a UI snapshot.
    ///
    /// Diablo II server packets update the world incrementally. A snapshot may
    /// therefore contain partial data early in a game, for example item records
    /// before the corresponding map area is revealed.
    #[allow(dead_code)]
    pub fn from_game_state(game_state: &GameState, capture: CaptureSnapshot) -> Self {
        Self::from_game_state_with_data(game_state, capture, None)
    }

    /// Copies the current game state and resolves known static ids when MPQ
    /// data has been loaded from a Classic/LoD install.
    pub fn from_game_state_with_data(
        game_state: &GameState,
        capture: CaptureSnapshot,
        game_data: Option<&GameData>,
    ) -> Self {
        Self::from_game_state_with_data_and_map(game_state, capture, game_data, None)
    }

    /// Copies current game state and optionally attaches a generated collision
    /// map for the active area.
    pub fn from_game_state_with_data_and_map(
        game_state: &GameState,
        capture: CaptureSnapshot,
        game_data: Option<&GameData>,
        generated_map: Option<Arc<GeneratedMap>>,
    ) -> Self {
        let players = game_state
            .players()
            .iter()
            .map(|(id, player)| {
                let location = player.location();
                PlayerSnapshot {
                    id: *id,
                    name: player.name().to_owned(),
                    class_name: format!("{:?}", player.class()),
                    level: player.level(),
                    x: location.x(),
                    y: location.y(),
                    world_location_known: player.world_location_known(),
                    is_local: game_state.local_player_id() == Some(*id),
                    life: player.vitals().and_then(|vitals| vitals.life()),
                    mana: player.vitals().and_then(|vitals| vitals.mana()),
                    stamina: player.vitals().and_then(|vitals| vitals.stamina()),
                    life_regen: player.vitals().and_then(|vitals| vitals.life_regen()),
                    mana_regen: player.vitals().and_then(|vitals| vitals.mana_regen()),
                    movement_dx: player.movement().map(|movement| movement.dx()),
                    movement_dy: player.movement().map(|movement| movement.dy()),
                }
            })
            .collect();

        let npcs = game_state
            .npcs()
            .iter()
            .map(|(id, npc)| {
                let location = npc.location();
                NpcSnapshot {
                    id: *id,
                    class_id: npc.class_id(),
                    name: npc
                        .class_id()
                        .and_then(|class_id| game_data.and_then(|data| data.monster_name(class_id)))
                        .map(str::to_owned),
                    life_percent: npc.life_percent(),
                    state: npc.state(),
                    x: location.x(),
                    y: location.y(),
                }
            })
            .collect();

        let objects = game_state
            .objects()
            .iter()
            .map(|(id, object)| {
                let location = object.location();
                ObjectSnapshot {
                    id: *id,
                    class_id: object.class_id(),
                    name: game_data
                        .and_then(|data| data.object_name(object.class_id()))
                        .map(str::to_owned),
                    object_type: object.object_type(),
                    state: object.state(),
                    portal_flags: object.portal_flags(),
                    is_targetable: object.is_targetable(),
                    x: location.x(),
                    y: location.y(),
                }
            })
            .collect();

        let items = game_state
            .items()
            .iter()
            .map(|(id, item)| {
                let packet_data = item.packet_data();
                let ground_position = packet_data.and_then(|data| match data.placement {
                    ItemPlacement::Ground { x, y } => Some((x, y)),
                    ItemPlacement::Container { .. } => None,
                });
                let code = packet_data
                    .and_then(|data| data.code.as_ref())
                    .map(|code| code.as_str().to_owned());
                ItemSnapshot {
                    id: *id,
                    action: format!("{:?}", item.action_kind()),
                    category: format!("{:?}", item.category_kind()),
                    name: code
                        .as_deref()
                        .and_then(|code| game_data.and_then(|data| data.item_name(code)))
                        .map(str::to_owned),
                    code,
                    quality: packet_data
                        .and_then(|data| data.quality)
                        .map(|quality| format!("{:?}", quality)),
                    state_flags: item.state_flags().map(ItemStateSnapshot::from),
                    x: ground_position.map(|position| position.0),
                    y: ground_position.map(|position| position.1),
                }
            })
            .collect();

        Self {
            capture,
            game: GameSnapshot::from_game_state(game_state),
            players,
            npcs,
            objects,
            items,
            generated_map,
        }
    }

    /// Computes the displayed map bounds from every known visible world point.
    pub fn marker_bounds(&self) -> Option<MapBounds> {
        let mut bounds = MapBounds::default();

        for player in &self.players {
            if player.has_known_world_position() {
                bounds.add(player.x, player.y);
            }
        }
        for npc in &self.npcs {
            bounds.add(npc.x, npc.y);
        }
        for object in &self.objects {
            bounds.add(object.x, object.y);
        }
        for item in &self.items {
            if let (Some(x), Some(y)) = (item.x, item.y) {
                bounds.add(x, y);
            }
        }
        for tile in &self.game.revealed_tiles {
            bounds.add(tile.x, tile.y);
        }

        bounds.into_option()
    }

    /// Returns the preferred automap focus point.
    ///
    /// Diablo II's automap is player-centered. Prefer the local player when the
    /// server has identified it and its coordinates are coherent with nearby
    /// packet-observed world entities. During early loading, some non-local
    /// players exist only as roster entries at `(0,0)`, and local resource
    /// packets can briefly disagree with monster/object assignment coordinates.
    /// In those cases, use live world-entity bounds so visible packets do not
    /// disappear offscreen.
    pub fn map_focus(&self) -> Option<MapFocus> {
        if let Some(player) = self.players.iter().find(|player| player.is_local) {
            if self.local_focus_is_coherent(player) {
                return Some(MapFocus {
                    x: player.x,
                    y: player.y,
                    source: MapFocusSource::LocalPlayer,
                });
            }
            if let Some(bounds) = self.world_entity_bounds() {
                let (x, y) = bounds.center();
                return Some(MapFocus {
                    x: x.round() as u16,
                    y: y.round() as u16,
                    source: MapFocusSource::KnownBounds,
                });
            }
            return Some(MapFocus {
                x: player.x,
                y: player.y,
                source: MapFocusSource::LocalPlayer,
            });
        }

        if let Some(player) = self
            .players
            .iter()
            .find(|player| player.has_known_world_position())
        {
            return Some(MapFocus {
                x: player.x,
                y: player.y,
                source: MapFocusSource::AnyPlayer,
            });
        }

        self.marker_bounds().map(|bounds| {
            let (x, y) = bounds.center();
            MapFocus {
                x: x.round() as u16,
                y: y.round() as u16,
                source: MapFocusSource::KnownBounds,
            }
        })
    }

    fn local_focus_is_coherent(&self, local: &PlayerSnapshot) -> bool {
        let Some(bounds) = self.world_entity_bounds() else {
            return true;
        };
        bounds.contains_with_margin(local.x, local.y, 240)
            || self.players.iter().any(|player| {
                !player.is_local
                    && player.has_known_world_position()
                    && player.distance_to(local.x, local.y) <= 240
            })
    }

    fn world_entity_bounds(&self) -> Option<MapBounds> {
        let mut bounds = MapBounds::default();
        for player in &self.players {
            if !player.is_local && player.has_known_world_position() {
                bounds.add(player.x, player.y);
            }
        }
        for npc in &self.npcs {
            bounds.add(npc.x, npc.y);
        }
        for object in &self.objects {
            bounds.add(object.x, object.y);
        }
        for item in &self.items {
            if let (Some(x), Some(y)) = (item.x, item.y) {
                bounds.add(x, y);
            }
        }
        bounds.into_option()
    }
}

/// Capture-worker status and counters shown in the top toolbar.
#[derive(Debug, Default, Clone)]
pub struct CaptureSnapshot {
    pub running: bool,
    pub status: String,
    pub total_events: u64,
    pub applied_messages: u64,
    pub parse_errors: u64,
    pub last_packet_id: Option<u8>,
    pub last_error: Option<String>,
}

impl CaptureSnapshot {
    /// Marks the worker as running before the first captured packet arrives.
    pub fn starting() -> Self {
        Self {
            running: true,
            status: "starting capture worker".to_owned(),
            ..Self::default()
        }
    }

    /// Marks the worker as blocked in libpnet waiting for matching traffic.
    pub fn waiting() -> Self {
        Self {
            running: true,
            status: "waiting for LoD D2GS traffic on TCP port 4000".to_owned(),
            ..Self::default()
        }
    }

    /// Marks the worker as stopped because the capture loop failed.
    pub fn failed(error: String) -> Self {
        Self {
            running: false,
            status: "capture stopped".to_owned(),
            parse_errors: 1,
            last_error: Some(error),
            ..Self::default()
        }
    }
}

/// Mutable packet counters owned by the capture thread.
#[derive(Debug, Default, Clone)]
pub struct CaptureCounters {
    pub total_events: u64,
    pub applied_messages: u64,
    pub parse_errors: u64,
    pub last_packet_id: Option<u8>,
    pub last_error: Option<String>,
}

impl CaptureCounters {
    /// Records a decoded packet event.
    pub fn record(&mut self, event: &ConnectionEvent) {
        self.total_events += 1;
        self.last_packet_id = Some(event.packet_id());
        match event {
            ConnectionEvent::ServerMessage { applied, .. } => {
                if *applied {
                    self.applied_messages += 1;
                }
                self.last_error = None;
            }
            ConnectionEvent::ParseError { error, .. } => {
                self.parse_errors += 1;
                self.last_error = Some(parse_error_label(error));
            }
        }
    }

    /// Produces a cloneable capture snapshot for the egui thread.
    pub fn snapshot(&self, running: bool) -> CaptureSnapshot {
        CaptureSnapshot {
            running,
            status: if running {
                "receiving LoD D2GS traffic".to_owned()
            } else {
                "capture stopped".to_owned()
            },
            total_events: self.total_events,
            applied_messages: self.applied_messages,
            parse_errors: self.parse_errors,
            last_packet_id: self.last_packet_id,
            last_error: self.last_error.clone(),
        }
    }
}

/// Static game metadata and revealed-map information.
#[derive(Debug, Default, Clone)]
pub struct GameSnapshot {
    /// Current act as reported by game-logon/map packets.
    pub act: Option<u8>,
    /// Current area identifier, when known.
    pub area_id: Option<u16>,
    /// Server-provided map seed/id used for generated-map reconstruction.
    pub map_seed: Option<u32>,
    /// Automap id observed from map-reveal packets.
    pub automap: Option<u32>,
    /// Human-readable difficulty name.
    pub difficulty: String,
    /// Whether the current character/game is expansion mode.
    pub is_expansion: bool,
    /// Whether the current character/game is ladder mode.
    pub is_ladder: bool,
    /// Whether the current character/game is hardcore mode.
    pub is_hardcore: bool,
    /// Unit id for the local player once identified.
    pub local_player_id: Option<u32>,
    /// Number of raw item-stat update streams seen from packet `0x3E`.
    pub item_stat_updates: usize,
    /// Revealed automap cells received from D2GS map-reveal packets.
    pub revealed_tiles: Vec<RevealedTileSnapshot>,
}

impl GameSnapshot {
    fn from_game_state(game_state: &GameState) -> Self {
        let mut revealed_tiles: Vec<_> = game_state
            .map()
            .revealed_tiles
            .iter()
            .map(RevealedTileSnapshot::from)
            .collect();
        revealed_tiles.sort_by_key(|tile| (tile.area_id, tile.y, tile.x));

        Self {
            act: game_state.map().act,
            area_id: game_state.map().area_id,
            map_seed: game_state.map().map_id,
            automap: game_state.map().automap,
            difficulty: difficulty_label(game_state.difficulty()).to_owned(),
            is_expansion: game_state.is_expansion(),
            is_ladder: game_state.is_ladder(),
            is_hardcore: game_state.is_hardcore(),
            local_player_id: game_state.local_player_id(),
            item_stat_updates: game_state.item_stat_updates().len(),
            revealed_tiles,
        }
    }
}

/// Player unit data needed by the character list and map markers.
#[derive(Debug, Clone)]
pub struct PlayerSnapshot {
    pub id: u32,
    pub name: String,
    pub class_name: String,
    pub level: u32,
    pub x: u16,
    pub y: u16,
    pub world_location_known: bool,
    pub is_local: bool,
    pub life: Option<u16>,
    pub mana: Option<u16>,
    pub stamina: Option<u16>,
    pub life_regen: Option<u8>,
    pub mana_regen: Option<u8>,
    pub movement_dx: Option<u8>,
    pub movement_dy: Option<u8>,
}

impl PlayerSnapshot {
    fn has_known_world_position(&self) -> bool {
        self.world_location_known
    }

    fn distance_to(&self, x: u16, y: u16) -> u16 {
        coordinate_distance(self.x, self.y, x, y)
    }
}

/// Monster/NPC unit data needed by the map markers and counters.
#[derive(Debug, Clone)]
pub struct NpcSnapshot {
    pub id: u32,
    pub class_id: Option<u16>,
    pub name: Option<String>,
    pub life_percent: Option<u8>,
    pub state: Option<u8>,
    pub x: u16,
    pub y: u16,
}

/// Object unit data needed by the map markers and counters.
#[derive(Debug, Clone)]
pub struct ObjectSnapshot {
    pub id: u32,
    pub class_id: u16,
    pub name: Option<String>,
    pub object_type: u8,
    pub state: u32,
    pub portal_flags: Option<u8>,
    pub is_targetable: Option<u8>,
    pub x: u16,
    pub y: u16,
}

/// Item unit data needed by ground-item markers.
#[derive(Debug, Clone)]
pub struct ItemSnapshot {
    pub id: u32,
    pub action: String,
    pub category: String,
    pub name: Option<String>,
    pub code: Option<String>,
    pub quality: Option<String>,
    pub state_flags: Option<ItemStateSnapshot>,
    pub x: Option<u16>,
    pub y: Option<u16>,
}

/// Raw item-state flags observed from D2GS packet `0x7D`.
#[derive(Debug, Clone, Copy)]
pub struct ItemStateSnapshot {
    pub unit_type: u8,
    pub unit_id: u32,
    pub and_value: u32,
    pub flags: u32,
}

impl From<libd2r::ItemStateFlags> for ItemStateSnapshot {
    fn from(flags: libd2r::ItemStateFlags) -> Self {
        Self {
            unit_type: flags.unit_type(),
            unit_id: flags.unit_id(),
            and_value: flags.and_value(),
            flags: flags.flags(),
        }
    }
}

/// A single revealed automap cell.
///
/// Diablo II sends revealed map chunks independently from full static map
/// generation. For the initial overlay this is the most reliable live geometry
/// source; seed-based map generation can later fill in unrevealed static cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RevealedTileSnapshot {
    pub x: u16,
    pub y: u16,
    pub area_id: u8,
}

impl From<&MapTile> for RevealedTileSnapshot {
    fn from(tile: &MapTile) -> Self {
        Self {
            x: tile.x,
            y: tile.y,
            area_id: tile.area_id,
        }
    }
}

/// Inclusive world-coordinate bounds for projecting Diablo II map coordinates.
#[derive(Debug, Default, Clone, Copy)]
pub struct MapBounds {
    pub min_x: u16,
    pub min_y: u16,
    pub max_x: u16,
    pub max_y: u16,
    has_value: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapFocus {
    pub x: u16,
    pub y: u16,
    pub source: MapFocusSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapFocusSource {
    LocalPlayer,
    AnyPlayer,
    KnownBounds,
}

impl MapBounds {
    fn add(&mut self, x: u16, y: u16) {
        if self.has_value {
            self.min_x = self.min_x.min(x);
            self.min_y = self.min_y.min(y);
            self.max_x = self.max_x.max(x);
            self.max_y = self.max_y.max(y);
        } else {
            self.min_x = x;
            self.min_y = y;
            self.max_x = x;
            self.max_y = y;
            self.has_value = true;
        }
    }

    fn into_option(self) -> Option<Self> {
        self.has_value.then_some(self)
    }

    /// Returns the center point used as the isometric projection origin.
    pub fn center(self) -> (f32, f32) {
        (
            (self.min_x as f32 + self.max_x as f32) * 0.5,
            (self.min_y as f32 + self.max_y as f32) * 0.5,
        )
    }

    fn contains_with_margin(self, x: u16, y: u16, margin: u16) -> bool {
        x.saturating_add(margin) >= self.min_x
            && x <= self.max_x.saturating_add(margin)
            && y.saturating_add(margin) >= self.min_y
            && y <= self.max_y.saturating_add(margin)
    }
}

fn coordinate_distance(ax: u16, ay: u16, bx: u16, by: u16) -> u16 {
    ax.abs_diff(bx).max(ay.abs_diff(by))
}

/// Replaces the shared snapshot if the lock is available.
pub fn replace_snapshot(shared: &SharedOverlayState, snapshot: OverlaySnapshot) {
    if let Ok(mut guard) = shared.write() {
        *guard = snapshot;
    }
}

/// Replaces only the capture portion of the shared snapshot.
pub fn replace_capture(shared: &SharedOverlayState, capture: CaptureSnapshot) {
    if let Ok(mut guard) = shared.write() {
        guard.capture = capture;
    }
}

/// Reads the latest shared snapshot, returning an empty one on poisoned locks.
pub fn read_snapshot(shared: &SharedOverlayState) -> OverlaySnapshot {
    shared.read().map(|guard| guard.clone()).unwrap_or_default()
}

/// Creates an empty shared overlay state for app startup.
pub fn empty_shared_state() -> SharedOverlayState {
    std::sync::Arc::new(std::sync::RwLock::new(OverlaySnapshot::default()))
}

/// Counts revealed automap cells by area id for compact debug display.
pub fn count_by_area(tiles: &[RevealedTileSnapshot]) -> Vec<(u8, usize)> {
    let mut counts = HashMap::<u8, usize>::new();
    for tile in tiles {
        *counts.entry(tile.area_id).or_default() += 1;
    }

    let mut counts: Vec<_> = counts.into_iter().collect();
    counts.sort_by_key(|(area, _)| *area);
    counts
}

fn difficulty_label(difficulty: Difficulty) -> &'static str {
    match difficulty {
        Difficulty::Normal => "Normal",
        Difficulty::Nightmare => "Nightmare",
        Difficulty::Hell => "Hell",
    }
}

fn parse_error_label(error: &ServerMessageParseError) -> String {
    match error {
        ServerMessageParseError::EmptyPacket => "empty packet".to_owned(),
        ServerMessageParseError::UnsupportedPacketId(packet_id) => {
            format!("unsupported packet 0x{packet_id:02X}")
        }
        ServerMessageParseError::UnexpectedLength {
            packet_id,
            expected,
            actual,
        } => format!("packet 0x{packet_id:02X} length {actual}, expected {expected}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn count_by_area_sorts_by_area_id() {
        let tiles = [
            RevealedTileSnapshot {
                x: 1,
                y: 1,
                area_id: 3,
            },
            RevealedTileSnapshot {
                x: 2,
                y: 1,
                area_id: 1,
            },
            RevealedTileSnapshot {
                x: 3,
                y: 1,
                area_id: 3,
            },
        ];

        assert_eq!(count_by_area(&tiles), vec![(1, 1), (3, 2)]);
    }

    #[test]
    fn marker_bounds_include_tiles_and_ground_items() {
        let mut snapshot = OverlaySnapshot::default();
        snapshot.game.revealed_tiles.push(RevealedTileSnapshot {
            x: 20,
            y: 40,
            area_id: 1,
        });
        snapshot.items.push(ItemSnapshot {
            id: 7,
            action: "Add".to_owned(),
            category: "Item".to_owned(),
            name: Some("El Rune".to_owned()),
            code: Some("r01".to_owned()),
            quality: None,
            state_flags: None,
            x: Some(10),
            y: Some(30),
        });
        snapshot.items.push(ItemSnapshot {
            id: 8,
            action: "Add".to_owned(),
            category: "Item".to_owned(),
            name: None,
            code: None,
            quality: None,
            state_flags: None,
            x: None,
            y: None,
        });

        let bounds = snapshot.marker_bounds().expect("bounds");
        assert_eq!((bounds.min_x, bounds.min_y), (10, 30));
        assert_eq!((bounds.max_x, bounds.max_y), (20, 40));
        assert_eq!(bounds.center(), (15.0, 35.0));
    }

    #[test]
    fn map_focus_prefers_local_player() {
        let mut snapshot = OverlaySnapshot::default();
        snapshot.players.push(PlayerSnapshot {
            id: 1,
            name: "Other".to_owned(),
            class_name: "Sorceress".to_owned(),
            level: 1,
            x: 10,
            y: 20,
            world_location_known: true,
            is_local: false,
            life: None,
            mana: None,
            stamina: None,
            life_regen: None,
            mana_regen: None,
            movement_dx: None,
            movement_dy: None,
        });
        snapshot.players.push(PlayerSnapshot {
            id: 2,
            name: "Local".to_owned(),
            class_name: "Paladin".to_owned(),
            level: 1,
            x: 30,
            y: 40,
            world_location_known: true,
            is_local: true,
            life: Some(1000),
            mana: Some(500),
            stamina: Some(750),
            life_regen: Some(1),
            mana_regen: Some(2),
            movement_dx: Some(3),
            movement_dy: Some(4),
        });

        let focus = snapshot.map_focus().expect("focus");
        assert_eq!(
            (focus.x, focus.y, focus.source),
            (30, 40, MapFocusSource::LocalPlayer)
        );
    }

    #[test]
    fn map_focus_uses_world_entities_when_local_position_is_incoherent() {
        let mut snapshot = OverlaySnapshot::default();
        snapshot.players.push(PlayerSnapshot {
            id: 2,
            name: "Local".to_owned(),
            class_name: "Paladin".to_owned(),
            level: 1,
            x: 0,
            y: 0,
            world_location_known: true,
            is_local: true,
            life: None,
            mana: None,
            stamina: None,
            life_regen: None,
            mana_regen: None,
            movement_dx: None,
            movement_dy: None,
        });
        snapshot.npcs.push(NpcSnapshot {
            id: 10,
            class_id: Some(19),
            name: Some("Fallen".to_owned()),
            life_percent: Some(100),
            state: None,
            x: 5200,
            y: 5100,
        });

        let focus = snapshot.map_focus().expect("focus");
        assert_eq!(
            (focus.x, focus.y, focus.source),
            (5200, 5100, MapFocusSource::KnownBounds)
        );
    }
}
