//! Parts Database — all ~307 car parts with properties, morphs, attachment points
//!
//! Loaded at startup from embedded JSON (originally `parts.hash.json` from mulle.js).
//! Each part has:
//!   - `part_id` (unique integer key)
//!   - `master` (0 = standalone, else parent part for morphed variants)
//!   - `morphs_to` (list of variant IDs this part can morph into)
//!   - `description`, `junk_view`, `use_view`, `use_view2` — Director member names
//!   - `offset` — pixel offset when placed on car
//!   - `properties` — gameplay-relevant stats (weight, speed, grip, …)
//!   - `requires` — attachment points this part needs (e.g. "#a6")
//!   - `covers` — attachment points this part blocks
//!   - `attachment_points` — new attachment points this part provides

use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A single car part definition
#[derive(Debug, Clone)]
pub struct PartData {
    pub part_id: u32,
    pub master: u32,
    pub morphs_to: Vec<u32>,
    pub description: String,
    pub junk_view: String,
    pub use_view: String,
    pub use_view2: String,
    pub offset: (i32, i32),
    pub properties: PartProperties,
    pub requires: Vec<String>,
    pub covers: Vec<String>,
    pub attachment_points: Vec<AttachmentPoint>,
}

impl PartData {
    /// Is this part a morph-parent (i.e. has variants but no own use_view)?
    pub fn is_morph_parent(&self) -> bool {
        !self.morphs_to.is_empty() && self.use_view.is_empty()
    }

    /// Is this a morph-child (placed variant of a parent)?
    pub fn is_morph_child(&self) -> bool {
        self.master != 0
    }

    /// Can this part be picked up from the junkyard? (has a junk_view)
    pub fn has_junk_view(&self) -> bool {
        !self.junk_view.is_empty()
    }

    /// Can this part be placed on a car? (has a use_view)
    pub fn has_use_view(&self) -> bool {
        !self.use_view.is_empty()
    }
}

/// Gameplay-relevant properties of a part
#[derive(Debug, Clone, Default)]
pub struct PartProperties {
    pub weight: i32,
    pub speed: i32,
    pub brake: i32,
    pub durability: i32,
    pub grip: i32,
    pub steering: i32,
    pub acceleration: i32,
    pub strength: i32,
    pub fuel_consumption: i32,
    pub fuel_volume: i32,
    pub electric_consumption: i32,
    pub electric_volume: i32,
    pub comfort: i32,
    pub funny_factor: i32,
    pub horn: i32,
    pub horn_type: i32,
    pub exhaust_pipe: i32,
    pub lamps: i32,
    pub pedals: i32,
    pub load_capacity: i32,
    pub engine_type: i32,
    pub color: i32,
}

/// An attachment point that a part provides (from the "new" field)
#[derive(Debug, Clone)]
pub struct AttachmentPoint {
    /// e.g. "#a1", "#b3"
    pub id: String,
    /// Sort index for layering: (foreground_z, background_z)
    pub sort_index: (i32, i32),
    /// Pixel offset relative to part origin
    pub offset: (i32, i32),
}

// ---------------------------------------------------------------------------
// Parts Database
// ---------------------------------------------------------------------------

/// The central parts database — provides lookup by ID, category queries, etc.
pub struct PartsDB {
    parts: HashMap<u32, PartData>,
}

/// Part category for junk distribution
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartCategory {
    JunkMan,
    Destination,
    Random,
}

impl PartsDB {
    /// Load the embedded parts database (parsed once at startup)
    pub fn load() -> Self {
        let json_str = include_str!("../../data/parts.hash.json");
        let raw: HashMap<String, serde_json::Value> =
            serde_json::from_str(json_str).expect("Failed to parse parts.hash.json");

        let mut parts = HashMap::new();
        for (_key, value) in &raw {
            if let Some(part) = parse_part(value) {
                parts.insert(part.part_id, part);
            }
        }

        tracing::info!("PartsDB loaded: {} parts", parts.len());
        Self { parts }
    }

    /// Get a part by ID
    pub fn get(&self, part_id: u32) -> Option<&PartData> {
        self.parts.get(&part_id)
    }

    /// Get all part IDs
    pub fn all_ids(&self) -> Vec<u32> {
        let mut ids: Vec<u32> = self.parts.keys().copied().collect();
        ids.sort();
        ids
    }

    /// Total number of parts
    pub fn len(&self) -> usize {
        self.parts.len()
    }

