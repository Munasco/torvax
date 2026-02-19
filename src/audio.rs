use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::thread;
use std::collections::VecDeque;
use rodio::{Decoder, OutputStream, Sink};

/// Configuration for voiceover providers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceoverConfig {
    pub enabled: bool,
    pub provider: VoiceoverProvider,
    pub api_key: Option<String>,
    pub voice_id: Option<String>,
    pub model_id: Option<String>,
    pub gemini_api_key: Option<String>,
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
            gemini_api_key: None,
            use_llm_explanations: false,
        }
    }
}

/// Represents a single voiceover segment with audio data
#[derive(Debug, Clone)]
pub struct VoiceoverSegment {
    pub text: String,
    pub audio_data: Option<Vec<u8>>,
    pub file_path: Option<String>,
    pub trigger_type: VoiceoverTrigger,
}

/// When to trigger this voiceover segment
#[derive(Debug, Clone, PartialEq)]
pub enum VoiceoverTrigger {
    FileOpen(String),      // Trigger when this file opens
    CommitStart,           // Trigger at commit start
    CommitEnd,             // Trigger at commit end
}

/// Audio player that handles synced voiceover playback
pub struct AudioPlayer {
    config: VoiceoverConfig,
    _stream: Option<OutputStream>,
    sink: Option<Arc<Mutex<Sink>>>,
    segment_queue: Arc<Mutex<VecDeque<VoiceoverSegment>>>,
}

impl AudioPlayer {
    pub fn new(config: VoiceoverConfig) -> Result<Self> {
        if !config.enabled {
            return Ok(Self {
                config,
                _stream: None,
                sink: None,
                segment_queue: Arc::new(Mutex::new(VecDeque::new())),
            });
        }

        let (_stream, stream_handle) = OutputStream::try_default()
            .context("Failed to create audio output stream")?;
        let sink = Sink::try_new(&stream_handle)
            .context("Failed to create audio sink")?;

        // Make sure sink is playing (not paused)
        sink.play();

        Ok(Self {
            config,
            _stream: Some(_stream),
            sink: Some(Arc::new(Mutex::new(sink))),
            segment_queue: Arc::new(Mutex::new(VecDeque::new())),
        })
    }

    /// Generate voiceover segments for a commit (one per file)
    pub fn generate_voiceover_segments(
        &self,
        commit_hash: String,
        author: String,
        message: String,
        file_changes: Vec<(String, String)>, // (filename, diff_text)
    ) -> Vec<VoiceoverSegment> {
        if !self.config.enabled || self.config.api_key.is_none() {
            return Vec::new();
        }

        let config = self.config.clone();
        let segment_queue = self.segment_queue.clone();

        eprintln!("[AUDIO] Pre-generating all voiceovers (this will take a few seconds)...");

        // Generate ALL audio synchronously BEFORE returning
        let rt = match tokio::runtime::Runtime::new() {
            Ok(rt) => rt,
            Err(e) => {
                eprintln!("Failed to create Tokio runtime for voiceover: {}", e);
                return Vec::new();
            }
        };

        rt.block_on(async {
                let mut segments = Vec::new();

                // Generate commit intro segment
                let intro_text = format!(
                    "Reviewing commit by {}. {}",
                    author,
                    message
                );

                eprintln!("[AUDIO] Generating intro voiceover...");
                if let Ok(audio_data) = Self::synthesize_speech_from_text(&config, &intro_text).await {
                    eprintln!("[AUDIO] Intro voiceover generated ({} bytes)", audio_data.len());

                    segments.push(VoiceoverSegment {
                        text: intro_text.clone(),
                        audio_data: Some(audio_data),
                        file_path: None,
                        trigger_type: VoiceoverTrigger::CommitStart,
                    });
                } else {
                    eprintln!("[AUDIO] Failed to generate intro voiceover");
                }

                // Limit to top 5 most important files to avoid rate limits
                let max_files = 5;
                let important_files: Vec<_> = file_changes.iter()
                    .filter(|(filename, _)| {
                        // Skip boring files
                        !filename.contains("package-lock.json") &&
                        !filename.contains("yarn.lock") &&
                        !filename.contains("pnpm-lock.yaml") &&
                        !filename.ends_with(".lock") &&
                        !filename.ends_with(".json") // Skip all JSON files for now
                    })
                    .take(max_files)
                    .collect();

                eprintln!("[AUDIO] Generating {} file voiceovers (limited from {})...",
                    important_files.len(), file_changes.len());
                for (i, (filename, diff)) in important_files.iter().enumerate() {
                    // Add delay between API calls to avoid rate limits
                    if i > 0 {
                        tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
                    }

                    eprintln!("[AUDIO] Processing file: {}", filename);
                    let narration = if config.use_llm_explanations && config.gemini_api_key.is_some() {
                        match Self::generate_file_explanation_with_retry(&config, &commit_hash, filename, diff).await {
                            Ok(explanation) => {
                                eprintln!("[AUDIO] Gemini explanation: {}", explanation);
                                explanation
                            }
                            Err(e) => {
                                eprintln!("[AUDIO] Failed to generate Gemini explanation for {}: {}", filename, e);
                                format!("Now reviewing changes in {}", filename)
                            }
                        }
                    } else {
                        format!("Now reviewing changes in {}", filename)
                    };

                    // Add delay before TTS call
                    tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;

                    match Self::synthesize_speech_from_text(&config, &narration).await {
                        Ok(audio_data) => {
                            eprintln!("[AUDIO] Generated audio for {} ({} bytes)", filename, audio_data.len());

                            // Store segment for later playback via triggers
                            segments.push(VoiceoverSegment {
                                text: narration.clone(),
                                audio_data: Some(audio_data),
                                file_path: Some(filename.clone()),
                                trigger_type: VoiceoverTrigger::FileOpen(filename.clone()),
                            });
                        }
                        Err(e) => {
                            eprintln!("[AUDIO] Failed to synthesize speech for {}: {}", filename, e);
                        }
                    }
                }

                eprintln!("[AUDIO] Generated {} total voiceover segments", segments.len());

                // Store segments in queue
                if let Ok(mut queue) = segment_queue.lock() {
                    *queue = segments.clone().into();
                }

                segments
            })
    }

