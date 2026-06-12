//! UI-owned snapshot types derived from `libd2::GameState`.
//!
//! The capture thread receives a borrowed `GameState` from `libd2` for every
//! decoded server event. The egui thread should not hold that borrow, so this
//! module copies only the fields needed by the overlay into small, cloneable
//! structs. This also keeps rendering insulated from packet-parser internals.

use std::{collections::HashMap, sync::Arc};

use libd2::core::entity::Entity;
use libd2::core::game_state::MapTile;
use libd2::{
    Area, CharacterExportOptions, CharacterFile, ConnectionEvent, ConnectionTransportWarning,
    Difficulty, GameData, GameState, GeneratedMap, ItemPlacement, PartyAffiliation, Player,
    ServerMessageParseError, UnitStat,
};

/// Shared state boundary between the blocking packet-capture worker and egui.
pub type SharedOverlayState = std::sync::Arc<std::sync::RwLock<OverlaySnapshot>>;

/// Immutable, render-ready view of the latest known game and capture state.
#[derive(Debug, Clone, Default)]
pub struct OverlaySnapshot {
    /// Packet-capture lifecycle and event counters.
    pub capture: CaptureSnapshot,
    /// Game metadata and revealed automap tiles.
    pub game: GameSnapshot,
    /// Known player units, including the local player when identified.
    pub players: Vec<PlayerSnapshot>,
    /// Known monster and NPC units.
    pub npcs: Vec<NpcSnapshot>,
    /// Known mercenary units assigned to players.
    pub mercenaries: Vec<MercenarySnapshot>,
    /// Known missile/projectile units.
    pub missiles: Vec<MissileSnapshot>,
    /// Known map objects such as portals, shrines, chests, and waypoints.
    pub objects: Vec<ObjectSnapshot>,
    /// Known items. Only ground items have world coordinates.
    pub items: Vec<ItemSnapshot>,
    /// Generated static collision map for the current seed/difficulty/area.
    pub generated_map: Option<Arc<GeneratedMap>>,
    /// Exportable local-player `.d2s` payload when the current game state is
    /// rich enough for `libd2`'s legacy save export.
    pub character_export: Option<CharacterExportSnapshot>,
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
        let mut players: Vec<_> = game_state
            .players()
            .iter()
            .map(|(id, player)| {
                let location = player.location();
                let is_local = game_state.local_player_id() == Some(*id);
                let remote_party_info = player.remote_party_info();
                let area_id = remote_party_info
                    .and_then(|info| info.area_id())
                    .or_else(|| player.area_id())
                    .or_else(|| {
                        (is_local || player.world_location_known())
                            .then_some(game_state.map().area_id)
                            .flatten()
                    });
                PlayerSnapshot {
                    id: *id,
                    name: player.name().to_owned(),
                    class_name: format!("{:?}", player.class()),
                    level: player.level(),
                    x: location.x(),
                    y: location.y(),
                    area_name: area_id.map(|area_id| display_area_name(area_id, game_data)),
                    world_location_known: player.world_location_known(),
                    is_local,
                    party_affiliation: player.party_affiliation(),
                    party_life: remote_party_info
                        .and_then(|info| info.life())
                        .map(|life| life.raw()),
                    life: player_life_value(player),
                    life_max: player.stat(UnitStat::LifeMax as u16),
                    mana: player_mana_value(player),
                    mana_max: player.stat(UnitStat::ManaMax as u16),
                    life_regen: player.vitals().and_then(|vitals| vitals.life_regen()),
                    mana_regen: player.vitals().and_then(|vitals| vitals.mana_regen()),
                    movement_dx: player.movement().map(|movement| movement.dx()),
                    movement_dy: player.movement().map(|movement| movement.dy()),
                }
            })
            .collect();
        sort_player_snapshots(&mut players);
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