    /// Iterate over all parts
    pub fn iter(&self) -> impl Iterator<Item = (&u32, &PartData)> {
        self.parts.iter()
    }

    /// Get all morph variants for a parent part
    pub fn get_morphs(&self, parent_id: u32) -> Vec<&PartData> {
        if let Some(parent) = self.parts.get(&parent_id) {
            parent
                .morphs_to
                .iter()
                .filter_map(|id| self.parts.get(id))
                .collect()
        } else {
            vec![]
        }
    }

    /// Get the master (parent) part for a morph child
    pub fn get_master(&self, part_id: u32) -> Option<&PartData> {
        let part = self.parts.get(&part_id)?;
        if part.master == 0 {
            None
        } else {
            self.parts.get(&part.master)
        }
    }

    /// Get all standalone parts (not morph children) that can be picked up
    pub fn junkyard_parts(&self) -> Vec<&PartData> {
        self.parts
            .values()
            .filter(|p| p.has_junk_view() && !p.is_morph_child())
            .collect()
    }

    /// Get parts that require a specific attachment point
    pub fn parts_for_attachment(&self, point: &str) -> Vec<&PartData> {
        self.parts
            .values()
            .filter(|p| p.requires.iter().any(|r| r == point))
            .collect()
    }

    /// Default car parts (chassis + battery + gearbox + brake)
    pub fn default_car_parts() -> &'static [u32] {
        &[1, 82, 133, 152]
    }

    // -----------------------------------------------------------------------
    // Junk pile distribution (from mulle.js MulleGame.initJunkPiles)
    // -----------------------------------------------------------------------

    /// Initial junk pile contents (pile index 0-5 = 6 piles)
    pub fn initial_pile_parts() -> [Vec<u32>; 6] {
        [
            vec![66, 29, 143, 178],  // Pile 0
            vec![215, 47, 12, 140],  // Pile 1
            vec![96, 76, 104, 271],  // Pile 2
            vec![126, 48, 113, 220], // Pile 3
            vec![146, 55, 107, 74],  // Pile 4
            vec![93, 81, 150, 196],  // Pile 5
        ]
    }

    /// Parts initially on the shop floor
    pub fn initial_shop_floor_parts() -> Vec<u32> {
        vec![239, 134, 257]
    }

    /// Parts initially in the yard
    pub fn initial_yard_parts() -> Vec<u32> {
        vec![162, 236, 265]
    }

    /// Part IDs assigned to JunkMan category
    pub fn junkman_part_ids() -> &'static [u32] {
        &[
            2, 6, 9, 12, 14, 17, 19, 21, 23, 24, 25, 29, 30, 31, 33, 41,
            43, 53, 54, 64, 65, 69, 74, 75, 91, 99, 100, 112, 119, 120,
            129, 130, 131, 132, 133, 149, 153, 154, 161, 172, 230, 242,
            248, 260, 272, 273, 288, 291, 297, 307,
        ]
    }

    /// Part IDs found at destinations
    pub fn destination_part_ids() -> &'static [u32] {
        &[5, 13, 35, 101, 121, 137, 200, 254, 283]
    }

    /// Part IDs that spawn randomly
    pub fn random_part_ids() -> &'static [u32] {
        &[
            18, 20, 22, 26, 27, 28, 32, 38, 42, 89, 90, 92, 108, 116,
            127, 140, 141, 143, 147, 155, 158, 162, 167, 168, 173, 174,
            175, 176, 177, 181, 184, 185, 186, 189, 190, 191, 192, 193,
            195, 203, 208, 209, 210, 211, 212, 213, 214, 221, 222, 227,
            228, 229, 233,
        ]
    }

    /// Determine what category a part belongs to
    pub fn part_category(&self, part_id: u32) -> Option<PartCategory> {
        if Self::junkman_part_ids().contains(&part_id) {
            Some(PartCategory::JunkMan)
        } else if Self::destination_part_ids().contains(&part_id) {
            Some(PartCategory::Destination)
        } else if Self::random_part_ids().contains(&part_id) {
            Some(PartCategory::Random)
        } else {
            None
        }
    }

    // -----------------------------------------------------------------------
    // Car property aggregation (from mulle.js getCarProperties)
    // -----------------------------------------------------------------------

    /// Compute aggregated car properties from a list of placed part IDs
    pub fn compute_car_properties(&self, placed_parts: &[u32]) -> CarProperties {
        let mut car = CarProperties::default();
        for &pid in placed_parts {
            if let Some(part) = self.parts.get(&pid) {
                let p = &part.properties;

                // Summed properties
                car.weight += p.weight;
                car.brake += p.brake;
                car.grip += p.grip;
                car.strength += p.strength;
                car.fuel_consumption += p.fuel_consumption;
                car.fuel_volume += p.fuel_volume;
                car.electric_consumption += p.electric_consumption;
                car.electric_volume += p.electric_volume;
                car.comfort += p.comfort;
                car.funny_factor += p.funny_factor;
                car.load_capacity += p.load_capacity;

                // Max properties
                car.durability = car.durability.max(p.durability);
                car.steering = car.steering.max(p.steering);
                car.acceleration = car.acceleration.max(p.acceleration);
                car.speed = car.speed.max(p.speed);
                car.engine_type = car.engine_type.max(p.engine_type);
                car.horn_type = car.horn_type.max(p.horn_type);

                // Flag properties (any non-zero sets the flag)
                if p.horn > 0 { car.horn = car.horn.max(p.horn); }
                if p.exhaust_pipe > 0 { car.exhaust_pipe = car.exhaust_pipe.max(p.exhaust_pipe); }
                if p.lamps > 0 { car.lamps = car.lamps.max(p.lamps); }
                if p.pedals > 0 { car.pedals = car.pedals.max(p.pedals); }

                // Tire count: each part with grip > 0 counts as one tire
                if p.grip > 0 { car.tire_count += 1; }
            }
        }
        car
    }
}

