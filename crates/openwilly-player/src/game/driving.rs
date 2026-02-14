//! Driving / World Map system
//!
//! Based on mulle.js MulleDriveCar and world map:
//!   - 5×6 grid of map tiles, each 640×396 pixels visible area
//!   - Topology bitmap (316×198) controls terrain: walls, mud, hills, holes
//!   - 16 compass directions, 5 tilt levels
//!   - Real-time car physics: acceleration, braking, steering, fuel consumption
//!   - Radius-based destination triggering
//!   - Session state for saving position when entering destinations

use std::collections::HashMap;
use rand::Rng;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Drive loop runs at 30 FPS (same as game loop)
pub const DRIVE_FPS: u32 = 30;
/// Number of discrete compass directions
pub const NUM_DIRECTIONS: usize = 16;
/// Visible map tile size
#[allow(dead_code)]
pub const MAP_WIDTH: i32 = 640;
#[allow(dead_code)]
pub const MAP_HEIGHT: i32 = 396;
/// Topology bitmap resolution (half of visible, with offset)
pub const TOPO_WIDTH: i32 = 316;
pub const TOPO_HEIGHT: i32 = 198;
/// Offset for topology coordinate conversion
pub const MAP_OFFSET_X: i32 = 4;
pub const MAP_OFFSET_Y: i32 = 2;
/// Terrain thresholds (topology pixel red channel)
pub const TERRAIN_WALL: u8 = 240;
pub const TERRAIN_MUD: u8 = 32;
pub const TERRAIN_HOLES: u8 = 16;
/// Property thresholds for terrain obstacles
pub const MUD_GRIP_THRESHOLD: i32 = 8;
pub const HOLES_DURABILITY_THRESHOLD: i32 = 3;
pub const BIG_HILL_STRENGTH_THRESHOLD: i32 = 3;
pub const SMALL_HILL_STRENGTH_THRESHOLD: i32 = 2;
/// Fuel starts at 80% of max
pub const FUEL_START_FRACTION: f32 = 0.8;
/// Map edge detection margin
#[allow(dead_code)]
pub const MAP_EDGE_MARGIN: i32 = 3;
/// Wheel offset factor (from direction vector)
pub const WHEEL_OFFSET_FACTOR: f32 = 8.0;
/// Horn sounds by horntype (1-5), from mulle.js drivecar.js
pub const HORN_SOUNDS: [&str; 5] = ["05e050v0", "05e049v0", "05e044v0", "05e042v0", "05d013v0"];
/// Cow mooing sound (played after horn)
pub const COW_MOO_SOUND: &str = "31e001v0";
/// "No horn" speech sound (Mulle says "we need a horn")
pub const NO_HORN_SOUND: &str = "31d001v0";
/// Mud blocking speech sound
#[allow(dead_code)]
pub const MUD_SOUND: &str = "05d003v0";
/// Rocks/holes blocking speech sound
#[allow(dead_code)]
pub const HOLES_SOUND: &str = "05d002v0";

/// Ferry drop-off positions (mulle.js Ferry.js), indexed by ferry_step (0 or 1)
pub const FERRY_DROP_POSITIONS: [(f32, f32); 2] = [(254.0, 177.0), (369.0, 224.0)];
/// Ferry drop-off directions, indexed by ferry_step (0 or 1)
pub const FERRY_DROP_DIRECTIONS: [u8; 2] = [14, 7];
/// Ferry boarding sound
pub const FERRY_SOUND: &str = "05e020v0";

/// Racing start sound (from objects.hash.json Sounds[0])
pub const RACING_START_SOUND: &str = "31e007v0";
/// Racing finish sound (from objects.hash.json Sounds[1])
pub const RACING_FINISH_SOUND: &str = "31e008v0";
/// Racing scoreboard sprite (from CDDATA.CXT)
#[allow(dead_code)]
pub const RACING_BOARD_SPRITE: &str = "31b045v0";

/// WBridge creaking sound (from objects.hash.json Sounds[0])
pub const WBRIDGE_CREAK_SOUND: &str = "31e003v0";
/// WBridge "too weak" speech (from objects.hash.json Sounds[1])
pub const WBRIDGE_WARN_SOUND: &str = "31d010v0";
/// CBridge sound (from objects.hash.json Sounds[0])
pub const CBRIDGE_SOUND: &str = "31e004v0";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A world map tile
#[derive(Debug, Clone)]
pub struct MapTile {
    /// Map tile ID (1-30)
    pub id: u32,
    /// Background image member name (e.g. "30b001v0")
    pub map_image: String,
    /// Topology bitmap member name (e.g. "30t001v0")
    pub topology: String,
    /// Objects on this tile
    pub objects: Vec<MapObject>,
}

/// An object on the world map
#[derive(Debug, Clone)]
pub struct MapObject {
    /// Object ID (from objects.hash.json)
    pub object_id: u32,
    /// Position on the tile
    pub x: i32,
    pub y: i32,
    /// Object type
    pub obj_type: MapObjectType,
    /// Inner detection radius (collision trigger)
    pub inner_radius: f32,
    /// Outer detection radius (approach trigger)
    pub outer_radius: f32,
    /// Director resource to switch to (for destinations)
    pub dir_resource: Option<String>,
    /// Whether this object is enabled (false = hidden by CheckFor/IfFound)
    pub enabled: bool,
    /// CheckFor.Cache — strings to look for in CacheList (from objects.hash.json)
    pub check_for_cache: Vec<String>,
    /// CheckFor.Medals — medal IDs to check (from objects.hash.json)
    pub check_for_medals: Vec<u32>,
    /// IfFound action when CheckFor matches ("#NoDisplay", "#NoEnter", "#None")
    pub if_found: Option<String>,
    /// SetWhenDone — cache entries and part rewards on destination arrival
    pub set_when_done: Option<SetWhenDone>,
    /// Sound ID for Sound trigger zones (from maps.hash.json per-tile opt)
    pub sound_id: Option<String>,
    /// Approach sound (Sounds[0] from objects.hash.json) — played on outer radius enter
    pub approach_sound: Option<String>,
    /// Sprite member name for the "normal" frame (from objects.hash.json FrameList)
    /// "Dummy" entries are skipped (= None).
    pub sprite_name: Option<String>,
    /// If true, render under (behind) the car; if false, render over (in front).
    pub z_under: bool,
}

/// Data applied when a destination is reached (SetWhenDone from objects.hash.json)
#[derive(Debug, Clone)]
pub struct SetWhenDone {
    /// Cache flags to add to CacheList
    pub cache: Vec<String>,
    /// Part IDs to give (u32 for specific, 0 = #Random)
    pub parts: Vec<u32>,
    /// Mission IDs to unlock (from objects.hash.json SetWhenDone.Missions)
    pub missions: Vec<u32>,
}

/// Type of map object
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MapObjectType {
    /// Fixed destination (scene transition)
    Destination,
    /// Random destination (position varies per game)
    RandomDestination,
    /// Custom behavior (gas station, ferry, cows, etc.)
    #[allow(dead_code)]
    Custom,
    /// Position correction (pushes car onto road)
    Correct,
    /// Stop zone (forces car to stop)
    Stop,
    /// Gas station — refuel interaction
    Gas,
    /// Hill / ramp — needs strength check
    Hill(HillType),
    /// Cows on the road
    Cows,
    /// Goats
    Goats,
    /// Ferry (boat transition)
    Ferry,
    /// Racing event — enter_dir is the required approach direction (1-16)
    Racing { enter_dir: u8 },
    /// Wooden bridge
    WBridge,
    /// Concrete bridge
    CBridge,
    /// Far-away marker
    FarAway,
    /// Picture overlay
    Picture,
    /// Sound trigger zone
    Sound,
}

/// Racing mini-game state (mulle.js Racing.js)
#[derive(Debug, Clone)]
pub struct RacingState {
    /// Whether a race is currently in progress
    pub is_racing: bool,
    /// Frame at which the race started
    pub race_start_frame: u64,
    /// Number of times passed through from exit side (need 1 to finish)
    pub nr_of_times_passed: i8,
    /// Was the car inside the racing object's inner radius last frame?
    pub in_zone: bool,
    /// Entry direction classification: 1 = correct side, -1 = wrong side
    pub entered_from: i8,
    /// Global frame counter (incremented each update)
    pub frame_count: u64,
    /// Position of the racing object (for exit direction calculation)
    pub obj_x: i32,
    pub obj_y: i32,
    /// Required entry direction (from maps.hash.json EnterDir)
    pub enter_dir: u8,
}

impl RacingState {
    pub fn new(enter_dir: u8) -> Self {
        Self {
            is_racing: false,
            race_start_frame: 0,
            nr_of_times_passed: 0,
            in_zone: false,
            entered_from: 0,
            frame_count: 0,
            obj_x: 0,
            obj_y: 0,
            enter_dir,
        }
    }
}

/// Calculate direction from point A to point B (1-16 compass, mulle.js calcDirection)
pub fn calc_direction(from_x: f32, from_y: f32, to_x: f32, to_y: f32) -> u8 {
    let diff_x = to_x - from_x;
    let diff_y = from_y - to_y; // note: Y inverted
    let mut diff_y_safe = diff_y;
    if diff_y_safe == 0.0 { diff_y_safe = 0.1; }
    let mut dir = (diff_x / diff_y_safe).atan();
    if diff_x > 0.0 {
        if diff_y_safe <= 0.0 {
            dir += std::f32::consts::PI;
        }
    } else if diff_y_safe > 0.0 {
        dir += 2.0 * std::f32::consts::PI;
    } else {
        dir += std::f32::consts::PI;
    }
    dir /= std::f32::consts::PI;
    let mut result = (dir * 16.0 / 2.0).round() as u8;
    if result == 0 { result = 16; }
    result
}

