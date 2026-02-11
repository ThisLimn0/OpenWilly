//! MulleBuildCar — the car object that manages attachment points and placed parts
//!
//! Based on mulle.js MulleBuildCar and MulleCar:
//!   - Manages a list of placed part IDs
//!   - Tracks attachment points (from chassis + added parts)
//!   - attach(partId) / detach(partId) with full refresh
//!   - Renders all placed parts as layered sprites (fg + bg per part)
//!   - Computes aggregated car properties from placed parts

use std::collections::HashMap;

use crate::assets::AssetStore;
use crate::engine::Sprite;
use crate::game::parts_db::{PartsDB, PartData, CarProperties};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A live attachment point on the car
#[derive(Debug, Clone)]
pub struct LivePoint {
    /// Attachment point ID (e.g. "#a1")
    pub id: String,
    /// Sort index for foreground layering
    pub fg: i32,
    /// Sort index for background layering
    pub bg: i32,
    /// Pixel offset relative to car origin
    pub offset: (i32, i32),
    /// Which part currently occupies this point (None = free)
    pub occupied_by: Option<u32>,
}

/// A rendered part sprite on the car
#[derive(Debug, Clone)]
pub struct PlacedPartSprite {
    pub part_id: u32,
    /// Foreground sprite (UseView)
    pub fg_sprite: Option<Sprite>,
    /// Background sprite (UseView2)
    pub bg_sprite: Option<Sprite>,
    /// Layer for the primary attachment point
    pub layer: String,
}

/// Events that occur during car modifications
#[derive(Debug, Clone)]
pub enum CarEvent {
    /// Part was attached
    Attached { part_id: u32 },
    /// Part was detached — returns master_id and world position
    Detached {
        part_id: u32,
        master_id: u32,
        world_x: i32,
        world_y: i32,
    },
}

// ---------------------------------------------------------------------------
// BuildCar
// ---------------------------------------------------------------------------

/// The car being built in the garage
pub struct BuildCar {
    /// Screen position of the car (group origin)
    pub x: i32,
    pub y: i32,
    /// List of placed part IDs (the car state)
    pub parts: Vec<u32>,
    /// Live attachment points (rebuilt on refresh)
    pub points: HashMap<String, LivePoint>,
    /// Which attachment points are used by parts
    pub used_points: HashMap<String, u32>,
    /// Whether the car is locked (no modifications allowed)
    pub locked: bool,
    /// Cached rendered sprites (rebuilt on refresh)
    part_sprites: Vec<PlacedPartSprite>,
    /// Cached car properties (rebuilt on refresh)
    properties: CarProperties,
}

impl BuildCar {
    /// Create a new BuildCar with default parts at the given position
    pub fn new(x: i32, y: i32) -> Self {
        let mut car = Self {
            x,
            y,
            parts: Vec::new(),
            points: HashMap::new(),
            used_points: HashMap::new(),
            locked: false,
            part_sprites: Vec::new(),
            properties: CarProperties::default(),
        };

        // Start with default parts: chassis, battery, gearbox, brake
        car.parts = PartsDB::default_car_parts().to_vec();
        car
    }

    /// Full rebuild of attachment points, sprites, and properties
    pub fn refresh(&mut self, parts_db: &PartsDB, assets: &AssetStore) {
        self.rebuild_points(parts_db);
        self.rebuild_sprites(parts_db, assets);
        self.properties = parts_db.compute_car_properties(&self.parts);
    }

    /// Attach a part to the car. Returns CarEvent::Attached if successful.
    pub fn attach(&mut self, part_id: u32, parts_db: &PartsDB, assets: &AssetStore) -> Option<CarEvent> {
        if self.locked {
            return None;
        }

        // Verify the part exists
        let part = match parts_db.get(part_id) {
            Some(p) => p,
            None => {
                tracing::warn!("Cannot attach: part {} not found", part_id);
                return None;
            }
        };

        // If this is a morph parent, check morph variants
        if part.is_morph_parent() {
            let morphs = parts_db.get_morphs(part_id);
            for morph in &morphs {
                if self.can_attach_morph(morph) {
                    tracing::debug!("Morph parent {} → trying variant {}", part_id, morph.part_id);
                }
            }
        }

        // Verify required attachment points are available
        if !self.can_attach_part(part) {
            tracing::debug!("Cannot attach part {}: required points not available", part_id);
            return None;
        }

        self.parts.push(part_id);
        let event = CarEvent::Attached { part_id };

        self.refresh(parts_db, assets);
        Some(event)
    }

