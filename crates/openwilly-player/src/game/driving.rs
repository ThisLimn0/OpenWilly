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

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Drive loop runs at 30 FPS (same as game loop)
pub const DRIVE_FPS: u32 = 30;
/// Number of discrete compass directions
pub const NUM_DIRECTIONS: usize = 16;
/// Visible map tile size
pub const MAP_WIDTH: i32 = 640;
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
pub const MAP_EDGE_MARGIN: i32 = 3;
/// Wheel offset factor (from direction vector)
pub const WHEEL_OFFSET_FACTOR: f32 = 8.0;

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
}

/// Type of map object
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MapObjectType {
    /// Fixed destination (scene transition)
    Destination,
    /// Random destination (position varies per game)
    RandomDestination,
    /// Custom behavior (gas station, ferry, cows, etc.)
    Custom,
    /// Position correction
    Correct,
    /// Stop zone
    Stop,
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

    /// Build a minimal default world map (5×6 grid)
    ///
    /// In production this should be loaded from Director data,
    /// but we construct a skeleton so the types are wired.
    pub fn default_map() -> Self {
        let mut tiles = HashMap::new();

        // Tile 1 with a gas-station object
        tiles.insert(1, MapTile {
            id: 1,
            map_image: "30b001v0".to_string(),
            topology: "30t001v0".to_string(),
            objects: vec![
                MapObject {
                    object_id: 100,
                    x: 320, y: 240,
                    obj_type: MapObjectType::Custom,
                    inner_radius: 30.0,
                    outer_radius: 60.0,
                    dir_resource: Some("89".to_string()),
                },
            ],
        });

        // Tile 2 with a fixed destination
        tiles.insert(2, MapTile {
            id: 2,
            map_image: "30b002v0".to_string(),
            topology: "30t002v0".to_string(),
            objects: vec![
                MapObject {
                    object_id: 200,
                    x: 400, y: 300,
                    obj_type: MapObjectType::Destination,
                    inner_radius: 25.0,
                    outer_radius: 50.0,
                    dir_resource: Some("92".to_string()),
                },
                MapObject {
                    object_id: 201,
                    x: 100, y: 100,
                    obj_type: MapObjectType::RandomDestination,
                    inner_radius: 20.0,
                    outer_radius: 40.0,
                    dir_resource: None,
                },
            ],
        });

        // Create remaining tiles with Correct / Stop variants
        for i in 3..=30u32 {
            let obj_type = match i % 5 {
                0 => MapObjectType::Stop,
                1 => MapObjectType::Correct,
                _ => MapObjectType::Destination,
            };
            tiles.insert(i, MapTile {
                id: i,
                map_image: format!("30b{:03}v0", i),
                topology: format!("30t{:03}v0", i),
                objects: vec![
                    MapObject {
                        object_id: 300 + i,
                        x: 320, y: 240,
                        obj_type,
                        inner_radius: 25.0,
                        outer_radius: 50.0,
                        dir_resource: None,
                    },
                ],
            });
        }

        // 5×6 grid using tile IDs 1–30
        let grid: Vec<Vec<u32>> = (0..5)
            .map(|row| (1..=6).map(|col| (row * 6 + col) as u32).collect())
            .collect();

        Self {
            grid,
            tiles,
            start_tile: (2, 2),
            start_pos: (320.0, 240.0),
            start_direction: 1,
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
        let steering_rate = (props.steering as f32 + 3.0) * 2.0 / 20.0 * 70.0;
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

/// Pre-computed direction vectors for 16 directions (1-based: 1=North, 5=East, 9=South, 13=West)
pub fn direction_vector(dir: u8) -> (f32, f32) {
    // dir is 1-based: subtract 1 so that dir=1 → angle=0 → north=(0,-1)
    let angle = std::f32::consts::PI * 2.0 * ((dir as f32 - 1.0) / NUM_DIRECTIONS as f32);
    (angle.sin(), -angle.cos())
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
    /// Reverse stop timer (10 frames)
    reverse_stop_timer: u8,
    /// Whether we're currently stopped
    pub stopped: bool,
    /// Whether fuel is empty
    pub fuel_empty: bool,
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
    pub fn update<F>(&mut self, objects: &[MapObject], get_terrain: F) -> DriveEvent
    where
        F: Fn(i32, i32) -> u8,
    {
        if self.fuel_empty {
            return DriveEvent::FuelEmpty;
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
        if self.throttle {
            self.speed += self.props.acceleration;
            if self.speed > self.props.max_speed {
                self.speed = self.props.max_speed;
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
                self.speed -= self.props.acceleration;
                if self.speed < -self.props.reverse_max {
                    self.speed = -self.props.reverse_max;
                }
            }
        } else {
            // Natural deceleration (friction)
            if self.speed > 0.0 {
                self.speed -= 0.01;
                if self.speed < 0.0 {
                    self.speed = 0.0;
                }
            } else if self.speed < 0.0 {
                self.speed += 0.01;
                if self.speed > 0.0 {
                    self.speed = 0.0;
                }
            }
        }

        self.stopped = self.speed.abs() < 0.001;

        // --- Movement ---
        let (dx, dy) = direction_vector(self.direction);
        let new_x = self.x + dx * self.speed;
        let new_y = self.y + dy * self.speed;

        // --- Terrain check ---
        let topo_x = ((new_x as i32 - MAP_OFFSET_X) / 2).clamp(0, TOPO_WIDTH - 1);
        let topo_y = ((new_y as i32 - MAP_OFFSET_Y) / 2).clamp(0, TOPO_HEIGHT - 1);
        let terrain = get_terrain(topo_x, topo_y);

        if terrain >= TERRAIN_WALL {
            // Wall — bounce back
            self.speed *= -0.1;
            return DriveEvent::TerrainBlocked { reason: "wall" };
        }

        let altitude = (terrain % 16) as i32;
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

        // Update tilt from altitude
        self.tilt = (altitude as i8).clamp(-2, 2);

        // Apply movement
        self.x = new_x;
        self.y = new_y;

        // --- Fuel consumption ---
        if self.speed.abs() > 0.001 {
            self.fuel -= self.speed.abs() * self.props.fuel_consumption / 100.0;
            if self.fuel <= 0.0 {
                self.fuel = 0.0;
                self.fuel_empty = true;
                self.speed = 0.0;
                return DriveEvent::FuelEmpty;
            }
        }

        // --- Map edge transition ---
        if self.x < MAP_EDGE_MARGIN as f32 {
            return DriveEvent::TileTransition { delta_col: -1, delta_row: 0 };
        }
        if self.x > (MAP_WIDTH - MAP_EDGE_MARGIN) as f32 {
            return DriveEvent::TileTransition { delta_col: 1, delta_row: 0 };
        }
        if self.y < MAP_EDGE_MARGIN as f32 {
            return DriveEvent::TileTransition { delta_col: 0, delta_row: -1 };
        }
        if self.y > (MAP_HEIGHT - MAP_EDGE_MARGIN) as f32 {
            return DriveEvent::TileTransition { delta_col: 0, delta_row: 1 };
        }

        // --- Object collision detection ---
        for obj in objects {
            let dist = ((self.x - obj.x as f32).powi(2) + (self.y - obj.y as f32).powi(2)).sqrt();

            if dist <= obj.inner_radius / 2.0 {
                match obj.obj_type {
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
                        // Position correction objects nudge the car
                        tracing::trace!("Position correction at ({}, {})", obj.x, obj.y);
                    }
                    MapObjectType::Custom => {
                        // Custom behavior (gas station, ferry, etc.)
                        tracing::trace!("Custom object {} at ({}, {})", obj.object_id, obj.x, obj.y);
                    }
                }
            } else if dist <= obj.outer_radius {
                // Approaching object — can be used for visual/audio cues
                tracing::trace!("Approaching object {} (dist={:.1})", obj.object_id, dist);
            }
        }

        DriveEvent::None
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

        // Wrap position
        if delta_col < 0 {
            self.x = (MAP_WIDTH - MAP_EDGE_MARGIN - 1) as f32;
        } else if delta_col > 0 {
            self.x = (MAP_EDGE_MARGIN + 1) as f32;
        }
        if delta_row < 0 {
            self.y = (MAP_HEIGHT - MAP_EDGE_MARGIN - 1) as f32;
        } else if delta_row > 0 {
            self.y = (MAP_EDGE_MARGIN + 1) as f32;
        }
    }

    /// Refuel (gas station interaction)
    pub fn refuel(&mut self) {
        self.fuel = self.props.fuel_max;
        self.fuel_empty = false;
    }

    /// Get fuel as percentage (0.0 - 1.0)
    pub fn fuel_percent(&self) -> f32 {
        if self.props.fuel_max > 0.0 {
            (self.fuel / self.props.fuel_max).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }

    /// Get the sprite name for current direction + tilt
    /// Sprites are in 05.DXR, members 78-157 (16 dirs × 5 tilts)
    pub fn sprite_member(&self) -> u32 {
        let dir_idx = (self.direction as u32 - 1) % NUM_DIRECTIONS as u32;
        let tilt_idx = (self.tilt + 2).clamp(0, 4) as u32;
        78 + dir_idx * 5 + tilt_idx
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
        // Direction 1 should be north = (0, -1)
        let (x, y) = direction_vector(1);
        assert!(x.abs() < 0.001, "North x should be ~0, got {}", x);
        assert!((y + 1.0).abs() < 0.001, "North y should be ~-1, got {}", y);
    }

    #[test]
    fn east_is_right() {
        // Direction 5 should be east = (1, 0)
        let (x, y) = direction_vector(5);
        assert!((x - 1.0).abs() < 0.001, "East x should be ~1, got {}", x);
        assert!(y.abs() < 0.001, "East y should be ~0, got {}", y);
    }

    #[test]
    fn south_is_down() {
        // Direction 9 should be south = (0, 1)
        let (x, y) = direction_vector(9);
        assert!(x.abs() < 0.001, "South x should be ~0, got {}", x);
        assert!((y - 1.0).abs() < 0.001, "South y should be ~1, got {}", y);
    }

    #[test]
    fn west_is_left() {
        // Direction 13 should be west = (-1, 0)
        let (x, y) = direction_vector(13);
        assert!((x + 1.0).abs() < 0.001, "West x should be ~-1, got {}", x);
        assert!(y.abs() < 0.001, "West y should be ~0, got {}", y);
    }

    #[test]
    fn acceleration_increases_speed() {
        let mut car = DriveCar::new(320.0, 200.0, 1, test_props());
        car.throttle = true;
        car.update(&[], |_, _| 0);
        assert!(car.speed > 0.0);
    }

    #[test]
    fn wall_stops_car() {
        let mut car = DriveCar::new(320.0, 200.0, 1, test_props());
        car.speed = 2.0;
        let event = car.update(&[], |_, _| 250); // everything is wall
        matches!(event, DriveEvent::TerrainBlocked { reason: "wall" });
    }

    #[test]
    fn fuel_consumption_reduces_fuel() {
        let mut car = DriveCar::new(320.0, 200.0, 1, test_props());
        let initial_fuel = car.fuel;
        car.speed = 2.0;
        car.update(&[], |_, _| 0);
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
        // Direction 1, tilt 0 → index (0, 2) → member 78 + 0*5 + 2 = 80
        assert_eq!(car.sprite_member(), 80);
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
        let event = car.update(&[], |_, _| 0);
        assert!(car.fuel_empty || matches!(event, DriveEvent::FuelEmpty));
    }
}
