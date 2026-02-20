use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};

/// Defines the order in which commits are played back during animation.
#[derive(Debug, Clone, Copy, Default, ValueEnum)]
pub enum PlaybackOrder {
    #[default]
    Random,
    Asc,
    Desc,
}

#[derive(Parser, Debug)]
#[command(
    name = "torvax",
    version,
    about = "Git review of your diffs, like a movie",
    long_about = "torvax replays your git history as a narrated code walkthrough — AI explains what changed and why while the code types itself on screen."
)]
pub struct Args {
    #[arg(short, long, value_name = "PATH",
          help = "Path to Git repository (defaults to current directory)")]
    pub path: Option<PathBuf>,

    #[arg(short, long, value_name = "HASH_OR_RANGE",
          help = "Replay a specific commit or commit range (e.g., HEAD~5..HEAD or abc123..)")]
    pub commit: Option<String>,

    #[arg(short, long, value_name = "MS",
          help = "Typing speed in milliseconds per character (overrides config file)")]
    pub speed: Option<u64>,

    #[arg(short, long, value_name = "NAME",
          help = "Theme to use (overrides config file)")]
    pub theme: Option<String>,

    #[arg(long, num_args = 0..=1, default_missing_value = "true", value_name = "BOOL",
          help = "Show background colors (use --background=false for transparent background)")]
    pub background: Option<bool>,

    #[arg(long, value_enum, value_name = "ORDER",
          help = "Commit playback order (overrides config file)")]
    pub order: Option<PlaybackOrder>,

    #[arg(long = "loop", num_args = 0..=1, default_missing_value = "true", value_name = "BOOL",
          help = "Loop the animation continuously")]
    pub loop_playback: Option<bool>,

    #[arg(long, help = "Display third-party license information")]
    pub license: bool,

    #[arg(short = 'a', long, value_name = "PATTERN",
          value_parser = |s: &str| if s.trim().is_empty() {
              Err("Author pattern cannot be empty".to_string())
          } else {
              Ok(s.to_string())
          },
          help = "Filter commits by author name or email (partial match, case-insensitive)")]
    pub author: Option<String>,

    #[arg(long, value_name = "DATE",
          help = "Show commits before this date (e.g., '2024-01-01', '1 week ago', 'yesterday')")]
    pub before: Option<String>,

    #[arg(long, value_name = "DATE",
          help = "Show commits after this date (e.g., '2024-01-01', '1 week ago', 'yesterday')")]
    pub after: Option<String>,

    #[arg(short = 'i', long = "ignore", value_name = "PATTERN",
          action = clap::ArgAction::Append,
          help = "Ignore files matching pattern (gitignore syntax, can be specified multiple times)")]
    pub ignore: Vec<String>,

    #[arg(long = "ignore-file", value_name = "PATH",
          help = "Path to file containing ignore patterns (one per line, like .gitignore)")]
    pub ignore_file: Option<PathBuf>,

    #[arg(long = "speed-rule", value_name = "PATTERN:MS",
          action = clap::ArgAction::Append,
          help = "Set typing speed for files matching pattern (e.g., '*.java:50'). Can be specified multiple times.")]
    pub speed_rule: Vec<String>,

    #[arg(long = "voiceover", num_args = 0..=1, default_missing_value = "true", value_name = "BOOL",
          help = "Enable voiceover narration (Inworld TTS + GPT-5.2 explanations)")]
    pub voiceover: Option<bool>,

    #[arg(long = "elevenlabs", conflicts_with = "voiceover_provider",
          help = "Use ElevenLabs TTS instead of Inworld")]
    pub elevenlabs: bool,

    #[arg(long = "voiceover-provider", value_name = "PROVIDER",
          help = "Voiceover provider to use: elevenlabs or inworld (overrides config file)")]
    pub voiceover_provider: Option<String>,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Theme management commands
    Theme {
        #[command(subcommand)]
        command: ThemeCommands,
    },
    /// Show staged working tree changes (use --unstaged for unstaged changes)
    Diff {
        #[arg(long, help = "Show unstaged changes instead of staged")]
        unstaged: bool,

        #[arg(short, long, value_name = "MS",
              help = "Typing speed in milliseconds per character")]
        speed: Option<u64>,

        #[arg(short, long, value_name = "NAME", help = "Theme to use")]
        theme: Option<String>,

        #[arg(long, num_args = 0..=1, default_missing_value = "true", value_name = "BOOL",
              help = "Show background colors (use --background=false for transparent)")]
        background: Option<bool>,

        #[arg(long = "loop", num_args = 0..=1, default_missing_value = "true", value_name = "BOOL",
              help = "Loop the animation continuously")]
        loop_playback: Option<bool>,

        #[arg(short = 'i', long = "ignore", value_name = "PATTERN",
              action = clap::ArgAction::Append,
              help = "Ignore files matching pattern (gitignore syntax)")]
        ignore: Vec<String>,

        #[arg(long = "speed-rule", value_name = "PATTERN:MS",
              action = clap::ArgAction::Append,
              help = "Set typing speed for files matching pattern (e.g., '*.java:50')")]
        speed_rule: Vec<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum ThemeCommands {
    /// List all available themes
    List,
    /// Set default theme in config file
    Set {
        #[arg(value_name = "NAME", help = "Theme name to set as default")]
        name: String,
    },
}

impl Args {
    /// Validate args and return the Git repository root path.
    pub fn validate(&self) -> Result<PathBuf> {
        let start_path = self.path.clone().unwrap_or_else(|| PathBuf::from("."));
        if !start_path.exists() {
            anyhow::bail!("Path does not exist: {}", start_path.display());
        }
        let canonical = start_path.canonicalize().context("Failed to resolve path")?;
        Self::find_git_root(&canonical).ok_or_else(|| {
            anyhow::anyhow!(
                "Not a Git repository: {} (or any parent directories)",
                start_path.display()
            )
        })
    }

    pub fn find_git_root(start_path: &Path) -> Option<PathBuf> {
        let mut current = if start_path.is_file() {
            start_path.parent()?.to_path_buf()
        } else {
            start_path.to_path_buf()
        };
        loop {
            if current.join(".git").exists() {
                return Some(current);
            }
            if !current.pop() {
                return None;
            }
        }
    }
}
