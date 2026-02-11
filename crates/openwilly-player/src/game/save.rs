//! Save/Load system — persistent game state via JSON files
//!
//! Based on mulle.js save system (MulleSave / UsersDB):
//!   - Multiple user profiles, keyed by player name
//!   - Each profile stores: car parts, junk piles, missions, items
//!   - Saved immediately on every state change
//!   - Loaded once at game start
//!
//! We use a JSON file on disk instead of localStorage.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Save data structures
// ---------------------------------------------------------------------------

/// Root save container — all user profiles
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UsersDB {
    pub users: HashMap<String, UserSave>,
}

/// A single user's saved game state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserSave {
    /// Player name (profile key)
    pub user_id: String,
    /// Car state
    pub car: CarSave,
    /// Junk pile contents: pile name → { part_id → position }
    pub junk: JunkSave,
    /// Completed mission IDs
    #[serde(default)]
    pub completed_missions: Vec<String>,
    /// Owned items / story flags
    #[serde(default)]
    pub own_stuff: Vec<String>,
    /// Active/given missions
    #[serde(default)]
    pub given_missions: Vec<String>,
    /// Last visited junk pile (1-6)
    #[serde(default = "default_pile")]
    pub my_last_pile: u8,
}

fn default_pile() -> u8 {
    1
}

/// Saved car state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CarSave {
    /// Part IDs attached to the car
    pub parts: Vec<u32>,
    /// Car name (player-given)
    #[serde(default)]
    pub name: String,
    /// Earned medals
    #[serde(default)]
    pub medals: Vec<String>,
    /// Story flags (e.g. "#GotDogOnce", "#Dog")
    #[serde(default)]
    pub cache_list: Vec<String>,
}

impl Default for CarSave {
    fn default() -> Self {
        Self {
            parts: vec![1, 82, 133, 152], // default car
            name: String::new(),
            medals: Vec::new(),
            cache_list: Vec::new(),
        }
    }
}

/// Junk pile saved positions: pile_name → { part_id → (x, y) }
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct JunkSave {
    pub pile1: HashMap<u32, (i32, i32)>,
    pub pile2: HashMap<u32, (i32, i32)>,
    pub pile3: HashMap<u32, (i32, i32)>,
    pub pile4: HashMap<u32, (i32, i32)>,
    pub pile5: HashMap<u32, (i32, i32)>,
    pub pile6: HashMap<u32, (i32, i32)>,
    pub shop_floor: HashMap<u32, (i32, i32)>,
    pub yard: HashMap<u32, (i32, i32)>,
}

impl JunkSave {
    /// Get a mutable reference to a pile by index (1-6)
    pub fn pile_mut(&mut self, index: u8) -> &mut HashMap<u32, (i32, i32)> {
        match index {
            1 => &mut self.pile1,
            2 => &mut self.pile2,
            3 => &mut self.pile3,
            4 => &mut self.pile4,
            5 => &mut self.pile5,
            6 => &mut self.pile6,
            _ => &mut self.pile1,
        }
    }

    /// Get a reference to a pile by index (1-6)
    pub fn pile(&self, index: u8) -> &HashMap<u32, (i32, i32)> {
        match index {
            1 => &self.pile1,
            2 => &self.pile2,
            3 => &self.pile3,
            4 => &self.pile4,
            5 => &self.pile5,
            6 => &self.pile6,
            _ => &self.pile1,
        }
    }

    /// Initialize default junk piles (from mulle.js initJunkPiles)
    pub fn init_defaults() -> Self {
        use crate::game::parts_db::PartsDB;

        let piles = PartsDB::initial_pile_parts();
        let default_positions: [(i32, i32); 4] = [
            (296, 234),
            (412, 311),
            (545, 222),
            (188, 333),
        ];

        let mut junk = Self::default();

        for (pile_idx, pile_parts) in piles.iter().enumerate() {
            let pile = junk.pile_mut((pile_idx + 1) as u8);
            for (i, &part_id) in pile_parts.iter().enumerate() {
                let pos = default_positions.get(i).copied().unwrap_or((300, 300));
                pile.insert(part_id, pos);
            }
        }

        // Shop floor parts
        for &part_id in &PartsDB::initial_shop_floor_parts() {
            junk.shop_floor.insert(part_id, (300, 400));
        }

        // Yard parts
        for &part_id in &PartsDB::initial_yard_parts() {
            junk.yard.insert(part_id, (400, 350));
        }

        junk
    }
}

impl UserSave {
    /// Create a new profile with defaults
    pub fn new(name: &str) -> Self {
        Self {
            user_id: name.to_string(),
            car: CarSave::default(),
            junk: JunkSave::init_defaults(),
            completed_missions: Vec::new(),
            own_stuff: Vec::new(),
            given_missions: Vec::new(),
            my_last_pile: 1,
        }
    }
}

// ---------------------------------------------------------------------------
// SaveManager — persistent I/O
// ---------------------------------------------------------------------------

