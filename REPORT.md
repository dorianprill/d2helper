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

Challenges and decisions:

- `libd2r::Client` capture is blocking, so d2helper starts it on a worker thread
  and keeps egui responsive by reading snapshots.
- The UI stores a lightweight `OverlaySnapshot` instead of an `Arc<RwLock<GameState>>`.
  This avoids coupling egui rendering to mutable parser internals while still
  preserving all data needed by the first overlay.
- The map view intentionally starts with abstract diamond cells rather than game
  tile art. This makes live parser validation possible before MPQ/static-map
  integration.

Known gaps:

- No generated-map layer from seed yet.
- No collision/pathfinding overlay yet.
- No MPQ-backed tile art.
- Resource bars display raw packet-unit values until libd2r exposes the
  corresponding max resource values.
- Missile/projectile markers are not implemented.
- Capture can be started but not stopped from the UI.

Next steps:

1. Feed `libd2r` generated-map output into the automap renderer as a background
   layer.
2. Add collision rendering once generated-map geometry is stable.
3. Extend `libd2r` packet state for missiles and max resources, then surface
   those fields in the snapshot.
4. Add item inspection UI after richer item records are available.
5. Add window anchoring/tracking so the debug view can behave like a practical
   overlay instead of a standalone transparent window.
