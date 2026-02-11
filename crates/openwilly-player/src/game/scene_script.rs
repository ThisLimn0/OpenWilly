//! Scene script system — data-driven dialog chains with branching.
//!
//! Each destination scene gets a `SceneScript` describing the sequence of
//! actions: talk, animate, set flags, branch on conditions, leave.
//! The script advances via events from the dialog and animation systems.
//!
//! Based on the mulle.js callback-chaining approach, but expressed as
//! a flat step list with conditional branching.

use std::collections::HashMap;

/// A condition that can gate a script step
#[derive(Debug, Clone)]
pub enum Condition {
    /// Check if a cache flag is set (e.g. "#Dog")
    HasCache(String),
    /// Check if a permanent/stuff flag is set (e.g. "#FerryTicket")
    HasStuff(String),
    /// Check if a permanent/stuff flag is NOT set
    #[allow(dead_code)] // Used by destination scripts (upcoming)
    NotStuff(String),
    /// Check if a part is attached to the car
    HasPart(u32),
    /// Check if a part is NOT on the car
    NotPart(u32),
    /// Always true
    Always,
}

/// An action performed by a script step
#[derive(Debug, Clone)]
pub enum Action {
    /// Play a dialog (audio_id) — script pauses until dialog finishes.
    /// Optional actor_name for lip-sync cue-point routing.
    Talk { audio_id: String, actor_name: Option<String> },
    /// Play an actor animation — script pauses until animation finishes
    PlayAnim {
        actor_name: String,
        anim_name: String,
    },
    /// Set a cache flag
    SetCache(String),
    /// Remove a cache flag
    RemoveCache(String),
    /// Set a permanent/stuff flag
    SetStuff(String),
    /// Give a part to the player (placed in yard)
    GivePart(u32),
    /// Refuel the car to maximum
    Refuel,
    /// Show/hide an actor
    SetActorVisible {
        actor_name: String,
        visible: bool,
    },
    /// Wait for a fixed duration (ms) before proceeding
    Delay(u32),
    /// Leave the scene (go back to world map)
    LeaveToWorld,
    /// Do nothing (used for conditional-only steps)
    Nop,
}

/// A single step in a scene script
#[derive(Debug, Clone)]
pub struct ScriptStep {
    /// Condition that must be true for this step to execute
    pub condition: Condition,
    /// The action to perform
    pub action: Action,
    /// If true, wait for this step to complete before advancing
    /// (dialogs and animations are blocking; flags/set operations are instant)
    pub blocking: bool,
    /// Optional label for jump targets
    pub label: Option<String>,
    /// If set, jump to this label after completing (for branching)
    pub jump_to: Option<String>,
}

impl ScriptStep {
    /// Create a blocking talk step (no specific actor — uses scene default)
    pub fn talk(audio_id: &str) -> Self {
        Self {
            condition: Condition::Always,
            action: Action::Talk { audio_id: audio_id.to_string(), actor_name: None },
            blocking: true,
            label: None,
            jump_to: None,
        }
    }

    /// Create a blocking talk step with a specific actor for lip-sync
    #[allow(dead_code)] // Used by destination script definitions
    pub fn talk_with_actor(audio_id: &str, actor: &str) -> Self {
        Self {
            condition: Condition::Always,
            action: Action::Talk { audio_id: audio_id.to_string(), actor_name: Some(actor.to_string()) },
            blocking: true,
            label: None,
            jump_to: None,
        }
    }

    /// Create a leave-to-world step
    pub fn leave() -> Self {
        Self {
            condition: Condition::Always,
            action: Action::LeaveToWorld,
            blocking: false,
            label: None,
            jump_to: None,
        }
    }

    /// Create a flag-setting step (instant, non-blocking)
    pub fn set_cache(flag: &str) -> Self {
        Self {
            condition: Condition::Always,
            action: Action::SetCache(flag.to_string()),
            blocking: false,
            label: None,
            jump_to: None,
        }
    }

    /// Create a cache-removal step
    pub fn remove_cache(flag: &str) -> Self {
        Self {
            condition: Condition::Always,
            action: Action::RemoveCache(flag.to_string()),
            blocking: false,
            label: None,
            jump_to: None,
        }
    }

    /// Create a permanent flag step
    pub fn set_stuff(flag: &str) -> Self {
        Self {
            condition: Condition::Always,
            action: Action::SetStuff(flag.to_string()),
            blocking: false,
            label: None,
            jump_to: None,
        }
    }

