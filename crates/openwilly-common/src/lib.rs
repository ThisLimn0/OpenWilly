//! Common utilities and types shared across OpenWilly crates

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Supported Willy Werkel games
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GameId {
    AutosBauen,
    FlugzeugeBauen,
    HauserBauen,
    RaumschiffeBauen,
    SchiffeBauen,
}

impl GameId {
    /// Get the display name for this game
    pub fn display_name(&self) -> &str {
        match self {
            GameId::AutosBauen => "Autos bauen mit Willy Werkel",
            GameId::FlugzeugeBauen => "Flugzeuge bauen mit Willy Werkel",
            GameId::HauserBauen => "Häuser bauen mit Willy Werkel",
            GameId::RaumschiffeBauen => "Raumschiffe bauen mit Willy Werkel",
            GameId::SchiffeBauen => "Schiffe bauen mit Willy Werkel",
        }
    }

    /// Get the expected ISO filename pattern
    pub fn iso_pattern(&self) -> &str {
        match self {
            GameId::AutosBauen => "Autos_Bauen",
            GameId::FlugzeugeBauen => "Flugzeuge",
            GameId::HauserBauen => "Hauser|Häuser",
            GameId::RaumschiffeBauen => "Raumschiff",
            GameId::SchiffeBauen => "Schiffe",
        }
    }

    /// Get all supported games
    pub fn all() -> Vec<GameId> {
        vec![
            GameId::AutosBauen,
            GameId::FlugzeugeBauen,
            GameId::HauserBauen,
            GameId::RaumschiffeBauen,
            GameId::SchiffeBauen,
        ]
    }
}

/// Configuration for a game instance
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameConfig {
    pub game_id: GameId,
    pub iso_path: Option<PathBuf>,
    pub install_path: Option<PathBuf>,
    pub windowed_mode: bool,
    pub resolution: Option<(u32, u32)>,
    pub enable_logging: bool,
}

impl Default for GameConfig {
    fn default() -> Self {
        Self {
            game_id: GameId::AutosBauen,
            iso_path: None,
            install_path: None,
            windowed_mode: true,
            resolution: None,
            enable_logging: true,
        }
    }
}

/// Application-wide configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub games: Vec<GameConfig>,
    pub default_install_path: PathBuf,
    pub log_level: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            games: Vec::new(),
            default_install_path: std::env::current_dir()
                .unwrap_or_default()
                .join("games"),
            log_level: "info".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_game_id_display_names() {
        assert_eq!(
            GameId::AutosBauen.display_name(),
            "Autos bauen mit Willy Werkel"
        );
    }

    #[test]
    fn test_all_games() {
        let games = GameId::all();
        assert_eq!(games.len(), 5);
    }
}
