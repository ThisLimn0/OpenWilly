//! Internationalization — UI text translations for German and English.
//!
//! The game's original voice acting and cast data remain in their original
//! language; this module only handles engine-drawn UI text (menus, HUD labels,
//! debug overlays, escape menu, dev menu).

/// Supported UI languages
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    German,
    English,
}

impl Language {
    /// Cycle to the next language
    pub fn next(self) -> Self {
        match self {
            Language::German => Language::English,
            Language::English => Language::German,
        }
    }

    /// Short display code
    pub fn code(&self) -> &'static str {
        match self {
            Language::German => "DE",
            Language::English => "EN",
        }
    }
}

/// All translatable UI strings, looked up by key.
/// Returns the translated string or "???" if the key is not found.
pub fn t(lang: Language, key: &str) -> &'static str {
    match (lang, key) {
        // ── Escape / Pause menu ──
        (Language::German, "pause_title") => "= PAUSE =",
        (Language::English, "pause_title") => "= PAUSED =",
        (Language::German, "menu_resume") => "Weiterspielen",
        (Language::English, "menu_resume") => "Resume",
        (Language::German, "menu_fullscreen") => "Vollbild umschalten",
        (Language::English, "menu_fullscreen") => "Toggle Fullscreen",
        (Language::German, "menu_detail_noise") => "Detail-Rauschen",
        (Language::English, "menu_detail_noise") => "Detail Noise",
        (Language::German, "menu_quit") => "Beenden",
        (Language::English, "menu_quit") => "Quit",
        (Language::German, "pause_hint") => "Pfeiltasten + Enter | Esc",
        (Language::English, "pause_hint") => "Arrow keys + Enter | Esc",

        // ── Main menu ──
        (Language::German, "lang_label") => "Sprache: Deutsch",
        (Language::English, "lang_label") => "Language: English",

        // ── Garage / build ──
        (Language::German, "road_legal") => "Fahrtauglich!",
        (Language::English, "road_legal") => "Road legal!",
        (Language::German, "not_road_legal") => "Noch nicht fahrtauglich",
        (Language::English, "not_road_legal") => "Not road legal yet",

        // ── Dev menu ──
        (Language::German, "dev_title") => "~ DEV MENU ~",
        (Language::English, "dev_title") => "~ DEV MENU ~",
        (Language::German, "dev_hint") => "Up/Down + Enter | Klick | 5x# schliesst",
        (Language::English, "dev_hint") => "Up/Down + Enter | Click | 5x# to close",
        (Language::German, "dev_infinite_fuel") => "Unendlich Benzin",
        (Language::English, "dev_infinite_fuel") => "Infinite Fuel",
        (Language::German, "dev_noclip") => "Noclip (durch Waende)",
        (Language::English, "dev_noclip") => "Noclip (through walls)",
        (Language::German, "dev_hitboxes") => "Hitboxen anzeigen",
        (Language::English, "dev_hitboxes") => "Show Hitboxes",
        (Language::German, "dev_skip_dialog") => "Dialoge ueberspringen",
        (Language::English, "dev_skip_dialog") => "Skip Dialogs",
        (Language::German, "dev_meme") => "Meme-Modus",
        (Language::English, "dev_meme") => "Meme Mode",
        (Language::German, "dev_detail_noise") => "Detail-Rauschen",
        (Language::English, "dev_detail_noise") => "Detail Noise",
        (Language::German, "dev_goto_garage") => "-> Werkstatt",
        (Language::English, "dev_goto_garage") => "-> Workshop",
        (Language::German, "dev_goto_yard") => "-> Hof",
        (Language::English, "dev_goto_yard") => "-> Yard",
        (Language::German, "dev_goto_world") => "-> Weltkarte",
        (Language::English, "dev_goto_world") => "-> World Map",
        (Language::German, "dev_goto_carshow") => "-> Autoshow",
        (Language::English, "dev_goto_carshow") => "-> Car Show",
        (Language::German, "dev_goto_junkyard") => "-> Schrottplatz",
        (Language::English, "dev_goto_junkyard") => "-> Junkyard",
        (Language::German, "dev_refuel") => "Tank auffuellen",
        (Language::English, "dev_refuel") => "Refuel Tank",
        (Language::German, "dev_figge") => "Figge in Werkstatt",
        (Language::English, "dev_figge") => "Figge in Workshop",
        (Language::German, "dev_close") => "Schliessen",
        (Language::English, "dev_close") => "Close",

        // ── Fallback ──
        _ => "???",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_german_keys_have_english() {
        let keys = [
            "pause_title", "menu_resume", "menu_fullscreen", "menu_detail_noise",
            "menu_quit", "pause_hint", "lang_label", "road_legal", "not_road_legal",
            "dev_title", "dev_hint", "dev_infinite_fuel", "dev_noclip",
            "dev_hitboxes", "dev_skip_dialog", "dev_meme", "dev_detail_noise",
            "dev_goto_garage", "dev_goto_yard", "dev_goto_world",
            "dev_goto_carshow", "dev_goto_junkyard", "dev_refuel",
            "dev_figge", "dev_close",
        ];
        for key in &keys {
            let de = t(Language::German, key);
            let en = t(Language::English, key);
            assert_ne!(de, *key, "German missing for '{}'", key);
            assert_ne!(en, *key, "English missing for '{}'", key);
        }
    }

    #[test]
    fn language_cycle() {
        assert_eq!(Language::German.next(), Language::English);
        assert_eq!(Language::English.next(), Language::German);
    }
}