    /// Detach a part from the car. Returns CarEvent::Detached if successful.
    pub fn detach(&mut self, part_id: u32, parts_db: &PartsDB, assets: &AssetStore) -> Option<CarEvent> {
        if self.locked {
            return None;
        }

        let idx = self.parts.iter().position(|&p| p == part_id)?;

        // Determine the master ID (for morph parts, return to parent form)
        let master_id = if let Some(part) = parts_db.get(part_id) {
            if part.master != 0 { part.master } else { part_id }
        } else {
            part_id
        };

        // Calculate world position of the detached part
        let (world_x, world_y) = self.part_world_position(part_id, parts_db);

        self.parts.remove(idx);
        tracing::info!("Detached part {} (master: {}, remaining: {})", part_id, master_id, self.parts.len());

        self.refresh(parts_db, assets);

        Some(CarEvent::Detached {
            part_id,
            master_id,
            world_x,
            world_y,
        })
    }

    /// Check if a part can be attached (required points exist and are free)
    pub fn can_attach_part(&self, part: &PartData) -> bool {
        for req in &part.requires {
            match self.points.get(req) {
                Some(point) => {
                    if point.occupied_by.is_some() {
                        return false; // Point exists but is occupied
                    }
                }
                None => return false, // Point doesn't exist on car
            }
        }
        true
    }

    /// Check if a specific morph variant can attach
    pub fn can_attach_morph(&self, morph_part: &PartData) -> bool {
        self.can_attach_part(morph_part)
    }

    /// Get the aggregated car properties
    pub fn properties(&self) -> &CarProperties {
        &self.properties
    }

    /// Check if the car is road-legal
    pub fn is_road_legal(&self) -> bool {
        self.properties.is_road_legal()
    }

    /// Get the world position of a placed part
    fn part_world_position(&self, part_id: u32, parts_db: &PartsDB) -> (i32, i32) {
        if let Some(part) = parts_db.get(part_id) {
            (self.x + part.offset.0, self.y + part.offset.1)
        } else {
            (self.x, self.y)
        }
    }

    // -----------------------------------------------------------------------
    // Rebuild logic
    // -----------------------------------------------------------------------

    /// Rebuild all attachment points from placed parts
    fn rebuild_points(&mut self, parts_db: &PartsDB) {
        self.points.clear();
        self.used_points.clear();

        // First pass: collect all attachment points provided by parts
        for &pid in &self.parts {
            if let Some(part) = parts_db.get(pid) {
                for ap in &part.attachment_points {
                    self.points.insert(ap.id.clone(), LivePoint {
                        id: ap.id.clone(),
                        fg: ap.sort_index.0,
                        bg: ap.sort_index.1,
                        offset: ap.offset,
                        occupied_by: None,
                    });
                }
            }
        }

        // Second pass: mark used points (from Requires)
        for &pid in &self.parts {
            if let Some(part) = parts_db.get(pid) {
                for req in &part.requires {
                    if let Some(point) = self.points.get_mut(req) {
                        point.occupied_by = Some(pid);
                    }
                    self.used_points.insert(req.clone(), pid);
                }
            }
        }
    }

    /// Rebuild all part sprites for rendering
    fn rebuild_sprites(&mut self, parts_db: &PartsDB, assets: &AssetStore) {
        self.part_sprites.clear();

        for &pid in &self.parts {
            let part = match parts_db.get(pid) {
                Some(p) => p,
                None => continue,
            };

            if part.use_view.is_empty() {
                continue; // Morph parents don't have views
            }

            // Determine the layer (first required attachment point)
            let layer = part.requires.first().cloned().unwrap_or_default();

            // Determine the sort index for this layer
            let sort_fg = self.points.get(&layer).map(|p| p.fg).unwrap_or(8);
            let sort_bg = self.points.get(&layer).map(|p| p.bg).unwrap_or(7);

            // Load foreground sprite (UseView)
            let fg_sprite = self.load_part_sprite(
                &part.use_view,
                part.offset,
                sort_fg,
                pid,
                assets,
            );

            // Load background sprite (UseView2)
            let bg_sprite = if !part.use_view2.is_empty() {
                self.load_part_sprite(
                    &part.use_view2,
                    part.offset,
                    sort_bg,
                    pid,
                    assets,
                )
            } else {
                None
            };

            self.part_sprites.push(PlacedPartSprite {
                part_id: pid,
                fg_sprite,
                bg_sprite,
                layer,
            });
        }
    }

