//! Quest / Dialog system
//!
//! Based on mulle.js MulleActor.talk(), MulleSubtitle, and cache-flag quest system:
//!   - Dialog is audio playback + subtitle text at screen bottom
//!   - Quests use cache flags on the car (e.g. "#Dog", "#ExtraTank")
//!   - Missions are defined in missions.hash.json (8 missions)
//!   - Scene-specific dialog chains use callback sequences

use std::collections::{HashMap, HashSet};

use crate::assets::director::CuePoint;
use crate::engine::sound_engine::PlaybackHandle;

// ---------------------------------------------------------------------------
// Dialog system
// ---------------------------------------------------------------------------

/// A single subtitle line
#[derive(Debug, Clone)]
pub struct SubtitleLine {
    /// The text to display (may contain {highlighted} words)
    pub text: String,
    /// Speaker name (e.g. "mulle", "figge")
    pub speaker: String,
    /// Duration in milliseconds (auto-computed from text length)
    pub duration_ms: u32,
}

impl SubtitleLine {
    pub fn new(text: &str, speaker: &str) -> Self {
        // Duration formula from mulle.js: 1000 * Math.log(text.length)
        let duration = (1000.0 * (text.len() as f32).max(2.0).ln()) as u32;
        Self {
            text: text.to_string(),
            speaker: speaker.to_string(),
            duration_ms: duration.max(500), // minimum 500ms
        }
    }

    /// Extract highlighted words (wrapped in {braces})
    pub fn highlighted_words(&self) -> Vec<String> {
        let mut words = Vec::new();
        let mut in_brace = false;
        let mut current = String::new();
        for ch in self.text.chars() {
            match ch {
                '{' => {
                    in_brace = true;
                    current.clear();
                }
                '}' => {
                    if in_brace {
                        words.push(current.clone());
                        in_brace = false;
                    }
                }
                _ => {
                    if in_brace {
                        current.push(ch);
                    }
                }
            }
        }
        words
    }

    /// Get plain text (without brace markers)
    pub fn plain_text(&self) -> String {
        self.text.replace('{', "").replace('}', "")
    }
}

/// A dialog sequence — multiple lines played in order
#[derive(Debug, Clone)]
pub struct DialogSequence {
    /// Audio member name (e.g. "03d012v0")
    pub audio_id: String,
    /// Subtitle lines
    pub lines: Vec<SubtitleLine>,
    /// Current line index
    pub current_line: usize,
    /// Elapsed time on current line (ms)
    pub elapsed_ms: u32,
    /// Whether the dialog has finished
    pub finished: bool,
}

impl DialogSequence {
    pub fn new(audio_id: &str, lines: Vec<SubtitleLine>) -> Self {
        Self {
            audio_id: audio_id.to_string(),
            lines,
            current_line: 0,
            elapsed_ms: 0,
            finished: false,
        }
    }

    /// Advance the dialog by `dt_ms` milliseconds. Returns true if line changed.
    pub fn advance(&mut self, dt_ms: u32) -> bool {
        if self.finished || self.lines.is_empty() {
            return false;
        }

        self.elapsed_ms += dt_ms;
        let line = &self.lines[self.current_line];

        if self.elapsed_ms >= line.duration_ms {
            self.elapsed_ms = 0;
            self.current_line += 1;
            if self.current_line >= self.lines.len() {
                self.finished = true;
            }
            return true;
        }
        false
    }

    /// Get the current subtitle to display
    pub fn current_subtitle(&self) -> Option<&SubtitleLine> {
        if self.finished {
            None
        } else {
            self.lines.get(self.current_line)
        }
    }

    /// Skip to the end
    pub fn skip(&mut self) {
        self.finished = true;
        self.current_line = self.lines.len();
    }
}

