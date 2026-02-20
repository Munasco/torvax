mod animation;
mod audio;
mod cli;
mod config;
mod git;
mod panes;
mod setup;
mod syntax;
mod theme;
mod ui;
mod widgets;

use anyhow::Result;
use clap::Parser;
use cli::{Args, Commands, PlaybackOrder, ThemeCommands};
use config::Config;
use git::{DiffMode, GitRepository};
use theme::Theme;
use ui::UI;

fn main() -> Result<()> {
    let args = Args::parse();

    if args.license {
        println!("{}", include_str!("../LICENSE-THIRD-PARTY"));
        return Ok(());
    }

    if let Some(ref command) = args.command {
        return handle_subcommand(command, &args);
    }

    run_playback(args)
}

fn handle_subcommand(command: &Commands, args: &Args) -> Result<()> {
    match command {
        Commands::Theme { command } => match command {
            ThemeCommands::List => {
                println!("Available themes:");
                for t in Theme::available_themes() {
                    println!("  - {}", t);
                }
            }
            ThemeCommands::Set { name } => {
                Theme::load(name)?;
                let mut config = Config::load().unwrap_or_default();
                config.theme = name.clone();
                config.save()?;
                let path = Config::config_path()?;
                println!("Theme set to '{}' in {}", name, path.display());
            }
        },
        Commands::Diff {
            unstaged,
            speed,
            theme,
            background,
            loop_playback,
            ignore,
            speed_rule,
        } => {
            let repo_path = args.validate()?;
            let repo = GitRepository::open(&repo_path)?;
            let mode = if *unstaged { DiffMode::Unstaged } else { DiffMode::Staged };
            let metadata = repo.get_working_tree_diff(mode)?;

            if metadata.changes.is_empty() {
                println!("No changes to display");
                return Ok(());
            }

            let config = Config::load()?;
            let mut patterns = config.ignore_patterns.clone();
            patterns.extend(ignore.clone());
            git::init_ignore_patterns(&patterns).ok();

            let speed_rules = build_speed_rules(speed_rule, &config.speed_rules);
            let theme_name = theme.as_deref().unwrap_or(&config.theme);
            let speed = speed.unwrap_or(config.speed);
            let background = background.unwrap_or(config.background);
            let loop_playback = loop_playback.unwrap_or(false);
            let mut theme = Theme::load(theme_name)?;
            if !background { theme = theme.with_transparent_background(); }

            let audio_player = setup::create_audio_player(&config, args)?;
            let repo_ref = if loop_playback { Some(&repo) } else { None };
            let mut ui = UI::new(speed, repo_ref, theme, PlaybackOrder::Asc, loop_playback,
                                 None, false, speed_rules, audio_player);
            ui.set_diff_mode(Some(mode));
            ui.load_commit(metadata);
            ui.run()?;
        }
    }
    Ok(())
}

fn run_playback(args: Args) -> Result<()> {
    let repo_path = args.validate()?;
    let mut repo = GitRepository::open(&repo_path)?;

    if args.author.is_some() { repo.set_author_filter(args.author.clone()); }

    if let Some(ref s) = args.before {
        repo.set_before_filter(Some(git::parse_date(s)?));
    }
    if let Some(ref s) = args.after {
        repo.set_after_filter(Some(git::parse_date(s)?));
    }

    let is_range = args.commit.as_ref().map(|c| c.contains("..")).unwrap_or(false);
    let is_filtered = args.author.is_some() || args.before.is_some() || args.after.is_some();
    let config = Config::load()?;

    let mut patterns = config.ignore_patterns.clone();
    if let Some(path) = &args.ignore_file {
        if let Ok(content) = std::fs::read_to_string(path) {
            patterns.extend(content.lines()
                .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
                .map(String::from));
        }
    }
    patterns.extend(args.ignore.clone());
    git::init_ignore_patterns(&patterns).ok();

    let theme_name = args.theme.as_deref().unwrap_or(&config.theme);
    let speed = args.speed.unwrap_or(config.speed);
    let background = args.background.unwrap_or(config.background);
    let loop_playback = args.loop_playback.unwrap_or(config.loop_playback);
    let mut order = args.order.unwrap_or(match config.order.as_str() {
        "asc" => PlaybackOrder::Asc,
        "desc" => PlaybackOrder::Desc,
        _ => PlaybackOrder::Random,
    });
    if (is_range || is_filtered) && args.order.is_none() {
        order = PlaybackOrder::Asc;
    }

    let mut theme = Theme::load(theme_name)?;
    if !background { theme = theme.with_transparent_background(); }

    if is_range { repo.set_commit_range(args.commit.as_ref().unwrap())?; }

    let metadata = if is_range {
        match order {
            PlaybackOrder::Random => repo.random_range_commit()?,
            PlaybackOrder::Asc => repo.next_range_commit_asc()?,
            PlaybackOrder::Desc => repo.next_range_commit_desc()?,
        }
    } else if let Some(ref hash) = args.commit {
        repo.get_commit(hash)?
    } else {
        match order {
            PlaybackOrder::Random => repo.random_commit()?,
            PlaybackOrder::Asc => repo.next_asc_commit()?,
            PlaybackOrder::Desc => repo.next_desc_commit()?,
        }
    };

    let speed_rules = build_speed_rules(&args.speed_rule, &config.speed_rules);
    let audio_player = setup::create_audio_player(&config, &args)?;
    let is_commit_specified = args.commit.is_some();
    let repo_ref = if is_range || is_filtered || !is_commit_specified || loop_playback {
        Some(&repo)
    } else {
        None
    };

    let mut ui = UI::new(speed, repo_ref, theme, order, loop_playback,
                         args.commit.clone(), is_range, speed_rules, audio_player);
    ui.load_commit(metadata);
    ui.run()?;
    Ok(())
}

fn build_speed_rules(
    cli_rules: &[String],
    config_rules: &[String],
) -> Vec<animation::SpeedRule> {
    cli_rules
        .iter()
        .chain(config_rules.iter())
        .filter_map(|s| {
            animation::SpeedRule::parse(s).or_else(|| {
                eprintln!("Warning: Invalid speed rule '{}', skipping", s);
                None
            })
        })
        .collect()
}
