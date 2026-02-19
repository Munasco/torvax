use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::thread;
use std::collections::VecDeque;
use rodio::{Decoder, OutputStream, Sink};
use crate::git::FileStatus;
use base64::{Engine as _, engine::general_purpose};

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

/// Project context for providing LLM with repository information
#[derive(Debug, Clone)]
pub struct ProjectContext {
    pub repo_name: String,
    pub description: String,
}

/// Estimate audio duration from text length
/// Average speaking rate: 150 words/minute = 2.5 words/second
fn estimate_audio_duration(text: &str) -> f32 {
    let word_count = text.split_whitespace().count() as f32;
    let words_per_second = 2.5;
    word_count / words_per_second
}

impl Default for VoiceoverConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            provider: VoiceoverProvider::Inworld,  // Default to Inworld
            api_key: None,
            voice_id: None,
            model_id: None,
            gemini_api_key: None,
            use_llm_explanations: false,
        }
    }
}

/// Extract project context from repository (no caching - always fresh, LLM-only)
fn extract_project_context() -> ProjectContext {
    let repo_name = extract_repo_name().unwrap_or_else(|| {
        std::env::current_dir()
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
            .unwrap_or_else(|| "repository".to_string())
    });

    // Placeholder - will be replaced by LLM generation (no README fallback)
    ProjectContext {
        repo_name,
        description: String::new(), // Empty until LLM fills it
    }
}

/// Generate AI-powered project description using Gemini (async version)
async fn generate_project_context_with_llm(config: &VoiceoverConfig) -> Result<String> {
    let api_key = config
        .gemini_api_key
        .as_ref()
        .context("Gemini API key not configured")?;

    // Sample key files from repository (deepwiki principle)
    let mut context_files = Vec::new();

    // Prioritize these files for context
    let key_files = ["README.md", "Cargo.toml", "package.json", "src/main.rs", "src/lib.rs"];

    for file_path in key_files {
        if let Ok(content) = std::fs::read_to_string(file_path) {
            let preview = content.chars().take(500).collect::<String>();
            context_files.push(format!("File: {}\n{}", file_path, preview));
        }
    }

    if context_files.is_empty() {
        anyhow::bail!("No key files found for context extraction");
    }

    let repo_context = context_files.join("\n\n---\n\n");

    // Use Gemini to generate project description
    let prompt = format!(
        "You are analyzing a code repository. Based on the following files, provide a thorough \
        description (200-400 words) of what this project does, its main purpose, key features, \
        and architecture. This will be used to provide context for teaching someone about code changes.\n\n\
        Repository files:\n{}\n\n\
        Provide ONLY the description, no additional commentary. Be detailed enough to give full context.",
        repo_context
    );

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
                "parts": [{"text": prompt}]
            }],
            "generationConfig": {
                "temperature": 0.5,
                "maxOutputTokens": 800  // Doubled for fuller project descriptions
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

    let description = response_json["candidates"][0]["content"]["parts"][0]["text"]
        .as_str()
        .context("Failed to extract description from Gemini response")?
        .trim()
        .to_string();

    Ok(description)
}


/// Extract repository name from .git/config
fn extract_repo_name() -> Option<String> {
    let config = std::fs::read_to_string(".git/config").ok()?;

    for line in config.lines() {
        if line.contains("url = ") {
            let url = line.split("url = ").nth(1)?.trim();

            let repo_part = url
                .trim_end_matches(".git")
                .rsplit('/')
                .next()?;

            return Some(repo_part.to_string());
        }
    }

    None
}


/// Represents a single voiceover chunk for a portion of a diff
#[derive(Debug, Clone)]
pub struct DiffChunk {
    pub chunk_id: usize,
    pub file_path: String,
    pub start_line: usize,
    pub end_line: usize,
    pub explanation: String,
    pub audio_data: Option<Vec<u8>>,
    pub estimated_duration_secs: f32,
}

