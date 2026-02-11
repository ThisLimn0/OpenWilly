use eframe::egui;
use tracing::info;

fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into())
        )
        .init();

    info!("Starting OpenWilly Launcher...");

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1024.0, 768.0])
            .with_min_inner_size([800.0, 600.0])
            .with_icon(
                // TODO: Load icon
                eframe::icon_data::from_png_bytes(&[])
                    .unwrap_or_default()
            ),
        ..Default::default()
    };

    eframe::run_native(
        "OpenWilly - Willy Werkel Game Launcher",
        native_options,
        Box::new(|cc| Ok(Box::new(OpenWillyApp::new(cc)))),
    )
    .map_err(|e| anyhow::anyhow!("Failed to run app: {}", e))
}

struct OpenWillyApp {
    games: Vec<GameInfo>,
    selected_game: Option<usize>,
    show_about: bool,
    show_settings: bool,
}

#[derive(Debug, Clone)]
struct GameInfo {
    name: String,
    iso_path: Option<std::path::PathBuf>,
    installed: bool,
    last_played: Option<chrono::DateTime<chrono::Local>>,
}

impl OpenWillyApp {
    fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        Self {
            games: vec![
                GameInfo {
                    name: "Autos bauen mit Willy Werkel".to_string(),
                    iso_path: None,
                    installed: false,
                    last_played: None,
                },
                GameInfo {
                    name: "Flugzeuge bauen mit Willy Werkel".to_string(),
                    iso_path: None,
                    installed: false,
                    last_played: None,
                },
                GameInfo {
                    name: "Häuser bauen mit Willy Werkel".to_string(),
                    iso_path: None,
                    installed: false,
                    last_played: None,
                },
                GameInfo {
                    name: "Raumschiffe bauen mit Willy Werkel".to_string(),
                    iso_path: None,
                    installed: false,
                    last_played: None,
                },
                GameInfo {
                    name: "Schiffe bauen mit Willy Werkel".to_string(),
                    iso_path: None,
                    installed: false,
                    last_played: None,
                },
            ],
            selected_game: None,
            show_about: false,
            show_settings: false,
        }
    }
}

impl eframe::App for OpenWillyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Top menu bar
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Settings").clicked() {
                        self.show_settings = true;
                    }
                    if ui.button("Quit").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
                ui.menu_button("Help", |ui| {
                    if ui.button("About").clicked() {
                        self.show_about = true;
                    }
                    if ui.button("Documentation").clicked() {
                        let _ = open::that("https://github.com/yourusername/openwilly");
                    }
                });
            });
        });

        // Main content
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("🎮 Willy Werkel Games Library");
            ui.add_space(10.0);

            // Game list
            egui::ScrollArea::vertical().show(ui, |ui| {
                for (idx, game) in self.games.iter_mut().enumerate() {
                    let is_selected = self.selected_game == Some(idx);
                    
                    let response = ui.selectable_label(
                        is_selected,
                        format!("🎲 {}", game.name)
                    );
                    
                    if response.clicked() {
                        self.selected_game = Some(idx);
                    }

                    ui.horizontal(|ui| {
                        ui.add_space(20.0);
                        
                        if game.installed {
                            ui.label("✅ Installed");
                        } else if game.iso_path.is_some() {
                            ui.label("📀 ISO configured");
                        } else {
                            ui.label("⚠️ No ISO");
                        }

                        if let Some(last_played) = game.last_played {
                            ui.label(format!("Last played: {}", last_played.format("%Y-%m-%d")));
                        }
                    });

                    ui.add_space(5.0);
                }
            });

            ui.separator();

            // Action buttons
            ui.horizontal(|ui| {
                let game_selected = self.selected_game.is_some();
                let can_play = game_selected && 
                    self.selected_game
                        .and_then(|idx| self.games.get(idx))
                        .and_then(|g| g.iso_path.as_ref())
                        .is_some();

                if ui.add_enabled(can_play, egui::Button::new("▶ Play")).clicked() {
                    if let Some(idx) = self.selected_game {
                        info!("Launching game: {}", self.games[idx].name);
                        // TODO: Actual launch logic
                    }
                }

                if ui.add_enabled(game_selected, egui::Button::new("📁 Select ISO")).clicked() {
                    if let Some(idx) = self.selected_game {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("ISO Image", &["iso"])
                            .pick_file()
                        {
                            info!("Selected ISO: {:?}", path);
                            self.games[idx].iso_path = Some(path);
                        }
                    }
                }

                if ui.add_enabled(game_selected, egui::Button::new("⚙️ Configure")).clicked() {
                    // TODO: Game-specific settings
                }
            });
        });

        // About dialog
        if self.show_about {
            egui::Window::new("About OpenWilly")
                .open(&mut self.show_about)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.heading("OpenWilly");
                    ui.label("Version 0.1.0");
                    ui.add_space(10.0);
                    ui.label("Compatibility wrapper for classic Willy Werkel games");
                    ui.add_space(10.0);
                    ui.hyperlink_to("GitHub Repository", "https://github.com/yourusername/openwilly");
                    ui.add_space(10.0);
                    ui.label("Licensed under MIT or Apache-2.0");
                });
        }

        // Settings dialog
        if self.show_settings {
            egui::Window::new("Settings")
                .open(&mut self.show_settings)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label("🚧 Settings coming soon...");
                    // TODO: Global settings
                });
        }
    }
}