/// Calculate the angular difference between two compass directions (0-8)
pub fn direction_diff(a: u8, b: u8) -> u8 {
    let diff = (a as i16 - b as i16).unsigned_abs() as u8;
    if diff > 8 { 16 - diff } else { diff }
}

/// Hill size classification
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HillType {
    SmallHill,
    BigHill,
}

impl MapObject {
    /// Check if this object should be disabled based on the car's CacheList and medals.
    /// Implements mulle.js mapobject.js doCheck():
    ///   if CheckFor.Cache contains a value in CacheList AND IfFound=="#NoDisplay"
    ///   → disable the object (invisible + no collision)
    ///   if CheckFor.Medals contains a medal the player has → same.
    pub fn do_check(&mut self, cache_list: &[String], medals: &[String]) {
        // Check cache flags
        for cache_val in &self.check_for_cache {
            if cache_list.iter().any(|c| c == cache_val) {
                if self.if_found.as_deref() == Some("#NoDisplay") {
                    self.enabled = false;
                    tracing::debug!("MapObject {} disabled by CheckFor {:?} (cache has {:?})",
                        self.object_id, self.check_for_cache, cache_val);
                    return;
                }
            }
        }
        // Check medals
        for &medal_id in &self.check_for_medals {
            let medal_str = medal_id.to_string();
            if medals.iter().any(|m| m == &medal_str) {
                if self.if_found.as_deref() == Some("#NoDisplay") {
                    self.enabled = false;
                    tracing::debug!("MapObject {} disabled by CheckFor.Medals (has medal {})",
                        self.object_id, medal_id);
                    return;
                }
            }
        }
    }
}

/// The world grid (5×6 tiles)
#[derive(Debug, Clone)]
pub struct WorldMap {
    /// 5 rows × 6 columns of map tile IDs
    pub grid: Vec<Vec<u32>>,
    /// All map tile definitions
    pub tiles: HashMap<u32, MapTile>,
    /// Starting tile position (grid col, row)
    pub start_tile: (usize, usize),
    /// Starting pixel position within tile
    pub start_pos: (f32, f32),
    /// Starting direction (1-16)
    pub start_direction: u8,
}

impl WorldMap {
    /// Get the tile ID at a grid position
    pub fn tile_at(&self, col: usize, row: usize) -> Option<u32> {
        self.grid.get(row).and_then(|r| r.get(col)).copied()
    }

    /// Get current tile data
    pub fn get_tile(&self, tile_id: u32) -> Option<&MapTile> {
        self.tiles.get(&tile_id)
    }

    /// Randomize random-destination objects (mulle.js calcRandomDestinations).
    ///
    /// For each rdest object_id (8=TreeInRoad, 9=Dog, 10=MudCar), collect all
    /// tile positions, pick ONE random tile per type, and disable the rest.
    /// This ensures only one instance of each random event appears per session.
    pub fn apply_random_destinations(&mut self) {
        // Collect: object_id → list of (tile_id, object_index)
        let mut rdest_map: HashMap<u32, Vec<(u32, usize)>> = HashMap::new();
        for (&tile_id, tile) in &self.tiles {
            for (idx, obj) in tile.objects.iter().enumerate() {
                if obj.obj_type == MapObjectType::RandomDestination {
                    rdest_map.entry(obj.object_id).or_default().push((tile_id, idx));
                }
            }
        }

        let mut rng = rand::thread_rng();
        for (obj_id, positions) in &rdest_map {
            if positions.len() <= 1 { continue; }
            // Pick one random position to keep
            let keep_idx = rng.gen_range(0..positions.len());
            tracing::info!("Random dest obj {}: keeping tile {} (out of {} candidates)",
                obj_id, positions[keep_idx].0, positions.len());
            for (i, &(tile_id, obj_idx)) in positions.iter().enumerate() {
                if i != keep_idx {
                    if let Some(tile) = self.tiles.get_mut(&tile_id) {
                        tile.objects[obj_idx].enabled = false;
                    }
                }
            }
        }
    }