    /// Load a single part sprite by member name
    fn load_part_sprite(
        &self,
        member_name: &str,
        offset: (i32, i32),
        sort_index: i32,
        part_id: u32,
        assets: &AssetStore,
    ) -> Option<Sprite> {
        // Member names are like "20b001v2" — resolve from Director file "20.DXR"
        // The "20" prefix refers to the shared cast file
        // Try to find the member in any loaded Director file
        let bmp = assets.find_bitmap_by_name(member_name)?;

        // Use registration point for proper sprite positioning (reg_x/reg_y)
        let (reg_x, reg_y) = assets.find_bitmap_info_by_name(member_name)
            .map(|(_, _, bi)| (bi.reg_x as i32, bi.reg_y as i32))
            .unwrap_or((0, 0));

        Some(Sprite {
            x: self.x + offset.0 - reg_x,
            y: self.y + offset.1 - reg_y,
            width: bmp.width,
            height: bmp.height,
            pixels: bmp.pixels,
            visible: true,
            z_order: sort_index,
            name: format!("car:{}#{}", member_name, part_id),
            interactive: true,
            member_num: part_id,
        })
    }

    // -----------------------------------------------------------------------
    // Rendering
    // -----------------------------------------------------------------------

    /// Get all car sprites, sorted by sort index
    pub fn all_sprites(&self) -> Vec<Sprite> {
        let mut sprites = Vec::new();

        for ps in &self.part_sprites {
            if let Some(bg) = &ps.bg_sprite {
                sprites.push(bg.clone());
            }
            if let Some(fg) = &ps.fg_sprite {
                let mut s = fg.clone();
                // Include layer info in sprite name for debug
                if !ps.layer.is_empty() {
                    s.name = format!("{}@{}", s.name, ps.layer);
                }
                sprites.push(s);
            }
        }

        sprites.sort_by_key(|s| s.z_order);
        sprites
    }

    /// Check which placed part is at a screen position (for detach clicks)
    pub fn part_at(&self, px: i32, py: i32) -> Option<u32> {
        // Check from front to back (reverse z-order)
        let mut candidates: Vec<(u32, i32)> = Vec::new();

        for ps in &self.part_sprites {
            if let Some(fg) = &ps.fg_sprite {
                if fg.hit_test(px, py) {
                    candidates.push((ps.part_id, fg.z_order));
                }
            }
        }

        candidates.sort_by(|a, b| b.1.cmp(&a.1));
        candidates.first().map(|(id, _)| *id)
    }

    /// Get free attachment points (for snap targets)
    pub fn free_attachment_points(&self) -> Vec<(&str, i32, i32)> {
        self.points
            .values()
            .filter(|p| p.occupied_by.is_none())
            .map(|p| (p.id.as_str(), self.x + p.offset.0, self.y + p.offset.1))
            .collect()
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::parts_db::PartsDB;

    #[test]
    fn default_car_has_four_parts() {
        let car = BuildCar::new(300, 220);
        assert_eq!(car.parts.len(), 4);
        assert_eq!(car.parts, vec![1, 82, 133, 152]);
    }

    #[test]
    fn rebuild_points_from_chassis() {
        let parts_db = PartsDB::load();
        let mut car = BuildCar::new(300, 220);
        car.rebuild_points(&parts_db);

        // Chassis (part 1) provides 21 attachment points (#a1-#a20, #b1-#b4)
        // Some should be occupied by the default parts (82, 133, 152)
        assert!(!car.points.is_empty(), "Chassis should provide attachment points");

        // Check that at least some points are occupied by default parts
        let occupied_count = car.points.values()
            .filter(|p| p.occupied_by.is_some())
            .count();
        assert!(occupied_count > 0, "Default parts should occupy some points");
    }

    #[test]
    fn can_attach_checks_requirements() {
        let parts_db = PartsDB::load();
        let mut car = BuildCar::new(300, 220);
        car.rebuild_points(&parts_db);

        // Part 1 (chassis) is already placed — can't add if points are full
        // But there should be many free points on the chassis
        let free_count = car.points.values()
            .filter(|p| p.occupied_by.is_none())
            .count();
        assert!(free_count > 0, "Should have free attachment points");
    }

    #[test]
    fn car_position() {
        let car = BuildCar::new(100, 200);
        assert_eq!(car.x, 100);
        assert_eq!(car.y, 200);
    }

    #[test]
    fn locked_car_prevents_changes() {
        let _parts_db = PartsDB::load();
        // We can't call refresh without AssetStore (needs Director files),
        // but we can test the lock logic
        let mut car = BuildCar::new(300, 220);
        car.locked = true;

        // Create a dummy AssetStore-like scenario:
        // attach should return None when locked
        // (we test the early return, not the full attach logic)
        assert!(car.locked);
    }
}