/// Events emitted by the dialog manager
#[derive(Debug, Clone)]
pub enum DialogEvent {
    /// A dialog sequence finished playing
    DialogFinished {
        /// The audio_id of the dialog that just ended
        audio_id: String,
    },
    /// The entire queue is now empty (no more dialogs)
    QueueEmpty,
    /// A cue point was reached during audio playback (for lip-sync)
    CuePoint {
        /// The audio_id of the playing dialog
        #[allow(dead_code)] // Included in event for handler use
        audio_id: String,
        /// The cue name: "talk", "silence", "point", etc.
        cue_name: String,
    },
}

/// Tracks cue-point playback for lip-sync (talk/silence animation switching)
struct CueTracker {
    /// Handle to the currently playing audio
    handle: PlaybackHandle,
    /// Cue points for this sound (sorted by time_ms)
    cue_points: Vec<CuePoint>,
    /// Indices of already-fired cue points
    completed: HashSet<usize>,
    /// Audio ID for events
    audio_id: String,
}

/// The dialog manager — handles active dialog and subtitle display
pub struct DialogManager {
    /// Currently playing dialog (if any)
    pub active_dialog: Option<DialogSequence>,
    /// Queued dialogs (played in order)
    pub queue: Vec<DialogSequence>,
    /// All registered subtitle data: audio_id → lines
    pub subtitle_db: HashMap<String, Vec<SubtitleLine>>,
    /// Active cue-point tracker (for lip-sync)
    cue_tracker: Option<CueTracker>,
}

impl DialogManager {
    pub fn new() -> Self {
        let mut mgr = Self {
            active_dialog: None,
            queue: Vec::new(),
            subtitle_db: HashMap::new(),
            cue_tracker: None,
        };
        mgr.register_default_subtitles();
        mgr
    }

    /// Register subtitle lines for an audio ID
    pub fn set_lines(&mut self, audio_id: &str, lines: Vec<SubtitleLine>) {
        self.subtitle_db.insert(audio_id.to_string(), lines);
    }

    /// Start a dialog (or queue it if one is playing)
    pub fn talk(&mut self, audio_id: &str) {
        let lines = self.subtitle_db
            .get(audio_id)
            .cloned()
            .unwrap_or_else(|| vec![SubtitleLine::new(&format!("[{}]", audio_id), "?")]);

        let seq = DialogSequence::new(audio_id, lines);

        if self.active_dialog.is_some() {
            self.queue.push(seq);
        } else {
            self.active_dialog = Some(seq);
        }
    }

    /// Set up cue-point tracking for the current dialog audio.
    /// Call this right after `talk()` when you have a PlaybackHandle and cue points.
    pub fn set_cue_tracking(&mut self, audio_id: &str, handle: PlaybackHandle, cue_points: Vec<CuePoint>) {
        if cue_points.is_empty() {
            return;
        }
        tracing::debug!("Cue-point tracking for '{}': {} cue points", audio_id, cue_points.len());
        self.cue_tracker = Some(CueTracker {
            handle,
            cue_points,
            completed: HashSet::new(),
            audio_id: audio_id.to_string(),
        });
    }

    /// Update the dialog manager (call every frame, dt_ms ≈ 33 for 30fps).
    /// Returns events for finished dialogs and cue points.
    pub fn update(&mut self, dt_ms: u32) -> Vec<DialogEvent> {
        let mut events = Vec::new();

        // Poll cue points against elapsed audio time
        if let Some(tracker) = &mut self.cue_tracker {
            let elapsed = tracker.handle.elapsed_ms();
            for (i, cp) in tracker.cue_points.iter().enumerate() {
                if !tracker.completed.contains(&i) && elapsed >= cp.time_ms {
                    tracker.completed.insert(i);
                    events.push(DialogEvent::CuePoint {
                        audio_id: tracker.audio_id.clone(),
                        cue_name: cp.name.clone(),
                    });
                }
            }
        }

        if let Some(dialog) = &mut self.active_dialog {
            dialog.advance(dt_ms);
            if dialog.finished {
                let audio_id = dialog.audio_id.clone();
                self.active_dialog = None;
                self.cue_tracker = None; // Clean up tracker when dialog finishes
                events.push(DialogEvent::DialogFinished { audio_id });
                // Start next queued dialog
                if !self.queue.is_empty() {
                    self.active_dialog = Some(self.queue.remove(0));
                } else {
                    events.push(DialogEvent::QueueEmpty);
                }
            }
        }
        events
    }