    /// Build the complete world map from mulle.js game data.
    ///
    /// 5×6 grid, 28 unique tiles (IDs 1–24, 26, 27, 28, 30).
    /// Tile 26 and 28 appear twice in the grid.
    /// Start at grid (4,3) = tile 16, position (300,250), direction 16.
    pub fn default_map() -> Self {
        let mut tiles = HashMap::new();

        // Helper to build a MapObject quickly
        let obj = |id: u32, x: i32, y: i32, t: MapObjectType, ir: f32, or: f32, dr: Option<&str>| {
            MapObject { object_id: id, x, y, obj_type: t, inner_radius: ir, outer_radius: or,
                        dir_resource: dr.map(|s| s.to_string()),
                        enabled: true, check_for_cache: Vec::new(), check_for_medals: Vec::new(),
                        if_found: None, set_when_done: None, sound_id: None,
                        approach_sound: None,
                        sprite_name: None, z_under: true }
        };
        let dest = |id: u32, x: i32, y: i32, ir: f32, or: f32, dr: &str| {
            obj(id, x, y, MapObjectType::Destination, ir, or, Some(dr))
        };
        let rdest = |id: u32, x: i32, y: i32, ir: f32, or: f32, dr: &str| {
            obj(id, x, y, MapObjectType::RandomDestination, ir, or, Some(dr))
        };
        let correct = |x: i32, y: i32, ir: f32| {
            obj(31, x, y, MapObjectType::Correct, ir, 1.0, None)
        };
        let stop = |x: i32, y: i32| {
            obj(32, x, y, MapObjectType::Stop, 25.0, 1.0, None)
        };
        let gas = |x: i32, y: i32| {
            obj(6, x, y, MapObjectType::Gas, 15.0, 25.0, None)
        };
        let hill_s = |x: i32, y: i32| {
            obj(30, x, y, MapObjectType::Hill(HillType::SmallHill), 25.0, 30.0, None)
        };
        let hill_b = |x: i32, y: i32| {
            obj(30, x, y, MapObjectType::Hill(HillType::BigHill), 25.0, 30.0, None)
        };
        let cows = |x: i32, y: i32| {
            obj(1, x, y, MapObjectType::Cows, 55.0, 85.0, None)
        };
        let goats = |x: i32, y: i32| {
            obj(25, x, y, MapObjectType::Goats, 55.0, 85.0, None)
        };

        let tile = |id: u32, objs: Vec<MapObject>| {
            MapTile {
                id,
                map_image: format!("30b{:03}v0", id),
                topology: format!("30t{:03}v0", id),
                objects: objs,
            }
        };

        // Tile 1
        tiles.insert(1, tile(1, vec![
            correct(146, 392, 50.0),
            dest(19, 390, 205, 25.0, 45.0, "84"),
            gas(120, 350),
        ]));
        // Tile 2
        tiles.insert(2, tile(2, vec![
            hill_s(540, 210),
            correct(500, 392, 50.0),
            rdest(10, 120, 175, 15.0, 25.0, "82"),
            rdest(9, 400, 270, 25.0, 45.0, "85"),
        ]));
        // Tile 3
        tiles.insert(3, tile(3, vec![
            hill_s(465, 178),
            hill_s(262, 358),
            hill_s(500, 255),
            rdest(8, 270, 125, 25.0, 45.0, "83"),
            gas(80, 212),
        ]));
        // Tile 4
        tiles.insert(4, tile(4, vec![
            hill_s(540, 200),
            stop(260, 40),
            gas(250, 167),
            rdest(9, 130, 210, 25.0, 45.0, "85"),
        ]));
        // Tile 5
        tiles.insert(5, tile(5, vec![
            hill_b(110, 110),
            goats(350, 50),
            dest(11, 543, 170, 25.0, 45.0, "91"),
            dest(20, 130, 330, 25.0, 45.0, "84"),
        ]));
        // Tile 6
        tiles.insert(6, tile(6, vec![
            gas(110, 65),
        ]));
        // Tile 7
        tiles.insert(7, tile(7, vec![
            stop(220, 233),
            correct(154, 12, 50.0),
            cows(370, 340),
        ]));
        // Tile 8
        tiles.insert(8, tile(8, vec![
            correct(507, 12, 50.0),
            obj(26, 469, 107, MapObjectType::WBridge, 15.0, 25.0, None),
            dest(12, 435, 203, 25.0, 45.0, "89"),
            obj(26, 362, 363, MapObjectType::WBridge, 15.0, 25.0, None),
        ]));
        // Tile 9
        tiles.insert(9, tile(9, vec![
            gas(170, 170),
        ]));
        // Tile 10
        tiles.insert(10, tile(10, vec![
            hill_s(290, 370),
            hill_s(470, 280),
            dest(2, 220, 95, 15.0, 25.0, "86"),
            rdest(9, 560, 120, 25.0, 45.0, "85"),
        ]));
        // Tile 11
        tiles.insert(11, tile(11, vec![
            hill_b(225, 200),
            gas(344, 150),
        ]));
        // Tile 12
        tiles.insert(12, tile(12, vec![
            stop(580, 344),
            correct(358, 388, 25.0),
            rdest(8, 320, 287, 25.0, 45.0, "83"),
        ]));
        // Tile 13
        tiles.insert(13, tile(13, vec![
            stop(260, 30),
            stop(100, 150),
            rdest(8, 275, 310, 25.0, 45.0, "83"),
        ]));
        // Tile 14
        tiles.insert(14, tile(14, vec![
            gas(310, 105),
            rdest(10, 416, 175, 15.0, 25.0, "82"),
            dest(21, 170, 300, 25.0, 45.0, "84"),
            rdest(9, 520, 75, 25.0, 45.0, "85"),
        ]));
        // Tile 15
        tiles.insert(15, tile(15, vec![
            dest(4, 380, 110, 15.0, 25.0, "94"),
            cows(438, 320),
        ]));
        // Tile 16 — START TILE
        tiles.insert(16, tile(16, vec![
            hill_s(300, 20),
            dest(14, 430, 60, 25.0, 45.0, "92"),
            dest(15, 310, 270, 25.0, 45.0, "04"),
            rdest(9, 540, 250, 25.0, 45.0, "85"),
        ]));
        // Tile 17
        tiles.insert(17, tile(17, vec![
            hill_s(250, 55),
            hill_s(125, 200),
            correct(180, 388, 50.0),
            rdest(10, 290, 260, 15.0, 25.0, "82"),
            gas(425, 203),
        ]));
        // Tile 18
        tiles.insert(18, tile(18, vec![
            correct(242, 390, 25.0),
            correct(302, 12, 25.0),
            goats(330, 70),
            dest(16, 440, 140, 25.0, 45.0, "93"),
            rdest(9, 170, 335, 25.0, 45.0, "85"),
        ]));
        // Tile 19
        tiles.insert(19, tile(19, vec![
            obj(28, 350, 140, MapObjectType::FarAway, 25.0, 45.0, None),
            dest(17, 145, 210, 25.0, 45.0, "90"),
        ]));
        // Tile 20
        tiles.insert(20, tile(20, vec![
            rdest(9, 400, 300, 25.0, 45.0, "85"),
        ]));
        // Tile 21
        tiles.insert(21, tile(21, vec![
            correct(85, 390, 25.0),
            gas(440, 40),
            dest(5, 480, 135, 25.0, 45.0, "88"),
        ]));
        // Tile 22
        tiles.insert(22, tile(22, vec![
            correct(372, 388, 205.0),
            obj(3, 285, 190, MapObjectType::Ferry, 35.0, 45.0, None),
            dest(22, 460, 320, 25.0, 45.0, "84"),
        ]));
        // Tile 23
        tiles.insert(23, tile(23, vec![
            hill_s(330, 163),
            hill_s(595, 235),
            correct(159, 12, 50.0),
            cows(410, 205),
        ]));
        // Tile 24
        tiles.insert(24, tile(24, vec![
            correct(250, 12, 25.0),
            correct(204, 392, 25.0),
            gas(220, 165),
        ]));
        // Tile 26 (appears 2× in grid)
        tiles.insert(26, tile(26, vec![
            dest(23, 310, 90, 25.0, 45.0, "84"),
        ]));
        // Tile 27
        tiles.insert(27, tile(27, vec![
            correct(71, 12, 55.0),
            gas(63, 50),
            obj(27, 360, 200, MapObjectType::CBridge, 20.0, 45.0, None),
            dest(18, 565, 308, 25.0, 45.0, "87"),
        ]));
        // Tile 28 (appears 2× in grid)
        {
            let mut snd_obj = obj(33, 468, 20, MapObjectType::Sound, 20.0, 0.0, None);
            snd_obj.sound_id = Some("31d007v0".to_string());
            tiles.insert(28, tile(28, vec![
                snd_obj,
                correct(468, 15, 205.0),
                obj(7, 415, 60, MapObjectType::Racing { enter_dir: 12 }, 25.0, 45.0, None),
                gas(360, 266),
                obj(29, 131, 266, MapObjectType::Picture, 0.0, 0.0, None),
            ]));
        }
        // Tile 30
        tiles.insert(30, tile(30, vec![
            correct(223, 12, 25.0),
            stop(110, 350),
            dest(24, 240, 160, 25.0, 45.0, "84"),
        ]));

        // 5×6 grid — note tile 26 and 28 appear twice
        let grid = vec![
            vec![ 1,  2,  3,  4,  5,  6],
            vec![ 7,  8,  9, 10, 11, 12],
            vec![13, 14, 15, 16, 17, 18],
            vec![19, 20, 21, 22, 23, 24],
            vec![26, 26, 27, 28, 28, 30],
        ];

        // --- Apply CheckFor / IfFound / SetWhenDone from objects.hash.json ---
        // Only CheckFor.Cache + IfFound="#NoDisplay" is actually implemented in mulle.js.
        // SetWhenDone is processed by the RoadThing scene (dest 82-85).
        for tile in tiles.values_mut() {
            for obj in &mut tile.objects {
                match obj.object_id {
                    // Object 8: TreeInRoad (#rdest)
                    8 => {
                        obj.check_for_cache = vec!["#TreeInRoad".into()];
                        obj.if_found = Some("#NoDisplay".into());
                        obj.set_when_done = Some(SetWhenDone {
                            cache: vec!["#TreeInRoad".into()],
                            parts: vec![0], // #Random
                            missions: Vec::new(),
                        });
                    }
                    // Object 9: Dog (#rdest)
                    9 => {
                        obj.check_for_cache = vec!["#GotDogOnce".into()];
                        obj.if_found = Some("#NoDisplay".into());
                        obj.set_when_done = Some(SetWhenDone {
                            cache: vec!["#Dog".into(), "#GotDogOnce".into()],
                            parts: Vec::new(),
                            missions: Vec::new(),
                        });
                    }
                    // Object 10: MudCar (#rdest)
                    10 => {
                        obj.check_for_cache = vec!["#RescuedMudCar".into()];
                        obj.if_found = Some("#NoDisplay".into());
                        obj.set_when_done = Some(SetWhenDone {
                            cache: vec!["#MudCar".into(), "#RescuedMudCar".into()],
                            parts: vec![0], // #Random
                            missions: Vec::new(),
                        });
                    }
                    // Object 19-24: RoadThing 1-6
                    id @ 19..=24 => {
                        let n = id - 18; // 1-6
                        let tag = format!("#RoadThing{}", n);
                        obj.check_for_cache = vec![tag.clone()];
                        obj.if_found = Some("#NoDisplay".into());
                        obj.set_when_done = Some(SetWhenDone {
                            cache: vec![tag],
                            parts: vec![0], // #Random (+ specific varies by object)
                            missions: Vec::new(),
                        });
                    }
                    // Object 28: FarAway
                    28 => {
                        obj.check_for_cache = vec![];
                        obj.check_for_medals = vec![2];
                        obj.if_found = Some("#NoDisplay".into());
                    }
                    // Object 2: Solhem (Mia) — dest 86
                    2 => {
                        obj.set_when_done = Some(SetWhenDone {
                            cache: Vec::new(),
                            parts: Vec::new(),
                            missions: vec![5], // #Telephone mission
                        });
                    }
                    // Object 4: Exhibition / CarShow — dest 94
                    4 => {
                        obj.check_for_cache = vec!["#Exhibition".into()];
                        // IfFound = #NoEnter (not #NoDisplay — object stays visible)
                        obj.if_found = Some("#NoEnter".into());
                        obj.set_when_done = Some(SetWhenDone {
                            cache: vec!["#Exhibition".into()],
                            parts: Vec::new(),
                            missions: vec![2], // #Mail mission
                        });
                    }
                    // Object 5: Lemonade destination — dest 88
                    5 => {
                        obj.check_for_cache = vec!["#Lemonade".into()];
                        obj.if_found = Some("#None".into());
                        obj.set_when_done = Some(SetWhenDone {
                            cache: vec![],
                            parts: vec![162],
                            missions: vec![3], // #Mail mission
                        });
                    }
                    // Object 7: Racing — Racing event
                    7 => {
                        obj.set_when_done = Some(SetWhenDone {
                            cache: Vec::new(),
                            parts: Vec::new(),
                            missions: vec![8], // #Mail mission
                        });
                    }
                    // Object 11: Luddelabb — dest 91
                    11 => {
                        obj.set_when_done = Some(SetWhenDone {
                            cache: Vec::new(),
                            parts: vec![99],
                            missions: vec![6], // #Telephone mission
                        });
                    }
                    // Object 12: Viola — dest 89
                    12 => {
                        obj.set_when_done = Some(SetWhenDone {
                            cache: Vec::new(),
                            parts: vec![172],
                            missions: vec![7], // #Mail mission
                        });
                    }
                    // Object 14: Figge — dest 92
                    14 => {
                        obj.check_for_cache = vec!["#Dog".into()];
                        obj.if_found = Some("#None".into());
                        obj.set_when_done = Some(SetWhenDone {
                            cache: vec!["#ExtraTank".into()],
                            parts: Vec::new(),
                            missions: vec![1], // #Telephone mission
                        });
                    }
                    // Object 17: DorisDigital — dest 90
                    17 => {
                        obj.set_when_done = Some(SetWhenDone {
                            cache: Vec::new(),
                            parts: vec![306],
                            missions: vec![4], // #Telephone mission
                        });
                    }
                    // Object 18: Lemonade factory
                    18 => {
                        obj.set_when_done = Some(SetWhenDone {
                            cache: vec!["#Lemonade".into()],
                            parts: Vec::new(),
                            missions: Vec::new(),
                        });
                    }
                    _ => {}
                }

                // --- Populate sprite_name from objects.hash.json FrameList ---
                // Only set if the normal frame is not "Dummy" and not empty.
                let sprite = match obj.object_id {
                    // Cows (obj 1) — direction-less, use first variant
                    1 => Some("31b003v0"),
                    // Ferry (obj 3)
                    3 => Some("31b015v0"),
                    // Gas (obj 6) — "Bensinmack"
                    6 => Some("Bensinmack"),
                    // Racing board (obj 7)
                    7 => Some("31b045v0"),
                    // TreeInRoad (obj 8)
                    8 => Some("31b011v0"),
                    // Dog (obj 9)
                    9 => Some("31b014v0"),
                    // MudCar (obj 10) — variant 1
                    10 => Some("31b009v0"),
                    // RoadThing 19-24
                    19..=24 => Some("31b044v0"),
                    // Goats (obj 25)
                    25 => Some("31b033v0"),
                    // WBridge (obj 26) — variant 1
                    26 => Some("31b020v0"),
                    // CBridge (obj 27)
                    27 => Some("31b030v0"),
                    // Picture (obj 29)
                    29 => Some("31b043v0"),
                    // Stop signs (obj 32) — variant 1
                    32 => Some("31b016v0"),
                    _ => None,
                };
                if let Some(s) = sprite {
                    obj.sprite_name = Some(s.to_string());
                }
                // z_under: most objects render behind car. Picture (29) renders over.
                obj.z_under = obj.object_id != 29;

                // --- Populate approach_sound from objects.hash.json Sounds[0] ---
                let asound = match obj.object_id {
                    1 => Some("31d001v0"),   // Cows
                    2 => Some("31d017v0"),   // Solhem
                    3 => Some("31d008v0"),   // Ferry
                    6 => Some("31e006v0"),   // Gas
                    7 => Some("31e007v0"),   // Racing
                    8 => Some("31d005v0"),   // TreeInRoad
                    9 => Some("31d006v0"),   // Dog
                    10 => Some("31d004v0"),  // MudCar
                    11 => Some("31d013v0"),  // Luddelabb
                    12 => Some("31d012v0"),  // Viola
                    14 => Some("31d014v0"),  // Figge
                    16 => Some("31d016v0"),  // Stansen
                    17 => Some("31d015v0"),  // DorisDigital
                    25 => Some("31d001v0"),  // Goats (reuse cow sound)
                    _ => None,
                };
                if let Some(s) = asound {
                    obj.approach_sound = Some(s.to_string());
                }
            }
        }

        // Start tile: grid position (4,3) = 1-based → 0-based (3,2)
        Self {
            grid,
            tiles,
            start_tile: (3, 2), // col=3, row=2 (0-based)
            start_pos: (300.0, 250.0),
            start_direction: 16,
        }
    }
}

