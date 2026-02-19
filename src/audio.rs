use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::thread;

#[cfg(feature = "audio")]
use rodio::{Decoder, OutputStream, Sink};

/// Configuration for voiceover providers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceoverConfig {
    pub enabled: bool,
    pub provider: VoiceoverProvider,
    pub api_key: Option<String>,
    pub voice_id: Option<String>,
    pub model_id: Option<String>,
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
        }
    }
}

/// Audio player that handles voiceover playback
pub struct AudioPlayer {
    config: VoiceoverConfig,
    #[cfg(feature = "audio")]
    _stream: Option<OutputStream>,
    #[cfg(feature = "audio")]
    sink: Option<std::sync::Arc<std::sync::Mutex<Sink>>>,
}

impl AudioPlayer {
    pub fn new(config: VoiceoverConfig) -> Result<Self> {
        if !config.enabled {
            return Ok(Self {
                config,
                #[cfg(feature = "audio")]
                _stream: None,
                #[cfg(feature = "audio")]
                sink: None,
            });
        }

        #[cfg(feature = "audio")]
        {
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

        #[cfg(not(feature = "audio"))]
        {
            Ok(Self { config })
        }
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
        files_changed: usize,
        insertions: usize,
        deletions: usize,
    ) {
        if !self.config.enabled || self.config.api_key.is_none() {
            return;
        }

        let config = self.config.clone();
        #[cfg(feature = "audio")]
        let sink = self.sink.clone();

        thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().ok()?;
            rt.block_on(async {
                match Self::synthesize_speech_static(&config, &commit_hash, &author, &message, files_changed, insertions, deletions).await {
                    Ok(audio_data) => {
                        #[cfg(feature = "audio")]
                        if let Some(sink_arc) = sink {
                            if let Ok(sink_guard) = sink_arc.lock() {
                                let cursor = std::io::Cursor::new(audio_data);
                                if let Ok(source) = Decoder::new(cursor) {
                                    sink_guard.append(source);
                                }
                            }
                        }
                        #[cfg(not(feature = "audio"))]
                        let _ = audio_data; // Silence unused warning when audio feature disabled
                    }
                    Err(e) => {
                        eprintln!("Voiceover error: {}", e);
                    }
                }
                Some(())
            });
            Some(())
        });
    }

    /// Static helper to synthesize speech (for use in spawned threads)
    async fn synthesize_speech_static(
        config: &VoiceoverConfig,
        commit_hash: &str,
        author: &str,
        message: &str,
        files_changed: usize,
        insertions: usize,
        deletions: usize,
    ) -> Result<Vec<u8>> {
        let narration = {
            let commit_short = &commit_hash[..7.min(commit_hash.len())];
            let file_text = if files_changed == 1 { "file" } else { "files" };
            format!(
                "Reviewing commit {} by {}. {}. This commit modified {} {}, adding {} lines and removing {} lines.",
                commit_short, author, message, files_changed, file_text, insertions, deletions
            )
        };

        match config.provider {
            VoiceoverProvider::ElevenLabs => Self::synthesize_elevenlabs_static(config, &narration).await,
            VoiceoverProvider::Inworld => Self::synthesize_inworld_static(config, &narration).await,
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
        #[cfg(feature = "audio")]
        {
            if let Some(sink_arc) = &self.sink {
                if let Ok(sink) = sink_arc.lock() {
                    sink.stop();
                }
            }
        }
    }

    /// Check if audio is currently playing
    #[allow(dead_code)]
    pub fn is_playing(&self) -> bool {
        #[cfg(feature = "audio")]
        {
            if let Some(sink_arc) = &self.sink {
                if let Ok(sink) = sink_arc.lock() {
                    return !sink.empty();
                }
            }
        }
        false
    }
}

