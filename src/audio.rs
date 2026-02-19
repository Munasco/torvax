use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::thread;
use rodio::{Decoder, OutputStream, Sink};

/// Configuration for voiceover providers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceoverConfig {
    pub enabled: bool,
    pub provider: VoiceoverProvider,
    pub api_key: Option<String>,
    pub voice_id: Option<String>,
    pub model_id: Option<String>,
    pub openai_api_key: Option<String>,
    pub use_llm_explanations: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum VoiceoverProvider {
    #[serde(rename = "elevenlabs")]
    ElevenLabs,
    #[serde(rename = "inworld")]
    Inworld,
}

impl Default for VoiceoverConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            provider: VoiceoverProvider::ElevenLabs,
            api_key: None,
            voice_id: None,
            model_id: None,
            openai_api_key: None,
            use_llm_explanations: false,
        }
    }
}

/// Audio player that handles voiceover playback
pub struct AudioPlayer {
    config: VoiceoverConfig,
    _stream: Option<OutputStream>,
    sink: Option<std::sync::Arc<std::sync::Mutex<Sink>>>,
}

impl AudioPlayer {
    pub fn new(config: VoiceoverConfig) -> Result<Self> {
        if !config.enabled {
            return Ok(Self {
                config,
                _stream: None,
                sink: None,
            });
        }

        let (_stream, stream_handle) = OutputStream::try_default()
            .context("Failed to create audio output stream")?;
        let sink = Sink::try_new(&stream_handle)
            .context("Failed to create audio sink")?;

        Ok(Self {
            config,
            _stream: Some(_stream),
            sink: Some(std::sync::Arc::new(std::sync::Mutex::new(sink))),
        })
    }

    /// Generate narration text from commit metadata
    #[allow(dead_code)]
    pub fn generate_narration(
        &self,
        commit_hash: &str,
        author: &str,
        message: &str,
        files_changed: usize,
        insertions: usize,
        deletions: usize,
    ) -> String {
        let commit_short = &commit_hash[..7.min(commit_hash.len())];
        
        let file_text = if files_changed == 1 {
            "file"
        } else {
            "files"
        };

        format!(
            "Reviewing commit {} by {}. {}. This commit modified {} {}, adding {} lines and removing {} lines.",
            commit_short,
            author,
            message,
            files_changed,
            file_text,
            insertions,
            deletions
        )
    }

    /// Play narration for a commit asynchronously in a background thread
    pub fn play_commit_narration_async(
        &self,
        commit_hash: String,
        author: String,
        message: String,
        file_changes: Vec<(String, String)>, // (filename, diff_text)
    ) {
        if !self.config.enabled || self.config.api_key.is_none() {
            return;
        }

        let config = self.config.clone();
        let sink = self.sink.clone();

        thread::spawn(move || {
            // Create runtime in the spawned thread to avoid blocking the UI
            let rt = match tokio::runtime::Runtime::new() {
                Ok(rt) => rt,
                Err(e) => {
                    eprintln!("Failed to create Tokio runtime for voiceover: {}", e);
                    return;
                }
            };
            
            rt.block_on(async {
                // Generate narration text (either via LLM or simple summary)
                let narration = if config.use_llm_explanations && config.openai_api_key.is_some() {
                    match Self::generate_llm_explanation(&config, &commit_hash, &author, &message, &file_changes).await {
                        Ok(explanation) => explanation,
                        Err(e) => {
                            eprintln!("Failed to generate LLM explanation: {}. Falling back to simple narration.", e);
                            Self::generate_simple_narration(&commit_hash, &author, &message, file_changes.len())
                        }
                    }
                } else {
                    Self::generate_simple_narration(&commit_hash, &author, &message, file_changes.len())
                };
                
                // Synthesize speech from narration
                match Self::synthesize_speech_from_text(&config, &narration).await {
                    Ok(audio_data) => {
                        if let Some(sink_arc) = sink {
                            if let Ok(sink_guard) = sink_arc.lock() {
                                let cursor = std::io::Cursor::new(audio_data);
                                if let Ok(source) = Decoder::new(cursor) {
                                    sink_guard.append(source);
                                }
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("Voiceover error: {}", e);
                    }
                }
            });
        });
    }

    /// Generate a simple narration without LLM
    fn generate_simple_narration(
        commit_hash: &str,
        author: &str,
        message: &str,
        files_changed: usize,
    ) -> String {
        let commit_short = &commit_hash[..7.min(commit_hash.len())];
        let file_text = if files_changed == 1 { "file" } else { "files" };
        
        format!(
            "Reviewing commit {} by {}. {}. This commit modified {} {}.",
            commit_short,
            author,
            message,
            files_changed,
            file_text
        )
    }

    /// Generate an intelligent explanation using OpenAI
    async fn generate_llm_explanation(
        config: &VoiceoverConfig,
        commit_hash: &str,
        author: &str,
        message: &str,
        file_changes: &[(String, String)],
    ) -> Result<String> {
        let api_key = config
            .openai_api_key
            .as_ref()
            .context("OpenAI API key not configured")?;

        let commit_short = &commit_hash[..7.min(commit_hash.len())];

        // Build context for the LLM
        let mut context = format!(
            "You are reviewing a git commit. Provide a clear, conversational explanation of what this code change does and why it matters. Keep it concise (2-3 sentences max).\n\n\
            Commit: {}\n\
            Author: {}\n\
            Message: {}\n\n\
            Files changed:\n",
            commit_short, author, message
        );

        // Include file diffs (limited to avoid token limits)
        for (filename, diff) in file_changes.iter().take(5) {
            context.push_str(&format!("\n--- {} ---\n", filename));
            // Limit diff size
            let diff_lines: Vec<&str> = diff.lines().take(50).collect();
            context.push_str(&diff_lines.join("\n"));
            context.push('\n');
        }

        if file_changes.len() > 5 {
            context.push_str(&format!("\n... and {} more files\n", file_changes.len() - 5));
        }

        // Call OpenAI API
        let client = reqwest::Client::new();
        let response = client
            .post("https://api.openai.com/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&serde_json::json!({
                "model": "gpt-3.5-turbo",
                "messages": [
                    {
                        "role": "system",
                        "content": "You are a code reviewer providing clear, conversational explanations of git commits. Explain what the code does and why, not just what files changed. Be concise and use natural language suitable for text-to-speech."
                    },
                    {
                        "role": "user",
                        "content": context
                    }
                ],
                "max_tokens": 200,
                "temperature": 0.7
            }))
            .send()
            .await
            .context("Failed to send request to OpenAI API")?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            anyhow::bail!("OpenAI API error: {}", error_text);
        }