/// Manages save file I/O
pub struct SaveManager {
    /// Path to the save file
    save_path: PathBuf,
    /// All user profiles in memory
    pub users_db: UsersDB,
    /// Currently active user profile name
    pub active_user: Option<String>,
}

impl SaveManager {
    /// Create a new SaveManager, loading from disk if the file exists
    pub fn new(save_dir: &Path) -> Self {
        let save_path = save_dir.join("openwilly_save.json");

        let users_db = if save_path.exists() {
            match std::fs::read_to_string(&save_path) {
                Ok(json) => match serde_json::from_str::<UsersDB>(&json) {
                    Ok(db) => {
                        tracing::info!(
                            "Loaded {} user profile(s) from {}",
                            db.users.len(),
                            save_path.display()
                        );
                        db
                    }
                    Err(e) => {
                        tracing::warn!("Failed to parse save file: {}", e);
                        UsersDB::default()
                    }
                },
                Err(e) => {
                    tracing::warn!("Failed to read save file: {}", e);
                    UsersDB::default()
                }
            }
        } else {
            tracing::info!("No save file found, starting fresh");
            UsersDB::default()
        };

        Self {
            save_path,
            users_db,
            active_user: None,
        }
    }

    /// Write all profiles to disk
    pub fn save(&self) {
        match serde_json::to_string_pretty(&self.users_db) {
            Ok(json) => {
                if let Some(parent) = self.save_path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                match std::fs::write(&self.save_path, &json) {
                    Ok(_) => tracing::debug!("Saved to {}", self.save_path.display()),
                    Err(e) => tracing::error!("Failed to save: {}", e),
                }
            }
            Err(e) => tracing::error!("Failed to serialize save data: {}", e),
        }
    }

    /// Get or create a user profile by name, and set it as active
    pub fn login(&mut self, name: &str) -> &UserSave {
        let name_str = name.to_string();

        if !self.users_db.users.contains_key(&name_str) {
            tracing::info!("Creating new profile: '{}'", name);
            self.users_db
                .users
                .insert(name_str.clone(), UserSave::new(name));
            self.save();
        } else {
            tracing::info!("Loading existing profile: '{}'", name);
        }

        self.active_user = Some(name_str.clone());
        self.users_db.users.get(&name_str).unwrap()
    }

    /// Get the active user profile (read-only)
    pub fn active(&self) -> Option<&UserSave> {
        self.active_user
            .as_ref()
            .and_then(|name| self.users_db.users.get(name))
    }

    /// Get the active user profile (mutable) — caller should call save() after
    pub fn active_mut(&mut self) -> Option<&mut UserSave> {
        if let Some(name) = &self.active_user {
            self.users_db.users.get_mut(name)
        } else {
            None
        }
    }

    /// List all saved profile names
    pub fn profile_names(&self) -> Vec<&str> {
        self.users_db.users.keys().map(|s| s.as_str()).collect()
    }

    /// Delete a profile
    pub fn delete_profile(&mut self, name: &str) {
        self.users_db.users.remove(name);
        if self.active_user.as_deref() == Some(name) {
            self.active_user = None;
        }
        self.save();
    }

    // -----------------------------------------------------------------------
    // Convenience methods — save specific state changes
    // -----------------------------------------------------------------------

    /// Save current car parts
    pub fn save_car_parts(&mut self, parts: &[u32]) {
        if let Some(user) = self.active_mut() {
            user.car.parts = parts.to_vec();
        }
        self.save();
    }

    /// Save car name
    pub fn save_car_name(&mut self, name: &str) {
        if let Some(user) = self.active_mut() {
            user.car.name = name.to_string();
        }
        self.save();
    }

    /// Save a junk pile's contents
    pub fn save_pile(&mut self, pile_index: u8, parts: &HashMap<u32, (i32, i32)>) {
        if let Some(user) = self.active_mut() {
            *user.junk.pile_mut(pile_index) = parts.clone();
        }
        self.save();
    }

    /// Save shop floor parts
    pub fn save_shop_floor(&mut self, parts: &HashMap<u32, (i32, i32)>) {
        if let Some(user) = self.active_mut() {
            user.junk.shop_floor = parts.clone();
        }
        self.save();
    }

    /// Save yard parts
    pub fn save_yard(&mut self, parts: &HashMap<u32, (i32, i32)>) {
        if let Some(user) = self.active_mut() {
            user.junk.yard = parts.clone();
        }
        self.save();
    }

    /// Mark a mission as completed
    #[allow(dead_code)] // Used by mission delivery system (upcoming)
    pub fn complete_mission(&mut self, mission_id: &str) {
        if let Some(user) = self.active_mut() {
            if !user.completed_missions.contains(&mission_id.to_string()) {
                user.completed_missions.push(mission_id.to_string());
            }
        }
        self.save();
    }

    /// Add an owned item / story flag
    pub fn add_stuff(&mut self, item: &str) {
        if let Some(user) = self.active_mut() {
            if !user.own_stuff.contains(&item.to_string()) {
                user.own_stuff.push(item.to_string());
            }
        }
        self.save();
    }

