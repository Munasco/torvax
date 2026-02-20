use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Gauge, Padding, Paragraph},
    Frame,
};
use unicode_width::UnicodeWidthStr;

use super::{UIState, UI};

impl<'a> UI<'a> {
    pub(super) fn render(&mut self, f: &mut Frame) {
        let size = f.area();

        // Split horizontally: left column | right column
        let main_layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(30), // Left column (file tree + commit info)
                Constraint::Percentage(70), // Right column (editor + terminal)
            ])
            .margin(0)
            .spacing(0)
            .split(size);

        // Split left column vertically: file tree | separator | commit info
        let left_layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(80), // File tree
                Constraint::Length(1),      // Horizontal separator
                Constraint::Percentage(20), // Commit info
            ])
            .margin(0)
            .spacing(0)
            .split(main_layout[0]);

        // Split right column vertically: editor | separator | terminal
        let right_layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(80), // Editor
                Constraint::Length(1),      // Horizontal separator
                Constraint::Percentage(20), // Terminal
            ])
            .margin(0)
            .spacing(0)
            .split(main_layout[1]);

        let separator_color = self.theme.separator;

        // Update file tree data if needed
        if let Some(metadata) = self.engine.current_metadata() {
            self.file_tree.set_commit_metadata(
                metadata,
                self.engine.current_file_index,
                &self.theme,
            );
        }

        // Render file tree
        self.file_tree.render(f, left_layout[0], &self.theme);

        // Render horizontal separator between file tree and commit info (left column)
        let left_sep = Paragraph::new(Line::from("─".repeat(left_layout[1].width as usize))).style(
            Style::default()
                .fg(separator_color)
                .bg(self.theme.background_left),
        );
        f.render_widget(left_sep, left_layout[1]);

        // Render commit info
        self.status_bar.render(
            f,
            left_layout[2],
            self.engine.current_metadata(),
            &self.theme,
        );

        // Render editor
        self.editor
            .render(f, right_layout[0], &self.engine, &self.theme);

        // Render horizontal separator between editor and terminal (right column)
        let right_sep = Paragraph::new(Line::from("─".repeat(right_layout[1].width as usize)))
            .style(
                Style::default()
                    .fg(separator_color)
                    .bg(self.theme.background_right),
            );
        f.render_widget(right_sep, right_layout[1]);

        // Render terminal
        self.terminal
            .render(f, right_layout[2], &self.engine, &self.theme);

        // Render dialog if present
        if let Some(ref title) = self.engine.dialog_title {
            let text = &self.engine.dialog_typing_text;
            let text_display_width = text.width();
            let dialog_width = (text_display_width + 10).max(60).min(size.width as usize) as u16;
            let dialog_height = 3;
            let dialog_x = (size.width.saturating_sub(dialog_width)) / 2;
            let dialog_y = (size.height.saturating_sub(dialog_height)) / 2;

            let dialog_area = Rect {
                x: dialog_x,
                y: dialog_y,
                width: dialog_width,
                height: dialog_height,
            };

            // Calculate content width (dialog_width - borders(2) - padding(2))
            let content_width = dialog_width.saturating_sub(4) as usize;
            let padding_len = content_width.saturating_sub(text_display_width);

            let spans = vec![
                Span::styled(
                    text.clone(),
                    Style::default().fg(self.theme.file_tree_current_file_fg),
                ),
                Span::styled(
                    " ".repeat(padding_len),
                    Style::default().bg(self.theme.editor_cursor_line_bg),
                ),
            ];

            let dialog_text = vec![Line::from(spans)];

            let block = Block::default()
                .borders(Borders::ALL)
                .title(title.clone())
                .padding(Padding::horizontal(1))
                .style(
                    Style::default()
                        .fg(self.theme.file_tree_current_file_fg)
                        .bg(self.theme.editor_cursor_line_bg),
                );

            let dialog = Paragraph::new(dialog_text).block(block);
            f.render_widget(dialog, dialog_area);
        }

        // Render menu / key bindings / about overlays
        match self.state {
            UIState::Menu => self.render_menu(f, size),
            UIState::KeyBindings => self.render_keybindings(f, size),
            UIState::About => self.render_about(f, size),
            UIState::GeneratingAudio => self.render_generating_audio(f, size),
            _ => {}
        }
    }

    pub(super) fn render_menu(&self, f: &mut Frame, size: Rect) {
        let items = ["Key Bindings", "About", "Exit"];
        let lines: Vec<Line> = items
            .iter()
            .enumerate()
            .map(|(i, item)| {
                let marker = if i == self.menu_index { "> " } else { "  " };
                let style = if i == self.menu_index {
                    Style::default().fg(self.theme.file_tree_current_file_fg)
                } else {
                    Style::default().fg(self.theme.status_message)
                };
                Line::from(Span::styled(format!("{marker}{item}"), style))
            })
            .collect();

        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Menu (Esc to close) ")
            .padding(Padding::new(2, 2, 1, 1))
            .style(
                Style::default()
                    .fg(self.theme.file_tree_current_file_fg)
                    .bg(self.theme.editor_cursor_line_bg),
            );

        let dialog_width = 30u16;
        let dialog_height = (items.len() as u16) + 4; // borders + padding
        let area = Self::centered_rect(size, dialog_width, dialog_height);

        f.render_widget(Clear, area);
        f.render_widget(Paragraph::new(lines).block(block), area);
    }

    pub(super) fn render_keybindings(&self, f: &mut Frame, size: Rect) {
        let lines = vec![
            Line::from(Span::styled(
                "General",
                Style::default().fg(self.theme.file_tree_current_file_fg),
            )),
            Line::from("  Esc     Menu"),
            Line::from("  q       Quit"),
            Line::from("  Ctrl+c  Quit"),
            Line::from(""),
            Line::from(Span::styled(
                "Playback Controls",
                Style::default().fg(self.theme.file_tree_current_file_fg),
            )),
            Line::from("  Space   Play / Pause"),
            Line::from("  h / l   Step line back / forward"),
            Line::from("  H / L   Step change back / forward"),
            Line::from("  p / n   Previous / Next commit"),
        ];

        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Key Bindings (Esc to close) ")
            .padding(Padding::new(2, 2, 1, 1))
            .style(
                Style::default()
                    .fg(self.theme.status_message)
                    .bg(self.theme.editor_cursor_line_bg),
            );

        let dialog_height = (lines.len() as u16) + 4;
        let area = Self::centered_rect(size, 44, dialog_height);

        f.render_widget(Clear, area);
        f.render_widget(Paragraph::new(lines).block(block), area);
    }

    pub(super) fn render_about(&self, f: &mut Frame, size: Rect) {
        let version = env!("CARGO_PKG_VERSION");
        let lines = vec![
            Line::from(Span::styled(
                "torvax",
                Style::default().fg(self.theme.file_tree_current_file_fg),
            )),
            Line::from(format!("Version {version}")),
            Line::from(""),
            Line::from("Git review of your diffs, like a movie."),
            Line::from(""),
            Line::from("https://github.com/Munasco/torvax"),
        ];

        let block = Block::default()
            .borders(Borders::ALL)
            .title(" About (Esc to close) ")
            .padding(Padding::new(2, 2, 1, 1))
            .style(
                Style::default()
                    .fg(self.theme.status_message)
                    .bg(self.theme.editor_cursor_line_bg),
            );

        let dialog_height = (lines.len() as u16) + 4;
        let area = Self::centered_rect(size, 48, dialog_height);

        f.render_widget(Clear, area);
        f.render_widget(Paragraph::new(lines).block(block), area);
    }

    pub(super) fn render_generating_audio(&self, f: &mut Frame, size: Rect) {
        let (status, progress) = self
            .audio_progress
            .lock()
            .ok()
            .map(|p| p.clone())
            .unwrap_or_else(|| ("Initializing...".to_string(), 0.0));

        let area = Self::centered_rect(size, 70, 11);
        f.render_widget(Clear, area);

        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Preparing AI Voiceover ")
            .padding(Padding::new(2, 2, 1, 1))
            .style(
                Style::default()
                    .fg(self.theme.status_message)
                    .bg(self.theme.editor_cursor_line_bg),
            );

        let inner = block.inner(area);
        f.render_widget(block, area);

        // Split inner area into: title line, progress bar, status line, quit hint
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // Title
                Constraint::Length(1), // Spacing
                Constraint::Length(1), // Progress bar
                Constraint::Length(1), // Spacing
                Constraint::Length(1), // Status message
                Constraint::Length(1), // Spacing
                Constraint::Length(1), // Quit hint
            ])
            .split(inner);

        let title = Paragraph::new(Line::from(Span::styled(
            "Generating voiceover narration",
            Style::default().fg(self.theme.file_tree_current_file_fg),
        )));
        f.render_widget(title, chunks[0]);

        let progress_bar = Gauge::default()
            .gauge_style(
                Style::default()
                    .fg(self.theme.file_tree_current_file_fg)
                    .bg(self.theme.background_right),
            )
            .ratio(progress as f64)
            .label(format!("{}%", (progress * 100.0) as u8));
        f.render_widget(progress_bar, chunks[2]);

        let status_line = Paragraph::new(Line::from(status));
        f.render_widget(status_line, chunks[4]);

        let quit_hint = Paragraph::new(Line::from(Span::styled(
            "q  quit",
            Style::default().fg(self.theme.status_message),
        )));
        f.render_widget(quit_hint, chunks[6]);
    }

    pub(super) fn centered_rect(outer: Rect, width: u16, height: u16) -> Rect {
        Rect {
            x: outer.x + (outer.width.saturating_sub(width)) / 2,
            y: outer.y + (outer.height.saturating_sub(height)) / 2,
            width: width.min(outer.width),
            height: height.min(outer.height),
        }
    }
}