        let mercenaries = game_state
            .mercenaries()
            .iter()
            .map(|(id, mercenary)| {
                let location = mercenary.location();
                MercenarySnapshot {
                    id: *id,
                    class_id: mercenary.class_id(),
                    class_name: mercenary.class().map(|class| class.to_string()),
                    owner_id: mercenary.owner_id(),
                    skill_id: mercenary.skill_id(),
                    world_location_known: mercenary.world_location_known(),
                    life_percent: mercenary.life_percent(),
                    life: mercenary.stat(UnitStat::Life as u16),
                    life_max: mercenary.stat(UnitStat::LifeMax as u16),
                    level: mercenary.stat(UnitStat::Level as u16),
                    experience: mercenary.stat(UnitStat::Experience as u16),
                    revive_cost: mercenary.revive_cost(),
                    x: location.x(),
                    y: location.y(),
                }
            })
            .collect();

        let missiles = game_state
            .missiles()
            .iter()
            .map(|(id, missile)| {
                let location = missile.location();
                let target = missile.target();
                MissileSnapshot {
                    id: *id,
                    class_id: missile.class_id(),
                    x: location.x(),
                    y: location.y(),
                    target_x: target.map(|target| target.x()),
                    target_y: target.map(|target| target.y()),
                    current_frame: missile.current_frame(),
                    owner_type: missile.owner_type(),
                    owner_id: missile.owner_id(),
                    skill_level: missile.skill_level(),
                    pierce_level: missile.pierce_level(),
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
            mercenaries,
            missiles,
            objects,
            items,
            generated_map,
            character_export: CharacterExportSnapshot::from_game_state(game_state),
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
        for mercenary in &self.mercenaries {
            if mercenary.world_location_known {
                bounds.add(mercenary.x, mercenary.y);
            }
        }
        for missile in &self.missiles {
            bounds.add(missile.x, missile.y);
            if let (Some(x), Some(y)) = (missile.target_x, missile.target_y) {
                bounds.add(x, y);
            }
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
        for mercenary in &self.mercenaries {
            if mercenary.world_location_known {
                bounds.add(mercenary.x, mercenary.y);
            }
        }
        for missile in &self.missiles {
            bounds.add(missile.x, missile.y);
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

/// Exportable local-player character-save payload for the UI download action.
#[derive(Debug, Clone)]
pub struct CharacterExportSnapshot {
    pub file_name: String,
    pub bytes: Arc<[u8]>,
}

impl CharacterExportSnapshot {
    fn from_game_state(game_state: &GameState) -> Option<Self> {
        let local_player = game_state
            .local_player_id()
            .and_then(|id| game_state.player(id))?;
        let file = CharacterFile::export_legacy_from_game_state(
            game_state,
            CharacterExportOptions::from_game_state_skills(),
        )
        .ok()?;

        Some(Self {
            file_name: format!("{}.d2s", local_player.name()),
            bytes: Arc::from(file.to_bytes()),
        })
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
    pub transport_warnings: u64,
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
    pub transport_warnings: u64,
    pub last_packet_id: Option<u8>,
    pub last_error: Option<String>,
}

impl CaptureCounters {
    /// Records a decoded packet event.
    pub fn record(&mut self, event: &ConnectionEvent) {
        self.total_events += 1;
        if let Some(packet_id) = event.packet_id() {
            self.last_packet_id = Some(packet_id);
        }
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
            ConnectionEvent::TransportWarning { warning } => {
                self.transport_warnings += 1;
                self.last_error = Some(transport_warning_label(warning));
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
            transport_warnings: self.transport_warnings,
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
    pub area_name: Option<String>,
    pub world_location_known: bool,
    pub is_local: bool,
    pub party_affiliation: PartyAffiliation,
    /// Remote party-member life fraction in Diablo II's 0..=128 scale.
    pub party_life: Option<u8>,
    /// Current life in raw Diablo II packet/stat units.
    ///
    /// The local player usually gets this from `0x18`/`0x95` resource packets.
    /// Remote players often only expose it as `UnitStat::Life` through the
    /// stat-update stream seen during join and party synchronization.
    pub life: Option<u32>,
    /// Maximum life from `UnitStat::LifeMax`.
    pub life_max: Option<u32>,
    /// Current mana in raw Diablo II packet/stat units.
    ///
    /// As with life, local-player resource packets and remote-player stat
    /// updates use different D2GS paths.
    pub mana: Option<u32>,
    /// Maximum mana from `UnitStat::ManaMax`.
    pub mana_max: Option<u32>,
    pub life_regen: Option<u8>,
    pub mana_regen: Option<u8>,
    pub movement_dx: Option<u8>,
    pub movement_dy: Option<u8>,
}

impl PlayerSnapshot {
    /// Returns whether this player has coordinates that should be projected.
    ///
    /// D2GS can report the local player's resource/position packet before the
    /// map-area load packet finishes refreshing visibility state. Treating a
    /// non-zero local coordinate as renderable keeps the automap centered on the
    /// player through that packet ordering.
    pub fn has_known_world_position(&self) -> bool {
        self.world_location_known || (self.is_local && (self.x != 0 || self.y != 0))
    }

    pub fn is_party_life_fraction(&self) -> bool {
        !self.is_local && self.party_life.is_some()
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

/// Mercenary unit data needed by the character list and map markers.
#[derive(Debug, Clone)]
pub struct MercenarySnapshot {
    pub id: u32,
    pub class_id: u16,
    pub class_name: Option<String>,
    pub owner_id: u32,
    pub skill_id: u8,
    pub world_location_known: bool,
    pub life_percent: Option<u8>,
    pub life: Option<u32>,
    pub life_max: Option<u32>,
    pub level: Option<u32>,
    pub experience: Option<u32>,
    pub revive_cost: Option<u16>,
    pub x: u16,
    pub y: u16,
}

/// Missile/projectile unit data needed by map markers.
#[derive(Debug, Clone)]
pub struct MissileSnapshot {
    pub id: u32,
    pub class_id: Option<u16>,
    pub x: u16,
    pub y: u16,
    pub target_x: Option<u16>,
    pub target_y: Option<u16>,
    pub current_frame: Option<u16>,
    pub owner_type: Option<u8>,
    pub owner_id: Option<u32>,
    pub skill_level: Option<u8>,
    pub pierce_level: Option<u8>,
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

impl From<libd2::ItemStateFlags> for ItemStateSnapshot {
    fn from(flags: libd2::ItemStateFlags) -> Self {
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

fn display_area_name(area_id: u16, game_data: Option<&GameData>) -> String {
    game_data
        .and_then(|data| data.level_name(area_id))
        .filter(|name| !name.trim().is_empty())
        .map(str::to_owned)
        .or_else(|| Area::from_id(area_id).map(|area| area.to_string()))
        .unwrap_or_else(|| format!("area {area_id}"))
}

fn sort_player_snapshots(players: &mut [PlayerSnapshot]) {
    players.sort_by(|left, right| {
        right
            .is_local
            .cmp(&left.is_local)
            .then_with(|| left.name.cmp(&right.name))
            .then_with(|| left.id.cmp(&right.id))
    });
}

fn player_life_value(player: &Player) -> Option<u32> {
    player
        .vitals()
        .and_then(|vitals| vitals.life().map(u32::from))
        .or_else(|| player.stat(UnitStat::Life as u16))
}

fn player_mana_value(player: &Player) -> Option<u32> {
    player
        .vitals()
        .and_then(|vitals| vitals.mana().map(u32::from))
        .or_else(|| player.stat(UnitStat::Mana as u16))
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

fn transport_warning_label(warning: &ConnectionTransportWarning) -> String {
    match warning {
        ConnectionTransportWarning::DuplicateTcpSegment { .. } => {
            "duplicate TCP segment ignored".to_owned()
        }
        ConnectionTransportWarning::OverlappingTcpSegment { .. } => {
            "overlapping TCP segment trimmed".to_owned()
        }
        ConnectionTransportWarning::OutOfOrderTcpSegment { .. } => {
            "out-of-order TCP segment buffered".to_owned()
        }
        ConnectionTransportWarning::BufferedTcpSegmentReleased { .. } => {
            "buffered TCP segment released".to_owned()
        }
        ConnectionTransportWarning::TcpGapReset { .. } => "TCP gap reset D2GS reader".to_owned(),
        ConnectionTransportWarning::BufferedD2gsPayload { .. } => {
            "partial D2GS payload buffered".to_owned()
        }
        ConnectionTransportWarning::D2gsFramingReset { .. } => {
            "D2GS framing reset after desync".to_owned()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use libd2::core::update::Update;

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
    fn player_snapshots_sort_local_player_first() {
        let mut players = vec![
            player_snapshot(3, "Zed", false),
            player_snapshot(2, "Self", true),
            player_snapshot(1, "Ada", false),
        ];

        sort_player_snapshots(&mut players);

        assert_eq!(
            players.iter().map(|player| player.id).collect::<Vec<_>>(),
            vec![2, 1, 3]
        );
    }

    #[test]
    fn remote_player_snapshot_uses_stat_life_and_mana_without_vitals() {
        let mut state = GameState::default();
        assert!(state.update(libd2::ServerMessage::PlayerJoined {
            packet_length: 36,
            player_id: 7,
            character_class: 1,
            character_name: name16("Remote"),
            character_level: 42,
            party_id: 0xffff,
            unknown: [0; 8],
        }));
        for (attribute, amount) in [
            (UnitStat::Life, 640),
            (UnitStat::LifeMax, 1280),
            (UnitStat::Mana, 320),
            (UnitStat::ManaMax, 960),
        ] {
            assert!(state.update(libd2::ServerMessage::AttributeUpdate {
                unit_id: 7,
                attribute: attribute as u8,
                amount,
            }));
        }

        let snapshot = OverlaySnapshot::from_game_state(&state, CaptureSnapshot::default());
        let player = snapshot
            .players
            .iter()
            .find(|player| player.id == 7)
            .expect("remote player snapshot");

        assert_eq!(player.life, Some(640));
        assert_eq!(player.life_max, Some(1280));
        assert_eq!(player.mana, Some(320));
        assert_eq!(player.mana_max, Some(960));
        assert_eq!(player.party_affiliation, PartyAffiliation::Unpartied);
    }

    #[test]
    fn remote_player_snapshot_uses_party_life_and_area_without_world_position() {
        let mut state = GameState::default();
        assert!(state.update(libd2::ServerMessage::PlayerJoined {
            packet_length: 36,
            player_id: 7,
            character_class: 1,
            character_name: name16("Remote"),
            character_level: 42,
            party_id: 0xffff,
            unknown: [0; 8],
        }));
        assert!(state.update(libd2::ServerMessage::AllyPartyInfo {
            unit_type: 0,
            unit_life: 87,
            unit_id: 7,
            unit_area: 2,
        }));

        let snapshot = OverlaySnapshot::from_game_state(&state, CaptureSnapshot::default());
        let player = snapshot
            .players
            .iter()
            .find(|player| player.id == 7)
            .expect("remote player snapshot");

        assert_eq!(player.party_life, Some(87));
        assert_eq!(player.life, None);
        assert_eq!(player.life_max, None);
        assert_eq!(player.area_name.as_deref(), Some("Blood Moor"));
        assert!(!player.world_location_known);
    }

    #[test]
    fn mercenary_and_missile_snapshots_copy_live_state() {
        let mut state = GameState::default();
        assert!(state.update(libd2::ServerMessage::AssignPlayer {
            unit_id: 7,
            class: 3,
            szname: name16("Owner"),
            x: 123,
            y: 456,
        }));
        assert!(state.update(libd2::ServerMessage::GameHandshake {
            unit_type: 0,
            unit_id: 7,
        }));
        assert!(state.update(libd2::ServerMessage::AssignPlayerToParty {
            player_id: 7,
            party_id: 0x1234,
        }));
        assert!(state.update(libd2::ServerMessage::AssignMerc {
            skill_id: 0x0A,
            summon_type: 0x0152,
            player_id: 7,
            merc_id: 0x5566_7788,
            seed2: 0x99AA_BBCC,
            init_seed: 0xDDEE_FF00,
        }));
        assert!(state.update(libd2::ServerMessage::NpcStop {
            unit_id: 0x5566_7788,
            x: 5200,
            y: 5100,
            unit_life: 73,
        }));
        assert!(state.update(libd2::ServerMessage::MercAttributeU8 {
            attribute: UnitStat::Level as u8,
            merc_id: 0x5566_7788,
            amount: 90,
        }));
        assert!(state.update(libd2::ServerMessage::MercAttributeU16 {
            attribute: UnitStat::Life as u8,
            merc_id: 0x5566_7788,
            amount: 1280,
        }));
        assert!(state.update(libd2::ServerMessage::MercAttributeU16 {
            attribute: UnitStat::LifeMax as u8,
            merc_id: 0x5566_7788,
            amount: 2560,
        }));
        assert!(state.update(libd2::ServerMessage::MercAddExpU16 {
            stat_id: UnitStat::Experience as u8,
            merc_id: 0x5566_7788,
            value: 12,
        }));
        assert!(state.update(libd2::ServerMessage::MercReviveCost {
            merc_name_id: 0x1234,
            revive_cost: 0x5678,
            unused: 0,
        }));
        assert!(state.update(libd2::ServerMessage::MissileData {
            missile_id: 0x1020_3040,
            missile_class: 0x009A,
            missile_x: 5210,
            missile_y: 5110,
            target_x: 5220,
            target_y: 5120,
            current_frame: 7,
            owner_type: 0,
            owner_id: 7,
            skill_level: 5,
            pierce_level: 1,
        }));

        let snapshot = OverlaySnapshot::from_game_state(&state, CaptureSnapshot::default());
        let mercenary = snapshot
            .mercenaries
            .iter()
            .find(|mercenary| mercenary.id == 0x5566_7788)
            .expect("mercenary snapshot");
        assert_eq!(mercenary.class_name.as_deref(), Some("Desert Mercenary"));
        assert_eq!(mercenary.owner_id, 7);
        assert_eq!(mercenary.skill_id, 0x0A);
        assert!(mercenary.world_location_known);
        assert_eq!((mercenary.x, mercenary.y), (5200, 5100));
        assert_eq!(mercenary.life_percent, Some(73));
        assert_eq!(mercenary.life, Some(1280));
        assert_eq!(mercenary.life_max, Some(2560));
        assert_eq!(mercenary.level, Some(90));
        assert_eq!(mercenary.experience, Some(12));
        assert_eq!(mercenary.revive_cost, Some(0x5678));

        let missile = snapshot
            .missiles
            .iter()
            .find(|missile| missile.id == 0x1020_3040)
            .expect("missile snapshot");
        assert_eq!(missile.class_id, Some(0x009A));
        assert_eq!((missile.x, missile.y), (5210, 5110));
        assert_eq!(
            (missile.target_x, missile.target_y),
            (Some(5220), Some(5120))
        );
        assert_eq!(missile.current_frame, Some(7));
        assert_eq!(missile.owner_id, Some(7));
        assert_eq!(missile.skill_level, Some(5));
        assert_eq!(missile.pierce_level, Some(1));
    }

    #[test]
    fn local_player_snapshot_exposes_character_export_when_state_is_exportable() {
        let mut state = GameState::default();
        assert!(state.update(libd2::ServerMessage::AssignPlayer {
            unit_id: 7,
            class: 3,
            szname: name16("Saver"),
            x: 123,
            y: 456,
        }));
        assert!(state.update(libd2::ServerMessage::GameHandshake {
            unit_type: 0,
            unit_id: 7,
        }));

        let snapshot = OverlaySnapshot::from_game_state(&state, CaptureSnapshot::default());
        let export = snapshot
            .character_export
            .expect("local player export should exist");

        assert_eq!(export.file_name, "Saver.d2s");
        assert!(!export.bytes.is_empty());
    }

    #[test]
    fn capture_counters_track_transport_warnings_without_packet_id_churn() {
        let mut counters = CaptureCounters::default();

        counters.record(&ConnectionEvent::ServerMessage {
            packet: libd2::D2GSPacket { data: vec![0x00] },
            message: libd2::ServerMessage::GameLoading,
            applied: true,
        });
        counters.record(&ConnectionEvent::TransportWarning {
            warning: ConnectionTransportWarning::OutOfOrderTcpSegment {
                sequence: 200,
                len: 6,
                expected_sequence: 100,
                buffered_segments: 1,
                buffered_bytes: 6,
            },
        });

        let snapshot = counters.snapshot(true);
        assert_eq!(snapshot.total_events, 2);
        assert_eq!(snapshot.applied_messages, 1);
        assert_eq!(snapshot.parse_errors, 0);
        assert_eq!(snapshot.transport_warnings, 1);
        assert_eq!(snapshot.last_packet_id, Some(0x00));
        assert_eq!(
            snapshot.last_error.as_deref(),
            Some("out-of-order TCP segment buffered")
        );
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
            area_name: Some("Blood Moor".to_owned()),
            world_location_known: true,
            is_local: false,
            party_affiliation: PartyAffiliation::Unknown,
            party_life: None,
            life: None,
            life_max: None,
            mana: None,
            mana_max: None,
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
            area_name: Some("Blood Moor".to_owned()),
            world_location_known: true,
            is_local: true,
            party_affiliation: PartyAffiliation::Unknown,
            party_life: None,
            life: Some(1000),
            life_max: Some(2000),
            mana: Some(500),
            mana_max: Some(1000),
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
            area_name: Some("Blood Moor".to_owned()),
            world_location_known: true,
            is_local: true,
            party_affiliation: PartyAffiliation::Unknown,
            party_life: None,
            life: None,
            life_max: None,
            mana: None,
            mana_max: None,
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

    #[test]
    fn nonzero_local_position_is_renderable_before_visibility_refresh() {
        let player = PlayerSnapshot {
            id: 2,
            name: "Local".to_owned(),
            class_name: "Paladin".to_owned(),
            level: 1,
            x: 5118,
            y: 5168,
            area_name: Some("Blood Moor".to_owned()),
            world_location_known: false,
            is_local: true,
            party_affiliation: PartyAffiliation::Unknown,
            party_life: None,
            life: None,
            life_max: None,
            mana: None,
            mana_max: None,
            life_regen: None,
            mana_regen: None,
            movement_dx: None,
            movement_dy: None,
        };

        assert!(player.has_known_world_position());
    }

    #[test]
    fn remote_roster_placeholder_is_not_renderable() {
        let player = PlayerSnapshot {
            id: 2,
            name: "Remote".to_owned(),
            class_name: "Amazon".to_owned(),
            level: 1,
            x: 0,
            y: 0,
            area_name: None,
            world_location_known: false,
            is_local: false,
            party_affiliation: PartyAffiliation::Unknown,
            party_life: None,
            life: None,
            life_max: None,
            mana: None,
            mana_max: None,
            life_regen: None,
            mana_regen: None,
            movement_dx: None,
            movement_dy: None,
        };

        assert!(!player.has_known_world_position());
    }

    #[test]
    fn area_name_falls_back_to_builtin_area_enum() {
        assert_eq!(display_area_name(2, None), "Blood Moor");
        assert_eq!(display_area_name(999, None), "area 999");
    }

    fn player_snapshot(id: u32, name: &str, is_local: bool) -> PlayerSnapshot {
        PlayerSnapshot {
            id,
            name: name.to_owned(),
            class_name: "Paladin".to_owned(),
            level: 1,
            x: 0,
            y: 0,
            area_name: None,
            world_location_known: false,
            is_local,
            party_affiliation: PartyAffiliation::Unknown,
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

    fn name16(name: &str) -> [u8; 16] {
        let mut bytes = [0; 16];
        let name = name.as_bytes();
        let len = name.len().min(bytes.len());
        bytes[..len].copy_from_slice(&name[..len]);
        bytes
    }
}