    /// Trigger voiceover for a specific event
    pub fn trigger_voiceover(&self, trigger_type: VoiceoverTrigger) {
        if !self.config.enabled || self.sink.is_none() {
            eprintln!("[AUDIO] Trigger skipped (enabled: {}, sink: {})",
                self.config.enabled,
                self.sink.is_some());
            return;
        }

        eprintln!("[AUDIO] Triggering voiceover for: {:?}", trigger_type);

        let segment_queue = self.segment_queue.clone();
        let sink = self.sink.clone();

        thread::spawn(move || {
            // Find matching segment
            let segment = {
                if let Ok(mut queue) = segment_queue.lock() {
                    eprintln!("[AUDIO] Queue has {} segments", queue.len());
                    let pos = queue.iter().position(|s| s.trigger_type == trigger_type);
                    if let Some(index) = pos {
                        eprintln!("[AUDIO] Found matching segment at index {}", index);
                        Some(queue.remove(index).unwrap())
                    } else {
                        eprintln!("[AUDIO] No matching segment found for trigger");
                        None
                    }
                } else {
                    eprintln!("[AUDIO] Failed to lock queue");
                    None
                }
            };

            if let Some(seg) = segment {
                eprintln!("[AUDIO] Playing segment: {}", seg.text);
                if let Some(audio_data) = seg.audio_data {
                    if let Some(sink_arc) = sink {
                        if let Ok(sink_guard) = sink_arc.lock() {
                            let cursor = std::io::Cursor::new(audio_data);
                            if let Ok(source) = Decoder::new(cursor) {
                                sink_guard.append(source);
                                sink_guard.play(); // Make sure sink is playing
                                eprintln!("[AUDIO] Audio appended to sink and playing");
                            } else {
                                eprintln!("[AUDIO] Failed to decode audio");
                            }
                        } else {
                            eprintln!("[AUDIO] Failed to lock sink");
                        }
                    }
                } else {
                    eprintln!("[AUDIO] Segment has no audio data");
                }
            }
        });
    }

    /// Generate explanation with retry logic for rate limits
    async fn generate_file_explanation_with_retry(
        config: &VoiceoverConfig,
        commit_hash: &str,
        filename: &str,
        diff: &str,
    ) -> Result<String> {
        let max_retries = 3;
        let mut retry_delay = 1000; // Start with 1 second

        for attempt in 0..max_retries {
            match Self::generate_file_explanation(config, commit_hash, filename, diff).await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    let error_msg = format!("{}", e);
                    if error_msg.contains("rate_limit") || error_msg.contains("429") {
                        if attempt < max_retries - 1 {
                            eprintln!("Rate limit hit for {}. Retrying in {}ms... (attempt {}/{})",
                                filename, retry_delay, attempt + 1, max_retries);
                            tokio::time::sleep(tokio::time::Duration::from_millis(retry_delay)).await;
                            retry_delay *= 2; // Exponential backoff
                            continue;
                        }
                    }
                    return Err(e);
                }
            }
        }

        anyhow::bail!("Max retries exceeded")
    }

    /// Generate explanation for a specific file change using Gemini
    async fn generate_file_explanation(
        config: &VoiceoverConfig,
        commit_hash: &str,
        filename: &str,
        diff: &str,
    ) -> Result<String> {
        let api_key = config
            .gemini_api_key
            .as_ref()
            .context("Gemini API key not configured")?;

        let commit_short = &commit_hash[..7.min(commit_hash.len())];

        // Build context for the LLM - MORE DETAILED
        let user_prompt = format!(
            "You're narrating a live code walkthrough. Explain what's happening in this file change. \
            Be detailed but natural - explain WHAT changed, WHY it matters, and HOW it works. \
            Talk like you're teaching someone while watching code being typed. \
            Keep it to 2-3 sentences (30-40 words). Use spoken language - no code syntax, no HTML tags.\n\n\
            File: {}\n\
            Commit: {}\n\n\
            Code changes:\n{}\n\n\
            Example: 'We're adding authentication middleware here that checks JWT tokens on every request. \
            This prevents unauthorized access by validating the token signature and expiration time before allowing the request through.'",
            filename, commit_short,
            diff.lines().take(50).collect::<Vec<_>>().join("\n")
        );

        // Use Gemini 3 Pro via REST API
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-3-pro-preview:generateContent?key={}",
            api_key
        );

        let client = reqwest::Client::new();
        let response = client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&serde_json::json!({
                "contents": [{
                    "parts": [{
                        "text": user_prompt
                    }]
                }],
                "generationConfig": {
                    "temperature": 0.6,
                    "maxOutputTokens": 200
                }
            }))
            .send()
            .await
            .context("Failed to send request to Gemini API")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            anyhow::bail!("Gemini API error ({}): {}", status, error_text);
        }

        let response_json: serde_json::Value = response
            .json()
            .await
            .context("Failed to parse Gemini response")?;

        let explanation = response_json["candidates"][0]["content"]["parts"][0]["text"]
            .as_str()
            .context("Failed to extract explanation from Gemini response")?
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
            .unwrap_or("eleven_flash_v2_5");

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