/// Session state — saved when entering a destination, restored when returning
#[derive(Debug, Clone, Default)]
pub struct DriveSession {
    /// Current tile grid position
    pub tile_col: usize,
    pub tile_row: usize,
    /// Position within current tile
    pub x: f32,
    pub y: f32,
    /// Current direction (1-16)
    pub direction: u8,
    /// Current fuel level
    pub fuel: f32,
    /// Whether a session is active (has been saved)
    pub active: bool,
}

/// Quick properties — pre-computed from CarProperties for efficient per-frame use
#[derive(Debug, Clone, Default)]
pub struct DriveProperties {
    pub acceleration: f32,
    pub brake_force: f32,
    pub max_speed: f32,
    pub reverse_max: f32,
    pub steering_rate: f32,
    pub fuel_consumption: f32,
    pub fuel_max: f32,
    pub grip: i32,
    pub durability: i32,
    pub strength: i32,
    pub engine_type: i32,
    pub horn_type: i32,
}

impl DriveProperties {
    /// Compute from car properties (mulle.js QuickProperty formulas)
    pub fn from_car_properties(props: &crate::game::parts_db::CarProperties) -> Self {
        let acceleration = props.acceleration as f32 * 2.0 / 100.0;
        let brake_force = props.brake as f32 * 3.0 / 100.0;
        let speed = props.speed as f32;
        let max_speed = if speed == 5.0 {
            speed * 27.0 / 25.0
        } else {
            speed * 20.0 / 25.0
        };
        let reverse_max = max_speed / 4.0;
        // mulle.js: steering + 3 * 2 / 20 * 70 → JS precedence: steering + ((3*2/20)*70) = steering + 21
        let steering_rate = props.steering as f32 + 3.0 * 2.0 / 20.0 * 70.0;
        let fuel_consumption = props.fuel_consumption as f32;
        let fuel_max = props.fuel_volume as f32 * 12.0;

        Self {
            acceleration,
            brake_force,
            max_speed,
            reverse_max,
            steering_rate,
            fuel_consumption,
            fuel_max,
            grip: props.grip,
            durability: props.durability,
            strength: props.strength,
            engine_type: props.engine_type,
            horn_type: props.horn_type,
        }
    }
}

// ---------------------------------------------------------------------------
// Direction vectors (16 compass directions)
// ---------------------------------------------------------------------------

/// Pre-computed direction vectors for 16 directions.
/// mulle.js uses 1-based indexing: dir 1 → angle = 2π×1/16 = 22.5° (NNE).
/// Direction 16 → 360° wraps to North.
pub fn direction_vector(dir: u8) -> (f32, f32) {
    // mulle.js: angle = 2π × i / 16, where i = dir (1-based, NOT 0-based)
    let angle = std::f32::consts::PI * 2.0 * (dir as f32 / NUM_DIRECTIONS as f32);
    (angle.sin(), -angle.cos())
}

/// Wrap direction into valid 1-16 range (mulle.js correctDirection)
fn correct_direction(d: u8) -> u8 {
    let d = ((d as i16 - 1).rem_euclid(NUM_DIRECTIONS as i16) + 1) as u8;
    d
}

// ---------------------------------------------------------------------------
// DriveCar — the car driving on the world map
// ---------------------------------------------------------------------------

/// Driving state for the car on the world map
pub struct DriveCar {
    /// Position within current tile (pixel coords, 640×396 space)
    pub x: f32,
    pub y: f32,
    /// Current speed (positive = forward, negative = reverse)
    pub speed: f32,
    /// Internal direction (high resolution, 100 units per compass step)
    pub internal_direction: f32,
    /// Current compass direction (1-16, rounded from internal)
    pub direction: u8,
    /// Current tilt (-2 to +2)
    pub tilt: i8,
    /// Current fuel level
    pub fuel: f32,
    /// Drive properties (from car stats)
    pub props: DriveProperties,
    /// Current grid position
    pub tile_col: usize,
    pub tile_row: usize,
    /// Input state
    pub throttle: bool,
    pub braking: bool,
    pub steer_left: bool,
    pub steer_right: bool,
    /// Forward/backward state: 1 = forward, -1 = reverse, 0 = neutral
    forward_backward: i8,
    /// Reverse stop timer (10 frames)
    reverse_stop_timer: u8,
    /// Whether we're currently stopped
    pub stopped: bool,
    /// Whether fuel is empty
    pub fuel_empty: bool,
    /// Input mode: true = keyboard, false = mouse
    pub key_steer: bool,
    /// Engine sound state: last played sound index (0-6), or None if not yet started
    pub engine_sound_state: Option<u8>,
    /// Ignition flag: false = startup sound on first update, true = normal operation
    ignition_done: bool,
    /// Refueling timer (counts down from 10, 0 = not refueling)
    pub refuel_ticks: u8,
    /// Frame counter for refueling (330ms per step @ 30fps ≈ 10 frames)
    refuel_frame_counter: u8,
    /// Position history ring buffer for stepback (10 entries)
    position_history: Vec<(f32, f32, u8)>,
    /// Out-of-bounds counter for wall recovery (mulle.js OutOfBounds)
    out_of_bounds: u32,
    /// Ferry step: toggles 0↔1 each crossing (mulle.js ferryStep)
    pub ferry_step: u8,
    /// Racing mini-game state (persists across frames within a tile)
    pub racing: Option<RacingState>,
    /// Pending approach sounds to play (filled during update, drained by caller)
    pub pending_approach_sounds: Vec<String>,
    /// Set of object IDs whose outer radius we've already entered (for one-shot approach sounds)
    entered_outer_set: std::collections::HashSet<u32>,
}