    /// Create a part-reward step
    pub fn give_part(part_id: u32) -> Self {
        Self {
            condition: Condition::Always,
            action: Action::GivePart(part_id),
            blocking: false,
            label: None,
            jump_to: None,
        }
    }

    /// Create a refuel step
    pub fn refuel() -> Self {
        Self {
            condition: Condition::Always,
            action: Action::Refuel,
            blocking: false,
            label: None,
            jump_to: None,
        }
    }

    /// Create a delay step (blocking)
    pub fn delay(ms: u32) -> Self {
        Self {
            condition: Condition::Always,
            action: Action::Delay(ms),
            blocking: true,
            label: None,
            jump_to: None,
        }
    }

    /// Create a conditional jump (non-blocking, just branches)
    pub fn branch(condition: Condition, target_label: &str) -> Self {
        Self {
            condition,
            action: Action::Nop,
            blocking: false,
            label: None,
            jump_to: Some(target_label.to_string()),
        }
    }

    /// Create a labeled marker (no-op, used as jump target)
    pub fn label(name: &str) -> Self {
        Self {
            condition: Condition::Always,
            action: Action::Nop,
            blocking: false,
            label: Some(name.to_string()),
            jump_to: None,
        }
    }

    /// Builder: add a label
    pub fn with_label(mut self, name: &str) -> Self {
        self.label = Some(name.to_string());
        self
    }

    /// Create an actor-animation step (blocking)
    pub fn play_anim(actor: &str, anim: &str) -> Self {
        Self {
            condition: Condition::Always,
            action: Action::PlayAnim {
                actor_name: actor.to_string(),
                anim_name: anim.to_string(),
            },
            blocking: true,
            label: None,
            jump_to: None,
        }
    }

    /// Create set-actor-visible step (instant)
    pub fn actor_visible(actor: &str, visible: bool) -> Self {
        Self {
            condition: Condition::Always,
            action: Action::SetActorVisible {
                actor_name: actor.to_string(),
                visible,
            },
            blocking: false,
            label: None,
            jump_to: None,
        }
    }
}

/// A running scene script instance
pub struct SceneScript {
    pub steps: Vec<ScriptStep>,
    pub current_step: usize,
    /// Waiting for a dialog to finish (keyed by audio_id)
    pub waiting_for_dialog: Option<String>,
    /// Waiting for an animation to finish (keyed by actor_name)
    pub waiting_for_anim: Option<String>,
    /// Delay timer remaining (ms)
    pub delay_remaining: u32,
    /// Whether the script has completed
    pub finished: bool,
    /// Label → step index (built on creation)
    label_map: HashMap<String, usize>,
}

/// Requests generated by the script that the game state must fulfill
#[derive(Debug, Clone)]
pub enum ScriptRequest {
    Talk { audio_id: String, actor_name: Option<String> },
    PlayAnim { actor_name: String, anim_name: String },
    SetCache(String),
    RemoveCache(String),
    SetStuff(String),
    GivePart(u32),
    Refuel,
    SetActorVisible { actor_name: String, visible: bool },
    LeaveToWorld,
}

/// Context needed to evaluate conditions
pub struct ScriptContext<'a> {
    pub cache: &'a [String],
    pub permanent: &'a [String],
    pub car_parts: &'a [u32],
}

impl SceneScript {
    pub fn new(steps: Vec<ScriptStep>) -> Self {
        let mut label_map = HashMap::new();
        for (i, step) in steps.iter().enumerate() {
            if let Some(lbl) = &step.label {
                label_map.insert(lbl.clone(), i);
            }
        }
        Self {
            steps,
            current_step: 0,
            waiting_for_dialog: None,
            waiting_for_anim: None,
            delay_remaining: 0,
            finished: false,
            label_map,
        }
    }

    /// Check if the script is waiting for something
    pub fn is_waiting(&self) -> bool {
        self.waiting_for_dialog.is_some()
            || self.waiting_for_anim.is_some()
            || self.delay_remaining > 0
    }

    /// Notify the script that a dialog finished
    pub fn on_dialog_finished(&mut self, audio_id: &str) {
        if let Some(ref waiting) = self.waiting_for_dialog {
            if waiting == audio_id {
                self.waiting_for_dialog = None;
            }
        }
    }

