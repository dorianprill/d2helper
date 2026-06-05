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
capture worker thread
          |
          v
OverlaySnapshot in Arc<RwLock<_>>
          |
          v
egui app panels + automap renderer
```

## Modules

- `main`: initializes tracing and creates the transparent native egui viewport.
- `capture`: owns the blocking packet-capture worker. The worker starts
  `libd2r::Client::start_with_events`, records packet counters, and publishes a
  fresh overlay snapshot after every event.
- `snapshot`: defines the thread boundary between packet capture and rendering.
  It copies selected `GameState` values into cloneable UI structs so egui never
  depends on a live parser borrow.
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
- players, NPCs, objects, and ground items

Only ground items have map coordinates. Container/equipment items are kept out
of the automap until item inspection views exist.

## Rendering Model

The renderer treats Diablo II world coordinates as isometric map coordinates:

```text
screen_x = (world_x - world_y) * tile_width / 2
screen_y = (world_x + world_y) * tile_height / 2
```

The first renderer uses diamond cells and simple markers instead of MPQ/DT1
tile art. This makes it useful immediately for parser validation and gives the
future generated-map renderer a stable projection target.

## Current Limitations

- No generated-map background from seed yet.
- No collision or pathfinding visualization yet.
- No MPQ/DS1/DT1 art ingestion yet.
- Health and mana bars are placeholders until the relevant packet fields are
  decoded into `libd2r::GameState`.
- Missile/projectile packets are not represented yet.
- Capture lifecycle is start-only; stopping/restarting capture will need a
  controllable capture abstraction in `libd2r`.
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
