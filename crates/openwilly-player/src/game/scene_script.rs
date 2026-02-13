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
    /// Play a sound effect (non-blocking, fire-and-forget)
    PlaySound(String),
    /// Leave the scene (go back to world map)
    LeaveToWorld,
    /// Change an actor's talk/silence animation pair mid-script
    SetTalkAnims {
        actor_name: String,
        talk_anim: String,
        silence_anim: String,
    },
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

    /// Create a sound-effect step (non-blocking, fire-and-forget)
    pub fn play_sound(sound_id: &str) -> Self {
        Self {
            condition: Condition::Always,
            action: Action::PlaySound(sound_id.to_string()),
            blocking: false,
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

    /// Create set-talk-anims step (instant) — changes an actor's lip-sync animations
    pub fn set_talk_anims(actor: &str, talk: &str, silence: &str) -> Self {
        Self {
            condition: Condition::Always,
            action: Action::SetTalkAnims {
                actor_name: actor.to_string(),
                talk_anim: talk.to_string(),
                silence_anim: silence.to_string(),
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
    SetTalkAnims { actor_name: String, talk_anim: String, silence_anim: String },
    PlaySound(String),
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
                Action::SetTalkAnims { actor_name, talk_anim, silence_anim } => {
                    requests.push(ScriptRequest::SetTalkAnims {
                        actor_name: actor_name.clone(),
                        talk_anim: talk_anim.clone(),
                        silence_anim: silence_anim.clone(),
                    });
                }
                Action::Delay(ms) => {
                    self.delay_remaining = *ms;
                }
                Action::PlaySound(sound_id) => {
                    requests.push(ScriptRequest::PlaySound(sound_id.clone()));
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

// ─── CarShow script (94.DXR) ─────────────────────────────────────────

/// Compute the car show rating (1–5 stars) from funny_factor.
/// Thresholds from mulle.js carshow.js:
///   ff < 2 → 1, ff < 3 → 2, ff < 5 → 3, ff < 7 → 4, else → 5
pub fn carshow_rating(funny_factor: i32) -> u8 {
    if funny_factor < 2 { 1 }
    else if funny_factor < 3 { 2 }
    else if funny_factor < 5 { 3 }
    else if funny_factor < 7 { 4 }
    else { 5 }
}

/// Build the CarShow scene script. `funny_factor` comes from `CarProperties`.
///
/// Sequence (from mulle.js carshow.js):
// ---------------------------------------------------------------------------
// Menu intro script
// ---------------------------------------------------------------------------

/// Build the menu intro script: jingle → ambient + Mulle greeting with lip-sync.
///
/// Sequence (from mulle.js menu.js):
/// 1. Play intro jingle (10e001v0), wait for it to finish
/// 2. Start ambient sound (10e002v0, one-shot — NOT looped)
/// 3. Mulle speaks greeting (11d001v0) with lip-sync on mulleMenuMouth
pub fn build_menu_script(jingle_duration_ms: u32) -> SceneScript {
    SceneScript::new(vec![
        // 1. Intro jingle — fire-and-forget, then wait for its duration
        ScriptStep::play_sound("10e001v0"),
        ScriptStep::delay(jingle_duration_ms.max(500)),
        // 2. Ambient sound — one-shot (mulle.js: playAudio, not loop)
        ScriptStep::play_sound("10e002v0"),
        // 3. Mulle greeting with lip-sync on mouth actor
        ScriptStep::talk_with_actor("11d001v0", "mulleMenuMouth"),
    ])
}

// ---------------------------------------------------------------------------
// Car show script
// ---------------------------------------------------------------------------

/// 1. Judge talks greeting (94d003v0) with talk/idle anims
/// 2. Judge plays raiseScore animation
/// 3. Score sprite appears (actor "score", hidden → visible)
/// 4. Judge switches to talkScore/idleScore anims
/// 5. Judge speaks rating dialog (94d004v0=5★ … 94d008v0=1★)
/// 6. Delay, then leave to world
pub fn build_carshow_script(funny_factor: i32) -> SceneScript {
    let rating = carshow_rating(funny_factor);
    // Audio IDs: rating 5→94d004v0, 4→94d005v0, 3→94d006v0, 2→94d007v0, 1→94d008v0
    let rating_audio = format!("94d{:03}v0", 4 + (5 - rating) as u32);

    let steps = vec![
        // 1. Judge greets
        ScriptStep::talk_with_actor("94d003v0", "judge"),

        // 2. Judge raises score board
        ScriptStep::play_anim("judge", "raiseScore"),

        // 3. Show the score sprite
        ScriptStep::actor_visible("score", true),

        // 4. Switch judge talk anims to score mode
        ScriptStep::set_talk_anims("judge", "talkScore", "idleScore"),

        // 5. Judge announces rating
        ScriptStep::talk_with_actor(&rating_audio, "judge"),

        // 6. Brief pause, then leave
        ScriptStep::delay(2000),
        ScriptStep::leave(),
    ];

    SceneScript::new(steps)
}

// ─── Figge garage cutscene ───────────────────────────────────────────────

/// Build the Figge-in-garage cutscene script.
///
/// Sequence (from mulle.js garage.js):
/// 1. Car sound (Figge drives up) → 03e009v0
/// 2. Narrator comment → 03d043v0
/// 3. Door opening sound → 02e016v0
/// 4. Figge enter animation
/// 5. Figge greeting dialog → 03d044v0
/// 6. Mulle looks left, then responds → 03d045v0
/// 7. Figge farewell dialog → 03d046v0
/// 8. Figge exit animation
/// 9. Door closing sound → 02e015v0
/// 10. Car departure sound → 03e010v0
///
/// Parts are given separately by the game state (not in this script).
pub fn build_figge_script() -> SceneScript {
    let steps = vec![
        // Figge drives up
        ScriptStep::play_sound("03e009v0"),
        ScriptStep::delay(500),

        // Narrator announcement
        ScriptStep::talk("03d043v0"),

        // Door opens — show Figge
        ScriptStep::play_sound("02e016v0"),
        ScriptStep::actor_visible("figge", true),
        ScriptStep::play_anim("figge", "enter"),

        // Figge greets
        ScriptStep::talk_with_actor("03d044v0", "figge"),

        // Mulle looks left (toward Figge), responds
        ScriptStep::play_anim("mulleDefault", "lookLeft"),
        ScriptStep::set_talk_anims("mulleDefault", "talkPlayer", "lookLeft"),
        ScriptStep::talk_with_actor("03d045v0", "mulleDefault"),

        // Figge farewell
        ScriptStep::talk_with_actor("03d046v0", "figge"),

        // Figge exits
        ScriptStep::play_anim("figge", "exit"),

        // Cleanup — hide Figge, door close, car departs
        ScriptStep::actor_visible("figge", false),
        ScriptStep::play_sound("02e015v0"),
        ScriptStep::delay(300),
        ScriptStep::play_sound("03e010v0"),
        ScriptStep::delay(500),

        // Restore Mulle to idle
        ScriptStep::set_talk_anims("mulleDefault", "talkPlayer", "lookPlayer"),
        ScriptStep::play_anim("mulleDefault", "lookPlayer"),
    ];

    SceneScript::new(steps)
}

// ─── Destination script definitions ──────────────────────────────────────

/// Build the script for a given destination scene number
pub fn build_destination_script(dest_num: u8) -> Option<SceneScript> {
    let steps = match dest_num {
        82 => script_mud_car(),
        83 => script_tree_in_road(),
        84 => script_road_thing(),
        85 => script_road_dog(),
        86 => script_solhem(),
        87 => script_saftfabrik(),
        88 => script_sture(),
        89 => script_gas_station(),
        90 => script_doris_digital(),
        91 => script_luddel_abb(),
        92 => script_figge(),
        93 => script_ocean(),
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
        ScriptStep::set_stuff("#FiggeIsComing"),     // Figge will deliver parts to garage
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

/// Destination 82 — MudCar (random dest, linear: talk → give random part → leave)
/// Mulle rescues a car stuck in the mud. Flags: #MudCar, #RescuedMudCar.
/// Note: not implemented in mulle.js either — follows RoadThing pattern.
fn script_mud_car() -> Vec<ScriptStep> {
    vec![
        ScriptStep::talk("82d001v0"),                // Mulle: "Oh, ein Auto im Schlamm!"
        ScriptStep::set_cache("#MudCar"),
        ScriptStep::set_cache("#RescuedMudCar"),
        ScriptStep::delay(500),
        ScriptStep::leave(),
    ]
}

/// Destination 83 — TreeInRoad (random dest, linear: talk → give random part → leave)
/// Mulle clears a tree from the road. Flag: #TreeInRoad.
/// Note: not implemented in mulle.js either — follows RoadThing pattern.
fn script_tree_in_road() -> Vec<ScriptStep> {
    vec![
        ScriptStep::talk("83d001v0"),                // Mulle: "Ein Baum auf der Straße!"
        ScriptStep::set_cache("#TreeInRoad"),
        ScriptStep::delay(500),
        ScriptStep::leave(),
    ]
}

/// Destination 90 — Doris Digital (NPC, gives part 306, mission 4)
/// Doris runs a computer shop. Part 306 = keyboard? as reward.
fn script_doris_digital() -> Vec<ScriptStep> {
    vec![
        // Already visited → revisit dialog
        ScriptStep::branch(Condition::HasCache("#DorisVisited".into()), "revisit"),

        // First visit
        ScriptStep::talk("90d002v0"),                // Doris: greeting
        ScriptStep::talk("90d003v0"),                // Doris: "Hier, die kannst du haben"
        ScriptStep::give_part(306),                  // Part 306 → yard
        ScriptStep::set_cache("#DorisVisited"),
        // TODO: Complete mission 4 when mission system is integrated
        ScriptStep::delay(500),
        ScriptStep::leave(),

        // Revisit
        ScriptStep::label("revisit"),
        ScriptStep::talk("90d004v0"),                // "Schön dich zu sehen!"
        ScriptStep::delay(500),
        ScriptStep::leave(),
    ]
}

/// Destination 91 — Luddel Abb (NPC blacksmith, gives part 99, mission 6)
/// Luddel runs a forge. Part 99 = metal part as reward.
fn script_luddel_abb() -> Vec<ScriptStep> {
    vec![
        // Already visited → revisit dialog
        ScriptStep::branch(Condition::HasCache("#LuddelVisited".into()), "revisit"),

        // First visit
        ScriptStep::talk("91d002v0"),                // Luddel: greeting
        ScriptStep::talk("91d003v0"),                // Luddel: "Hier, das habe ich für dich"
        ScriptStep::give_part(99),                   // Part 99 → yard
        ScriptStep::set_cache("#LuddelVisited"),
        // TODO: Complete mission 6 when mission system is integrated
        ScriptStep::delay(500),
        ScriptStep::leave(),

        // Revisit
        ScriptStep::label("revisit"),
        ScriptStep::talk("91d004v0"),                // Revisit talk
        ScriptStep::delay(500),
        ScriptStep::leave(),
    ]
}

/// Destination 93 — Ocean/Hafen (gives part 54)
/// Mulle visits the harbor and finds a part.
fn script_ocean() -> Vec<ScriptStep> {
    vec![
        // Already visited → revisit dialog
        ScriptStep::branch(Condition::HasCache("#OceanVisited".into()), "revisit"),

        // First visit
        ScriptStep::talk("93d002v0"),                // Harbor talk
        ScriptStep::give_part(54),                   // Part 54 → yard
        ScriptStep::set_cache("#OceanVisited"),
        ScriptStep::delay(500),
        ScriptStep::leave(),

        // Revisit
        ScriptStep::label("revisit"),
        ScriptStep::talk("93d003v0"),                // Revisit dialog
        ScriptStep::delay(500),
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

    #[test]
    fn carshow_rating_thresholds() {
        assert_eq!(carshow_rating(0), 1);
        assert_eq!(carshow_rating(1), 1);
        assert_eq!(carshow_rating(2), 2);
        assert_eq!(carshow_rating(3), 3);
        assert_eq!(carshow_rating(4), 3);
        assert_eq!(carshow_rating(5), 4);
        assert_eq!(carshow_rating(6), 4);
        assert_eq!(carshow_rating(7), 5);
        assert_eq!(carshow_rating(10), 5);
    }

    #[test]
    fn carshow_script_sequence() {
        let mut script = build_carshow_script(0); // rating 1 → audio 94d008v0
        let ctx = empty_ctx();

        // Step 1: Judge greeting
        let reqs = script.advance(&ctx);
        assert!(reqs.iter().any(|r| matches!(r, ScriptRequest::Talk { audio_id, .. } if audio_id == "94d003v0")));
        assert!(script.is_waiting());

        script.on_dialog_finished("94d003v0");

        // Step 2: raiseScore animation
        let reqs = script.advance(&ctx);
        assert!(reqs.iter().any(|r| matches!(r, ScriptRequest::PlayAnim { actor_name, anim_name }
            if actor_name == "judge" && anim_name == "raiseScore")));
        assert!(script.is_waiting());

        script.on_anim_finished("judge");

        // Steps 3-5: show score, set talk anims, talk rating
        let reqs = script.advance(&ctx);
        assert!(reqs.iter().any(|r| matches!(r, ScriptRequest::SetActorVisible { actor_name, visible }
            if actor_name == "score" && *visible)));
        assert!(reqs.iter().any(|r| matches!(r, ScriptRequest::SetTalkAnims { actor_name, talk_anim, silence_anim }
            if actor_name == "judge" && talk_anim == "talkScore" && silence_anim == "idleScore")));
        assert!(reqs.iter().any(|r| matches!(r, ScriptRequest::Talk { audio_id, .. } if audio_id == "94d008v0")));

        script.on_dialog_finished("94d008v0");

        // Step 6-7: delay → leave
        let _reqs = script.advance(&ctx);
        assert!(script.is_waiting()); // delay
        script.tick(2000);
        let reqs = script.advance(&ctx);
        assert!(reqs.iter().any(|r| matches!(r, ScriptRequest::LeaveToWorld)));
        assert!(script.finished);
    }

    #[test]
    fn carshow_five_star_audio() {
        let script = build_carshow_script(10); // rating 5 → audio 94d004v0
        // Check that rating 5 uses 94d004v0
        let has_five_star = script.steps.iter().any(|s| matches!(&s.action,
            Action::Talk { audio_id, .. } if audio_id == "94d004v0"));
        assert!(has_five_star);
    }
}
