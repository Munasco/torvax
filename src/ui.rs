use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Padding, Paragraph},
    Frame, Terminal,
};
use unicode_width::UnicodeWidthStr;

use crate::animation::{AnimationEngine, SpeedRule, StepMode};
use crate::audio::AudioPlayer;
use crate::git::{CommitMetadata, DiffMode, GitRepository};
use crate::panes::{EditorPane, FileTreePane, StatusBarPane, TerminalPane};
use crate::theme::Theme;
use crate::PlaybackOrder;

#[derive(Debug, Clone, PartialEq)]
enum UIState {
    Playing,
    WaitingForNext { resume_at: Instant },
    GeneratingAudio,
    Menu,
    KeyBindings,
    About,
    Finished,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum PlaybackState {
    Playing,
    Paused,
}

/// Main UI controller for the torvax terminal interface.
pub struct UI<'a> {
    state: UIState,
    speed_ms: u64,
    file_tree: FileTreePane,
    editor: EditorPane,
    terminal: TerminalPane,
    status_bar: StatusBarPane,
    engine: AnimationEngine,
    repo: Option<&'a GitRepository>,
    should_exit: Arc<AtomicBool>,
    theme: Theme,
    order: PlaybackOrder,
    loop_playback: bool,
    commit_spec: Option<String>,
    is_range_mode: bool,
    diff_mode: Option<DiffMode>,
    playback_state: PlaybackState,
    history: Vec<CommitMetadata>,
    history_index: Option<usize>,
    menu_index: usize,
    prev_state: Option<Box<UIState>>,
    audio_player: Option<Arc<AudioPlayer>>,
    audio_gen_handle: Option<std::thread::JoinHandle<()>>,
    pending_metadata: Option<CommitMetadata>,
    audio_progress: Arc<Mutex<String>>,
}

