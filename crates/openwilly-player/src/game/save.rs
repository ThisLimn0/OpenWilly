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

    /// Remove a part from ALL locations (piles, shop_floor, yard).
    /// Call this before inserting into a new location to prevent duplication.
    pub fn remove_part_everywhere(&mut self, part_id: u32) {
        self.pile1.remove(&part_id);
        self.pile2.remove(&part_id);
        self.pile3.remove(&part_id);
        self.pile4.remove(&part_id);
        self.pile5.remove(&part_id);
        self.pile6.remove(&part_id);
        self.shop_floor.remove(&part_id);
        self.yard.remove(&part_id);
    }

    /// Initialize default junk piles (from mulle.js savedata.js setDefaults)
    ///
    /// Each pile has specific part IDs with individual x,y positions.
    pub fn init_defaults() -> Self {
        let mut junk = Self::default();

        // Pile 1 — from mulle.js savedata.js
        junk.pile1.insert(66,  (296, 234));
        junk.pile1.insert(29,  (412, 311));
        junk.pile1.insert(143, (416, 186));
        junk.pile1.insert(178, (570, 255));

        // Pile 2
        junk.pile2.insert(215, (545, 222));
        junk.pile2.insert(47,  (386, 304));
        junk.pile2.insert(12,  (239, 269));
        junk.pile2.insert(140, (352, 187));

        // Pile 3 (5 parts)
        junk.pile3.insert(153, (512, 153));
        junk.pile3.insert(131, (464, 298));
        junk.pile3.insert(307, (246, 285));
        junk.pile3.insert(112, (561, 293));
        junk.pile3.insert(30,  (339, 189));

        // Pile 4
        junk.pile4.insert(190, (182, 143));
        junk.pile4.insert(23,  (346, 203));
        junk.pile4.insert(126, (178, 301));
        junk.pile4.insert(211, (75, 193));

        // Pile 5 (5 parts)
        junk.pile5.insert(6,   (192, 377));
        junk.pile5.insert(90,  (102, 290));
        junk.pile5.insert(203, (33, 122));
        junk.pile5.insert(158, (186, 164));
        junk.pile5.insert(119, (375, 268));

        // Pile 6
        junk.pile6.insert(2,   (160, 351));
        junk.pile6.insert(214, (130, 172));
        junk.pile6.insert(210, (281, 300));
        junk.pile6.insert(121, (85, 275));

        // Shop floor — from mulle.js junkpile.js initialJunk
        junk.shop_floor.insert(200, (160, 351));

        // Yard starts empty (parts are earned via gameplay / SetWhenDone)

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
                    if let Err(e) = std::fs::create_dir_all(parent) {
                        tracing::warn!("Failed to create save directory {}: {}", parent.display(), e);
                    }
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

    /// Give a mission (add to given_missions if not already given or completed)
    pub fn give_mission(&mut self, mission_id: u32) {
        let mid = mission_id.to_string();
        if let Some(user) = self.active_mut() {
            if !user.given_missions.contains(&mid) && !user.completed_missions.contains(&mid) {
                user.given_missions.push(mid.clone());
                tracing::info!("Mission {} added to given_missions", mid);
            }
        }
        self.save();
    }

    /// Check if there are pending (given but not completed) missions
    pub fn has_pending_missions(&self) -> bool {
        self.active().map(|u| !u.given_missions.is_empty()).unwrap_or(false)
    }

    /// Get a pending mission ID and remove it from given_missions
    pub fn pop_pending_mission(&mut self) -> Option<u32> {
        let mid = self.active_mut().and_then(|u| {
            if u.given_missions.is_empty() { None }
            else { Some(u.given_missions.remove(0)) }
        });
        if let Some(ref m) = mid {
            // Move to completed
            if let Some(user) = self.active_mut() {
                if !user.completed_missions.contains(m) {
                    user.completed_missions.push(m.clone());
                }
            }
            self.save();
        }
        mid.and_then(|s| s.parse().ok())
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

    /// Check if the player has a specific stuff flag
    pub fn has_stuff(&self, item: &str) -> bool {
        self.active().map_or(false, |u| u.own_stuff.iter().any(|s| s == item))
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

    /// Check if a part is already in the yard inventory
    pub fn has_yard_part(&self, part_id: u32) -> bool {
        self.active().map_or(false, |u| u.junk.yard.contains_key(&part_id))
    }

    /// Get a random part that isn't already owned (in piles, yard, car, or shop).
    /// Used by SetWhenDone #Random rewards (mulle.js savedata.js getRandomPart).
    pub fn random_unowned_part(&self) -> Option<u32> {
        use crate::game::parts_db::PartsDB;
        use rand::seq::SliceRandom;

        let user = self.active()?;

        // Collect all owned part IDs
        let mut owned = std::collections::HashSet::new();
        for pile_idx in 1..=6u8 {
            for &id in user.junk.pile(pile_idx).keys() {
                owned.insert(id);
            }
        }
        for &id in user.junk.yard.keys() {
            owned.insert(id);
        }
        for &id in user.junk.shop_floor.keys() {
            owned.insert(id);
        }
        for &id in &user.car.parts {
            owned.insert(id);
        }

        // Collect all unowned random parts, then pick one at random
        let available: Vec<u32> = PartsDB::random_part_ids().iter()
            .filter(|&&id| !owned.contains(&id))
            .copied()
            .collect();
        let mut rng = rand::thread_rng();
        available.choose(&mut rng).copied()
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
