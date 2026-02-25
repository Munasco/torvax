mod playback;
mod rendering;

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
use ratatui::{backend::CrosstermBackend, Terminal};

use crate::animation::{AnimationEngine, SpeedRule};
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
    audio_progress: Arc<Mutex<(String, f32)>>, // (status message, progress 0.0-1.0)
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
            audio_progress: Arc::new(Mutex::new((String::new(), 0.0))),
        }
    }

    /// Sets the diff mode for working tree diff playback.
    pub fn set_diff_mode(&mut self, mode: Option<DiffMode>) {
        self.diff_mode = mode;
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
}