        let response_json: serde_json::Value = response
            .json()
            .await
            .context("Failed to parse OpenAI response")?;

        let explanation = response_json["choices"][0]["message"]["content"]
            .as_str()
            .context("Failed to extract explanation from OpenAI response")?
            .trim()
            .to_string();

        Ok(explanation)
    }

    /// Synthesize speech from text using configured TTS provider
    async fn synthesize_speech_from_text(config: &VoiceoverConfig, text: &str) -> Result<Vec<u8>> {
        match config.provider {
            VoiceoverProvider::ElevenLabs => Self::synthesize_elevenlabs_static(config, text).await,
            VoiceoverProvider::Inworld => Self::synthesize_inworld_static(config, text).await,
        }
    }

    /// Synthesize speech using ElevenLabs API (static version)
    async fn synthesize_elevenlabs_static(config: &VoiceoverConfig, text: &str) -> Result<Vec<u8>> {
        let api_key = config
            .api_key
            .as_ref()
            .context("ElevenLabs API key not configured")?;

        let voice_id = config
            .voice_id
            .as_ref()
            .map(|s| s.as_str())
            .unwrap_or("21m00Tcm4TlvDq8ikWAM"); // Default: Rachel voice

        let model_id = config
            .model_id
            .as_ref()
            .map(|s| s.as_str())
            .unwrap_or("eleven_monolingual_v1");

        let url = format!(
            "https://api.elevenlabs.io/v1/text-to-speech/{}",
            voice_id
        );

        let client = reqwest::Client::new();
        let response = client
            .post(&url)
            .header("xi-api-key", api_key)
            .header("Content-Type", "application/json")
            .json(&serde_json::json!({
                "text": text,
                "model_id": model_id,
                "voice_settings": {
                    "stability": 0.5,
                    "similarity_boost": 0.75
                }
            }))
            .send()
            .await
            .context("Failed to send request to ElevenLabs API")?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            anyhow::bail!("ElevenLabs API error: {}", error_text);
        }

        let audio_data = response
            .bytes()
            .await
            .context("Failed to read audio response")?
            .to_vec();

        Ok(audio_data)
    }

    /// Synthesize speech using Inworld API (static version)
    async fn synthesize_inworld_static(config: &VoiceoverConfig, text: &str) -> Result<Vec<u8>> {
        let api_key = config
            .api_key
            .as_ref()
            .context("Inworld API key not configured")?;

        // Inworld TTS API endpoint
        // Note: This is a placeholder - actual Inworld API structure may vary
        let url = "https://api.inworld.ai/v1/text-to-speech";

        let client = reqwest::Client::new();
        let response = client
            .post(url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&serde_json::json!({
                "text": text,
                "voice": config.voice_id.as_ref().unwrap_or(&"default".to_string()),
            }))
            .send()
            .await
            .context("Failed to send request to Inworld API")?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            anyhow::bail!("Inworld API error: {}", error_text);
        }

        let audio_data = response
            .bytes()
            .await
            .context("Failed to read audio response")?
            .to_vec();

        Ok(audio_data)
    }

    /// Stop any currently playing audio
    #[allow(dead_code)]
    pub fn stop(&mut self) {
        if let Some(sink_arc) = &self.sink {
            if let Ok(sink) = sink_arc.lock() {
                sink.stop();
            }
        }
    }

    /// Check if audio is currently playing
    #[allow(dead_code)]
    pub fn is_playing(&self) -> bool {
        if let Some(sink_arc) = &self.sink {
            if let Ok(sink) = sink_arc.lock() {
                return !sink.empty();
            }
        }
        false
    }
}

