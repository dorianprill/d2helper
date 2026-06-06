# D2helper

D2helper is an early Diablo II Classic/Lord of Destruction helper overlay built
with Rust, `egui`, and [`libd2r`](https://github.com/dorianprill/libd2).

The current target is a passive LoD 1.14 debug overlay: it listens to legacy D2GS
traffic on port `4000`, reconstructs live game state through `libd2r`, and
renders a compact automap-style view. It does not read or modify game memory.

## Feature Status

- [x] Transparent native `egui` window
- [x] Runtime controls for native frame on/off, maximize/restore, and close
- [x] Background-only opacity slider, including fully transparent mode
- [x] Auto-started passive LoD D2GS capture worker using `libd2r::Client`
- [x] Red/green capture toggle for pausing or resuming UI snapshot updates
- [x] UI-safe snapshot layer between the blocking capture thread and egui
- [x] Capture counters and packet parse-error display
- [x] Bottom status strip for difficulty, act, area, seed, automap id, mode flags,
      packet id, item-stat stream count, and entity counts
- [x] Character list populated from decoded player packets, including level,
      current-position coordinates, and inferred current area where known
- [x] Local-player HP/mana values, max-stat-based HP/MP bars, regeneration
      counters, and movement-verification bytes from decoded LoD packets
- [x] Player-centered isometric automap debug renderer for revealed map cells,
      generated collision walls, and live markers
- [x] Live markers for local/remote players, NPCs, objects, and ground items
- [x] Read-only Classic/LoD MPQ static-data loading through `libd2r`
- [x] Monster, object, and item labels resolved from `MonStats.bin`,
      `Objects.bin`, item `.bin` files, and language `.tbl` files when an
      install path is available
- [x] Compact object/warp labels with raw object state/portal metadata where
      available
- [x] Basic item marker colors for runes, uniques, sets, and other ground items
- [x] Raw item-state flag highlighting from decoded item state packets
- [x] Optional generated-map JSON loading for current seed/difficulty/area via
      `D2HELPER_MAP_JSON` or `D2HELPER_MAP_JSON_DIR`
- [x] Generated collision wall rendering and generated exit markers when map
      JSON is available
- [ ] Native generated static map background from seed/difficulty/act/area
- [ ] Pathfinding visualization
- [ ] MPQ/DS1/DT1-backed tile art
- [ ] Exact Diablo II window tracking/anchoring
- [ ] Missile, spell, and projectile tracking
- [ ] Item filtering rules and item inspection
- [ ] Battle.net character download actions

## Build Requirements

- Rust stable toolchain
- Git
- Native graphics/windowing dependencies required by `eframe`/`winit`
- Packet-capture dependencies required by `libd2r`

Inside the `d2suite` checkout, `d2helper` depends on the sibling `libd2`
checkout so both projects can be developed in lockstep:

```toml
libd2r = { path = "../libd2" }
```

### Windows Notes

`libd2r` currently uses packet-capture support through `pnet`. On Windows this
typically needs a WinPcap-compatible installation and development libraries:

- Install Npcap, preferably with WinPcap API-compatible mode enabled.
- Install the Npcap SDK or WinPcap developer package if the build cannot find
  `Packet.lib` or `wpcap.lib`.
- If needed, add the SDK library directory, for example `Lib\x64`, to the `LIB`
  environment variable before building.

The runtime packet-capture DLLs must also be available through the Npcap/WinPcap
installation.

### Linux Notes

Building usually works with standard Rust plus desktop OpenGL/windowing
development packages. Running packet capture may require elevated permissions or
capabilities for the capture backend.

On Linux, prefer file capabilities over running the whole GUI as root:

```text
cargo build
sudo setcap cap_net_raw,cap_net_admin=eip target/debug/d2helper
getcap target/debug/d2helper
./target/debug/d2helper
```

Run the executable directly after setting capabilities. If you rebuild, Cargo
may replace the binary and drop the file capabilities, so re-run `setcap`.

## Build

From this repository:

```text
cargo build
```

Run tests:

```text
cargo test
```

Run the debug overlay:

```text
cargo run
```

Or use the helper script, which builds, applies Linux packet-capture
capabilities when available, detects a local Classic/LoD install for MPQ-backed
labels, and launches the binary:

```text
./run-d2helper.sh
```

Useful variants:

```text
./run-d2helper.sh --build-only
./run-d2helper.sh --release
./run-d2helper.sh --skip-setcap
```

The debug executable is written to:

```text
target/debug/d2helper
```

On Windows:

```text
target\debug\d2helper.exe
```

## Usage

1. Start `d2helper`.
2. Move or resize the normal decorated window as needed.
3. Use `Hide Bar` to switch to borderless overlay mode once positioned.
4. Use `Max` or `Restore` if you want a maximized debug overlay.
5. Adjust background opacity. Text and markers stay opaque.
6. Start Diablo II LoD 1.14 and join a game. Capture starts automatically and
   waits for D2GS traffic.
7. Use the red/green capture button only if you want to pause or resume UI
   snapshot updates.

You can open the UI without Diablo II running. In that case, capture remains in
the waiting state until a matching LoD game connection appears.

For MPQ-backed monster/object/item names, d2helper looks for a Classic/LoD
install in this order:

```text
D2HELPER_D2_PATH
LIBD2_D2_INSTALL
~/Games/Diablo II*
```

The install directory is only read. It should contain legacy MPQs such as
`patch_d2.mpq`, `d2data.mpq`, and usually `d2exp.mpq`.

For wall and entrance rendering, d2helper needs generated collision JSON because
live D2GS packets do not contain full wall data. It accepts JSON compatible with
the `@diablo2/map` generator:

```text
D2HELPER_MAP_JSON=/path/to/current-area-or-act-response.json
D2HELPER_MAP_JSON_DIR=/path/to/generated-map-cache
```

Directory mode checks names such as:

```text
0x3607656c-2-74.json
0x3607656c_2_74.json
906454380-2-74.json
74.json
```

The fields are map seed, difficulty index (`0` normal, `1` nightmare, `2`
hell), and area id. The JSON may be one generated level or an act/response
containing a `levels` array.

## Logs

D2helper writes tracing output to stderr and to:

```text
logs/d2helper.log
```

For now this file is truncated on every launch so fast capture iterations only
show the current run.

If capture appears idle, check the toolbar status and this log file. Common
causes are missing packet-capture permissions, a wrong selected network
interface, or no matching LoD D2GS traffic on TCP port `4000`.

On Linux the log includes `/proc/self/status` capability fields. `CapEff` must
contain the `cap_net_raw` bit for libpnet to create a raw packet channel.

## Architecture

The capture loop is blocking, so d2helper runs it on a worker thread. The worker
receives `ConnectionEvent`s from `libd2r`, converts the current `GameState` into
a small `OverlaySnapshot`, and writes it behind an `Arc<RwLock<_>>`.

The egui thread never waits for packets. It reads the latest snapshot each frame
and periodically repaints the character list, status strip, and automap debug
renderer.

See [ARCHITECTURE.md](ARCHITECTURE.md) for module boundaries and current design
notes.