    /// Notify the script that an animation finished
    pub fn on_anim_finished(&mut self, actor_name: &str) {
        if let Some(ref waiting) = self.waiting_for_anim {
            if waiting == actor_name {
                self.waiting_for_anim = None;
            }
        }
    }

    /// Advance time for delay steps
    pub fn tick(&mut self, dt_ms: u32) {
        if self.delay_remaining > 0 {
            self.delay_remaining = self.delay_remaining.saturating_sub(dt_ms);
        }
    }

    /// Try to advance the script, returning any requests.
    /// Call this every frame — it will process instant steps immediately
    /// and block on dialog/anim/delay steps.
    pub fn advance(&mut self, ctx: &ScriptContext<'_>) -> Vec<ScriptRequest> {
        let mut requests = Vec::new();

        // Don't advance if waiting for something
        if self.is_waiting() || self.finished {
            return requests;
        }

        // Process steps (may process multiple instant steps in one frame)
        while self.current_step < self.steps.len() && !self.is_waiting() {
            let step = &self.steps[self.current_step];

            // Check condition
            if !evaluate_condition(&step.condition, ctx) {
                // Condition not met — skip this step
                self.current_step += 1;
                continue;
            }

            // Check for jump
            if let Some(ref target) = step.jump_to {
                if let Some(&idx) = self.label_map.get(target) {
                    self.current_step = idx;
                    continue;
                } else {
                    tracing::warn!("SceneScript: unknown label '{}'", target);
                    self.current_step += 1;
                    continue;
                }
            }

            // Execute the action
            match &step.action {
                Action::Talk { audio_id, actor_name } => {
                    requests.push(ScriptRequest::Talk {
                        audio_id: audio_id.clone(),
                        actor_name: actor_name.clone(),
                    });
                    if step.blocking {
                        self.waiting_for_dialog = Some(audio_id.clone());
                    }
                }
                Action::PlayAnim { actor_name, anim_name } => {
                    requests.push(ScriptRequest::PlayAnim {
                        actor_name: actor_name.clone(),
                        anim_name: anim_name.clone(),
                    });
                    if step.blocking {
                        self.waiting_for_anim = Some(actor_name.clone());
                    }
                }
                Action::SetCache(flag) => {
                    requests.push(ScriptRequest::SetCache(flag.clone()));
                }
                Action::RemoveCache(flag) => {
                    requests.push(ScriptRequest::RemoveCache(flag.clone()));
                }
                Action::SetStuff(flag) => {
                    requests.push(ScriptRequest::SetStuff(flag.clone()));
                }
                Action::GivePart(part_id) => {
                    requests.push(ScriptRequest::GivePart(*part_id));
                }
                Action::Refuel => {
                    requests.push(ScriptRequest::Refuel);
                }
                Action::SetActorVisible { actor_name, visible } => {
                    requests.push(ScriptRequest::SetActorVisible {
                        actor_name: actor_name.clone(),
                        visible: *visible,
                    });
                }
                Action::Delay(ms) => {
                    self.delay_remaining = *ms;
                }
                Action::LeaveToWorld => {
                    requests.push(ScriptRequest::LeaveToWorld);
                }
                Action::Nop => {}
            }

            self.current_step += 1;
        }

        // Check if script is done
        if self.current_step >= self.steps.len() && !self.is_waiting() {
            self.finished = true;
        }

        requests
    }
}

fn evaluate_condition(cond: &Condition, ctx: &ScriptContext<'_>) -> bool {
    match cond {
        Condition::Always => true,
        Condition::HasCache(flag) => ctx.cache.iter().any(|f| f == flag),
        Condition::HasStuff(flag) => ctx.permanent.iter().any(|f| f == flag),
        Condition::NotStuff(flag) => !ctx.permanent.iter().any(|f| f == flag),
        Condition::HasPart(id) => ctx.car_parts.contains(id),
        Condition::NotPart(id) => !ctx.car_parts.contains(id),
    }
}

// ─── Destination script definitions ──────────────────────────────────────

/// Build the script for a given destination scene number
pub fn build_destination_script(dest_num: u8) -> Option<SceneScript> {
    let steps = match dest_num {
        84 => script_road_thing(),
        85 => script_road_dog(),
        86 => script_solhem(),
        87 => script_saftfabrik(),
        88 => script_sture(),
        89 => script_gas_station(),
        92 => script_figge(),
        _ => return None,
    };
    Some(SceneScript::new(steps))
}

