# D2helper Implementation Agent

D2helper is a Diablo 2 (Classic/LoD for now) game data visualization program. 


# Technologies
- Rust for the main implementation language, chosen for its performance, safety, and ecosystem.
- `egui` for the GUI, providing a simple and efficient way to create the overlay and debug views.
- `libd2` for parsing D2GS network packets, allowing us to capture game state without memory reading or hacks.


# GUI Layout
The GUI will consist of a transparent, borderless window with a background opacity that can be altered by a sliderthat displays the following elements:

- To the left side of the window is a Character List for all Characters and their Mercenearies in the game. 
- Below the name field of the character is a (vertically) split health and mana bar, with the name of the character above it.
- Right to the Character name (but inside the list section) are three Buttons: 1. A download button, that saves the character data to a locally playable d2s file. 2. An inspect button, being able to inspect the character's equipped items in a new pop up window or pane/tab.
- to the right of the character List is a Map section, showing the current area and full generated map in an Auto/Minimap format similar to the original game. The local player is represented by a marker, and other players/NPCs/items (Runes (Red Item Marker), Uniques (Gold Item Marker) and Sets (Green Item Marker) /objects are shown as colored markers. Missisles/Spells and projectiles are shown as small moving markers. Tiles are shown as grid/isometric cells like in the original game.

# Implementation 
- Make sure to implement good logging, as we will use the program to debug the packet parsing/decoding and game state tracking inside `libd2`.
- Start with a simple packet capture thread that listens on port 4000 and updates a shared `Arc<RwLock<GameState>>` with the parsed data.
- Implement the GUI in `egui`, starting with a simple debug view that shows the local player position and some basic stats (act, area, seed, difficulty, player count, NPC count, item count).
- Gradually add more features to the GUI, such as the character list, the map renderer, and the item tracking.
- For the map renderer, start with a simple grid-based representation of the full generated map tiles in an true-to-game isometric view.


# Reporting and Architecture
After every significant implementation step, report the progress and any challenges faced inside `REPORT.md`. The architecture should be modular, allowing for easy addition of new features and improvements in the future. The architecture must be documented and maintained in `ARCHITECTURE.md`, detailing the main components, their interactions, and the overall design decisions.
The `README.md` should be updated with a clear and concise overview of the project, its goals, and how to use it.