/// Result of a drive frame update
#[derive(Debug, Clone)]
pub enum DriveEvent {
    /// Nothing special
    None,
    /// Car reached a destination
    ReachedDestination {
        object_id: u32,
        dir_resource: String,
    },
    /// Car entered tile edge — need to transition
    TileTransition {
        delta_col: i32,
        delta_row: i32,
    },
    /// Fuel ran out
    FuelEmpty,
    /// Hit terrain obstacle
    TerrainBlocked {
        reason: &'static str,
    },
    /// Hit a gas station — game should start refueling
    GasStation,
    /// Animals block the road (cows/goats)
    AnimalsBlocking {
        /// true if car has a horn to honk them away
        has_horn: bool,
        /// horn type (1-5) for sound selection, 0 = no horn
        horn_type: i32,
    },
    /// Hill sound feedback (actual blocking is in terrain)
    HillSound {
        big: bool,
    },
    /// Ferry boarding — car teleports to other shore
    FerryBoard,
    /// Race started — play start sound
    RaceStarted,
    /// Race finished — play finish sound + award medal
    RaceFinished { time_secs: f32 },
    /// Bridge sound effect
    BridgeSound { wooden: bool },
    /// FarAway landmark reached (medal trigger)
    FarAwayReached { object_id: u32 },
    /// Sound trigger zone entered
    SoundTrigger { sound_id: String },
    /// Approach sound — played when entering outer radius of a map object
    ApproachSound { sound_id: String },
}

/// Dev cheat flags passed from the dev menu to the drive update loop
#[derive(Debug, Clone, Copy, Default)]
pub struct DriveCheat {
    pub infinite_fuel: bool,
    pub noclip: bool,
    pub meme_mode: bool,
}

impl DriveCar {
    /// Create a new drive car at given position
    pub fn new(x: f32, y: f32, direction: u8, props: DriveProperties) -> Self {
        let fuel = props.fuel_max * FUEL_START_FRACTION;
        Self {
            x,
            y,
            speed: 0.0,
            internal_direction: direction as f32 * 100.0,
            direction,
            tilt: 0,
            fuel,
            props,
            tile_col: 0,
            tile_row: 0,
            throttle: false,
            braking: false,
            steer_left: false,
            steer_right: false,
            reverse_stop_timer: 0,
            stopped: false,
            fuel_empty: false,
            key_steer: true,
            engine_sound_state: None,
            ignition_done: false,
            refuel_ticks: 0,
            refuel_frame_counter: 0,
            position_history: Vec::with_capacity(10),
            out_of_bounds: 0,
            forward_backward: 0,
            ferry_step: 0,
            racing: None,
            pending_approach_sounds: Vec::new(),
            entered_outer_set: std::collections::HashSet::new(),
        }
    }

    /// Restore from a saved session
    pub fn restore_session(&mut self, session: &DriveSession) {
        self.x = session.x;
        self.y = session.y;
        self.direction = session.direction;
        self.internal_direction = session.direction as f32 * 100.0;
        self.fuel = session.fuel;
        self.tile_col = session.tile_col;
        self.tile_row = session.tile_row;
        self.speed = 0.0;
    }

    /// Save current state to a session
    pub fn save_session(&self) -> DriveSession {
        DriveSession {
            tile_col: self.tile_col,
            tile_row: self.tile_row,
            x: self.x,
            y: self.y,
            direction: self.direction,
            fuel: self.fuel,
            active: true,
        }
    }