/// Destination 84 — RoadThing (linear: give part → talk → leave)
/// Mulle finds a car part on the road. Part 287 default, #RoadThing1 flag.
fn script_road_thing() -> Vec<ScriptStep> {
    vec![
        ScriptStep::give_part(287),
        ScriptStep::set_cache("#RoadThing1"),
        ScriptStep::talk("84d001v0"),
        ScriptStep::delay(2000),
        ScriptStep::leave(),
    ]
}

/// Destination 85 — Road Dog (linear)
/// Mulle finds Salka → sets #GotDogOnce, #Dog → back to world
fn script_road_dog() -> Vec<ScriptStep> {
    vec![
        ScriptStep::talk("85d002v0"),               // "Oh Salka, du hast dich verlaufen..."
        ScriptStep::set_cache("#GotDogOnce"),
        ScriptStep::set_cache("#Dog"),
        ScriptStep::delay(500),
        ScriptStep::leave(),
    ]
}

/// Destination 92 — Figge Ferrum (branching on #ExtraTank, #Dog)
fn script_figge() -> Vec<ScriptStep> {
    vec![
        // If already got extra tank → short revisit dialog → leave
        ScriptStep::branch(Condition::HasCache("#ExtraTank".into()), "revisit"),

        // First visit: Figge asks about Salka
        ScriptStep::talk("92d002v0"),                // Figge: "Salka ist weggelaufen..."

        // Branch on having the dog
        ScriptStep::branch(Condition::HasCache("#Dog".into()), "has_dog"),

        // No dog → Mulle says no → leave
        ScriptStep::talk("92d003v0"),                // Mulle: "Nein, hab ihn nicht gesehen"
        ScriptStep::delay(500),
        ScriptStep::leave().with_label("end"),

        // Has dog branch
        ScriptStep::label("has_dog"),
        ScriptStep::talk("92d004v0"),                // Mulle: "Ja klar!"
        ScriptStep::talk("92d005v0"),                // Figge: "Danke, Mulle"
        ScriptStep::talk("92d006v0"),                // Mulle: "Danke schön"
        ScriptStep::set_cache("#ExtraTank"),
        ScriptStep::remove_cache("#Dog"),
        ScriptStep::refuel(),                        // Tank full as reward
        ScriptStep::delay(500),
        ScriptStep::leave(),

        // Revisit branch
        ScriptStep::label("revisit"),
        ScriptStep::talk("92d007v0"),                // Mulle revisit talk
        ScriptStep::delay(500),
        ScriptStep::leave(),
    ]
}

/// Destination 87 — Saftfabrik (branching on Part 172 = Tank)
fn script_saftfabrik() -> Vec<ScriptStep> {
    vec![
        ScriptStep::talk("87d002v0"),                // Garson: "Wir haben kein Saft mehr"

        // Branch if car has tank (part 172)
        ScriptStep::branch(Condition::HasPart(172), "has_tank"),

        // No tank → sorry dialog
        ScriptStep::talk("87d003v0"),                // Mulle: "Naja..."
        ScriptStep::delay(1000),
        ScriptStep::leave(),

        // Has tank → fill with lemonade
        ScriptStep::label("has_tank"),
        ScriptStep::talk("87d004v0"),                // Mulle: "Na klar!"
        // (splash animation + sound would play here)
        ScriptStep::talk("87d005v0"),                // Garson: instructions
        ScriptStep::talk("87d006v0"),                // Mulle: "Verstanden"
        ScriptStep::set_cache("#Lemonade"),
        ScriptStep::delay(1000),
        ScriptStep::leave(),
    ]
}

/// Destination 88 — Sture Stortand (branching on #Lemonade + Part 172)
fn script_sture() -> Vec<ScriptStep> {
    vec![
        // If has lemonade → delivery branch
        ScriptStep::branch(Condition::HasCache("#Lemonade".into()), "has_lemonade"),

        // No lemonade: Sture is sad
        ScriptStep::talk("88d002v0"),                // Sture: "Wir haben ein Problem"

        // Sub-branch on part 172 (tank)
        ScriptStep::branch(Condition::NotPart(172), "no_tank"),

        // Has tank but no lemonade
        ScriptStep::talk("88d004v0"),                // Mulle: "Natürlich"
        ScriptStep::delay(1000),
        ScriptStep::leave(),

        // No tank either
        ScriptStep::label("no_tank"),
        ScriptStep::talk("88d003v0"),                // Mulle: "Ich kann versuchen zu helfen"
        ScriptStep::delay(1000),
        ScriptStep::leave(),

        // Delivery branch — Sture is happy
        ScriptStep::label("has_lemonade"),
        ScriptStep::talk("88d005v0"),                // Sture: "Danke, mehr Saft!"
        ScriptStep::talk("88d006v0"),                // Mulle: "Aber gerne"
        ScriptStep::remove_cache("#Lemonade"),
        ScriptStep::give_part(162),                  // Reward: part 162 → yard
        ScriptStep::delay(1000),
        ScriptStep::leave(),
    ]
}

