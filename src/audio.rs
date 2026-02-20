use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::sync::mpsc::{channel, Sender, Receiver};
use std::thread;
use std::collections::VecDeque;
use rodio::{Decoder, OutputStream, Sink};
use crate::git::FileStatus;
use base64::{Engine as _, engine::general_purpose};
use async_openai::{
    Client,
    config::OpenAIConfig,
    types::{
        ChatCompletionRequestMessage,
        ChatCompletionRequestUserMessageArgs,
        CreateChatCompletionRequestArgs,
    }
};

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
            openai_api_key: None,
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
        .openai_api_key
        .as_ref()
        .context("OpenAI API key not configured")?;

    // Sample key files from repository (deepwiki principle)
    let mut context_files = Vec::new();

    // Prioritize these files for context - take substantial content for full understanding
    let key_files = [
        ("Cargo.toml", 5000),      // Full manifest
        ("package.json", 5000),    // Full package config
        ("src/main.rs", 8000),     // Main entry point - take more
        ("src/lib.rs", 8000),      // Library root - take more
        ("src/index.ts", 8000),    // TypeScript entry
        ("main.py", 8000),         // Python entry
        ("README.md", 3000),       // Overview but not too long
    ];

    for (file_path, max_chars) in key_files {
        if let Ok(content) = std::fs::read_to_string(file_path) {
            let preview = content.chars().take(max_chars).collect::<String>();
            context_files.push(format!("File: {}\n{}", file_path, preview));
        }
    }

    if context_files.is_empty() {
        anyhow::bail!("No key files found for context extraction");
    }

    let repo_context = context_files.join("\n\n---\n\n");

    // Use OpenAI GPT-5.2 to generate project description
    let prompt = format!(
        "You are analyzing a code repository using the DeepWiki principle. Based on the key files below, \
        provide a comprehensive technical description (300-500 words) covering:\n\
        1. What this project does and its core purpose\n\
        2. Main architecture and tech stack\n\
        3. Key components and how they interact\n\
        4. Important patterns or design decisions\n\n\
        IMPORTANT: Write for TEXT-TO-SPEECH pronunciation. Use natural spoken language:\n\
        - Say 'Node' not 'Node.js' or 'Node dot JS'\n\
        - Say 'TypeScript' not 'TS'\n\
        - Say 'React' not 'React.js'\n\
        - Avoid symbols, abbreviations, file extensions when possible\n\
        - Write how developers actually speak about code\n\n\
        This context will be used in voice narration, so be specific and technical but naturally speakable.\n\n\
        Repository files:\n{}\n\n\
        Provide ONLY the description, no preamble or meta-commentary.",
        repo_context
    );

    let config = OpenAIConfig::new().with_api_key(api_key);
    let client = Client::with_config(config);

    let request = CreateChatCompletionRequestArgs::default()
        .model("gpt-5.2")
        .messages(vec![
            ChatCompletionRequestMessage::User(
                ChatCompletionRequestUserMessageArgs::default()
                    .content(prompt)
                    .build()?
            )
        ])
        .temperature(0.5)
        .max_completion_tokens(2048u32)
        .build()?;

    let response = client
        .chat()
        .create(request)
        .await
        .context("Failed to call OpenAI API")?;

    let description = response
        .choices
        .first()
        .and_then(|choice| choice.message.content.as_ref())
        .context("No content in OpenAI response")?
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


/// Calculate estimated animation duration for a chunk based on line count
/// Based on animation.rs constants:
/// - Base typing: 30ms per char, ~50 chars/line = 1.5s per line
/// - Line insertions: 6.7x multiplier = ~200ms per line
/// - Line deletions: 10.0x multiplier = ~300ms per line
/// - Cursor movements: ~0.5s per movement
/// - Hunk pauses: 1.5s between hunks
fn estimate_animation_duration(line_count: usize) -> f32 {
    // Conservative estimate: assume most lines are typed (1.5s) with some insertions (0.2s)
    let typing_time = (line_count as f32) * 1.5;  // Base typing
    let insertion_time = (line_count as f32) * 0.2;  // Line insertion pauses
    let cursor_time = ((line_count / 5) as f32) * 0.5;  // Cursor movements (~1 per 5 lines)
    let hunk_time = ((line_count / 15) as f32) * 1.5;  // Hunk pauses (~1 per 15 lines)

    typing_time + insertion_time + cursor_time + hunk_time
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
    pub estimated_duration_secs: f32,  // Animation duration (used for sync)
    pub audio_duration_secs: f32,      // Actual audio duration
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
    /// Channel to signal when audio chunks finish
    chunk_finished_tx: Sender<usize>,
    chunk_finished_rx: Arc<Mutex<Receiver<usize>>>,
}

