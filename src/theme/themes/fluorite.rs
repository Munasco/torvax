use super::super::Theme;
use ratatui::style::Color;

/// Fluorite - Elegant dark theme with purple and pink tones
/// Soft and sophisticated color scheme inspired by the mineral fluorite
/// Created by Rnbsov (https://github.com/Rnbsov)
pub fn fluorite() -> Theme {
    Theme {
        background_left: Color::Rgb(18, 12, 23),  // Sidebar (#120c17)
        background_right: Color::Rgb(22, 17, 27), // Editor (#16111b)

        editor_line_number: Color::Rgb(42, 28, 54),
        editor_line_number_cursor: Color::Rgb(65, 43, 84),
        editor_separator: Color::Rgb(42, 28, 54),
        editor_cursor_char_bg: Color::Rgb(163, 109, 207),
        editor_cursor_char_fg: Color::Rgb(22, 17, 27),
        editor_cursor_line_bg: Color::Rgb(25, 17, 31),

        file_tree_added: Color::Rgb(129, 184, 139),
        file_tree_deleted: Color::Rgb(91, 60, 117),
        file_tree_modified: Color::Rgb(136, 92, 173),
        file_tree_renamed: Color::Rgb(139, 203, 255),
        file_tree_directory: Color::Rgb(136, 92, 173),
        file_tree_current_file_bg: Color::Rgb(25, 17, 31),
        file_tree_current_file_fg: Color::Rgb(136, 92, 173),
        file_tree_default: Color::Rgb(91, 60, 117),
        file_tree_stats_added: Color::Rgb(129, 184, 139),
        file_tree_stats_deleted: Color::Rgb(91, 60, 117),

        terminal_command: Color::Rgb(136, 92, 173),
        terminal_output: Color::Rgb(110, 155, 188),
        terminal_cursor_bg: Color::Rgb(163, 109, 207),
        terminal_cursor_fg: Color::Rgb(22, 17, 27),

        status_hash: Color::Rgb(173, 133, 245),
        status_author: Color::Rgb(139, 203, 255),
        status_date: Color::Rgb(209, 123, 159),
        status_message: Color::Rgb(110, 155, 188),
        status_no_commit: Color::Rgb(65, 43, 84),

        separator: Color::Rgb(42, 28, 54),

        syntax_keyword: Color::Rgb(222, 127, 236),
        syntax_type: Color::Rgb(173, 133, 245),
        syntax_function: Color::Rgb(139, 203, 255),
        syntax_variable: Color::Rgb(215, 215, 215),
        syntax_string: Color::Rgb(139, 215, 137),
        syntax_number: Color::Rgb(209, 123, 159),
        syntax_comment: Color::Rgb(91, 60, 117),
        syntax_operator: Color::Rgb(110, 155, 188),
        syntax_punctuation: Color::Rgb(110, 155, 188),
        syntax_constant: Color::Rgb(209, 123, 159),
        syntax_parameter: Color::Rgb(173, 133, 245),
        syntax_property: Color::Rgb(139, 215, 137),
        syntax_label: Color::Rgb(222, 127, 236),
    }
}