/// Destination 86 — Solhem / Mia (branching on #FerryTicket + Part 173)
fn script_solhem() -> Vec<ScriptStep> {
    vec![
        // Already completed → revisit
        ScriptStep::branch(Condition::HasStuff("#FerryTicket".into()), "revisit"),

        // First visit: Mia asks for help
        ScriptStep::talk("86d002v0"),                // Mia: "Oh gut, dass du kommst"

        // Branch on ladder (part 173)
        ScriptStep::branch(Condition::HasPart(173), "has_ladder"),

        // No ladder
        ScriptStep::talk("86d003v0"),                // Mulle: "Naja..."
        ScriptStep::delay(500),
        ScriptStep::leave(),

        // Has ladder → cat rescue sequence
        ScriptStep::label("has_ladder"),
        ScriptStep::talk("86d004v0"),                // Mulle: "Na klar!"
        // Cat jump animation would play here
        ScriptStep::play_anim("cat", "jump1"),
        ScriptStep::play_anim("cat", "jump2"),
        ScriptStep::actor_visible("cat", false),
        ScriptStep::talk("86d005v0"),                // Mia: "Danke! Hier ist ein Fährticket"
        ScriptStep::set_stuff("#FerryTicket"),
        ScriptStep::talk("86d006v0"),                // Mulle: "Danke"
        ScriptStep::delay(500),
        ScriptStep::leave(),

        // Revisit
        ScriptStep::label("revisit"),
        ScriptStep::talk("86d007v0"),                // Mulle: "Hab Mia schon geholfen"
        ScriptStep::delay(500),
        ScriptStep::leave(),
    ]
}

/// Destination 89 — Gas station (simple refuel)
fn script_gas_station() -> Vec<ScriptStep> {
    vec![
        ScriptStep::refuel(),
        ScriptStep::delay(1000),
        ScriptStep::leave(),
    ]
}