    /// Update one frame of driving physics
    ///
    /// `topology` is a function that returns the terrain value (red channel)
    /// at a given topology coordinate. Pass None if topology not available.
    pub fn update<F>(&mut self, objects: &[MapObject], get_terrain: F, cheats: DriveCheat) -> DriveEvent
    where
        F: Fn(i32, i32) -> u8,
    {
        if self.fuel_empty && !cheats.infinite_fuel {
            return DriveEvent::FuelEmpty;
        }

        // --- Refueling ---
        if self.refuel_ticks > 0 {
            self.refuel_frame_counter += 1;
            // 330ms per step at 30fps ≈ 10 frames
            if self.refuel_frame_counter >= 10 {
                self.refuel_frame_counter = 0;
                self.fuel = (self.fuel + self.props.fuel_max / 10.0).min(self.props.fuel_max);
                self.refuel_ticks -= 1;
                tracing::trace!("Refueling: {} ticks left, fuel={:.1}", self.refuel_ticks, self.fuel);
            }
            self.speed = 0.0;
            return DriveEvent::None; // Car stopped during refueling
        }

        // --- Save position history (for stepback) ---
        self.position_history.push((self.x, self.y, self.direction));
        if self.position_history.len() > 10 {
            self.position_history.remove(0);
        }

        // --- Steering ---
        if self.steer_left {
            self.internal_direction -= self.props.steering_rate;
        }
        if self.steer_right {
            self.internal_direction += self.props.steering_rate;
        }

        // Wrap direction to [0, 1600)
        while self.internal_direction < 0.0 {
            self.internal_direction += (NUM_DIRECTIONS as f32) * 100.0;
        }
        while self.internal_direction >= (NUM_DIRECTIONS as f32) * 100.0 {
            self.internal_direction -= (NUM_DIRECTIONS as f32) * 100.0;
        }

        // Round to compass direction
        self.direction = ((self.internal_direction / 100.0).round() as u8) % NUM_DIRECTIONS as u8;
        if self.direction == 0 {
            self.direction = NUM_DIRECTIONS as u8;
        }

        // --- Acceleration / Braking ---
        let max_speed = if cheats.meme_mode { self.props.max_speed * 30.0 } else { self.props.max_speed };
        let accel = if cheats.meme_mode { self.props.acceleration * 5.0 } else { self.props.acceleration };
        if self.throttle {
            self.forward_backward = 1;
            self.speed += accel;
            if self.speed > max_speed {
                self.speed = max_speed;
            }
        } else if self.braking {
            if self.speed > 0.0 {
                self.speed -= self.props.brake_force;
                if self.speed < 0.0 {
                    // Direction change: forward → reverse requires stop
                    self.speed = 0.0;
                    self.reverse_stop_timer = 10;
                }
            } else if self.reverse_stop_timer > 0 {
                self.reverse_stop_timer -= 1;
            } else {
                self.forward_backward = -1;
                self.speed -= self.props.acceleration;
                if self.speed < -self.props.reverse_max {
                    self.speed = -self.props.reverse_max;
                }
            }
        } else {
            // mulle.js has NO friction — car coasts at constant speed.
            // Player must actively brake to slow down.
        }

        self.stopped = self.speed.abs() < 0.001;

        // --- Movement ---
        // --- mulle.js OutOfBounds recovery ---
        // If stuck in a wall for >5 frames, try 4 directions to escape
        if self.out_of_bounds > 5 {
            for r in 0..4u8 {
                let mut try_x = self.x;
                let mut try_y = self.y;
                match r {
                    0 => try_y -= self.out_of_bounds as f32, // UP
                    1 => try_x += self.out_of_bounds as f32, // RIGHT
                    2 => try_y += self.out_of_bounds as f32, // DOWN
                    3 => try_x -= self.out_of_bounds as f32, // LEFT
                    _ => {}
                }
                let tx = ((try_x as i32 - MAP_OFFSET_X) / 2).clamp(0, TOPO_WIDTH - 1);
                let ty = ((try_y as i32 - MAP_OFFSET_Y) / 2).clamp(0, TOPO_HEIGHT - 1);
                if get_terrain(tx, ty) < TERRAIN_WALL {
                    self.x = try_x;
                    self.y = try_y;
                    self.out_of_bounds = 0;
                    return DriveEvent::None;
                }
            }
            self.out_of_bounds += 1;
            return DriveEvent::None;
        }

        // --- mulle.js side-check / wall slide ---
        // Check left (dir-1) and right (dir+1) sides of the car.
        // If one side hits a wall, turn away and reduce speed by 0.9×.
        if !cheats.noclip && !cheats.meme_mode {
        for d in 0..=1u8 {
            let dir_offset: i8 = if d == 0 { -1 } else { 1 };
            let check_dir = correct_direction((self.direction as i8 + dir_offset) as u8);
            let (sdx, sdy) = direction_vector(check_dir);
            let amt = if self.forward_backward == 1 {
                7.0 + self.speed
            } else {
                -3.0
            };
            let try_x = self.x + sdx * amt;
            let try_y = self.y + sdy * amt;
            let tx = ((try_x as i32 - MAP_OFFSET_X) / 2).clamp(0, TOPO_WIDTH - 1);
            let ty = ((try_y as i32 - MAP_OFFSET_Y) / 2).clamp(0, TOPO_HEIGHT - 1);
            if get_terrain(tx, ty) >= TERRAIN_WALL {
                // Turn away from wall: subtract dir_offset → turns opposite
                let new_dir = correct_direction((self.direction as i8 - dir_offset) as u8);
                self.direction = new_dir;
                self.internal_direction = new_dir as f32 * 100.0;
                self.speed *= 0.9;
                if self.speed.abs() < 0.1 {
                    self.speed = 0.0;
                }
                break;
            }
        }
        } // noclip

        let (dx, dy) = direction_vector(self.direction);
        let new_x = self.x + dx * self.speed;
        let new_y = self.y + dy * self.speed;

        // --- Terrain check (forward position) ---
        let topo_x = ((new_x as i32 - MAP_OFFSET_X) / 2).clamp(0, TOPO_WIDTH - 1);
        let topo_y = ((new_y as i32 - MAP_OFFSET_Y) / 2).clamp(0, TOPO_HEIGHT - 1);
        let terrain = get_terrain(topo_x, topo_y);

        if terrain >= TERRAIN_WALL && !cheats.noclip {
            // Frontal wall hit — stop and increment OutOfBounds counter
            self.speed = 0.0;
            self.out_of_bounds += 1;
            return DriveEvent::TerrainBlocked { reason: "wall" };
        }

        // Reset out_of_bounds on successful move
        self.out_of_bounds = 0;

        let altitude = (terrain % 16) as i32;
        if !cheats.noclip {
            if altitude > 2 && self.props.strength <= BIG_HILL_STRENGTH_THRESHOLD {
                self.speed = 0.0;
                return DriveEvent::TerrainBlocked { reason: "big_hill" };
            }
            if altitude > 1 && self.props.strength <= SMALL_HILL_STRENGTH_THRESHOLD {
                self.speed = 0.0;
                return DriveEvent::TerrainBlocked { reason: "small_hill" };
            }
            if terrain == TERRAIN_MUD && self.props.grip <= MUD_GRIP_THRESHOLD {
                self.speed = 0.0;
                return DriveEvent::TerrainBlocked { reason: "mud" };
            }
            if terrain == TERRAIN_HOLES && self.props.durability <= HOLES_DURABILITY_THRESHOLD {
                self.speed = 0.0;
                return DriveEvent::TerrainBlocked { reason: "holes" };
            }
        }

        // Update tilt from altitude
        self.tilt = (altitude as i8).clamp(-2, 2);

        // Apply movement
        self.x = new_x;
        self.y = new_y;

        // --- Fuel consumption ---
        if self.speed.abs() > 0.001 && !cheats.infinite_fuel {
            self.fuel -= self.speed.abs() * self.props.fuel_consumption / 100.0;
            if self.fuel <= 0.0 {
                self.fuel = 0.0;
                self.fuel_empty = true;
                self.speed = 0.0;
                return DriveEvent::FuelEmpty;
            }
        }

        // --- Map edge transition ---
        // mulle.js checks in topology-space (316×198): coord = (pixel - offset) / 2
        // Thresholds: <3 and >314 (x), <3 and >196 (y)
        // Converted back to pixel-space: x<10, x>632, y<8, y>394
        if self.x < 10.0 {
            return DriveEvent::TileTransition { delta_col: -1, delta_row: 0 };
        }
        if self.x > 632.0 {
            return DriveEvent::TileTransition { delta_col: 1, delta_row: 0 };
        }
        if self.y < 8.0 {
            return DriveEvent::TileTransition { delta_col: 0, delta_row: -1 };
        }
        if self.y > 394.0 {
            return DriveEvent::TileTransition { delta_col: 0, delta_row: 1 };
        }

        // --- Object collision detection ---
        let mut racing_zone_hit = false;
        let mut pending_event: Option<DriveEvent> = None;
        for obj in objects {
            // Skip disabled objects (hidden by CheckFor/IfFound="#NoDisplay")
            if !obj.enabled { continue; }

            let dist = ((self.x - obj.x as f32).powi(2) + (self.y - obj.y as f32).powi(2)).sqrt();

            if dist <= obj.inner_radius / 2.0 {
                match &obj.obj_type {
                    MapObjectType::Destination | MapObjectType::RandomDestination => {
                        if let Some(ref res) = obj.dir_resource {
                            self.speed = 0.0;
                            return DriveEvent::ReachedDestination {
                                object_id: obj.object_id,
                                dir_resource: res.clone(),
                            };
                        }
                    }
                    MapObjectType::Stop => {
                        self.speed = 0.0;
                    }
                    MapObjectType::Correct => {
                        // Position correction: snap car to object position (on tile transitions)
                        self.x = obj.x as f32;
                        self.y = obj.y as f32;
                        tracing::trace!("Position snap to ({}, {})", obj.x, obj.y);
                    }
                    MapObjectType::Gas => {
                        if self.refuel_ticks == 0 && self.fuel < self.props.fuel_max {
                            self.refuel_ticks = 10;
                            self.refuel_frame_counter = 0;
                            self.speed = 0.0;
                            return DriveEvent::GasStation;
                        }
                    }
                    MapObjectType::Hill(hill_type) => {
                        // Hill sound feedback — actual blocking is in terrain
                        let (needed, big) = match hill_type {
                            HillType::BigHill => (BIG_HILL_STRENGTH_THRESHOLD, true),
                            HillType::SmallHill => (SMALL_HILL_STRENGTH_THRESHOLD, false),
                        };
                        if self.props.strength < needed {
                            return DriveEvent::HillSound { big };
                        }
                    }
                    MapObjectType::Cows | MapObjectType::Goats => {
                        let has_horn = self.props.horn_type > 0;
                        if !has_horn {
                            // No horn: stepback 2 positions
                            self.speed = 0.0;
                            self.stepback(2);
                        }
                        return DriveEvent::AnimalsBlocking { has_horn, horn_type: self.props.horn_type };
                    }
                    MapObjectType::Ferry => {
                        self.speed = 0.0;
                        return DriveEvent::FerryBoard;
                    }
                    MapObjectType::Racing { enter_dir } => {
                        racing_zone_hit = true;
                        if let Some(racing) = &mut self.racing {
                            if !racing.in_zone {
                                // Just entered the inner zone — handle enter event
                                racing.in_zone = true;
                                racing.obj_x = obj.x;
                                racing.obj_y = obj.y;
                                let car_dir = calc_direction(self.x, self.y, obj.x as f32, obj.y as f32);
                                let diff = direction_diff(*enter_dir, car_dir);
                                if diff <= 3 {
                                    // Entered from correct direction
                                    racing.entered_from = 1;
                                    if racing.is_racing {
                                        if racing.nr_of_times_passed >= 1 {
                                            // Finish race!
                                            let elapsed = racing.frame_count - racing.race_start_frame;
                                            let time_secs = elapsed as f32 / DRIVE_FPS as f32;
                                            racing.is_racing = false;
                                            tracing::info!("Race finished in {:.2}s", time_secs);
                                            pending_event = Some(DriveEvent::RaceFinished { time_secs });
                                        }
                                    } else {
                                        // Start race!
                                        racing.is_racing = true;
                                        racing.nr_of_times_passed = 0;
                                        racing.race_start_frame = racing.frame_count;
                                        tracing::info!("Race started!");
                                        pending_event = Some(DriveEvent::RaceStarted);
                                    }
                                } else {
                                    racing.entered_from = -1;
                                }
                            }
                            // else: still in zone, do nothing
                        }
                    }
                    MapObjectType::WBridge => {
                        return DriveEvent::BridgeSound { wooden: true };
                    }
                    MapObjectType::CBridge => {
                        return DriveEvent::BridgeSound { wooden: false };
                    }
                    MapObjectType::FarAway => {
                        return DriveEvent::FarAwayReached { object_id: obj.object_id };
                    }
                    MapObjectType::Sound => {
                        if let Some(ref sid) = obj.sound_id {
                            return DriveEvent::SoundTrigger { sound_id: sid.clone() };
                        }
                    }
                    MapObjectType::Custom | MapObjectType::Picture => {
                        tracing::trace!("Object {} at ({}, {})", obj.object_id, obj.x, obj.y);
                    }
                }
            } else if dist <= obj.outer_radius {
                // Entering outer radius — play approach sound if not yet played
                if !self.entered_outer_set.contains(&obj.object_id) {
                    self.entered_outer_set.insert(obj.object_id);
                    if let Some(ref asnd) = obj.approach_sound {
                        self.pending_approach_sounds.push(asnd.clone());
                    }
                }
            } else {
                // Outside outer radius — reset tracking
                self.entered_outer_set.remove(&obj.object_id);
            }
        }

        // --- Racing zone exit detection ---
        if let Some(racing) = &mut self.racing {
            racing.frame_count += 1;
            if racing.in_zone && !racing_zone_hit {
                // Car has left the racing inner zone
                racing.in_zone = false;
                if racing.is_racing {
                    // Calculate exit direction to determine pass count
                    let exit_dir = calc_direction(self.x, self.y, racing.obj_x as f32, racing.obj_y as f32);
                    let diff = direction_diff(racing.enter_dir, exit_dir);
                    if diff <= 3 {
                        // Exiting from same side as entry
                        if racing.entered_from == -1 {
                            racing.nr_of_times_passed -= 1;
                            tracing::debug!("Racing: passed minus → {}", racing.nr_of_times_passed);
                        }
                    } else {
                        // Exiting from opposite side
                        if racing.entered_from == 1 {
                            racing.nr_of_times_passed += 1;
                            tracing::debug!("Racing: passed plus → {}", racing.nr_of_times_passed);
                        }
                    }
                }
            }
        }

        pending_event.unwrap_or(DriveEvent::None)
    }

    /// Initialize racing state from tile objects (call when entering a new tile).
    /// Preserves existing race-in-progress if the enter_dir matches.
    pub fn init_racing_for_tile(&mut self, objects: &[MapObject]) {
        let racing_obj = objects.iter().find(|o| matches!(o.obj_type, MapObjectType::Racing { .. }));
        match racing_obj {
            Some(obj) => {
                if let MapObjectType::Racing { enter_dir } = obj.obj_type {
                    let preserve = self.racing.as_ref().map_or(false, |r| r.enter_dir == enter_dir);
                    if !preserve {
                        self.racing = Some(RacingState::new(enter_dir));
                        tracing::debug!("Racing state initialized: enter_dir={}", enter_dir);
                    }
                }
            }
            None => {
                // No racing on this tile — clear state (but keep race timer running
                // so cross-tile races can work if the same tile appears twice in the grid)
                if let Some(racing) = &mut self.racing {
                    racing.in_zone = false;
                }
            }
        }
    }

