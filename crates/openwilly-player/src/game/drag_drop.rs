//! Drag & Drop engine — handles part dragging, snapping, and drop targets
//!
//! Based on mulle.js MulleCarPart drag behavior:
//!   - Mouse down on draggable part → start drag (record grab offset)
//!   - Mouse move → update position, check snap to attachment points
//!   - Mouse up → attach to car / drop to target / bounce back
//!
//! The DragDropState is owned by SceneHandler and consulted each frame.

use std::collections::HashMap;

use crate::engine::Sprite;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Distance threshold (pixels) for snapping to an attachment point
pub const SNAP_DISTANCE: f64 = 40.0;

// ---------------------------------------------------------------------------
// Drop Target
// ---------------------------------------------------------------------------

/// Where a dragged part can be dropped (door, arrow, trash, etc.)
#[derive(Debug, Clone)]
pub struct DropTarget {
    /// Bounding rectangle in screen coordinates
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    /// Identifier for the drop action (e.g. "door_junk", "door_side", "arrow_left")
    pub id: String,
    /// Name for display / debug
    pub name: String,
}

impl DropTarget {
    pub fn hit_test(&self, px: i32, py: i32) -> bool {
        px >= self.x
            && py >= self.y
            && px < self.x + self.width as i32
            && py < self.y + self.height as i32
    }
}

/// Bounding rectangle for valid drop areas (junk piles, floor, etc.)
#[derive(Debug, Clone)]
pub struct DropRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl DropRect {
    pub fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x
            && py >= self.y
            && px < self.x + self.width as i32
            && py < self.y + self.height as i32
    }

    /// Random point inside this rect
    pub fn random_point(&self) -> (i32, i32) {
        // Simple deterministic fallback (center) — proper random needs rand crate
        (
            self.x + self.width as i32 / 2,
            self.y + self.height as i32 / 2,
        )
    }
}

// ---------------------------------------------------------------------------
// Draggable Item
// ---------------------------------------------------------------------------

/// A draggable item in the scene
#[derive(Debug, Clone)]
pub struct DraggableItem {
    /// Part ID from PartsDB
    pub part_id: u32,
    /// Position in screen coordinates
    pub x: i32,
    pub y: i32,
    /// Junk-view sprite data (how part looks when loose)
    pub junk_sprite: Sprite,
    /// Z-order for rendering
    pub z_order: i32,
    /// Whether this item is currently being dragged
    pub dragging: bool,
    /// Whether the item can snap/attach to the car
    pub can_attach: bool,
    /// Active morph index (None = no morph, Some(i) = morphs_to[i])
    pub active_morph: Option<usize>,
    /// Drag ticks counter (how long it's been dragged)
    pub drag_ticks: u32,
}

impl DraggableItem {
    pub fn new(part_id: u32, x: i32, y: i32, junk_sprite: Sprite, z_order: i32) -> Self {
        let mut item = Self {
            part_id,
            x,
            y,
            junk_sprite: junk_sprite.clone(),
            z_order,
            dragging: false,
            can_attach: false,
            active_morph: None,
            drag_ticks: 0,
        };
        item.junk_sprite.x = x;
        item.junk_sprite.y = y;
        item.junk_sprite.z_order = z_order;
        item
    }

    /// Hit-test using the junk_sprite
    pub fn hit_test(&self, px: i32, py: i32) -> bool {
        self.junk_sprite.hit_test(px, py)
    }

    /// Bounding box hit-test
    pub fn bbox_hit(&self, px: i32, py: i32) -> bool {
        self.junk_sprite.bbox_hit(px, py)
    }

    /// Update sprite position to match current x, y
    fn sync_sprite_pos(&mut self) {
        self.junk_sprite.x = self.x;
        self.junk_sprite.y = self.y;
    }

    /// Get the renderable sprite (junk_view for now, use_view during snap)
    pub fn as_sprite(&self) -> Sprite {
        self.junk_sprite.clone()
    }
}

// ---------------------------------------------------------------------------
// Snap Target (attachment point on the car)
// ---------------------------------------------------------------------------

/// An attachment point on the car where a part can snap to
#[derive(Debug, Clone)]
pub struct SnapTarget {
    /// Attachment point ID (e.g. "#a6")
    pub point_id: String,
    /// Position on screen where this attachment point is
    pub x: i32,
    pub y: i32,
    /// Whether this point is currently occupied
    pub occupied: bool,
    /// Part IDs that are covered (blocked) by whatever occupies this slot
    pub covered_by: Option<u32>,
}

// ---------------------------------------------------------------------------
// Drag & Drop State Machine
// ---------------------------------------------------------------------------

/// What happened when a drop occurred
#[derive(Debug, Clone)]
pub enum DropResult {
    /// Part attached to car at the given attachment point
    Attached {
        part_id: u32,
        morph_id: Option<u32>,
        point_id: String,
    },
    /// Part dropped onto a drop target (door, arrow)
    DroppedOnTarget {
        part_id: u32,
        target_id: String,
    },
    /// Part dropped elsewhere (stays where it is or bounces back)
    Dropped {
        part_id: u32,
    },
    /// Nothing happened (no drag was active)
    Nothing,
}

