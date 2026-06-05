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
- [x] Passive LoD D2GS capture worker using `libd2r::Client`
- [x] UI-safe snapshot layer between the blocking capture thread and egui
- [x] Capture counters and packet parse-error display
- [x] Bottom status strip for difficulty, act, area, seed, automap id, mode flags,
      packet id, and entity counts
- [x] Character list populated from decoded player packets
- [x] Placeholder health and mana bars
- [x] Player-centered isometric automap debug renderer for revealed map cells
- [x] Live markers for local/remote players, NPCs, objects, and ground items
- [x] Compact object/warp class-id labels for packet-observed entrances/objects
- [x] Basic item marker colors for runes, uniques, sets, and other ground items
- [ ] Generated static map background from seed/difficulty/act/area
- [ ] Generated map borders, exits, and special rooms
- [ ] Collision and pathfinding visualization
- [ ] MPQ/DS1/DT1-backed tile art
- [ ] Exact Diablo II window tracking/anchoring
- [ ] Decoded real health/mana values
- [ ] Missile, spell, and projectile tracking
- [ ] Item filtering rules and item inspection
- [ ] Battle.net character download actions

## Build Requirements

- Rust stable toolchain
- Git
- Native graphics/windowing dependencies required by `eframe`/`winit`
- Packet-capture dependencies required by `libd2r`

`d2helper` depends on the Git version of `libd2r`:

```toml
libd2r = { git = "https://github.com/dorianprill/libd2", branch = "main" }
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
6. Start Diablo II LoD 1.14 and join a game.
7. Click `Start LoD capture`.

You can open the UI without Diablo II running. In that case, leave capture idle
and use the window controls to inspect the shell.

## Logs

D2helper writes tracing output to stderr and to:

```text
logs/d2helper.log
```

For now this file is truncated on every launch so fast capture iterations only
show the current run.

If `Start LoD capture` appears idle, check the toolbar status and this log file.
Common causes are missing packet-capture permissions, a wrong selected network
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
