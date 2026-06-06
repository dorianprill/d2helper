# D2helper Architecture

D2helper is a passive Diablo II Classic/LoD visualization overlay. It uses
`libd2r` for packet capture, packet decoding, and game-state reconstruction, and
keeps the UI layer focused on rendering a read-only snapshot of the latest known
state.

## Goals

- Run as a transparent egui window above or beside Diablo II, with runtime
  controls for borderless overlay mode and maximized placement.
- Capture legacy D2GS traffic on port `4000` without reading or modifying game
  memory.
- Render the local player, other players, monsters, objects, items, and revealed
  map cells in a Diablo-style isometric debug map.
- Provide enough live debug information to validate and improve `libd2r` packet
  parsing before adding polished overlay features.

## Component Overview

```text
Diablo II client/server traffic
          |
          v
libd2r::Client capture loop
          |
          v
ConnectionEvent + libd2r::GameState
          |
          v
capture worker thread <--- read-only libd2r::GameData from Classic/LoD MPQs
          ^
          |
 optional generated-map JSON from @diablo2/map-compatible output
          |
          v
OverlaySnapshot in Arc<RwLock<_>>
          |
          v
egui app panels + automap renderer
```

## Modules

- `main`: truncates the current log file, initializes tracing, and creates the
  transparent native egui viewport.
- `capture`: owns the blocking packet-capture worker. The app starts this
  worker at launch; the toolbar toggle pauses or resumes UI snapshot
  publication while the raw-channel listener remains alive. The worker starts
  `libd2r::Client::start_with_events`, records packet counters, and publishes a
  fresh overlay snapshot after every event. It also loads optional read-only
  Classic/LoD MPQ static data once at startup for name resolution and optional
  generated-map JSON for wall/exit rendering.
- `generated_map`: caches generated collision maps keyed by seed, difficulty,
  and area. It imports `@diablo2/map`-compatible JSON from
  `D2HELPER_MAP_JSON` or `D2HELPER_MAP_JSON_DIR`.
- `snapshot`: defines the thread boundary between packet capture and rendering.
  It copies selected `GameState` values into cloneable UI structs so egui never
  depends on a live parser borrow. When `GameData` is available, monster,
  object, and item ids are resolved into display names during snapshot creation.
- `app`: owns egui layout: toolbar, character list, bottom status strip, and map
  section.
- `render`: draws the current snapshot as a simple automap-style isometric
  debug view.

## Threading Model

The capture loop is blocking by design. D2helper therefore starts capture on a
single worker thread and keeps egui on the main thread.

The shared boundary is:

```text
Arc<RwLock<OverlaySnapshot>>
```

The worker writes whole snapshots. The UI reads and clones the newest snapshot
once per frame. This deliberately favors a simple, robust prototype over a more
complex event queue. If packet volume or lock contention becomes visible later,
the same module boundary can switch to channel-delivered snapshots.

## Game-State Snapshot

`OverlaySnapshot` currently contains:

- capture counters and last parse error
- act, area id, map seed/id, automap id, difficulty, and mode flags
- revealed automap cells from map-reveal packets
- players with level, inferred current area when the player has a known world
  position, HP/mana values, max-stat-based HP/MP bar inputs, regeneration
  counters, and movement verification bytes when known; roster membership is
  separate from whether a current world position should be rendered
- NPCs, objects with raw object-state metadata, and items with raw item-state
  flags when known
- optional MPQ-backed monster, object, and item names
- optional generated-map collision and exit data for the current seed,
  difficulty, and area
- raw `0x3E` item-stat stream count

Only ground items have map coordinates. Container/equipment items are kept out
of the automap until item inspection views exist.

## Rendering Model

The renderer treats Diablo II world coordinates as isometric map coordinates:

```text
screen_x = (world_x - world_y) * tile_width / 2
screen_y = (world_x + world_y) * tile_height / 2
```

The first renderer uses a fixed-scale, player-centered automap camera with
diamond cells and simple markers instead of MPQ/DT1 tile art. This makes it
useful immediately for parser validation and gives the future generated-map
renderer a stable projection target.

When generated-map JSON is configured, the renderer draws blocked collision
cells as the wall layer and generated exit objects as entrance markers. The JSON
uses map-local coordinates plus a world offset; d2helper applies that offset
before projecting the collision and exit points into the same isometric space as
live packet units.

Packet-observed `0x07 MapReveal` tile coordinates are not treated as exact unit
coordinates. In current LoD captures they line up with unit/NPC coordinates
after adding the legacy `4096` world-origin offset, so the debug renderer
chooses the raw or shifted tile coordinate depending on which is nearer the
current focus. This is a display normalization only; generated maps should later
replace it with explicit area/room origins.

The camera prefers the local player. It accepts non-zero local coordinates even
when the world-location visibility flag is still catching up, because live LoD
captures can report local movement/resource data before the area-load packet has
completed. It ignores `(0,0)` non-local roster placeholders and falls back to
live world-entity bounds when the local coordinate is clearly incoherent with
packet-observed monsters, objects, items, or other known-position players.

## Current Limitations

- No native generated-map background from seed yet; generated collision maps
  must currently be supplied as JSON from an external map generator.
- No pathfinding visualization yet.
- No MPQ/DS1/DT1 art ingestion yet.
- MPQ static data is used for labels only; no tile art or map asset data is
  loaded by d2helper yet.
- Revealed-tile rendering currently uses a debug `4096` world-origin
  normalization. This should be replaced by generated-map room origins once
  native map generation/static map ingestion is available.
- HP/MP bars require max-life/max-mana stats. They remain unfilled until those
  stats are observed in the decoded state.
- Missile/projectile packets are not represented yet.
- The red/green capture toggle pauses or resumes snapshot publication. It does
  not terminate the underlying blocking raw-channel worker.
- Runtime decoration toggling depends on backend/window-manager support. egui
  exposes the command on all native platforms, but individual desktops may apply
  it slightly differently.

## Next Architecture Steps

1. Add a generated-map layer sourced from `libd2r` seed/difficulty/act/area map
   generation.
2. Add collision tiles as a separate render layer once static map generation is
   reliable.
3. Add an item-inspection data path after `libd2r` exposes richer item records.
4. Add window tracking or anchoring for a real overlay mode.
