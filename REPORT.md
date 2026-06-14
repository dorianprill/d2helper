# D2helper Report

## 2026-06-05

Started the d2helper implementation as a Rust/egui binary that depends on
`libd2`.

Implemented:

- Cargo project setup with `eframe`, `tracing`, and `libd2`.
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
- Switched the `libd2` dependency from a sibling path checkout to the public
  Git repository at `https://github.com/dorianprill/libd2`.
- Added persistent tracing output to `logs/d2helper.log` and surfaced capture
  worker waiting/failure status in the toolbar.
- Added Linux process capability logging and packet-capture interface indices to
  diagnose raw-channel permission and interface-binding failures.
- Updated the locked `libd2` Git dependency to the capture fix that disables
  unnecessary pnet promiscuous mode for local LoD traffic.
- Changed the automap renderer from whole-known-map bounds fitting to a
  fixed-scale camera centered on the local player.
- Removed noisy debug labels from NPC/object markers and added per-packet
  parse-error logging with packet id, expected/actual length, and packet bytes.
- Updated the locked `libd2` Git dependency to include live-capture direction
  filtering and packet-observed level-warp tracking.
- Changed startup logging to truncate `logs/d2helper.log` on every launch while
  capture iteration is still fast and noisy.
- Added compact class-id labels to packet-observed object/warp markers so
  entrance/object packets are visible before static object-name data exists.
- Updated the locked `libd2` Git dependency to the player-vitals/item-state
  packet work.
- Surfaced raw HP/mana/stamina, regeneration counters, and movement
  verification bytes in the character panel.
- Added raw `0x3E` item-stat stream counts to the status strip.
- Added raw object state/portal metadata to object marker labels and
  targetability styling.
- Added item-state flag highlighting for ground item markers.
- Switched the `libd2` dependency back to the sibling `../libd2` checkout so
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
- Consumed libd2's split between player roster membership and current
  world-location visibility. Player rows remain in the character list after
  visibility-only `0x0A` removals, while map markers disappear until a fresh
  position arrives.
- Added optional generated-map JSON loading from `D2HELPER_MAP_JSON` or
  `D2HELPER_MAP_JSON_DIR`, keyed by current seed, difficulty, and area.
- Added generated collision-wall rendering and generated exit labels when
  `@diablo2/map`-compatible JSON is available.
- Changed capture to start automatically when the app launches. The toolbar now
  shows a red/green capture toggle that pauses or resumes UI snapshot
  publication while the blocking capture worker remains alive.
- Added player level and inferred current-area display to the character panel.
  Area names come from MPQ `Levels.bin` data when available, otherwise from the
  built-in `Area` enum; remote roster-only players still show an unknown area
  until they have a current world position.
- Adjusted local-player marker/focus logic so non-zero local coordinates remain
  renderable even if the world-location flag has not refreshed yet.
- Increased the egui repaint cadence from roughly 10 FPS to roughly 30 FPS for
  smoother automap updates.
- Removed the stamina display from player rows and changed HP/MP bars to use
  decoded max-life/max-mana stats, including Diablo fixed-point normalization
  when live packet values are in 1/256 units.
- Enabled the local-player `Download` button. D2helper now uses
  `libd2::CharacterFile::export_legacy_from_game_state` to precompute a legacy
  `.d2s` payload from the live local-player `GameState` and writes it to the
  platform default Downloads directory on click.
- Added UI/log status for character-save exports and filename deconfliction so
  repeated clicks create `Name.d2s`, `Name-1.d2s`, and so on instead of
  overwriting the previous test export.
- Added focused tests for export availability in the snapshot layer and for the
  filename suffixing behavior used by the download action.
- Strengthened the snapshot export regression test so the UI download payload is
  parsed back as a `.d2s` file and verified to include inventory gold, stash
  gold, quest completion words, and waypoint bits from libd2 game state.
- Added Downloads-directory resolution that prefers the platform-reported user
  Downloads path and falls back to `<home>/Downloads` when the OS does not
  expose one directly.
- Added logging/snapshot surfacing for `libd2`'s D2GS framing-resync warning so
  live capture no longer appears permanently frozen when a poisoned buffered
  payload forces the packet splitter to restart from a later TCP boundary.
- Added mercenary and missile snapshots from `libd2::GameState`. The character
  panel now shows mercenary assignment/life/revive details under the owner
  player, the status strip counts mercenaries and missiles, and the automap
  renders mercenary markers plus missile/projectile markers with target lines.
- Added a focused snapshot regression test that drives libd2 with mercenary and
  missile packets and verifies d2helper's UI-facing copied state.
- Switched capture-interface diagnostics to libd2's route-probed selector so
  logs show the same adapter that the capture worker will bind.
- Reduced capture hot-path UI/log pressure by moving generic parsed-packet
  logs to debug level and publishing full overlay snapshots at a bounded cadence
  instead of after every packet. Transport recovery resets still publish
  immediately so desync diagnostics remain visible.
- Surfaced libd2's timed TCP gap recovery warning in the UI-facing capture
  status labels and log output.

Challenges and decisions:

- `libd2::Client` capture is blocking, so d2helper starts it on a worker thread
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
- The capture toggle does not stop the raw packet-capture worker; it only pauses
  snapshot publication. A true stop/restart requires a cancellable capture
  abstraction below `libd2::Client`.

Known gaps:

- No native generated-map layer from seed yet; wall rendering currently needs
  externally generated JSON.
- No pathfinding overlay yet.
- No MPQ-backed tile art or DS1/DT1 map asset rendering.
- HP/MP bars remain unfilled until max-life/max-mana stats are available in the
  decoded player state.
- Missile/projectile markers are packet-class based and do not yet resolve to
  skill/art names.
- Capture can be paused from the UI, but the raw capture worker is not
  terminated until process exit.

Next steps:

1. Add a first-class generated-map producer path so d2helper can invoke/cache
   map generation from seed/difficulty/area without manual JSON setup.
2. Add pathfinding visualization over the generated collision grid.
3. Improve missile/projectile classification after `libd2` can resolve missile
   ids to skill/art metadata.
4. Add item inspection UI after richer item records are available.
5. Add window anchoring/tracking so the debug view can behave like a practical
   overlay instead of a standalone transparent window.

Verification:

```text
cargo fmt --check
cargo test
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps
git diff --check
cargo build --release
```

Result: passed. 30 tests.
Latest focused verification also passed `cargo test` and
`cargo clippy --all-targets -- -D warnings` with 33 tests.