    /// Get the current subtitle text to render (if any)
    pub fn current_subtitle(&self) -> Option<&SubtitleLine> {
        self.active_dialog.as_ref().and_then(|d| d.current_subtitle())
    }

    /// Skip the current dialog
    pub fn skip_current(&mut self) {
        if let Some(dialog) = &mut self.active_dialog {
            dialog.skip();
        }
        self.active_dialog = None;
        self.cue_tracker = None;
        if !self.queue.is_empty() {
            self.active_dialog = Some(self.queue.remove(0));
        }
    }

    /// Whether any dialog is currently playing
    pub fn is_talking(&self) -> bool {
        self.active_dialog.is_some()
    }

    /// Clear all dialogs
    pub fn clear(&mut self) {
        self.active_dialog = None;
        self.cue_tracker = None;
        self.queue.clear();
    }

    /// Register known subtitle texts (from mulle.js hardcoded data)
    fn register_default_subtitles(&mut self) {
        // Garage — Mulle comments
        self.set_lines("03d012v0", vec![
            SubtitleLine::new("- Keine Räder, kein Spaß...", "mulle"),
        ]);

        // Part descriptions are loaded dynamically from parts.hash.json
        // (the "description" field is an audio member name like "20d038v0")

        // Road legality hints (from isRoadLegal talk=true)
        self.set_lines("03d040v0", vec![
            SubtitleLine::new("- Du brauchst noch einen Motor!", "mulle"),
        ]);
        self.set_lines("03d041v0", vec![
            SubtitleLine::new("- Du brauchst noch Räder!", "mulle"),
        ]);
        self.set_lines("03d042v0", vec![
            SubtitleLine::new("- Du brauchst noch Bremsen!", "mulle"),
        ]);
        self.set_lines("03d043v0", vec![
            SubtitleLine::new("- Du brauchst noch ein Lenkrad!", "mulle"),
        ]);
        self.set_lines("03d044v0", vec![
            SubtitleLine::new("- Du brauchst noch einen Tank!", "mulle"),
        ]);
        self.set_lines("03d045v0", vec![
            SubtitleLine::new("- Du brauchst noch eine Batterie!", "mulle"),
        ]);
        self.set_lines("03d046v0", vec![
            SubtitleLine::new("- Du brauchst noch ein Getriebe!", "mulle"),
        ]);

        // Fuel empty on road
        self.set_lines("05d011v0", vec![
            SubtitleLine::new("- Oh nein, der Tank ist leer!", "mulle"),
        ]);

        // Figge Ferrum dialog
        self.set_lines("92d002v0", vec![
            SubtitleLine::new("- Mein {Salka} ist schon wieder weggelaufen,", "figge"),
            SubtitleLine::new("- Hast du ihn gesehen?", "figge"),
        ]);
        self.set_lines("92d003v0", vec![
            SubtitleLine::new("- Danke, dass du {Salka} zurückgebracht hast!", "figge"),
            SubtitleLine::new("- Hier, nimm diesen Extra-Tank als Belohnung.", "figge"),
        ]);
        self.set_lines("92d004v0", vec![
            SubtitleLine::new("- Nein, ich habe deinen Hund nicht gesehen.", "mulle"),
        ]);
    }
}

// ---------------------------------------------------------------------------
// Cache-based quest system
// ---------------------------------------------------------------------------

/// Quest/flag manager using cache flags on the car
///
/// Flags are simple strings like "#Dog", "#ExtraTank", "#Lemonade"
/// that track quest progress.
pub struct QuestState {
    /// Active cache flags (reset when leaving yard)
    pub cache: Vec<String>,
    /// Permanent flags (persisted in save, never reset)
    pub permanent: Vec<String>,
}

impl QuestState {
    pub fn new() -> Self {
        Self {
            cache: Vec::new(),
            permanent: Vec::new(),
        }
    }