/// Aggregated car properties (computed from all placed parts)
#[derive(Debug, Clone, Default)]
pub struct CarProperties {
    // Summed
    pub weight: i32,
    pub brake: i32,
    pub grip: i32,
    pub strength: i32,
    pub fuel_consumption: i32,
    pub fuel_volume: i32,
    pub electric_consumption: i32,
    pub electric_volume: i32,
    pub comfort: i32,
    pub funny_factor: i32,
    pub load_capacity: i32,

    // Max
    pub durability: i32,
    pub steering: i32,
    pub acceleration: i32,
    pub speed: i32,
    pub engine_type: i32,
    pub horn_type: i32,

    // Flags
    pub horn: i32,
    pub exhaust_pipe: i32,
    pub lamps: i32,
    pub pedals: i32,

    // Derived
    /// Number of parts with grip > 0 (= tire count)
    pub tire_count: i32,
}

impl CarProperties {
    /// Check if the car is road-legal (8 conditions from mulle.js isRoadLegal)
    ///
    /// Requirements:
    ///   1. engine_type > 0    — has a motor
    ///   2. tire_count >= 2    — at least 2 parts with grip > 0
    ///   3. brake > 0          — has brakes
    ///   4. fuel_consumption > 0 — engine consumes fuel
    ///   5. electric_volume > 0  — has a battery
    ///   6. fuel_volume > 0      — has a fuel tank
    ///   7. acceleration > 0     — has a gearbox
    ///   8. steering > 0         — has steering
    pub fn is_road_legal(&self) -> bool {
        self.engine_type > 0
            && self.tire_count >= 2
            && self.brake > 0
            && self.fuel_consumption > 0
            && self.electric_volume > 0
            && self.fuel_volume > 0
            && self.acceleration > 0
            && self.steering > 0
    }

    /// Detailed check returning which conditions fail (for Mulle's dialog hints)
    pub fn road_legal_failures(&self) -> Vec<&'static str> {
        let mut failures = Vec::new();
        if self.engine_type <= 0 { failures.push("engine"); }
        if self.tire_count < 2 { failures.push("tires"); }
        if self.brake <= 0 { failures.push("brake"); }
        if self.fuel_consumption <= 0 { failures.push("fuel_consumption"); }
        if self.electric_volume <= 0 { failures.push("battery"); }
        if self.fuel_volume <= 0 { failures.push("fuel_tank"); }
        if self.acceleration <= 0 { failures.push("gearbox"); }
        if self.steering <= 0 { failures.push("steering"); }
        failures
    }
}

// ---------------------------------------------------------------------------
// JSON parsing helpers
// ---------------------------------------------------------------------------