// ─── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_ctx() -> ScriptContext<'static> {
        ScriptContext {
            cache: &[],
            permanent: &[],
            car_parts: &[],
        }
    }

    #[test]
    fn road_dog_linear() {
        let mut script = build_destination_script(85).unwrap();
        let ctx = empty_ctx();

        let reqs = script.advance(&ctx);
        // First request: Talk
        assert!(reqs.iter().any(|r| matches!(r, ScriptRequest::Talk { audio_id, .. } if audio_id == "85d002v0")));
        // Script should be waiting for dialog
        assert!(script.is_waiting());

        // Signal dialog finished
        script.on_dialog_finished("85d002v0");
        assert!(!script.is_waiting());

        // Advance — should set cache flags, delay, and eventually leave
        let reqs = script.advance(&ctx);
        assert!(reqs.iter().any(|r| matches!(r, ScriptRequest::SetCache(f) if f == "#GotDogOnce")));
        assert!(reqs.iter().any(|r| matches!(r, ScriptRequest::SetCache(f) if f == "#Dog")));
        // Now waiting for delay
        assert!(script.is_waiting());

        script.tick(500);
        let reqs = script.advance(&ctx);
        assert!(reqs.iter().any(|r| matches!(r, ScriptRequest::LeaveToWorld)));
        assert!(script.finished);
    }

    #[test]
    fn figge_no_dog() {
        let mut script = build_destination_script(92).unwrap();
        let ctx = empty_ctx(); // no #Dog, no #ExtraTank

        let reqs = script.advance(&ctx);
        // Should start with Figge's dialog (92d002v0)
        assert!(reqs.iter().any(|r| matches!(r, ScriptRequest::Talk { audio_id, .. } if audio_id == "92d002v0")));

        script.on_dialog_finished("92d002v0");
        let reqs = script.advance(&ctx);
        // No dog → 92d003v0
        assert!(reqs.iter().any(|r| matches!(r, ScriptRequest::Talk { audio_id, .. } if audio_id == "92d003v0")));
    }

    #[test]
    fn figge_with_dog() {
        let mut script = build_destination_script(92).unwrap();
        let cache = vec!["#Dog".to_string()];
        let ctx = ScriptContext {
            cache: &cache,
            permanent: &[],
            car_parts: &[],
        };

        let reqs = script.advance(&ctx);
        // First: Figge's dialog
        assert!(reqs.iter().any(|r| matches!(r, ScriptRequest::Talk { audio_id, .. } if audio_id == "92d002v0")));
        script.on_dialog_finished("92d002v0");

        let reqs = script.advance(&ctx);
        // Has dog → jumps to has_dog → 92d004v0
        assert!(reqs.iter().any(|r| matches!(r, ScriptRequest::Talk { audio_id, .. } if audio_id == "92d004v0")));
    }

    #[test]
    fn figge_revisit() {
        let mut script = build_destination_script(92).unwrap();
        let cache = vec!["#ExtraTank".to_string()];
        let ctx = ScriptContext {
            cache: &cache,
            permanent: &[],
            car_parts: &[],
        };

        let reqs = script.advance(&ctx);
        // Has #ExtraTank → jumps to revisit → 92d007v0
        assert!(reqs.iter().any(|r| matches!(r, ScriptRequest::Talk { audio_id, .. } if audio_id == "92d007v0")));
    }

    #[test]
    fn saftfabrik_no_tank() {
        let mut script = build_destination_script(87).unwrap();
        let ctx = empty_ctx();

        let reqs = script.advance(&ctx);
        assert!(reqs.iter().any(|r| matches!(r, ScriptRequest::Talk { audio_id, .. } if audio_id == "87d002v0")));
        script.on_dialog_finished("87d002v0");

        let reqs = script.advance(&ctx);
        // No part 172 → 87d003v0
        assert!(reqs.iter().any(|r| matches!(r, ScriptRequest::Talk { audio_id, .. } if audio_id == "87d003v0")));
    }

    #[test]
    fn saftfabrik_with_tank() {
        let mut script = build_destination_script(87).unwrap();
        let ctx = ScriptContext {
            cache: &[],
            permanent: &[],
            car_parts: &[172],
        };

        let reqs = script.advance(&ctx);
        assert!(reqs.iter().any(|r| matches!(r, ScriptRequest::Talk { audio_id, .. } if audio_id == "87d002v0")));
        script.on_dialog_finished("87d002v0");

        let reqs = script.advance(&ctx);
        // Has part 172 → 87d004v0
        assert!(reqs.iter().any(|r| matches!(r, ScriptRequest::Talk { audio_id, .. } if audio_id == "87d004v0")));
    }

    #[test]
    fn sture_with_lemonade() {
        let mut script = build_destination_script(88).unwrap();
        let cache = vec!["#Lemonade".to_string()];
        let ctx = ScriptContext {
            cache: &cache,
            permanent: &[],
            car_parts: &[],
        };

        let reqs = script.advance(&ctx);
        // Has #Lemonade → happy branch → 88d005v0
        assert!(reqs.iter().any(|r| matches!(r, ScriptRequest::Talk { audio_id, .. } if audio_id == "88d005v0")));
    }

    #[test]
    fn solhem_revisit() {
        let mut script = build_destination_script(86).unwrap();
        let perm = vec!["#FerryTicket".to_string()];
        let ctx = ScriptContext {
            cache: &[],
            permanent: &perm,
            car_parts: &[],
        };

        let reqs = script.advance(&ctx);
        // Already has ticket → revisit → 86d007v0
        assert!(reqs.iter().any(|r| matches!(r, ScriptRequest::Talk { audio_id, .. } if audio_id == "86d007v0")));
    }

    #[test]
    fn gas_station_refuels() {
        let mut script = build_destination_script(89).unwrap();
        let ctx = empty_ctx();

        let reqs = script.advance(&ctx);
        assert!(reqs.iter().any(|r| matches!(r, ScriptRequest::Refuel)));
        // Delay active
        assert!(script.is_waiting());
        script.tick(1000);
        let reqs = script.advance(&ctx);
        assert!(reqs.iter().any(|r| matches!(r, ScriptRequest::LeaveToWorld)));
    }
}