    /// Set a cache flag
    pub fn add_cache(&mut self, flag: &str) {
        if !self.cache.contains(&flag.to_string()) {
            self.cache.push(flag.to_string());
            tracing::debug!("Quest flag set: {}", flag);
        }
    }

    /// Check if a cache flag is set
    #[allow(dead_code)] // Used in tests and scene_script via ScriptContext
    pub fn has_cache(&self, flag: &str) -> bool {
        self.cache.iter().any(|f| f == flag)
    }

    /// Remove a cache flag
    pub fn remove_cache(&mut self, flag: &str) {
        self.cache.retain(|f| f != flag);
    }

    /// Reset all cache flags (called when leaving yard/starting drive)
    pub fn reset_cache(&mut self) {
        tracing::debug!("Quest cache reset ({} flags cleared)", self.cache.len());
        self.cache.clear();
    }

    /// Set a permanent flag
    pub fn add_permanent(&mut self, flag: &str) {
        if !self.permanent.contains(&flag.to_string()) {
            self.permanent.push(flag.to_string());
        }
    }

    /// Check a permanent flag
    #[allow(dead_code)] // Used in tests and scene_script via ScriptContext
    pub fn has_permanent(&self, flag: &str) -> bool {
        self.permanent.iter().any(|f| f == flag)
    }

    /// Load flags from save data
    pub fn load_from_save(&mut self, cache_list: &[String], own_stuff: &[String]) {
        self.cache = cache_list.to_vec();
        self.permanent = own_stuff.to_vec();
    }

    /// Get cache list for saving
    pub fn cache_list(&self) -> &[String] {
        &self.cache
    }

    /// Get permanent list for saving
    pub fn permanent_list(&self) -> &[String] {
        &self.permanent
    }
}

// ---------------------------------------------------------------------------
// Mission data
// ---------------------------------------------------------------------------

/// A mission definition (from missions.hash.json)
#[derive(Debug, Clone)]
#[allow(dead_code)] // Fields used by mission delivery system (upcoming)
pub struct Mission {
    pub mission_id: u32,
    /// Delivery type: Telephone or Mail
    pub delivery: MissionDelivery,
    /// Director member name for the mail image (empty for telephone)
    pub image: String,
    /// Audio member name for mission sound
    pub sound: String,
}

/// How a mission is delivered
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MissionDelivery {
    Telephone,
    Mail,
}

/// Mission database
pub struct MissionDB {
    pub missions: HashMap<u32, Mission>,
}

impl MissionDB {
    /// Load missions from embedded data
    pub fn load() -> Self {
        // Hardcoded from missions.hash.json (8 missions)
        let mut missions = HashMap::new();

        let data = [
            (1, MissionDelivery::Telephone, "", "50d001v0"),
            (2, MissionDelivery::Mail, "50b001v0", "50d016v0"),
            (3, MissionDelivery::Telephone, "", "50d003v0"),
            (4, MissionDelivery::Mail, "50b002v0", "50d004v0"),
            (5, MissionDelivery::Telephone, "", "50d005v0"),
            (6, MissionDelivery::Mail, "50b003v0", "50d006v0"),
            (7, MissionDelivery::Telephone, "", "50d007v0"),
            (8, MissionDelivery::Mail, "50b004v0", "50d008v0"),
        ];

        for (id, delivery, image, sound) in data {
            missions.insert(id, Mission {
                mission_id: id,
                delivery,
                image: image.to_string(),
                sound: sound.to_string(),
            });
        }

        Self { missions }
    }

    /// Get a mission by ID
    #[allow(dead_code)] // Used by mission delivery system (upcoming)
    pub fn get(&self, id: u32) -> Option<&Mission> {
        self.missions.get(&id)
    }
}

// ---------------------------------------------------------------------------
// Road legality dialog helper
// ---------------------------------------------------------------------------