    /// Remove an owned item
    #[allow(dead_code)] // Will be used when scene_script gets RemoveStuff
    pub fn remove_stuff(&mut self, item: &str) {
        if let Some(user) = self.active_mut() {
            user.own_stuff.retain(|s| s != item);
        }
        self.save();
    }

    /// Update last visited pile
    pub fn save_last_pile(&mut self, pile: u8) {
        if let Some(user) = self.active_mut() {
            user.my_last_pile = pile;
        }
        self.save();
    }

    /// Add a part to the player's yard inventory (quest reward)
    pub fn add_yard_part(&mut self, part_id: u32) {
        if let Some(user) = self.active_mut() {
            // Place in yard with a default position
            let x = 100 + (user.junk.yard.len() as i32 % 5) * 80;
            let y = 200 + (user.junk.yard.len() as i32 / 5) * 60;
            user.junk.yard.insert(part_id, (x, y));
            tracing::info!("Added part {} to yard inventory", part_id);
        }
        self.save();
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    fn temp_save_dir() -> PathBuf {
        env::temp_dir().join("openwilly_test_save")
    }

    fn cleanup(dir: &Path) {
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn new_profile_has_defaults() {
        let user = UserSave::new("TestPlayer");
        assert_eq!(user.user_id, "TestPlayer");
        assert_eq!(user.car.parts, vec![1, 82, 133, 152]);
        assert!(user.completed_missions.is_empty());
        assert_eq!(user.my_last_pile, 1);
        // Junk piles should have initial parts
        assert!(!user.junk.pile1.is_empty());
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = temp_save_dir().join("roundtrip");
        cleanup(&dir);

        // Create and save
        {
            let mut mgr = SaveManager::new(&dir);
            mgr.login("Alice");
            mgr.save_car_parts(&[1, 82, 133, 152, 60, 61]);
            mgr.save_car_name("Rusty");
            mgr.complete_mission("mission_01");
            mgr.add_stuff("#GotDogOnce");
        }

        // Load from disk
        {
            let mgr = SaveManager::new(&dir);
            assert_eq!(mgr.profile_names().len(), 1);
            let alice = mgr.users_db.users.get("Alice").unwrap();
            assert_eq!(alice.car.parts, vec![1, 82, 133, 152, 60, 61]);
            assert_eq!(alice.car.name, "Rusty");
            assert!(alice.completed_missions.contains(&"mission_01".to_string()));
            assert!(alice.own_stuff.contains(&"#GotDogOnce".to_string()));
        }

        cleanup(&dir);
    }

    #[test]
    fn multiple_profiles() {
        let dir = temp_save_dir().join("multi");
        cleanup(&dir);

        let mut mgr = SaveManager::new(&dir);
        mgr.login("Alice");
        mgr.save_car_name("AliceCar");
        mgr.login("Bob");
        mgr.save_car_name("BobCar");

        assert_eq!(mgr.profile_names().len(), 2);
        assert_eq!(
            mgr.users_db.users.get("Alice").unwrap().car.name,
            "AliceCar"
        );
        assert_eq!(
            mgr.users_db.users.get("Bob").unwrap().car.name,
            "BobCar"
        );

        mgr.delete_profile("Alice");
        assert_eq!(mgr.profile_names().len(), 1);
        assert!(mgr.users_db.users.get("Alice").is_none());

        cleanup(&dir);
    }

    #[test]
    fn car_save_default() {
        let car = CarSave::default();
        assert_eq!(car.parts, vec![1, 82, 133, 152]);
        assert!(car.name.is_empty());
        assert!(car.medals.is_empty());
    }

    #[test]
    fn junk_pile_access() {
        let mut junk = JunkSave::default();
        junk.pile_mut(3).insert(42, (100, 200));
        assert_eq!(junk.pile(3).get(&42), Some(&(100, 200)));
        assert!(junk.pile(1).is_empty());
    }

    #[test]
    fn quest_state_roundtrip_via_save() {
        // Verify that cache_list and own_stuff survive save/load
        let dir = temp_save_dir().join("quest_rt");
        cleanup(&dir);

        {
            let mut mgr = SaveManager::new(&dir);
            mgr.login("QuestPlayer");
            // Simulate quest state write-back
            if let Some(user) = mgr.active_mut() {
                user.car.cache_list = vec!["#Dog".to_string(), "#ExtraTank".to_string()];
                user.own_stuff = vec!["#GotDogOnce".to_string()];
            }
            mgr.save();
        }

        {
            let mgr = SaveManager::new(&dir);
            let user = mgr.users_db.users.get("QuestPlayer").unwrap();
            assert_eq!(user.car.cache_list, vec!["#Dog", "#ExtraTank"]);
            assert_eq!(user.own_stuff, vec!["#GotDogOnce"]);
        }

        cleanup(&dir);
    }
}