    /// Handle tile transition (wrap coordinates)
    pub fn do_tile_transition(&mut self, delta_col: i32, delta_row: i32) {
        let new_col = self.tile_col as i32 + delta_col;
        let new_row = self.tile_row as i32 + delta_row;

        if new_col < 0 || new_col >= 6 || new_row < 0 || new_row >= 5 {
            // Edge of world — bounce
            self.speed = 0.0;
            return;
        }

        self.tile_col = new_col as usize;
        self.tile_row = new_row as usize;

        // Wrap position (mulle.js values)
        if delta_col < 0 {
            self.x = 631.0;  // mulle.js: 640 - 8 - 1
        } else if delta_col > 0 {
            self.x = 12.0;   // mulle.js: 4 + 8
        }
        if delta_row < 0 {
            self.y = 395.0;  // mulle.js: 396 - 1
        } else if delta_row > 0 {
            self.y = 12.0;   // mulle.js: 4 + 8
        }
    }

    /// Refuel (gas station interaction)
    pub fn refuel(&mut self) {
        self.fuel = self.props.fuel_max;
        self.fuel_empty = false;
    }

    /// Step back N positions in the history (used when blocked by animals)
    pub fn stepback(&mut self, n: usize) {
        if let Some(&(px, py, pd)) = self.position_history.iter().rev().nth(n) {
            self.x = px;
            self.y = py;
            self.direction = pd;
            self.internal_direction = pd as f32 * 100.0;
        }
    }

    /// Ferry teleport: move car to drop-off position and toggle ferry_step.
    /// mulle.js Ferry.js: drop positions [(254,177), (369,224)], dirs [14, 7].
    pub fn ferry_teleport(&mut self) {
        let (dx, dy) = FERRY_DROP_POSITIONS[self.ferry_step as usize];
        let dir = FERRY_DROP_DIRECTIONS[self.ferry_step as usize];
        self.x = dx;
        self.y = dy;
        self.direction = dir;
        self.internal_direction = dir as f32 * 100.0;
        self.speed = 0.0;
        self.ferry_step = 1 - self.ferry_step; // toggle 0↔1
        tracing::info!("Ferry teleport to ({}, {}), dir={}, next_step={}", dx, dy, dir, self.ferry_step);
    }

    /// Get fuel as percentage (0.0 - 1.0)
    pub fn fuel_percent(&self) -> f32 {
        if self.props.fuel_max > 0.0 {
            (self.fuel / self.props.fuel_max).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }

    /// Get the car's maximum speed
    pub fn max_speed(&self) -> f32 {
        self.props.max_speed
    }

    /// Get the sprite member for current direction + tilt.
    /// mulle.js layout: 5 tilt blocks × 16 directions each:
    ///   tilt -2: members 78–93,  tilt -1: 94–109,  tilt 0: 110–125,
    ///   tilt +1: 126–141,  tilt +2: 142–157
    /// Formula: 78 + (tilt+2)*16 + (direction-1)
    pub fn sprite_member(&self) -> u32 {
        let dir_idx = (self.direction as u32 - 1) % NUM_DIRECTIONS as u32;
        let tilt_idx = (self.tilt + 2).clamp(0, 4) as u32;
        78 + tilt_idx * 16 + dir_idx
    }

    /// Get the wheel visual offset based on direction (for wheel sprite rendering)
    pub fn wheel_offset(&self) -> (f32, f32) {
        let (dx, dy) = direction_vector(self.direction);
        (dx * WHEEL_OFFSET_FACTOR, dy * WHEEL_OFFSET_FACTOR)
    }

    /// Get the engine type (for engine sound selection)
    pub fn engine_type(&self) -> i32 {
        self.props.engine_type
    }

    /// Compute the engine sound that should play this frame.
    /// Returns `Some(audio_id)` if a new sound should start (state changed),
    /// or `None` if the current sound continues unchanged.
    pub fn engine_sound_update(&mut self) -> Option<&'static str> {
        // 9 engine types × 7 states: [startup, shutdown, idle, speed1, speed2, speed3, speed4]
        const ENGINE_SOUNDS: [[&str; 7]; 9] = [
            ["05e073v0", "05e079v0", "05e074v0", "05e075v0", "05e076v0", "05e077v0", "05e078v0"], // type 1
            ["05e067v0", "05e073v0", "05e068v0", "05e069v0", "05e070v0", "05e071v0", "05e072v0"], // type 2
            ["05e025v0", "05e031v0", "05e026v0", "05e027v0", "05e028v0", "05e029v0", "05e030v0"], // type 3
            ["05e004v0", "05e010v0", "05e005v0", "05e006v0", "05e007v0", "05e008v0", "05e009v0"], // type 4
            ["05e011v0", "05e017v0", "05e012v0", "05e013v0", "05e014v0", "05e015v0", "05e016v0"], // type 5
            ["05e053v0", "05e059v0", "05e054v0", "05e055v0", "05e056v0", "05e057v0", "05e058v0"], // type 6
            ["05e018v0", "05e024v0", "05e019v0", "05e020v0", "05e021v0", "05e022v0", "05e023v0"], // type 7
            ["05e060v0", "05e066v0", "05e061v0", "05e062v0", "05e063v0", "05e064v0", "05e065v0"], // type 8
            ["05e032v0", "05e038v0", "05e033v0", "05e034v0", "05e035v0", "05e036v0", "05e037v0"], // type 9
        ];

        let et = self.props.engine_type;
        if et < 1 || et > 9 { return None; }
        let sounds = &ENGINE_SOUNDS[(et - 1) as usize];

        // Determine current state index
        let state: u8 = if !self.ignition_done {
            self.ignition_done = true;
            0 // startup
        } else if self.props.max_speed <= 0.0 {
            2 // idle (safety)
        } else {
            let perc = 100.0 * self.speed.abs() / self.props.max_speed;
            if perc >= 70.0 { 6 }
            else if perc >= 40.0 { 5 }
            else if perc >= 20.0 { 4 }
            else if perc >= 10.0 { 3 }
            else { 2 } // idle
        };

        // Only switch if state changed
        if self.engine_sound_state == Some(state) {
            return None;
        }
        self.engine_sound_state = Some(state);
        Some(sounds[state as usize])
    }

    /// Apply mouse-based steering.
    ///
    /// When the mouse button is down, compute the angle from the car to the
    /// mouse position, derive steering direction and forward/reverse intent.
    ///
    /// Direction system: dir 1 = North = (0,-1), angles go clockwise.
    /// In atan2 space (screen coords, y-down):
    ///   game_angle_atan2 = (dir - 1) * 22.5 - 90
    ///   relative_angle = atan2(mouse_to_car) - game_angle_atan2
    /// Deadzone: ±22.5° (one direction sector).
    pub fn mouse_steer(&mut self, mouse_x: i32, mouse_y: i32, mouse_down: bool) {
        if !mouse_down {
            // No mouse input → coast (no throttle, no steering from mouse)
            self.throttle = false;
            self.braking = false;
            self.steer_left = false;
            self.steer_right = false;
            return;
        }

        // Angle from car to mouse in screen coordinates (atan2, degrees)
        let dx = mouse_x as f32 - self.x;
        let dy = mouse_y as f32 - self.y;
        let car_to_mouse = dy.atan2(dx).to_degrees();

        // Car's forward direction in atan2 space
        // dir=1 → game_angle=0° (North) → atan2=-90°
        // dir=5 → game_angle=90° (East) → atan2=0°
        let car_dir_atan2 = (self.direction as f32 - 1.0) * 22.5 - 90.0;

        // Relative angle: 0° = straight ahead
        let mut ang = car_to_mouse - car_dir_atan2;

        // Wrap to [-180, 180]
        while ang > 180.0 { ang -= 360.0; }
        while ang < -180.0 { ang += 360.0; }

        // Steering: ±22.5° deadzone (one direction sector)
        self.steer_left = ang < -22.5;
        self.steer_right = ang > 22.5;

        // Forward/reverse: mouse behind car → reverse
        if ang < -90.0 || ang > 90.0 {
            self.braking = true;
            self.throttle = false;
        } else {
            self.throttle = true;
            self.braking = false;
        }
    }