/// Represents a single voiceover segment with audio data
#[derive(Debug, Clone)]
pub struct VoiceoverSegment {
    pub text: String,
    pub audio_data: Option<Vec<u8>>,
    pub file_path: Option<String>,
    pub trigger_type: VoiceoverTrigger,
    pub estimated_duration_secs: f32,  // Estimated audio duration in seconds
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
    /// Diff chunks with pre-generated audio (chunk_id -> chunk)
    chunks: Arc<Mutex<std::collections::HashMap<usize, DiffChunk>>>,
}

impl AudioPlayer {
    pub fn new(config: VoiceoverConfig) -> Result<Self> {
        if !config.enabled {
            return Ok(Self {
                config,
                _stream: None,
                sink: None,
                segment_queue: Arc::new(Mutex::new(VecDeque::new())),
                chunks: Arc::new(Mutex::new(std::collections::HashMap::new())),
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
            chunks: Arc::new(Mutex::new(std::collections::HashMap::new())),
        })
    }

    /// Trigger playback of a specific audio chunk
    pub fn trigger_chunk(&self, chunk_id: usize) {
        if !self.config.enabled || self.sink.is_none() {
            return;
        }

        eprintln!("[AUDIO] Playing chunk {}", chunk_id);

        let chunks = self.chunks.clone();
        let sink = self.sink.clone();

        thread::spawn(move || {
            let chunk = {
                if let Ok(chunks_guard) = chunks.lock() {
                    chunks_guard.get(&chunk_id).cloned()
                } else {
                    None
                }
            };

            if let Some(chunk) = chunk {
                if let Some(audio_data) = chunk.audio_data {
                    if let Some(sink_arc) = sink {
                        if let Ok(sink_guard) = sink_arc.lock() {
                            let cursor = std::io::Cursor::new(audio_data);
                            if let Ok(source) = Decoder::new(cursor) {
                                sink_guard.append(source);
                                sink_guard.play();
                                eprintln!("[AUDIO] Chunk {} playing (~{:.1}s)", chunk_id, chunk.estimated_duration_secs);

                                // TODO: Signal animation engine when audio finishes
                                // For now, we'll rely on the estimated duration
                            }
                        }
                    }
                }
            }
        });
    }

    /// Split a file's diff into explainable chunks using LLM
    async fn split_diff_into_chunks(
        config: &VoiceoverConfig,
        project_context: &ProjectContext,
        commit_message: &str,
        filename: &str,
        diff: &str,
    ) -> Result<Vec<DiffChunk>> {
        let api_key = config
            .gemini_api_key
            .as_ref()
            .context("Gemini API key not configured")?;

        let prompt = format!(
            "You are analyzing a code diff for teaching purposes. Split this diff into logical, explainable chunks.\n\n\
            PROJECT: {} - {}\n\
            COMMIT: \"{}\"\n\
            FILE: {}\n\n\
            DIFF:\n{}\n\n\
            INSTRUCTIONS:\n\
            Break this diff into 2-5 logical chunks where each chunk represents a cohesive change that can be \
            explained independently. For example:\n\
            - Chunk 1: Lines 10-25 (adding imports and setup)\n\
            - Chunk 2: Lines 30-60 (implementing main function)\n\
            - Chunk 3: Lines 65-80 (adding error handling)\n\n\
            For EACH chunk, provide:\n\
            1. start_line: The starting line number\n\
            2. end_line: The ending line number\n\
            3. explanation: A 40-60 word explanation of what this chunk does and why (conversational teaching style)\n\n\
            Return ONLY valid JSON array in this exact format:\n\
            [\n\
              {{\"start_line\": 10, \"end_line\": 25, \"explanation\": \"We're adding the imports needed...\"}},\n\
              {{\"start_line\": 30, \"end_line\": 60, \"explanation\": \"Now we're implementing...\"}}\n\
            ]\n\n\
            Return ONLY the JSON array, no other text.",
            project_context.repo_name,
            project_context.description,
            commit_message,
            filename,
            diff.lines().take(200).collect::<Vec<_>>().join("\n")
        );

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
                    "parts": [{"text": prompt}]
                }],
                "generationConfig": {
                    "temperature": 0.3,
                    "maxOutputTokens": 1024
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

        let json_text = response_json["candidates"][0]["content"]["parts"][0]["text"]
            .as_str()
            .context("Failed to extract text from Gemini response")?
            .trim();

        // Parse the JSON array
        let chunk_data: Vec<serde_json::Value> = serde_json::from_str(json_text)
            .context("Failed to parse chunks JSON from LLM")?;

        let mut chunks = Vec::new();
        for (idx, chunk) in chunk_data.iter().enumerate() {
            chunks.push(DiffChunk {
                chunk_id: idx,
                file_path: filename.to_string(),
                start_line: chunk["start_line"].as_u64().unwrap_or(0) as usize,
                end_line: chunk["end_line"].as_u64().unwrap_or(0) as usize,
                explanation: chunk["explanation"].as_str().unwrap_or("").to_string(),
                audio_data: None,
                estimated_duration_secs: 0.0,
            });
        }

        Ok(chunks)
    }