impl AudioPlayer {
    pub fn new(config: VoiceoverConfig) -> Result<Self> {
        let (chunk_finished_tx, chunk_finished_rx) = channel();

        if !config.enabled {
            return Ok(Self {
                config,
                _stream: None,
                sink: None,
                segment_queue: Arc::new(Mutex::new(VecDeque::new())),
                chunks: Arc::new(Mutex::new(std::collections::HashMap::new())),
                chunk_finished_tx,
                chunk_finished_rx: Arc::new(Mutex::new(chunk_finished_rx)),
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
            chunk_finished_tx,
            chunk_finished_rx: Arc::new(Mutex::new(chunk_finished_rx)),
        })
    }

    /// Check if any audio chunks have finished (non-blocking)
    pub fn poll_finished_chunks(&self) -> Vec<usize> {
        let mut finished = Vec::new();
        if let Ok(rx) = self.chunk_finished_rx.lock() {
            while let Ok(chunk_id) = rx.try_recv() {
                finished.push(chunk_id);
            }
        }
        finished
    }

    /// Get chunks for a specific file
    pub fn get_chunks_for_file(&self, file_path: &str) -> Vec<DiffChunk> {
        if let Ok(chunks_guard) = self.chunks.lock() {
            chunks_guard.values()
                .filter(|chunk| chunk.file_path == file_path)
                .cloned()
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Trigger playback of a specific audio chunk
    pub fn trigger_chunk(&self, chunk_id: usize) {
        if !self.config.enabled || self.sink.is_none() {
            return;
        }

        eprintln!("[AUDIO] Playing chunk {}", chunk_id);

        let chunks = self.chunks.clone();
        let sink = self.sink.clone();
        let finished_tx = self.chunk_finished_tx.clone();

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
                                eprintln!("[AUDIO] Chunk {} playing (~{:.1}s audio)", chunk_id, chunk.audio_duration_secs);

                                // Wait for audio to finish based on actual audio duration
                                let duration_ms = (chunk.audio_duration_secs * 1000.0) as u64;
                                thread::sleep(std::time::Duration::from_millis(duration_ms));

                                // Signal that this chunk finished
                                eprintln!("[AUDIO] Chunk {} finished", chunk_id);
                                let _ = finished_tx.send(chunk_id);
                            }
                        }
                    }
                }
            }
        });
    }

    /// Split a file's diff into explainable chunks using TWO-PHASE LLM approach
    /// Phase 1: Get line ranges only
    /// Phase 2: Generate explanations with word counts matching animation duration
    async fn split_diff_into_chunks(
        config: &VoiceoverConfig,
        project_context: &ProjectContext,
        commit_message: &str,
        filename: &str,
        diff: &str,
    ) -> Result<Vec<DiffChunk>> {
        let api_key = config
            .openai_api_key
            .as_ref()
            .context("OpenAI API key not configured")?;

        // PHASE 1: Get logical chunk boundaries (line ranges only)
        let boundaries_prompt = format!(
            "You are analyzing a code diff. Split it into 2-5 logical chunks based on semantic changes.\n\n\
            PROJECT: {} - {}\n\
            COMMIT: \"{}\"\n\
            FILE: {}\n\n\
            DIFF:\n{}\n\n\
            INSTRUCTIONS:\n\
            Break this diff into 2-5 logical chunks where each chunk represents a cohesive change.\n\
            Return ONLY the line ranges - no explanations yet.\n\n\
            Respond with JSON:\n\
            {{\n\
              \"chunks\": [\n\
                {{\n\
                  \"start_line\": 1,\n\
                  \"end_line\": 10\n\
                }}\n\
              ]\n\
            }}",
            project_context.repo_name,
            project_context.description,
            commit_message,
            filename,
            diff.lines().take(200).collect::<Vec<_>>().join("\n")
        );

        let openai_config = OpenAIConfig::new().with_api_key(api_key);
        let client = Client::with_config(openai_config);

        // Phase 1: Get chunk boundaries
        eprintln!("[AUDIO] Phase 1: Getting chunk boundaries for {}", filename);
        let boundaries_request = CreateChatCompletionRequestArgs::default()
            .model("gpt-5.2")
            .messages(vec![
                ChatCompletionRequestMessage::User(
                    ChatCompletionRequestUserMessageArgs::default()
                        .content(boundaries_prompt)
                        .build()?
                )
            ])
            .temperature(0.3)
            .max_completion_tokens(1024u32)
            .build()?;

        let boundaries_response = client
            .chat()
            .create(boundaries_request)
            .await
            .context("Failed to get chunk boundaries from OpenAI")?;

        let boundaries_content = boundaries_response
            .choices
            .first()
            .and_then(|choice| choice.message.content.as_ref())
            .context("No content in boundaries response")?;

        let boundaries_parsed: serde_json::Value = serde_json::from_str(boundaries_content)
            .context("Failed to parse boundaries JSON")?;

        let boundaries_array = boundaries_parsed["chunks"]
            .as_array()
            .context("No chunks array in boundaries response")?;

        // Phase 2: For each chunk, calculate animation duration and generate explanation
        let mut chunks = Vec::new();
        for (idx, boundary) in boundaries_array.iter().enumerate() {
            let start_line = boundary["start_line"].as_u64().unwrap_or(0) as usize;
            let end_line = boundary["end_line"].as_u64().unwrap_or(0) as usize;
            let line_count = end_line.saturating_sub(start_line);

            // Calculate animation duration for this chunk
            let animation_duration = estimate_animation_duration(line_count);

            // Calculate target words: duration (seconds) × 2.5 words/sec (Inworld TTS)
            let target_words = (animation_duration * 2.5) as usize;
            let target_words = target_words.max(50).min(300); // Clamp to reasonable range

            eprintln!("[AUDIO] Chunk {}: {} lines = {:.1}s animation = {} target words",
                idx, line_count, animation_duration, target_words);

            // Extract diff content for this chunk
            let chunk_diff: String = diff.lines()
                .skip(start_line.saturating_sub(1))
                .take(line_count + 1)
                .collect::<Vec<_>>()
                .join("\n");

            // Generate explanation with exact word count
            let explanation_prompt = format!(
                "You are narrating live code changes for teaching.\n\n\
                PROJECT: {} - {}\n\
                COMMIT: \"{}\"\n\
                FILE: {} (lines {}-{})\n\n\
                CODE CHANGES:\n{}\n\n\
                Generate a {} word explanation in conversational teaching style.\n\
                OPTIMIZE FOR TEXT-TO-SPEECH:\n\
                - Say 'Node' not 'Node.js', 'React' not 'React.js', 'TypeScript' not 'TS'\n\
                - Avoid symbols, file extensions\n\
                - Write how developers speak\n\n\
                Explain WHAT changed, WHY it matters for this project, and HOW it works.\n\
                Respond with ONLY the explanation text, no JSON.",
                project_context.repo_name,
                project_context.description,
                commit_message,
                filename,
                start_line,
                end_line,
                chunk_diff,
                target_words
            );

            tokio::time::sleep(tokio::time::Duration::from_millis(300)).await; // Rate limit

            let explanation_request = CreateChatCompletionRequestArgs::default()
                .model("gpt-5.2")
                .messages(vec![
                    ChatCompletionRequestMessage::User(
                        ChatCompletionRequestUserMessageArgs::default()
                            .content(explanation_prompt)
                            .build()?
                    )
                ])
                .temperature(0.7)
                .max_completion_tokens(target_words.saturating_mul(2) as u32) // 2x for safety
                .build()?;

            let explanation_response = client
                .chat()
                .create(explanation_request)
                .await
                .context("Failed to generate explanation from OpenAI")?;

            let explanation = explanation_response
                .choices
                .first()
                .and_then(|choice| choice.message.content.as_ref())
                .context("No content in explanation response")?
                .trim()
                .to_string();

            // Calculate actual audio duration from word count
            let actual_word_count = explanation.split_whitespace().count();
            let audio_duration = (actual_word_count as f32) / 2.5; // 150 WPM

            chunks.push(DiffChunk {
                chunk_id: idx,
                file_path: filename.to_string(),
                start_line,
                end_line,
                explanation,
                audio_data: None,
                estimated_duration_secs: animation_duration,  // For animation sync
                audio_duration_secs: audio_duration,          // For audio playback
            });
        }

        Ok(chunks)
    }

    /// Generate audio chunks for all files in a commit
    pub fn generate_audio_chunks(
        &self,
        commit_hash: String,
        author: String,
        message: String,
        file_changes: Vec<(String, String, FileStatus)>, // (filename, diff_text, status)
    ) -> Vec<DiffChunk> {
        if !self.config.enabled || self.config.api_key.is_none() {
            return Vec::new();
        }

        let config = self.config.clone();
        let chunks_map = self.chunks.clone();

        eprintln!("[AUDIO] Generating audio chunks (using LLM to split diffs)...");

        // Generate ALL audio chunks synchronously BEFORE returning
        let rt = match tokio::runtime::Runtime::new() {
            Ok(rt) => rt,
            Err(e) => {
                eprintln!("Failed to create Tokio runtime for voiceover: {}", e);
                return Vec::new();
            }
        };

        rt.block_on(async {
                let mut all_chunks = Vec::new();
                let mut global_chunk_id = 0;

                // Extract project context once for all files (always fresh, LLM-only)
                let mut project_context = extract_project_context();

                // Generate description with LLM (REQUIRED - no fallbacks)
                if config.use_llm_explanations && config.openai_api_key.is_some() {
                    eprintln!("[AUDIO] Generating AI-powered project description...");
                    match generate_project_context_with_llm(&config).await {
                        Ok(llm_description) => {
                            eprintln!("[AUDIO] Generated description ({} chars)", llm_description.len());
                            project_context.description = llm_description;
                        }
                        Err(e) => {
                            eprintln!("[AUDIO] FATAL: LLM context generation failed: {}", e);
                            eprintln!("[AUDIO] Cannot proceed without project context - skipping audio chunks");
                            return Vec::new();
                        }
                    }
                } else {
                    eprintln!("[AUDIO] LLM explanations disabled - no audio chunks will be generated");
                    return Vec::new();
                }

                let desc_preview = &project_context.description[..50.min(project_context.description.len())];
                eprintln!("[AUDIO] Project: {} - {}...", project_context.repo_name, desc_preview);

                // Limit to top 5 most important files to avoid rate limits
                let max_files = 5;
                let important_files: Vec<_> = file_changes.iter()
                    .filter(|(filename, _, _)| {
                        // Skip boring files
                        !filename.contains("package-lock.json") &&
                        !filename.contains("yarn.lock") &&
                        !filename.contains("pnpm-lock.yaml") &&
                        !filename.ends_with(".lock") &&
                        !filename.ends_with(".json")
                    })
                    .take(max_files)
                    .collect();

                eprintln!("[AUDIO] Splitting {} files into explainable chunks...", important_files.len());

                for (i, (filename, diff, _file_status)) in important_files.iter().enumerate() {
                    // Add delay between API calls to avoid rate limits
                    if i > 0 {
                        tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
                    }

                    eprintln!("[AUDIO] Splitting file: {}", filename);

                    // Split this file's diff into logical chunks
                    match Self::split_diff_into_chunks(&config, &project_context, &message, filename, diff).await {
                        Ok(mut file_chunks) => {
                            eprintln!("[AUDIO] Split {} into {} chunks", filename, file_chunks.len());

                            // Generate audio for each chunk (explanations already generated with target word counts)
                            for chunk in &mut file_chunks {
                                // Assign global chunk ID
                                chunk.chunk_id = global_chunk_id;
                                global_chunk_id += 1;

                                let word_count = chunk.explanation.split_whitespace().count();
                                let line_count = chunk.end_line.saturating_sub(chunk.start_line);
                                eprintln!("[AUDIO] Chunk {} ({} lines, {} words, target {:.1}s animation): {}",
                                    chunk.chunk_id, line_count, word_count, chunk.estimated_duration_secs,
                                    &chunk.explanation[..50.min(chunk.explanation.len())]);

                                // Add delay before TTS call
                                tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;

                                match Self::synthesize_speech_from_text(&config, &chunk.explanation).await {
                                    Ok(audio_data) => {
                                        // Use word-based audio duration calculation
                                        let audio_duration = (word_count as f32) / 2.5; // 150 WPM = 2.5 words/sec
                                        eprintln!("[AUDIO] Generated audio: {:.1}s audio vs {:.1}s animation (ratio: {:.2})",
                                            audio_duration, chunk.estimated_duration_secs,
                                            audio_duration / chunk.estimated_duration_secs);

                                        chunk.audio_data = Some(audio_data);
                                        // Keep the animation duration estimate - audio will use actual word-based duration
                                    }
                                    Err(e) => {
                                        eprintln!("[AUDIO] Failed to synthesize speech for chunk {}: {}", chunk.chunk_id, e);
                                    }
                                }
                            }

                            all_chunks.extend(file_chunks);
                        }
                        Err(e) => {
                            eprintln!("[AUDIO] Failed to split {} into chunks: {}", filename, e);
                        }
                    }
                }

                eprintln!("[AUDIO] Generated {} total audio chunks", all_chunks.len());

                // Store chunks in HashMap for animation to access
                if let Ok(mut chunks_guard) = chunks_map.lock() {
                    for chunk in &all_chunks {
                        chunks_guard.insert(chunk.chunk_id, chunk.clone());
                    }
                }

                all_chunks
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
            .openai_api_key
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