    /// Frames per second for the driving simulation
    pub fn fps() -> u32 {
        DRIVE_FPS
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn world_map_has_all_tiles() {
        let wm = WorldMap::default_map();
        assert_eq!(wm.grid.len(), 5, "5 rows");
        assert!(wm.grid.iter().all(|r| r.len() == 6), "6 cols per row");
        // All referenced tile IDs must resolve
        for row in &wm.grid {
            for &tid in row {
                assert!(wm.get_tile(tid).is_some(), "tile {} missing", tid);
            }
        }
        // 28 unique tiles
        assert_eq!(wm.tiles.len(), 28);
        // Start tile exists
        let start_id = wm.tile_at(wm.start_tile.0, wm.start_tile.1).unwrap();
        assert_eq!(start_id, 16, "start should be tile 16");
    }

    #[test]
    fn world_map_start_tile_home() {
        let wm = WorldMap::default_map();
        let t = wm.get_tile(16).unwrap();
        // Home/Yard destination should be on the start tile
        let home = t.objects.iter().find(|o| o.dir_resource.as_deref() == Some("04"));
        assert!(home.is_some(), "start tile should contain home destination");
    }

    fn test_props() -> DriveProperties {
        DriveProperties {
            acceleration: 0.06,
            brake_force: 0.15,
            max_speed: 4.32,
            reverse_max: 1.08,
            steering_rate: 42.0,
            fuel_consumption: 3.0,
            fuel_max: 120.0,
            grip: 10,
            durability: 5,
            strength: 4,
            engine_type: 4,
            horn_type: 1,
        }
    }

    #[test]
    fn direction_vectors_are_unit() {
        for d in 1..=16u8 {
            let (x, y) = direction_vector(d);
            let mag = (x * x + y * y).sqrt();
            assert!((mag - 1.0).abs() < 0.001, "Direction {} has magnitude {}", d, mag);
        }
    }

    #[test]
    fn north_is_up() {
        // mulle.js: Direction 1 = 22.5° (NNE), Direction 16 = 360° = North
        // Direction 16 should be closest to pure north = (0, -1)
        let (x, y) = direction_vector(16);
        assert!(x.abs() < 0.001, "North x should be ~0, got {}", x);
        assert!((y + 1.0).abs() < 0.001, "North y should be ~-1, got {}", y);
    }

    #[test]
    fn east_is_right() {
        // mulle.js: Direction 4 = 90° = East
        let (x, y) = direction_vector(4);
        assert!((x - 1.0).abs() < 0.001, "East x should be ~1, got {}", x);
        assert!(y.abs() < 0.001, "East y should be ~0, got {}", y);
    }

    #[test]
    fn south_is_down() {
        // mulle.js: Direction 8 = 180° = South
        let (x, y) = direction_vector(8);
        assert!(x.abs() < 0.001, "South x should be ~0, got {}", x);
        assert!((y - 1.0).abs() < 0.001, "South y should be ~1, got {}", y);
    }

    #[test]
    fn west_is_left() {
        // mulle.js: Direction 12 = 270° = West
        let (x, y) = direction_vector(12);
        assert!((x + 1.0).abs() < 0.001, "West x should be ~-1, got {}", x);
        assert!(y.abs() < 0.001, "West y should be ~0, got {}", y);
    }

    #[test]
    fn acceleration_increases_speed() {
        let mut car = DriveCar::new(320.0, 200.0, 1, test_props());
        car.throttle = true;
        car.update(&[], |_, _| 0, DriveCheat::default());
        assert!(car.speed > 0.0);
    }

    #[test]
    fn wall_stops_car() {
        let mut car = DriveCar::new(320.0, 200.0, 1, test_props());
        car.speed = 2.0;
        let event = car.update(&[], |_, _| 250, DriveCheat::default()); // everything is wall
        matches!(event, DriveEvent::TerrainBlocked { reason: "wall" });
    }

    #[test]
    fn fuel_consumption_reduces_fuel() {
        let mut car = DriveCar::new(320.0, 200.0, 1, test_props());
        let initial_fuel = car.fuel;
        car.speed = 2.0;
        car.update(&[], |_, _| 0, DriveCheat::default());
        assert!(car.fuel < initial_fuel, "Fuel should decrease while moving");
    }

    #[test]
    fn tile_transition_wraps() {
        let mut car = DriveCar::new(5.0, 200.0, 1, test_props());
        car.tile_col = 2;
        car.tile_row = 3;
        car.do_tile_transition(-1, 0);
        assert_eq!(car.tile_col, 1);
        assert!(car.x > (MAP_WIDTH / 2) as f32); // wrapped to right side
    }

    #[test]
    fn sprite_member_calculation() {
        let car = DriveCar::new(320.0, 200.0, 1, test_props());
        // Direction 1, tilt 0 → 78 + (0+2)*16 + (1-1) = 78 + 32 = 110
        assert_eq!(car.sprite_member(), 110);
    }

    #[test]
    fn session_save_restore() {
        let mut car = DriveCar::new(100.0, 150.0, 5, test_props());
        car.tile_col = 3;
        car.tile_row = 2;
        car.fuel = 50.0;

        let session = car.save_session();
        assert!(session.active);

        let mut car2 = DriveCar::new(0.0, 0.0, 1, test_props());
        car2.restore_session(&session);
        assert_eq!(car2.tile_col, 3);
        assert_eq!(car2.tile_row, 2);
        assert!((car2.x - 100.0).abs() < 0.01);
        assert!((car2.fuel - 50.0).abs() < 0.01);
        assert_eq!(car2.direction, 5);
    }

    #[test]
    fn fuel_empty_stops_car() {
        let mut car = DriveCar::new(320.0, 200.0, 1, test_props());
        car.fuel = 0.01;
        car.speed = 2.0;
        let event = car.update(&[], |_, _| 0, DriveCheat::default());
        assert!(car.fuel_empty || matches!(event, DriveEvent::FuelEmpty));
    }

    #[test]
    fn engine_sound_startup_then_idle() {
        let mut car = DriveCar::new(320.0, 200.0, 1, test_props());
        // test_props has engine_type=4 → row index 3
        // First call: startup sound (05e004v0)
        let sound = car.engine_sound_update();
        assert_eq!(sound, Some("05e004v0"));
        assert!(car.ignition_done);
        // Second call at speed 0: idle (05e005v0)
        let sound = car.engine_sound_update();
        assert_eq!(sound, Some("05e005v0"));
        // Third call still idle: no change → None
        let sound = car.engine_sound_update();
        assert_eq!(sound, None);
    }

    #[test]
    fn engine_sound_speed_levels() {
        let mut car = DriveCar::new(320.0, 200.0, 1, test_props());
        car.ignition_done = true; // skip startup
        car.engine_sound_state = Some(2); // idle
        // test_props has max_speed=4.32, engine_type=4
        // speed1 threshold: 10% of 4.32 = 0.432
        car.speed = 0.5; // >10% → speed1 (state 3) → 05e006v0
        let sound = car.engine_sound_update();
        assert_eq!(sound, Some("05e006v0"));
        // ≥70%: 0.7 * 4.32 = 3.024
        car.speed = 3.1; // >70% → speed4 (state 6) → 05e009v0
        let sound = car.engine_sound_update();
        assert_eq!(sound, Some("05e009v0"));
    }

    #[test]
    fn mouse_steer_forward_straight() {
        // Car at (320, 200), direction 1 (North = up), mouse north of car → forward, no steer
        let mut car = DriveCar::new(320.0, 200.0, 1, test_props());
        car.mouse_steer(320, 50, true); // directly above
        assert!(car.throttle, "should accelerate forward");
        assert!(!car.braking);
        assert!(!car.steer_left);
        assert!(!car.steer_right);
    }

    #[test]
    fn mouse_steer_reverse() {
        // Car at (320, 200), direction 1 (North), mouse south → behind car → reverse
        let mut car = DriveCar::new(320.0, 200.0, 1, test_props());
        car.mouse_steer(320, 350, true); // directly below
        assert!(car.braking, "should brake/reverse");
        assert!(!car.throttle);
    }

    #[test]
    fn mouse_steer_no_input_when_released() {
        let mut car = DriveCar::new(320.0, 200.0, 1, test_props());
        car.throttle = true;
        car.steer_left = true;
        car.mouse_steer(100, 100, false); // button released
        assert!(!car.throttle);
        assert!(!car.braking);
        assert!(!car.steer_left);
        assert!(!car.steer_right);
    }

    #[test]
    fn gas_station_starts_refueling() {
        let mut car = DriveCar::new(120.0, 350.0, 1, test_props());
        car.fuel = 60.0; // half tank
        let gas = MapObject {
            object_id: 6, x: 120, y: 350,
            obj_type: MapObjectType::Gas, inner_radius: 15.0, outer_radius: 25.0,
            dir_resource: None, enabled: true,
            check_for_cache: Vec::new(), check_for_medals: Vec::new(),
            if_found: None, set_when_done: None,
            sound_id: None, approach_sound: None, sprite_name: None, z_under: true,
        };
        let event = car.update(&[gas], |_, _| 0, DriveCheat::default());
        assert!(matches!(event, DriveEvent::GasStation));
        assert_eq!(car.refuel_ticks, 10);
        assert_eq!(car.speed, 0.0);
    }

    #[test]
    fn refueling_fills_tank_over_time() {
        let mut car = DriveCar::new(320.0, 200.0, 1, test_props());
        car.fuel = 0.0;
        car.refuel_ticks = 1; // last step
        car.refuel_frame_counter = 9; // about to tick
        let event = car.update(&[], |_, _| 0, DriveCheat::default());
        assert!(matches!(event, DriveEvent::None));
        assert!(car.fuel > 0.0, "fuel should increase");
        assert_eq!(car.refuel_ticks, 0, "refueling should be done");
    }

    #[test]
    fn cows_block_without_horn() {
        let mut props = test_props();
        props.horn_type = 0; // no horn
        let mut car = DriveCar::new(370.0, 340.0, 1, props);
        car.speed = 2.0;
        let cows = MapObject {
            object_id: 1, x: 370, y: 340,
            obj_type: MapObjectType::Cows, inner_radius: 55.0, outer_radius: 85.0,
            dir_resource: None, enabled: true,
            check_for_cache: Vec::new(), check_for_medals: Vec::new(),
            if_found: None, set_when_done: None,
            sound_id: None, approach_sound: None, sprite_name: None, z_under: true,
        };
        let event = car.update(&[cows], |_, _| 0, DriveCheat::default());
        assert!(matches!(event, DriveEvent::AnimalsBlocking { has_horn: false, horn_type: 0 }));
        assert_eq!(car.speed, 0.0);
    }
}