    /// Generate voiceover segments for a commit (one per file)
    pub fn generate_voiceover_segments(
        &self,
        commit_hash: String,
        author: String,
        message: String,
        file_changes: Vec<(String, String, FileStatus)>, // (filename, diff_text, status)
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

                // Extract project context once for all files (always fresh, LLM-only)
                let mut project_context = extract_project_context();

                // Generate description with LLM (REQUIRED - no fallbacks)
                if config.use_llm_explanations && config.gemini_api_key.is_some() {
                    eprintln!("[AUDIO] Generating AI-powered project description...");
                    match generate_project_context_with_llm(&config).await {
                        Ok(llm_description) => {
                            eprintln!("[AUDIO] Generated description ({} chars)", llm_description.len());
                            project_context.description = llm_description;
                        }
                        Err(e) => {
                            eprintln!("[AUDIO] FATAL: LLM context generation failed: {}", e);
                            eprintln!("[AUDIO] Cannot proceed without project context - skipping voiceovers");
                            return Vec::new(); // Return empty segments if LLM fails
                        }
                    }
                } else {
                    eprintln!("[AUDIO] LLM explanations disabled - no voiceovers will be generated");
                    return Vec::new();
                }

                let desc_preview = &project_context.description[..50.min(project_context.description.len())];
                eprintln!("[AUDIO] Project: {} - {}...", project_context.repo_name, desc_preview);

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
                        estimated_duration_secs: estimate_audio_duration(&intro_text),
                    });
                } else {
                    eprintln!("[AUDIO] Failed to generate intro voiceover");
                }

                // Limit to top 5 most important files to avoid rate limits
                let max_files = 5;
                let important_files: Vec<_> = file_changes.iter()
                    .filter(|(filename, _, _)| {
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
                for (i, (filename, diff, file_status)) in important_files.iter().enumerate() {
                    // Add delay between API calls to avoid rate limits
                    if i > 0 {
                        tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
                    }

                    eprintln!("[AUDIO] Processing file: {}", filename);
                    let narration = if config.use_llm_explanations && config.gemini_api_key.is_some() {
                        match Self::generate_file_explanation_with_retry(&config, &project_context, &message, &commit_hash, filename, file_status, diff).await {
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
                            let duration = estimate_audio_duration(&narration);
                            eprintln!("[AUDIO] Estimated duration: {:.1}s", duration);

                            segments.push(VoiceoverSegment {
                                text: narration.clone(),
                                audio_data: Some(audio_data),
                                file_path: Some(filename.clone()),
                                trigger_type: VoiceoverTrigger::FileOpen(filename.clone()),
                                estimated_duration_secs: duration,
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
        project_context: &ProjectContext,
        commit_message: &str,
        commit_hash: &str,
        filename: &str,
        file_status: &FileStatus,
        diff: &str,
    ) -> Result<String> {
        let max_retries = 3;
        let mut retry_delay = 1000; // Start with 1 second

        for attempt in 0..max_retries {
            match Self::generate_file_explanation(config, project_context, commit_message, commit_hash, filename, file_status, diff).await {
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
        project_context: &ProjectContext,
        commit_message: &str,
        commit_hash: &str,
        filename: &str,
        file_status: &FileStatus,
        diff: &str,
    ) -> Result<String> {
        let api_key = config
            .gemini_api_key
            .as_ref()
            .context("Gemini API key not configured")?;

        let commit_short = &commit_hash[..7.min(commit_hash.len())];

        // Map file status to natural language verb
        let status_verb = match file_status {
            FileStatus::Added => "creating",
            FileStatus::Deleted => "removing",
            FileStatus::Modified => "modifying",
            FileStatus::Renamed => "renaming",
            FileStatus::Copied => "copying",
            FileStatus::Unmodified => "reviewing",
        };

        // Build context for the LLM with project context
        let user_prompt = format!(
            "PROJECT CONTEXT: {} - {}\n\n\
            COMMIT GOAL: \"{}\"\n\n\
            You're teaching a developer by doing a live code walkthrough. Right now we're {} the file: {}\n\n\
            CODE DIFF:\n{}\n\n\
            INSTRUCTIONS:\n\
            You are an experienced developer teaching someone through a code review voiceover that plays \
            WHILE the code is being typed on screen. Be conversational but CONCISE.\n\n\
            TEACH ME by explaining:\n\
            1. WHAT changed - Mention the key changes. Focus on the most important parts.\n\
            2. WHY it matters - Explain the reasoning and how it fits into this project.\n\
            3. YOUR THOUGHTS - Share a brief opinion: Is this a good approach? Any notable tradeoffs?\n\n\
            CRITICAL CONSTRAINTS:\n\
            - Keep it to 60-80 words MAX (about 30-40 seconds of speech)\n\
            - Be punchy and focused - hit the highlights, skip minor details\n\
            - Use natural spoken language like you're explaining to a friend\n\
            - Avoid code syntax in speech (say 'the status check' not 'status === FileStatus::Added')\n\
            - DO NOT cut off mid-sentence. Complete your explanation fully.\n\n\
            Example length: 'We're adding project context extraction here that samples key repository files \
            and feeds them to Gemini for analysis. This is smart because now the LLM understands what the \
            whole project does, not just individual diffs. I like the fallback chain from LLM to README to \
            package manifests - ensures we always have some context.' [~60 words]",
            project_context.repo_name,
            project_context.description,
            commit_message,
            status_verb,
            filename,
            diff.lines().take(80).collect::<Vec<_>>().join("\n")
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
                    "temperature": 0.75,
                    "maxOutputTokens": 512  // Reduced for concise 60-80 word explanations (~30-40 seconds audio)
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
            .context("Inworld API key not configured (Basic auth base64)")?;

        let voice_id = config
            .voice_id
            .as_ref()
            .map(|s| s.as_str())
            .unwrap_or("Ashley"); // Default: Ashley voice

        let model_id = config
            .model_id
            .as_ref()
            .map(|s| s.as_str())
            .unwrap_or("inworld-tts-1.5-max"); // Latest Inworld model

        // Inworld TTS API endpoint (non-streaming)
        let url = "https://api.inworld.ai/tts/v1/voice";

        let client = reqwest::Client::new();
        let response = client
            .post(url)
            .header("Authorization", format!("Basic {}", api_key))
            .header("Content-Type", "application/json")
            .json(&serde_json::json!({
                "text": text,
                "voiceId": voice_id,
                "modelId": model_id,
            }))
            .send()
            .await
            .context("Failed to send request to Inworld API")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            anyhow::bail!("Inworld API error ({}): {}", status, error_text);
        }

        let response_json: serde_json::Value = response
            .json()
            .await
            .context("Failed to parse Inworld response")?;

        // Inworld returns base64-encoded audio in the "audioContent" field
        let audio_base64 = response_json["audioContent"]
            .as_str()
            .context("Failed to extract audioContent from Inworld response")?;

        let audio_data = general_purpose::STANDARD.decode(audio_base64)
            .context("Failed to decode base64 audio from Inworld")?;

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