/// Get audio IDs for missing car parts (for Mulle's road-legal hints)
pub fn road_legal_hint_sounds(failures: &[&str]) -> Vec<&'static str> {
    let mut sounds = Vec::new();
    for &failure in failures {
        let sound = match failure {
            "engine" => "03d040v0",
            "tires" => "03d041v0",
            "brake" => "03d042v0",
            "steering" => "03d043v0",
            "fuel_tank" => "03d044v0",
            "battery" => "03d045v0",
            "gearbox" => "03d046v0",
            "fuel_consumption" => "03d040v0", // same as engine
            _ => continue,
        };
        sounds.push(sound);
    }
    sounds
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subtitle_line_duration() {
        let line = SubtitleLine::new("Hello, this is a test sentence!", "mulle");
        assert!(line.duration_ms >= 500);
        assert!(line.duration_ms < 10000);
    }

    #[test]
    fn subtitle_highlighted_words() {
        let line = SubtitleLine::new("My {Salka} ran away, find {him} please!", "figge");
        let words = line.highlighted_words();
        assert_eq!(words, vec!["Salka", "him"]);
    }

    #[test]
    fn subtitle_plain_text() {
        let line = SubtitleLine::new("Find {Salka} now!", "figge");
        assert_eq!(line.plain_text(), "Find Salka now!");
    }

    #[test]
    fn dialog_sequence_advances() {
        let lines = vec![
            SubtitleLine::new("Line one", "a"),
            SubtitleLine::new("Line two", "a"),
        ];
        let mut seq = DialogSequence::new("test", lines);

        assert!(!seq.finished);
        assert_eq!(seq.current_subtitle().unwrap().text, "Line one");

        // Advance past first line
        let d = seq.lines[0].duration_ms;
        seq.advance(d + 1);
        assert_eq!(seq.current_subtitle().unwrap().text, "Line two");

        // Advance past second line
        let d = seq.lines[1].duration_ms;
        seq.advance(d + 1);
        assert!(seq.finished);
        assert!(seq.current_subtitle().is_none());
    }

    #[test]
    fn dialog_manager_queue() {
        let mut mgr = DialogManager::new();
        mgr.set_lines("a", vec![SubtitleLine::new("First", "x")]);
        mgr.set_lines("b", vec![SubtitleLine::new("Second", "x")]);

        mgr.talk("a");
        mgr.talk("b"); // queued

        assert!(mgr.is_talking());
        assert_eq!(mgr.current_subtitle().unwrap().text, "First");

        // Skip first
        mgr.skip_current();
        assert!(mgr.is_talking());
        assert_eq!(mgr.current_subtitle().unwrap().text, "Second");

        mgr.skip_current();
        assert!(!mgr.is_talking());
    }

    #[test]
    fn quest_state_cache_flags() {
        let mut qs = QuestState::new();
        assert!(!qs.has_cache("#Dog"));

        qs.add_cache("#Dog");
        assert!(qs.has_cache("#Dog"));

        qs.reset_cache();
        assert!(!qs.has_cache("#Dog"));
    }

    #[test]
    fn quest_state_permanent_flags() {
        let mut qs = QuestState::new();
        qs.add_permanent("#GotDogOnce");
        assert!(qs.has_permanent("#GotDogOnce"));

        // Reset cache does NOT clear permanent
        qs.reset_cache();
        assert!(qs.has_permanent("#GotDogOnce"));
    }

    #[test]
    fn mission_db_loads() {
        let db = MissionDB::load();
        assert_eq!(db.missions.len(), 8);
        let m1 = db.get(1).unwrap();
        assert_eq!(m1.delivery, MissionDelivery::Telephone);
        let m2 = db.get(2).unwrap();
        assert_eq!(m2.delivery, MissionDelivery::Mail);
        assert!(!m2.image.is_empty());
    }

    #[test]
    fn road_legal_hints() {
        let failures = vec!["engine", "tires", "steering"];
        let sounds = road_legal_hint_sounds(&failures);
        assert_eq!(sounds.len(), 3);
        assert_eq!(sounds[0], "03d040v0");
    }
}