fn parse_part(v: &serde_json::Value) -> Option<PartData> {
    let obj = v.as_object()?;

    let part_id = obj.get("partId")?.as_u64()? as u32;
    let master = obj.get("master").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
    let morphs_to = parse_id_list(obj.get("MorphsTo"));
    let description = obj
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let junk_view = obj
        .get("junkView")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let use_view = obj
        .get("UseView")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let use_view2 = obj
        .get("UseView2")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let offset = parse_offset(obj.get("offset"));
    let properties = parse_properties(obj.get("Properties"));
    let requires = parse_string_list(obj.get("Requires"));
    let covers = parse_string_list(obj.get("Covers"));
    let attachment_points = parse_attachment_points(obj.get("new"));

    Some(PartData {
        part_id,
        master,
        morphs_to,
        description,
        junk_view,
        use_view,
        use_view2,
        offset,
        properties,
        requires,
        covers,
        attachment_points,
    })
}

/// Parse MorphsTo: can be `0` or `[3, 4]`
fn parse_id_list(v: Option<&serde_json::Value>) -> Vec<u32> {
    match v {
        Some(serde_json::Value::Array(arr)) => arr
            .iter()
            .filter_map(|x| x.as_u64().map(|n| n as u32))
            .collect(),
        _ => vec![],
    }
}

/// Parse Requires/Covers: can be `0` or `["#a6"]`
fn parse_string_list(v: Option<&serde_json::Value>) -> Vec<String> {
    match v {
        Some(serde_json::Value::Array(arr)) => arr
            .iter()
            .filter_map(|x| x.as_str().map(String::from))
            .collect(),
        _ => vec![],
    }
}

/// Parse offset: `[x, y]`
fn parse_offset(v: Option<&serde_json::Value>) -> (i32, i32) {
    match v {
        Some(serde_json::Value::Array(arr)) if arr.len() >= 2 => {
            let x = arr[0].as_i64().unwrap_or(0) as i32;
            let y = arr[1].as_i64().unwrap_or(0) as i32;
            (x, y)
        }
        _ => (0, 0),
    }
}

/// Parse Properties: can be `0` or `{"Weight": 4, "speed": 2, ...}`
///
/// Keys have inconsistent casing in the JSON (e.g. "FuelConsumption" vs "Fuelconsumption").
/// We normalize all keys to lowercase for matching.
fn parse_properties(v: Option<&serde_json::Value>) -> PartProperties {
    let obj = match v {
        Some(serde_json::Value::Object(m)) => m,
        _ => return PartProperties::default(),
    };

    // Build lowercase key→value map
    let map: HashMap<String, i32> = obj
        .iter()
        .filter_map(|(k, v)| {
            let val = v.as_i64()? as i32;
            Some((k.to_lowercase(), val))
        })
        .collect();

    PartProperties {
        weight: map.get("weight").copied().unwrap_or(0),
        speed: map.get("speed").copied().unwrap_or(0),
        brake: map.get("break").copied().unwrap_or(0),
        durability: map.get("durability").copied().unwrap_or(0),
        grip: map.get("grip").copied().unwrap_or(0),
        steering: map.get("steering").copied().unwrap_or(0),
        acceleration: map.get("acceleration").copied().unwrap_or(0),
        strength: map.get("strength").copied().unwrap_or(0),
        fuel_consumption: map.get("fuelconsumption").copied().unwrap_or(0),
        fuel_volume: map.get("fuelvolume").copied().unwrap_or(0),
        electric_consumption: map.get("electricconsumption").copied().unwrap_or(0),
        electric_volume: map.get("electricvolume").copied().unwrap_or(0),
        comfort: map.get("comfort").copied().unwrap_or(0),
        funny_factor: map.get("funnyfactor").copied().unwrap_or(0),
        horn: map.get("horn").copied().unwrap_or(0),
        horn_type: map.get("horntype").copied().unwrap_or(0),
        exhaust_pipe: map.get("exhaustpipe").copied().unwrap_or(0),
        lamps: map.get("lamps").copied().unwrap_or(0),
        pedals: map.get("pedals").copied().unwrap_or(0),
        load_capacity: map.get("loadcapacity").copied().unwrap_or(0),
        engine_type: map.get("enginetype").copied().unwrap_or(0),
        color: map.get("color").copied().unwrap_or(0),
    }
}

