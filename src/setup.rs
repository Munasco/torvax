use anyhow::Result;
use std::sync::Arc;

use crate::audio::{AudioPlayer, VoiceoverProvider};
use crate::cli::Args;
use crate::config;
use crate::config::Config;

/// Interactively prompt for an API key, persist it to config, and return it.
pub fn prompt_for_key(label: &str, help_url: &str, config_field: &str) -> Option<String> {
    use std::io::Write;
    println!();
    println!("torvax needs your {} to enable voiceover.", label);
    println!("  Get yours at: {}", help_url);
    print!("  Paste key (or press Enter to skip): ");
    std::io::stdout().flush().ok();

    let mut input = String::new();
    if std::io::stdin().read_line(&mut input).is_err() {
        return None;
    }

    let key = input.trim().to_string();
    if key.is_empty() {
        println!("  Skipping voiceover.");
        return None;
    }

    match config::Config::save_voiceover_key(config_field, &key) {
        Ok(()) => println!("  Saved to ~/.config/torvax/config.toml"),
        Err(e) => println!("  Warning: could not save key to config: {}", e),
    }

    Some(key)
}

/// Build an AudioPlayer from config + CLI args, prompting for missing keys if needed.
#[allow(clippy::arc_with_non_send_sync)]
pub fn create_audio_player(config: &Config, args: &Args) -> Result<Option<Arc<AudioPlayer>>> {
    let mut vc = config.voiceover.clone();

    if let Some(enabled) = args.voiceover {
        vc.enabled = enabled;
    }
    if args.elevenlabs {
        vc.provider = VoiceoverProvider::ElevenLabs;
        vc.enabled = true;
    }
    if let Some(ref p) = args.voiceover_provider {
        vc.provider = match p.to_lowercase().as_str() {
            "elevenlabs" => VoiceoverProvider::ElevenLabs,
            "inworld" => VoiceoverProvider::Inworld,
            _ => {
                eprintln!(
                    "Warning: Unknown voiceover provider '{}', using default (inworld)",
                    p
                );
                vc.provider
            }
        };
    }

    // Fill from environment variables
    if vc.enabled && vc.api_key.is_none() {
        let env_key = match vc.provider {
            VoiceoverProvider::ElevenLabs => std::env::var("ELEVENLABS_API_KEY"),
            VoiceoverProvider::Inworld => std::env::var("INWORLD_API_KEY"),
        };
        if let Ok(k) = env_key {
            vc.api_key = Some(k);
        }
    }
    if vc.openai_api_key.is_none() {
        if let Ok(k) = std::env::var("OPENAI_API_KEY") {
            vc.openai_api_key = Some(k);
        }
    }

    if !vc.enabled {
        return Ok(None);
    }

    // Prompt for missing keys
    if vc.openai_api_key.is_none() {
        vc.openai_api_key = prompt_for_key(
            "OpenAI API key (for GPT-5.2 explanations)",
            "https://platform.openai.com/api-keys",
            "openai_api_key",
        );
        if vc.openai_api_key.is_none() {
            return Ok(None);
        }
    }
    if vc.api_key.is_none() {
        vc.api_key = prompt_for_key(
            "Inworld API key (for text-to-speech)",
            "https://inworld.ai  →  API  →  Basic Auth key",
            "api_key",
        );
        if vc.api_key.is_none() {
            return Ok(None);
        }
    }

    // Enable LLM explanations — required for narration, persist to config
    vc.use_llm_explanations = true;
    let _ = Config::enable_voiceover();
    let _ = Config::save_voiceover_key("use_llm_explanations", "true");

    eprintln!("[SETUP] Creating AudioPlayer...");
    match AudioPlayer::new(vc) {
        Ok(player) => {
            eprintln!("[SETUP] AudioPlayer created successfully, wrapping in Arc...");
            let arc_player = Arc::new(player);
            eprintln!("[SETUP] Arc<AudioPlayer> created, returning...");
            Ok(Some(arc_player))
        }
        Err(e) => {
            eprintln!("\ntorvax: Failed to initialize audio: {}", e);
            Ok(None)
        }
    }
}