impl<'a> UI<'a> {
    /// Creates a new UI instance with the specified configuration.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        speed_ms: u64,
        repo: Option<&'a GitRepository>,
        theme: Theme,
        order: PlaybackOrder,
        loop_playback: bool,
        commit_spec: Option<String>,
        is_range_mode: bool,
        speed_rules: Vec<SpeedRule>,
        audio_player: Option<Arc<AudioPlayer>>,
    ) -> Self {
        let should_exit = Arc::new(AtomicBool::new(false));
        Self::setup_signal_handler(should_exit.clone());

        let mut engine = AnimationEngine::new(speed_ms);
        engine.set_speed_rules(speed_rules);

        // Pass audio player to animation engine for synced voiceovers
        if let Some(ref player) = audio_player {
            engine.set_audio_player(Arc::clone(player));
        }

        Self {
            state: UIState::Playing,
            speed_ms,
            file_tree: FileTreePane::new(),
            editor: EditorPane,
            terminal: TerminalPane,
            status_bar: StatusBarPane,
            engine,
            repo,
            should_exit,
            theme,
            order,
            loop_playback,
            commit_spec,
            is_range_mode,
            diff_mode: None,
            playback_state: PlaybackState::Playing,
            history: Vec::new(),
            history_index: None,
            menu_index: 0,
            prev_state: None,
            audio_player,
            audio_gen_handle: None,
            pending_metadata: None,
            audio_progress: Arc::new(Mutex::new(String::new())),
        }
    }

    /// Sets the diff mode for working tree diff playback.
    pub fn set_diff_mode(&mut self, mode: Option<DiffMode>) {
        self.diff_mode = mode;
    }

    fn open_menu(&mut self) {
        self.prev_state = Some(Box::new(self.state.clone()));
        self.menu_index = 0;
        self.state = UIState::Menu;
        self.engine.pause();
    }

    fn close_menu(&mut self) {
        let restored = self
            .prev_state
            .take()
            .map(|s| *s)
            .unwrap_or(UIState::Playing);
        self.state = match restored {
            UIState::WaitingForNext { .. } => UIState::Playing,
            other => other,
        };
        if self.playback_state == PlaybackState::Playing {
            self.engine.resume();
        }
    }

    fn setup_signal_handler(should_exit: Arc<AtomicBool>) {
        ctrlc::set_handler(move || {
            // Restore terminal state before exiting
            let _ = disable_raw_mode();
            let _ = execute!(
                io::stdout(),
                LeaveAlternateScreen,
                DisableMouseCapture,
                crossterm::cursor::Show
            );
            should_exit.store(true, Ordering::SeqCst);
            // Exit immediately for external signals (SIGTERM)
            std::process::exit(0);
        })
        .expect("Error setting Ctrl-C handler");
    }

    /// Loads a commit and starts the animation.
    pub fn load_commit(&mut self, metadata: CommitMetadata) {
        self.play_commit(metadata, true);
    }

    fn play_commit(&mut self, metadata: CommitMetadata, record_history: bool) {
        if record_history {
            self.record_history(&metadata);
        }

        // If audio is enabled, generate chunks in a background thread
        // and WAIT for completion before starting the video.
        // Show progress modal during generation.
        if let Some(audio_player) = &self.audio_player {
            let config = audio_player.voiceover_config().clone();
            let chunks_map = audio_player.chunks_handle();
            let file_changes: Vec<(String, String, crate::git::FileStatus)> = metadata
                .changes
                .iter()
                .filter(|c| !c.is_excluded)
                .map(|c| (c.path.clone(), Self::build_diff_text(c), c.status.clone()))
                .collect();
            let message = metadata.message.clone();
            let speed_ms = self.speed_ms;
            let progress = self.audio_progress.clone();

            self.pending_metadata = Some(metadata);
            self.state = UIState::GeneratingAudio;
            self.audio_gen_handle = Some(std::thread::spawn(move || {
                crate::audio::generate_audio_chunks_with_progress(
                    config,
                    chunks_map,
                    message,
                    file_changes,
                    speed_ms,
                    progress,
                );
            }));
            return;
        }

        // No audio - start video immediately
        self.finish_play_commit(metadata);
    }

    /// Called once audio generation is done (or skipped) to actually start
    /// the animation with whatever audio chunks are available.
    fn finish_play_commit(&mut self, metadata: CommitMetadata) {
        self.engine.load_commit(&metadata);
        match self.playback_state {
            PlaybackState::Playing => self.engine.resume(),
            PlaybackState::Paused => self.engine.pause(),
        }
        self.state = UIState::Playing;
    }

    /// Build a text representation of file diff (including @@ hunk headers for duration calculation)
    fn build_diff_text(change: &crate::git::FileChange) -> String {
        let mut diff = String::new();

        for hunk in &change.hunks {
            // Include hunk header so calculate_animation_duration can parse it
            diff.push_str(&format!(
                "@@ -{},{} +{},{} @@\n",
                hunk.old_start, hunk.old_lines, hunk.new_start, hunk.new_lines
            ));
            for line in &hunk.lines {
                match line.change_type {
                    crate::git::LineChangeType::Addition => {
                        diff.push_str(&format!("+{}\n", line.content));
                    }
                    crate::git::LineChangeType::Deletion => {
                        diff.push_str(&format!("-{}\n", line.content));
                    }
                    crate::git::LineChangeType::Context => {
                        diff.push_str(&format!(" {}\n", line.content));
                    }
                }
            }
        }

        diff
    }

    fn record_history(&mut self, metadata: &CommitMetadata) {
        if let Some(index) = self.history_index {
            if index + 1 < self.history.len() {
                self.history.truncate(index + 1);
            }
        } else {
            self.history.clear();
        }

        self.history.push(metadata.clone());
        self.history_index = Some(self.history.len() - 1);
    }

    fn play_history_commit(&mut self, index: usize) -> bool {
        if let Some(metadata) = self.history.get(index).cloned() {
            self.history_index = Some(index);
            self.play_commit(metadata, false);
            return true;
        }

        false
    }

    fn toggle_pause(&mut self) {
        match self.playback_state {
            PlaybackState::Playing => {
                self.playback_state = PlaybackState::Paused;
                self.engine.pause();
            }
            PlaybackState::Paused => {
                self.playback_state = PlaybackState::Playing;
                self.engine.resume();
            }
        }
    }

    fn ensure_manual_pause(&mut self) {
        if self.playback_state != PlaybackState::Paused {
            self.playback_state = PlaybackState::Paused;
            self.engine.pause();
        }
    }

    fn step_line(&mut self) {
        self.ensure_manual_pause();
        let _ = self.engine.manual_step(StepMode::Line);
    }

    fn step_change(&mut self) {
        self.ensure_manual_pause();
        let _ = self.engine.manual_step(StepMode::Change);
    }

    fn step_line_back(&mut self) {
        self.ensure_manual_pause();
        let _ = self.engine.restore_line_checkpoint();
    }

    fn step_change_back(&mut self) {
        self.ensure_manual_pause();
        let _ = self.engine.restore_change_checkpoint();
    }

    fn handle_prev(&mut self) {
        if let Some(index) = self.history_index {
            if index > 0 {
                let target = index - 1;
                self.play_history_commit(target);
            }
        }
    }

    fn handle_next(&mut self) {
        if let Some(index) = self.history_index {
            if index + 1 < self.history.len() {
                let target = index + 1;
                if self.play_history_commit(target) {
                    return;
                }
            }
        }

        if self.repo.is_none() && self.diff_mode.is_none() {
            return;
        }

        self.advance_to_next_commit();
    }

    fn advance_to_next_commit(&mut self) -> bool {
        if let Some(diff_mode) = self.diff_mode {
            if let Some(repo) = self.repo {
                match repo.get_working_tree_diff(diff_mode) {
                    Ok(metadata) if !metadata.changes.is_empty() => {
                        self.load_commit(metadata);
                        return true;
                    }
                    _ => {
                        self.state = UIState::Finished;
                        return false;
                    }
                }
            }
            self.state = UIState::Finished;
            return false;
        }

        let Some(repo) = self.repo else {
            self.state = UIState::Finished;
            return false;
        };

        match self.fetch_repo_commit(repo) {
            Ok(metadata) => {
                self.load_commit(metadata);
                true
            }
            Err(_) => {
                if self.loop_playback {
                    repo.reset_index();
                    if let Ok(metadata) = self.fetch_repo_commit(repo) {
                        self.load_commit(metadata);
                        true
                    } else {
                        self.state = UIState::Finished;
                        false
                    }
                } else {
                    self.state = UIState::Finished;
                    false
                }
            }
        }
    }

    fn fetch_repo_commit(&self, repo: &GitRepository) -> Result<CommitMetadata> {
        if self.is_range_mode {
            return match self.order {
                PlaybackOrder::Random => repo.random_range_commit(),
                PlaybackOrder::Asc => repo.next_range_commit_asc(),
                PlaybackOrder::Desc => repo.next_range_commit_desc(),
            };
        }

        if let Some(spec) = &self.commit_spec {
            return repo.get_commit(spec);
        }

        match self.order {
            PlaybackOrder::Random => repo.random_commit(),
            PlaybackOrder::Asc => repo.next_asc_commit(),
            PlaybackOrder::Desc => repo.next_desc_commit(),
        }
    }

    /// Runs the main UI event loop.
    pub fn run(&mut self) -> Result<()> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        let result = self.run_loop(&mut terminal);

        self.cleanup(&mut terminal)?;

        result
    }

    fn cleanup(&mut self, terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
        disable_raw_mode()?;
        execute!(
            terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture
        )?;
        terminal.show_cursor()?;
        Ok(())
    }

    fn run_loop(&mut self, terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
        loop {
            // Check for Ctrl+C signal
            if self.should_exit.load(Ordering::Relaxed) {
                self.state = UIState::Finished;
            }

            // Update viewport dimensions for scroll calculation
            let size = terminal.size()?;
            // Editor area: 70% (right column) × 80% (editor pane) = 56% of total height
            let viewport_height = (size.height as f32 * 0.70 * 0.80) as usize;
            // Editor width: 70% (right column)
            let content_width = (size.width as f32 * 0.70) as usize;
            self.engine.set_viewport_height(viewport_height);
            self.engine.set_content_width(content_width);

            // Tick the animation engine (force redraw during audio generation)
            let needs_redraw = self.engine.tick() || matches!(self.state, UIState::GeneratingAudio);

            if needs_redraw {
                terminal.draw(|f| self.render(f))?;
            }

            // Poll for keyboard events at frame rate
            if event::poll(std::time::Duration::from_millis(8))? {
                if let Event::Key(key) = event::read()? {
                    match &self.state {
                        UIState::Menu => match key.code {
                            KeyCode::Esc => self.close_menu(),
                            KeyCode::Up | KeyCode::Char('k') => {
                                self.menu_index = self.menu_index.saturating_sub(1);
                            }
                            KeyCode::Down | KeyCode::Char('j') => {
                                self.menu_index = (self.menu_index + 1).min(2);
                            }
                            KeyCode::Enter => match self.menu_index {
                                0 => self.state = UIState::KeyBindings,
                                1 => self.state = UIState::About,
                                _ => self.state = UIState::Finished,
                            },
                            _ => {}
                        },
                        UIState::KeyBindings | UIState::About => match key.code {
                            KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => {
                                self.state = UIState::Menu;
                            }
                            _ => {}
                        },
                        UIState::Finished => match key.code {
                            KeyCode::Char('q') => {
                                self.state = UIState::Finished;
                            }
                            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                                self.state = UIState::Finished;
                            }
                            _ => {}
                        },
                        UIState::GeneratingAudio => match key.code {
                            KeyCode::Char('q') => {
                                self.audio_gen_handle = None;
                                self.pending_metadata = None;
                                self.state = UIState::Finished;
                            }
                            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                                self.audio_gen_handle = None;
                                self.pending_metadata = None;
                                self.state = UIState::Finished;
                            }
                            _ => {}
                        },
                        _ => match key.code {
                            KeyCode::Esc => self.open_menu(),
                            KeyCode::Char('q') => {
                                self.state = UIState::Finished;
                            }
                            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                                self.state = UIState::Finished;
                            }
                            KeyCode::Char(' ') => {
                                self.toggle_pause();
                            }
                            KeyCode::Char(ch) => match ch {
                                'h' => self.step_line_back(),
                                'l' => self.step_line(),
                                'H' => self.step_change_back(),
                                'L' => self.step_change(),
                                'p' => self.handle_prev(),
                                'n' => self.handle_next(),
                                _ => {}
                            },
                            _ => {}
                        },
                    }
                }
            }

            // State machine
            match self.state {
                UIState::Playing => {
                    if self.engine.is_finished() {
                        if self.repo.is_some() {
                            self.state = UIState::WaitingForNext {
                                resume_at: Instant::now()
                                    + Duration::from_millis(self.speed_ms * 100),
                            };
                        } else {
                            self.state = UIState::Finished;
                        }
                    }
                }
                UIState::WaitingForNext { resume_at } => {
                    if Instant::now() >= resume_at {
                        if matches!(self.playback_state, PlaybackState::Paused) {
                            continue;
                        }

                        self.advance_to_next_commit();
                    }
                }
                UIState::GeneratingAudio => {
                    // Check if background audio generation finished
                    if self
                        .audio_gen_handle
                        .as_ref()
                        .map(|h| h.is_finished())
                        .unwrap_or(true)
                    {
                        let _ = self.audio_gen_handle.take().map(|h| h.join());
                        if let Some(metadata) = self.pending_metadata.take() {
                            self.finish_play_commit(metadata);
                        }
                    }
                }
                UIState::Menu | UIState::KeyBindings | UIState::About => {
                    // Paused while in menu/dialog
                }
                UIState::Finished => {
                    break;
                }
            }
        }

        Ok(())
    }

    fn render(&mut self, f: &mut Frame) {
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

    fn render_menu(&self, f: &mut Frame, size: Rect) {
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

    fn render_keybindings(&self, f: &mut Frame, size: Rect) {
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

    fn render_about(&self, f: &mut Frame, size: Rect) {
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

    fn render_generating_audio(&self, f: &mut Frame, size: Rect) {
        let progress = self
            .audio_progress
            .lock()
            .ok()
            .map(|p| p.clone())
            .unwrap_or_else(|| "Initializing...".to_string());

        let lines = vec![
            Line::from(Span::styled(
                "Generating voiceover narration",
                Style::default().fg(self.theme.file_tree_current_file_fg),
            )),
            Line::from(""),
            Line::from(progress),
            Line::from(""),
            Line::from(Span::styled(
                "q  quit",
                Style::default().fg(self.theme.status_message),
            )),
        ];

        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Preparing AI Voiceover ")
            .padding(Padding::new(2, 2, 1, 1))
            .style(
                Style::default()
                    .fg(self.theme.status_message)
                    .bg(self.theme.editor_cursor_line_bg),
            );

        let area = Self::centered_rect(size, 60, 9);
        f.render_widget(Clear, area);
        f.render_widget(Paragraph::new(lines).block(block), area);
    }

    fn centered_rect(outer: Rect, width: u16, height: u16) -> Rect {
        Rect {
            x: outer.x + (outer.width.saturating_sub(width)) / 2,
            y: outer.y + (outer.height.saturating_sub(height)) / 2,
            width: width.min(outer.width),
            height: height.min(outer.height),
        }
    }
}