/// Parse "new" (attachment points): can be `0` or `[["#a1", [32, 2], [0, 0]], ...]`
fn parse_attachment_points(v: Option<&serde_json::Value>) -> Vec<AttachmentPoint> {
    let arr = match v {
        Some(serde_json::Value::Array(a)) => a,
        _ => return vec![],
    };

    arr.iter()
        .filter_map(|entry| {
            let tuple = entry.as_array()?;
            if tuple.len() < 3 {
                return None;
            }
            let id = tuple[0].as_str()?.to_string();
            let sort_arr = tuple[1].as_array()?;
            let off_arr = tuple[2].as_array()?;
            let sort_index = (
                sort_arr.get(0).and_then(|v| v.as_i64()).unwrap_or(0) as i32,
                sort_arr.get(1).and_then(|v| v.as_i64()).unwrap_or(0) as i32,
            );
            let offset = (
                off_arr.get(0).and_then(|v| v.as_i64()).unwrap_or(0) as i32,
                off_arr.get(1).and_then(|v| v.as_i64()).unwrap_or(0) as i32,
            );
            Some(AttachmentPoint {
                id,
                sort_index,
                offset,
            })
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_parts_db() {
        let db = PartsDB::load();
        assert!(db.len() > 200, "Expected 200+ parts, got {}", db.len());
    }

    #[test]
    fn chassis_part_1() {
        let db = PartsDB::load();
        let chassis = db.get(1).expect("Part 1 (chassis) must exist");
        assert_eq!(chassis.part_id, 1);
        assert_eq!(chassis.master, 0);
        assert!(chassis.morphs_to.is_empty());
        assert_eq!(chassis.properties.weight, 4);
        assert!(!chassis.attachment_points.is_empty());
        // Chassis provides 21 attachment points
        assert!(chassis.attachment_points.len() >= 20);
    }

    #[test]
    fn morph_parent_child() {
        let db = PartsDB::load();
        // Part 2 is a morph parent (MorphsTo: [3, 4])
        let parent = db.get(2).expect("Part 2 must exist");
        assert!(parent.is_morph_parent());
        assert_eq!(parent.morphs_to, vec![3, 4]);

        // Part 3 is a morph child of part 2
        let child = db.get(3).expect("Part 3 must exist");
        assert!(child.is_morph_child());
        assert_eq!(child.master, 2);

        let master = db.get_master(3).expect("Part 3 must have a master");
        assert_eq!(master.part_id, 2);
    }

    #[test]
    fn properties_case_insensitive() {
        let db = PartsDB::load();
        // Part 3 has EngineType: 4 (uppercase "E")
        let p3 = db.get(3).expect("Part 3");
        assert_eq!(p3.properties.engine_type, 4);

        // Part 233 has "Enginetype": 1 (lowercase "t")
        let p233 = db.get(233).expect("Part 233");
        assert_eq!(p233.properties.engine_type, 1);
    }

    #[test]
    fn car_properties_aggregation() {
        let db = PartsDB::load();
        let default_parts = PartsDB::default_car_parts();
        let props = db.compute_car_properties(default_parts);
        // Chassis (1): weight=4, Battery (82): weight=2+ElectricVolume=2,
        // Gearbox (133): weight=2+acceleration=3, Brake (152): weight=1+break=5
        assert_eq!(props.weight, 4 + 2 + 2 + 1); // = 9
        assert_eq!(props.acceleration, 3);
        assert_eq!(props.brake, 5);
    }

    #[test]
    fn road_legal_check() {
        let db = PartsDB::load();
        // Default car (chassis + battery + gearbox + brake) is NOT road-legal
        // Missing: engine, tires (grip), fuel_consumption, fuel_volume, steering
        let default_props = db.compute_car_properties(PartsDB::default_car_parts());
        assert!(!default_props.is_road_legal());
        let failures = default_props.road_legal_failures();
        assert!(failures.contains(&"engine"), "missing engine");
        assert!(failures.contains(&"tires"), "missing tires");
        assert!(failures.contains(&"steering"), "missing steering");
    }

    #[test]
    fn tire_count_from_grip() {
        let db = PartsDB::load();
        // Default car has no tires
        let default_props = db.compute_car_properties(PartsDB::default_car_parts());
        assert_eq!(default_props.tire_count, 0);

        // Part 60 is a tire (morph child of 153) with Grip=2
        let p60 = db.get(60).expect("Part 60");
        assert!(p60.properties.grip > 0, "Part 60 should have grip (it's a tire)");

        // Adding 2 tires should give tire_count=2
        let mut parts = PartsDB::default_car_parts().to_vec();
        parts.push(60);  // front tire
        parts.push(61);  // rear tire (if exists, else another)
        let props = db.compute_car_properties(&parts);
        assert!(props.tire_count >= 2, "Should count at least 2 tires");
    }
}