/// Central drag & drop state
pub struct DragDropState {
    /// All draggable items in the current scene
    pub items: Vec<DraggableItem>,
    /// Drop targets (doors, arrows, etc.)
    pub drop_targets: Vec<DropTarget>,
    /// Valid drop areas (where parts can rest)
    pub drop_rects: Vec<DropRect>,
    /// Snap targets on the car (attachment points)
    pub snap_targets: Vec<SnapTarget>,
    /// Index of the currently dragged item (if any)
    dragging_idx: Option<usize>,
    /// Offset from item origin to grab point
    grab_offset_x: i32,
    grab_offset_y: i32,
    /// Whether the mouse was down last frame
    prev_mouse_down: bool,
}

impl DragDropState {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            drop_targets: Vec::new(),
            drop_rects: Vec::new(),
            snap_targets: Vec::new(),
            dragging_idx: None,
            grab_offset_x: 0,
            grab_offset_y: 0,
            prev_mouse_down: false,
        }
    }

    /// Is anything currently being dragged?
    pub fn is_dragging(&self) -> bool {
        self.dragging_idx.is_some()
    }

    /// Get the currently dragged item (if any)
    pub fn dragged_item(&self) -> Option<&DraggableItem> {
        self.dragging_idx.map(|i| &self.items[i])
    }

    /// Add a draggable item and return its index
    pub fn add_item(&mut self, item: DraggableItem) -> usize {
        let idx = self.items.len();
        self.items.push(item);
        idx
    }

    /// Remove a draggable item by index. Adjusts dragging_idx if needed.
    pub fn remove_item(&mut self, idx: usize) {
        if idx >= self.items.len() {
            return;
        }
        // If we're removing the dragged item, cancel drag
        if self.dragging_idx == Some(idx) {
            self.dragging_idx = None;
        } else if let Some(drag_idx) = self.dragging_idx {
            if drag_idx > idx {
                self.dragging_idx = Some(drag_idx - 1);
            }
        }
        self.items.remove(idx);
    }

    /// Collect current positions of all draggable items as part_id → (x, y)
    pub fn item_positions(&self) -> HashMap<u32, (i32, i32)> {
        self.items
            .iter()
            .map(|item| (item.part_id, (item.x, item.y)))
            .collect()
    }

    /// Find the topmost draggable item at (px, py) using hit-test (alpha-aware)
    pub fn item_at(&self, px: i32, py: i32) -> Option<usize> {
        // Iterate from back to front (last = topmost)
        for (i, item) in self.items.iter().enumerate().rev() {
            // Fast bbox pre-check before expensive alpha-aware hit_test
            if item.bbox_hit(px, py) && item.hit_test(px, py) {
                return Some(i);
            }
        }
        None
    }

    // -----------------------------------------------------------------------
    // Input handlers
    // -----------------------------------------------------------------------

    /// Call on mouse down. Returns true if drag started.
    pub fn on_mouse_down(&mut self, mx: i32, my: i32) -> bool {
        if self.dragging_idx.is_some() {
            return false; // Already dragging
        }

        if let Some(idx) = self.item_at(mx, my) {
            // Compute max z-order before mutating
            let max_z = self.items.iter().map(|i| i.z_order).max().unwrap_or(100);

            let item = &mut self.items[idx];
            item.dragging = true;
            item.drag_ticks = 0;
            item.can_attach = false;
            item.active_morph = None;
            item.z_order = max_z + 1;
            item.junk_sprite.z_order = max_z + 1;

            let part_id = item.part_id;
            self.grab_offset_x = item.x - mx;
            self.grab_offset_y = item.y - my;
            self.dragging_idx = Some(idx);

            tracing::debug!("Drag start: part {} at ({}, {})", part_id, mx, my);
            true
        } else {
            false
        }
    }

    /// Call on mouse move while button is held.
    pub fn on_mouse_move(&mut self, mx: i32, my: i32) {
        if let Some(idx) = self.dragging_idx {
            let item = &mut self.items[idx];
            item.x = mx + self.grab_offset_x;
            item.y = my + self.grab_offset_y;
            item.drag_ticks += 1;
            item.sync_sprite_pos();

            // Check snap targets
            self.check_snap(idx, mx, my);
        }
    }

    /// Call on mouse up. Returns the drop result.
    pub fn on_mouse_up(&mut self, mx: i32, my: i32) -> DropResult {
        let idx = match self.dragging_idx.take() {
            Some(i) => i,
            None => return DropResult::Nothing,
        };

        self.items[idx].dragging = false;
        let part_id = self.items[idx].part_id;
        let can_attach = self.items[idx].can_attach;
        let active_morph = self.items[idx].active_morph;

        tracing::debug!("Drag end: part {} at ({}, {}), can_attach={}", part_id, mx, my, can_attach);

        // 1. Check if snapped to an attachment point
        if can_attach {
            let morph_id = active_morph.map(|_| part_id); // TODO: resolve actual morph ID
            // Use item center (consistent with check_snap during drag)
            let item_cx = self.items[idx].x + self.items[idx].junk_sprite.width as i32 / 2;
            let item_cy = self.items[idx].y + self.items[idx].junk_sprite.height as i32 / 2;
            if let Some(point_id) = self.find_closest_snap_target(item_cx, item_cy) {
                return DropResult::Attached {
                    part_id,
                    morph_id,
                    point_id,
                };
            }
        }

        // 2. Check drop targets (doors, arrows)
        for target in &self.drop_targets {
            if target.hit_test(mx, my) {
                tracing::debug!("Dropped on target '{}' ({})", target.name, target.id);
                return DropResult::DroppedOnTarget {
                    part_id,
                    target_id: target.id.clone(),
                };
            }
        }

        // 3. Check if within valid drop rects
        if !self.drop_rects.is_empty() {
            let item_x = self.items[idx].x;
            let item_y = self.items[idx].y;
            let in_bounds = self.drop_rects.iter().any(|r| r.contains(item_x, item_y));
            if !in_bounds {
                // Bounce back to a valid area (center of first rect for now)
                if let Some(rect) = self.drop_rects.first() {
                    let (rx, ry) = rect.random_point();
                    self.items[idx].x = rx;
                    self.items[idx].y = ry;
                    self.items[idx].sync_sprite_pos();
                }
            }
        }

        DropResult::Dropped { part_id }
    }

    /// Process mouse input for a frame. Call this with current mouse state.
    ///
    /// Returns a DropResult if a drop just occurred.
    pub fn process_mouse(&mut self, mx: i32, my: i32, mouse_down: bool) -> DropResult {
        let was_down = self.prev_mouse_down;
        self.prev_mouse_down = mouse_down;

        if mouse_down && !was_down {
            // Mouse just pressed
            self.on_mouse_down(mx, my);
            DropResult::Nothing
        } else if mouse_down && was_down {
            // Mouse held — dragging
            self.on_mouse_move(mx, my);
            DropResult::Nothing
        } else if !mouse_down && was_down {
            // Mouse just released
            self.on_mouse_up(mx, my)
        } else {
            DropResult::Nothing
        }
    }

    // -----------------------------------------------------------------------
    // Snap logic
    // -----------------------------------------------------------------------

    fn check_snap(&mut self, idx: usize, _mx: i32, _my: i32) {
        let item = &self.items[idx];
        let item_cx = item.x + item.junk_sprite.width as i32 / 2;
        let item_cy = item.y + item.junk_sprite.height as i32 / 2;

        let mut closest_dist = f64::MAX;
        let mut closest_point: Option<String> = None;

        for snap in &self.snap_targets {
            if snap.occupied {
                continue;
            }
            // Skip points that are covered by another part
            if snap.covered_by.is_some() {
                continue;
            }
            let dx = (item_cx - snap.x) as f64;
            let dy = (item_cy - snap.y) as f64;
            let dist = (dx * dx + dy * dy).sqrt();

            if dist < SNAP_DISTANCE && dist < closest_dist {
                closest_dist = dist;
                closest_point = Some(snap.point_id.clone());
            }
        }

        let item = &mut self.items[idx];
        if closest_point.is_some() {
            item.can_attach = true;
        } else {
            item.can_attach = false;
            item.active_morph = None;
        }
    }

    fn find_closest_snap_target(&self, cx: i32, cy: i32) -> Option<String> {
        let mut closest_dist = f64::MAX;
        let mut closest = None;

        for snap in &self.snap_targets {
            if snap.occupied {
                continue;
            }
            let dx = (cx - snap.x) as f64;
            let dy = (cy - snap.y) as f64;
            let dist = (dx * dx + dy * dy).sqrt();

            // Use same SNAP_DISTANCE as check_snap for consistency
            if dist < SNAP_DISTANCE && dist < closest_dist {
                closest_dist = dist;
                closest = Some(snap.point_id.clone());
            }
        }

        closest
    }

    // -----------------------------------------------------------------------
    // Rendering
    // -----------------------------------------------------------------------

    /// Get all item sprites, sorted by z-order
    pub fn all_sprites(&self) -> Vec<Sprite> {
        let mut sprites: Vec<Sprite> = self
            .items
            .iter()
            .map(|item| item.as_sprite())
            .collect();
        sprites.sort_by_key(|s| s.z_order);
        sprites
    }

    /// Get the hover info for a position (part name/id if hovering over draggable)
    pub fn hover_info(&self, px: i32, py: i32) -> Option<String> {
        // Show drag state if actively dragging
        if let Some(item) = self.dragged_item() {
            return Some(format!("Dragging Part #{}", item.part_id));
        }
        if let Some(idx) = self.item_at(px, py) {
            Some(format!("Part #{}", self.items[idx].part_id))
        } else {
            None
        }
    }
}

impl Default for DragDropState {
    fn default() -> Self {
        Self::new()
    }
}
