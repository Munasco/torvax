mod themes;

use anyhow::{Context, Result};
use ratatui::style::Color;

#[derive(Debug, Clone)]
pub struct Theme {
    // Background colors
    pub background_left: Color,  // FileTree and StatusBar side (darker)
    pub background_right: Color, // Editor and Terminal side

    // Editor colors
    pub editor_line_number: Color,
    pub editor_line_number_cursor: Color,
    pub editor_separator: Color,
    pub editor_cursor_char_bg: Color,
    pub editor_cursor_char_fg: Color,
    pub editor_cursor_line_bg: Color,

    // File tree colors
    pub file_tree_added: Color,
    pub file_tree_deleted: Color,
    pub file_tree_modified: Color,
    pub file_tree_renamed: Color,
    pub file_tree_directory: Color,
    pub file_tree_current_file_bg: Color,
    pub file_tree_current_file_fg: Color,
    pub file_tree_default: Color,
    pub file_tree_stats_added: Color,
    pub file_tree_stats_deleted: Color,

    // Terminal colors
    pub terminal_command: Color,
    pub terminal_output: Color,
    pub terminal_cursor_bg: Color,
    pub terminal_cursor_fg: Color,

    // Status bar colors
    pub status_hash: Color,
    pub status_author: Color,
    pub status_date: Color,
    pub status_message: Color,
    pub status_no_commit: Color,

    // Separator colors
    pub separator: Color,

    // Syntax highlighting colors
    pub syntax_keyword: Color,
    pub syntax_type: Color,
    pub syntax_function: Color,
    pub syntax_variable: Color,
    pub syntax_string: Color,
    pub syntax_number: Color,
    pub syntax_comment: Color,
    pub syntax_operator: Color,
    pub syntax_punctuation: Color,
    pub syntax_constant: Color,
    pub syntax_parameter: Color,
    pub syntax_property: Color,
    pub syntax_label: Color,
}

impl Default for Theme {
    fn default() -> Self {
        themes::tokyo_night()
    }
}

impl Theme {
    /// Load theme by name
    pub fn load(name: &str) -> Result<Self> {
        match name {
            "ayu-dark" => Ok(themes::ayu_dark()),
            "catppuccin" => Ok(themes::catppuccin()),
            "dracula" => Ok(themes::dracula()),
            "fluorite" => Ok(themes::fluorite()),
            "everforest" => Ok(themes::everforest()),
            "github-dark" => Ok(themes::github_dark()),
            "gruvbox" => Ok(themes::gruvbox()),
            "material" => Ok(themes::material()),
            "monokai" => Ok(themes::monokai()),
            "night-owl" => Ok(themes::night_owl()),
            "nord" => Ok(themes::nord()),
            "one-dark" => Ok(themes::one_dark()),
            "rose-pine" => Ok(themes::rose_pine()),
            "solarized-dark" => Ok(themes::solarized_dark()),
            "solarized-light" => Ok(themes::solarized_light()),
            "telemetry" => Ok(themes::telemetry()),
            "tokyo-night" => Ok(themes::tokyo_night()),
            _ => Err(anyhow::anyhow!("Unknown theme: {}", name))
                .context("Available themes: ayu-dark, catppuccin, dracula, everforest, github-dark, gruvbox, material, monokai, night-owl, nord, one-dark, rose-pine, solarized-dark, solarized-light, telemetry, tokyo-night"),
        }
    }

    /// Remove background colors for transparent terminal background
    pub fn with_transparent_background(mut self) -> Self {
        self.background_left = Color::Reset;
        self.background_right = Color::Reset;
        self
    }

    /// List all available built-in themes
    pub fn available_themes() -> Vec<&'static str> {
        vec![
            "ayu-dark",
            "catppuccin",
            "dracula",
            "fluorite"
            "everforest",
            "github-dark",
            "gruvbox",
            "material",
            "monokai",
            "night-owl",
            "nord",
            "one-dark",
            "rose-pine",
            "solarized-dark",
            "solarized-light",
            "telemetry",
            "tokyo-night",
        ]
    }
}
