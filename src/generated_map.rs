//! Optional generated-map loading for collision/wall rendering.
//!
//! Live D2GS packets tell the overlay about units, objects, and revealed
//! automap chunks, but they do not carry full wall collision. Until native Rust
//! seed generation is complete, d2helper can import JSON emitted by the
//! reverse-engineered `@diablo2/map` generator and attach the matching
//! [`libd2::GeneratedMap`] to each UI snapshot.

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::Arc,
};

use libd2::{Area, Difficulty, GameState, GeneratedMap, MapGenerationRequest};
use tracing::{info, warn};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct GeneratedMapKey {
    seed: u32,
    difficulty: u8,
    area_id: u16,
}

#[derive(Debug, Clone)]
enum GeneratedMapSource {
    SingleFile(PathBuf),
    Directory(PathBuf),
}

/// Small cache for generated collision maps keyed by seed, difficulty, and area.
#[derive(Debug, Default)]
pub struct GeneratedMapCache {
    sources: Vec<GeneratedMapSource>,
    loaded: HashMap<GeneratedMapKey, Arc<GeneratedMap>>,
    missing_logged: HashSet<GeneratedMapKey>,
}

impl GeneratedMapCache {
    /// Builds a cache from environment configuration.
    ///
    /// Supported sources:
    ///
    /// - `D2HELPER_MAP_JSON`: one map-generator JSON file. It may be either a
    ///   single generated level object or a response containing `levels`.
    /// - `D2HELPER_MAP_JSON_DIR`: directory containing generated map JSON files
    ///   named by seed/difficulty/area, for example
    ///   `0x3607656c-2-74.json`.
    pub fn from_env() -> Self {
        let mut sources = Vec::new();
        if let Some(path) = std::env::var_os("D2HELPER_MAP_JSON").map(PathBuf::from) {
            info!(path = %path.display(), "configured generated-map JSON file");
            sources.push(GeneratedMapSource::SingleFile(path));
        }
        if let Some(path) = std::env::var_os("D2HELPER_MAP_JSON_DIR").map(PathBuf::from) {
            info!(path = %path.display(), "configured generated-map JSON directory");
            sources.push(GeneratedMapSource::Directory(path));
        }

        Self {
            sources,
            loaded: HashMap::new(),
            missing_logged: HashSet::new(),
        }
    }

    /// Returns the generated map matching the current packet-derived map state.
    pub fn current_map(&mut self, game_state: &GameState) -> Option<Arc<GeneratedMap>> {
        let key = map_key(game_state)?;
        if let Some(map) = self.loaded.get(&key) {
            return Some(map.clone());
        }

        let area = Area::from_id(key.area_id)?;
        let request =
            MapGenerationRequest::for_area(key.seed, game_state.difficulty(), area).ok()?;
        let loaded = self.load_map(key, request);
        if let Some(map) = loaded {
            self.loaded.insert(key, map.clone());
            return Some(map);
        }

        if !self.sources.is_empty() && self.missing_logged.insert(key) {
            warn!(
                seed = %format_args!("0x{:08x}", key.seed),
                difficulty = key.difficulty,
                area_id = key.area_id,
                "no generated map JSON found for current area"
            );
        }
        None
    }

    fn load_map(
        &self,
        key: GeneratedMapKey,
        request: MapGenerationRequest,
    ) -> Option<Arc<GeneratedMap>> {
        for source in &self.sources {
            let candidates = source.candidates(key);
            for path in candidates {
                if !path.exists() {
                    continue;
                }
                if let Some(map) = std::fs::read_to_string(&path)
                    .ok()
                    .and_then(|json| request.normalize_mapgen_json(&json).ok()) {
                    info!(
                        path = %path.display(),
                        seed = %format_args!("0x{:08x}", key.seed),
                        difficulty = key.difficulty,
                        area_id = key.area_id,
                        "loaded generated collision map"
                    );
                    return Some(Arc::new(map));
                }
            }
        }
        None
    }
}

impl GeneratedMapSource {
    fn candidates(&self, key: GeneratedMapKey) -> Vec<PathBuf> {
        match self {
            Self::SingleFile(path) => vec![path.clone()],
            Self::Directory(path) => generated_map_file_candidates(path, key),
        }
    }
}

fn map_key(game_state: &GameState) -> Option<GeneratedMapKey> {
    Some(GeneratedMapKey {
        seed: game_state.map().map_id?,
        difficulty: difficulty_index(game_state.difficulty()),
        area_id: game_state.map().area_id?,
    })
}

fn difficulty_index(difficulty: Difficulty) -> u8 {
    match difficulty {
        Difficulty::Normal => 0,
        Difficulty::Nightmare => 1,
        Difficulty::Hell => 2,
    }
}

fn generated_map_file_candidates(path: &Path, key: GeneratedMapKey) -> Vec<PathBuf> {
    let hex_seed = format!("0x{:08x}", key.seed);
    let dec_seed = key.seed.to_string();
    let difficulty = key.difficulty;
    let area = key.area_id;
    [
        format!("{hex_seed}-{difficulty}-{area}.json"),
        format!("{hex_seed}_{difficulty}_{area}.json"),
        format!("{dec_seed}-{difficulty}-{area}.json"),
        format!("{dec_seed}_{difficulty}_{area}.json"),
        format!("{area}.json"),
    ]
    .into_iter()
    .map(|name| path.join(name))
    .collect()
}

#[cfg(test)]
mod tests {
    use super::{generated_map_file_candidates, GeneratedMapKey};
    use std::path::Path;

    #[test]
    fn generated_map_candidates_include_seed_difficulty_and_area_names() {
        let paths = generated_map_file_candidates(
            Path::new("maps"),
            GeneratedMapKey {
                seed: 0x3607_656c,
                difficulty: 2,
                area_id: 74,
            },
        );

        assert_eq!(paths[0], Path::new("maps/0x3607656c-2-74.json"));
        assert!(paths.contains(&Path::new("maps/906454380_2_74.json").to_path_buf()));
        assert_eq!(paths.last().unwrap(), Path::new("maps/74.json"));
    }
}
