# D2helper Report

## 2026-06-05

Started the d2helper implementation as a Rust/egui binary that depends on
`libd2r`.

Implemented:

- Cargo project setup with `eframe`, `tracing`, and `libd2r`.
- Transparent, borderless egui window with dark visuals.
- Toolbar with packet-capture start button, opacity slider, event counters, and
  parse-error display.
- Blocking LoD D2GS capture loop on a worker thread using
  `Client::start_with_events`.
- Snapshot boundary between capture and UI via `Arc<RwLock<OverlaySnapshot>>`.
- Character list populated from decoded player packets, with resource display
  and disabled inspect/download buttons.
- Bottom debug status strip for difficulty, act, area, map seed, packet id, and
  entity counts.
- Isometric automap debug renderer for revealed map cells and live player, NPC,
  object, and ground-item markers.
- Documentation in `README.md` and `ARCHITECTURE.md`.
- Focused unit tests for snapshot area counting, marker bounds, and isometric
  projection behavior.

Follow-up UI refinements:

- Background opacity now ranges from 0 to 255 and is only applied to app-painted
  background fills, not text or markers.
- Added a small top-right `X` button for closing the borderless window.
- Split Linux-only `eframe` X11/Wayland features into target-specific Cargo
  dependencies so Windows builds do not inherit Linux window-system flags.
- Switched startup back to a decorated native window for easy move/resize, then
  added toolbar controls for hiding/restoring the native frame and toggling
  maximize.
- Removed the automap canvas' second background fill so the opacity slider has
  the same visual progression in the character list and map sections.
- Switched the `libd2r` dependency from a sibling path checkout to the public
  Git repository at `https://github.com/dorianprill/libd2`.
- Added persistent tracing output to `logs/d2helper.log` and surfaced capture
  worker waiting/failure status in the toolbar.
- Added Linux process capability logging and packet-capture interface indices to
  diagnose raw-channel permission and interface-binding failures.
- Updated the locked `libd2r` Git dependency to the capture fix that disables
  unnecessary pnet promiscuous mode for local LoD traffic.
- Changed the automap renderer from whole-known-map bounds fitting to a
  fixed-scale camera centered on the local player.
- Removed noisy debug labels from NPC/object markers and added per-packet
  parse-error logging with packet id, expected/actual length, and packet bytes.
- Updated the locked `libd2r` Git dependency to include live-capture direction
  filtering and packet-observed level-warp tracking.
- Changed startup logging to truncate `logs/d2helper.log` on every launch while
  capture iteration is still fast and noisy.
- Added compact class-id labels to packet-observed object/warp markers so
  entrance/object packets are visible before static object-name data exists.
- Updated the locked `libd2r` Git dependency to the player-vitals/item-state
  packet work.
- Surfaced raw HP/mana/stamina, regeneration counters, and movement
  verification bytes in the character panel.
- Added raw `0x3E` item-stat stream counts to the status strip.
- Added raw object state/portal metadata to object marker labels and
  targetability styling.
- Added item-state flag highlighting for ground item markers.
- Switched the `libd2r` dependency back to the sibling `../libd2` checkout so
  d2helper can consume newly implemented library APIs during suite development.
- Added optional read-only Classic/LoD MPQ static-data loading on the capture
  worker. The path is resolved from `D2HELPER_D2_PATH`, `LIBD2_D2_INSTALL`, or a
  `~/Games/Diablo II*` fallback.
- Added MPQ-backed monster, object, and item labels to snapshots and the
  automap renderer, while preserving raw class/code labels as fallbacks.
- Doubled the default character-panel width, made player rows reserve space for
  disabled action buttons, and kept class/level/id/position metadata on a single
  row for normal overlay widths.
- Fixed the debug automap hiding parsed world data by ignoring `(0,0)` remote
  roster placeholders, falling back from incoherent local-player coordinates to
  live world-entity bounds, and normalizing LoD `0x07 MapReveal` tile origins
  with the temporary `4096` world-coordinate offset when that is closer to the
  current focus.
- Started the overlay at full background opacity (`255`) by default.
- Reduced the isometric tile scale so the automap shows more surrounding world
  at the same window size.
- Consumed libd2r's split between player roster membership and current
  world-location visibility. Player rows remain in the character list after
  visibility-only `0x0A` removals, while map markers disappear until a fresh
  position arrives.
- Added optional generated-map JSON loading from `D2HELPER_MAP_JSON` or
  `D2HELPER_MAP_JSON_DIR`, keyed by current seed, difficulty, and area.
- Added generated collision-wall rendering and generated exit labels when
  `@diablo2/map`-compatible JSON is available.

Challenges and decisions:

- `libd2r::Client` capture is blocking, so d2helper starts it on a worker thread
  and keeps egui responsive by reading snapshots.
- The UI stores a lightweight `OverlaySnapshot` instead of an `Arc<RwLock<GameState>>`.
  This avoids coupling egui rendering to mutable parser internals while still
  preserving all data needed by the first overlay.
- The map view intentionally starts with abstract diamond cells rather than game
  tile art. This makes live parser validation possible before MPQ/static-map
  integration.
- `0x07 MapReveal` packets use tile-origin coordinates, while monster/player
  packets use unit-world coordinates. The renderer currently applies a
  nearest-focus `4096` offset normalization for visible debug output; generated
  map origins should replace this heuristic later.
- Full wall/collision data is not present in the live packet stream. The first
  wall layer therefore consumes generated-map JSON from the reverse-engineered
  map generator contract until native Rust generation is available.

Known gaps:

- No native generated-map layer from seed yet; wall rendering currently needs
  externally generated JSON.
- No pathfinding overlay yet.
- No MPQ-backed tile art or DS1/DT1 map asset rendering.
- Resource bars display raw packet-unit values until libd2r exposes the
  corresponding max resource values.
- Missile/projectile markers are not implemented.
- Capture can be started but not stopped from the UI.

Next steps:

1. Add a first-class generated-map producer path so d2helper can invoke/cache
   map generation from seed/difficulty/area without manual JSON setup.
2. Add pathfinding visualization over the generated collision grid.
3. Extend `libd2r` packet state for missiles and max resources, then surface
   those fields in the snapshot.
4. Add item inspection UI after richer item records are available.
5. Add window anchoring/tracking so the debug view can behave like a practical
   overlay instead of a standalone transparent window.

Verification:

```text
cargo fmt --check
cargo test
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps
git diff --check
```

Result: passed. 9 tests.
