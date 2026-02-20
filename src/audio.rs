use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::sync::mpsc::{channel, Sender, Receiver};
use std::thread;
use std::collections::VecDeque;
use rodio::{Decoder, OutputStream, Sink, Source};
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
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub provider: VoiceoverProvider,
    pub api_key: Option<String>,
    pub voice_id: Option<String>,
    pub model_id: Option<String>,
    pub openai_api_key: Option<String>,
    #[serde(default)]
    pub use_llm_explanations: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum VoiceoverProvider {
    #[serde(rename = "elevenlabs")]
    ElevenLabs,
    #[default]
    #[serde(rename = "inworld")]
    Inworld,
}

/// Project context for providing LLM with repository information
#[derive(Debug, Clone)]
pub struct ProjectContext {
    pub repo_name: String,
    pub description: String,
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

/// Generate AI-powered project description using OpenAI (async version)
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


/// Calculate animation duration from actual diff content.
/// Mirrors the timing constants in animation.rs exactly.
///
/// speed_ms: base typing delay per character (default 30ms)
fn calculate_animation_duration(diff_lines: &[&str], speed_ms: u64) -> f32 {
    // Animation timing multipliers (from animation.rs)
    const INSERT_LINE_PAUSE: f64 = 6.7;
    const DELETE_LINE_PAUSE: f64 = 10.0;
    const HUNK_PAUSE: f64 = 50.0;
    const CURSOR_MOVE_PAUSE: f64 = 0.5;

    let speed = speed_ms as f64;
    let mut total_ms: f64 = 0.0;
    let mut in_hunk = false;
    let mut hunk_count = 0;

    for line in diff_lines {
        if line.starts_with("@@") {
            // Hunk header - add hunk pause (except first)
            if hunk_count > 0 {
                total_ms += HUNK_PAUSE * speed;
            }
            // Cursor movement to new hunk location
            total_ms += CURSOR_MOVE_PAUSE * speed * 5.0; // ~5 steps average
            hunk_count += 1;
            in_hunk = true;
            continue;
        }

        if !in_hunk {
            continue; // Skip diff headers (---, +++, etc)
        }

        if line.starts_with('+') && !line.starts_with("+++") {
            // Addition: type each character + line insertion pause
            let char_count = line.len().saturating_sub(1); // skip the '+' prefix
            total_ms += (char_count as f64) * speed as f64; // typing
            total_ms += INSERT_LINE_PAUSE * speed;          // line insertion pause
        } else if line.starts_with('-') && !line.starts_with("---") {
            // Deletion: just the deletion pause
            total_ms += DELETE_LINE_PAUSE * speed;
        }
        // Context lines (space prefix): cursor moves past them quickly
    }

    // Convert ms to seconds, minimum 5s
    (total_ms / 1000.0).max(5.0) as f32
}

/// Calculate target word count from animation duration.
/// Guarantees: audio_duration >= animation_duration
///
/// Uses Inworld TTS rate of ~150 WPM (2.5 words/sec).
/// Adds 15% buffer so audio always outlasts the video.
fn words_for_duration(animation_secs: f32) -> usize {
    let words = (animation_secs * 2.5 * 2.0) as usize; // 2x animation duration
    words.max(40).min(400) // Clamp: at least 40, at most 400
}

/// Represents a single voiceover chunk for a portion of a diff
#[derive(Debug, Clone)]
pub struct DiffChunk {
    pub chunk_id: usize,
    pub file_path: String,
    pub hunk_indices: Vec<usize>,      // Which hunks this chunk covers (0-indexed)
    pub explanation: String,
    pub audio_data: Option<Vec<u8>>,
    pub has_audio: bool,               // Quick check without cloning audio bytes
    pub estimated_duration_secs: f32,  // Animation duration (used for sync)
    pub audio_duration_secs: f32,      // Actual audio duration (measured from decoded audio)
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

                                // Wait for audio to finish based on actual audio duration
                                let duration_ms = (chunk.audio_duration_secs * 1000.0) as u64;
                                thread::sleep(std::time::Duration::from_millis(duration_ms));

                                // Signal that this chunk finished
                                let _ = finished_tx.send(chunk_id);
                            }
                        }
                    }
                }
            }
        });
    }

    /// Order files by logical development flow using LLM.
    /// Returns files in the order a developer would logically write them.
    async fn order_files_by_development_flow(
        config: &VoiceoverConfig,
        project_context: &ProjectContext,
        commit_message: &str,
        files: &[(String, String, FileStatus)],
    ) -> Vec<(String, String, FileStatus)> {
        if files.len() <= 1 {
            return files.to_vec();
        }

        let api_key = match config.openai_api_key.as_ref() {
            Some(key) => key,
            None => return files.to_vec(),
        };

        let file_list: Vec<String> = files.iter().enumerate()
            .map(|(i, (name, diff, status))| {
                let status_str = match status {
                    FileStatus::Added => "new file",
                    FileStatus::Deleted => "deleted",
                    FileStatus::Modified => "modified",
                    FileStatus::Renamed => "renamed",
                    FileStatus::Copied => "copied",
                    FileStatus::Unmodified => "unchanged",
                };
                let line_count = diff.lines().count();
                format!("{}: {} ({}, {} diff lines)", i, name, status_str, line_count)
            })
            .collect();

        let prompt = format!(
            "You are ordering files for a code walkthrough narration.\n\n\
            PROJECT: {} - {}\n\
            COMMIT: \"{}\"\n\n\
            FILES:\n{}\n\n\
            Order these files by how a developer would logically write them:\n\
            - Config/setup files first (Cargo.toml, package.json, .env)\n\
            - Type definitions and data models next\n\
            - Core logic and business rules\n\
            - Integration points (API calls, database)\n\
            - UI/presentation last\n\
            - New files before modifications to existing ones\n\
            - Dependencies before dependents\n\n\
            Respond with ONLY a JSON array of the file indices in order.\n\
            Example: [2, 0, 3, 1]",
            project_context.repo_name,
            &project_context.description.chars().take(200).collect::<String>(),
            commit_message,
            file_list.join("\n")
        );

        let openai_config = OpenAIConfig::new().with_api_key(api_key);
        let client = Client::with_config(openai_config);

        let request = match CreateChatCompletionRequestArgs::default()
            .model("gpt-5.2")
            .messages(vec![
                ChatCompletionRequestMessage::User(
                    match ChatCompletionRequestUserMessageArgs::default()
                        .content(prompt)
                        .build() {
                        Ok(msg) => msg,
                        Err(_) => return files.to_vec(),
                    }
                )
            ])
            .temperature(0.2)
            .max_completion_tokens(128u32)
            .build() {
            Ok(req) => req,
            Err(_) => return files.to_vec(),
        };

        match client.chat().create(request).await {
            Ok(response) => {
                if let Some(content) = response.choices.first()
                    .and_then(|c| c.message.content.as_ref())
                {
                    // Parse JSON array of indices
                    if let Ok(indices) = serde_json::from_str::<Vec<usize>>(content.trim()) {
                        let mut ordered = Vec::with_capacity(files.len());
                        let mut used = std::collections::HashSet::new();

                        for idx in &indices {
                            if *idx < files.len() && used.insert(*idx) {
                                ordered.push(files[*idx].clone());
                            }
                        }

                        // Append any files the LLM missed
                        for (i, file) in files.iter().enumerate() {
                            if !used.contains(&i) {
                                ordered.push(file.clone());
                            }
                        }

                        return ordered;
                    }
                }
                files.to_vec()
            }
            Err(_) => {
                files.to_vec()
            }
        }
    }

    /// Split a file's diff into semantic chunks using hunk indices.
    ///
    /// Phase 1: Parse diff into hunks, ask LLM to group them semantically
    /// Phase 2: For each chunk, calculate EXACT animation duration from diff content,
    ///          derive the word count needed to fill that time, generate explanation
    ///
    /// Guarantee: audio_duration >= animation_duration (2x buffer)
    async fn split_diff_into_chunks(
        config: &VoiceoverConfig,
        project_context: &ProjectContext,
        commit_message: &str,
        filename: &str,
        diff: &str,
        speed_ms: u64,
    ) -> Result<Vec<DiffChunk>> {
        let api_key = config
            .openai_api_key
            .as_ref()
            .context("OpenAI API key not configured")?;

        // Pre-parse diff into hunk groups (split by @@ headers)
        let diff_lines: Vec<&str> = diff.lines().collect();
        let mut hunks: Vec<Vec<&str>> = Vec::new(); // Each hunk = its lines (including @@ header)
        let mut hunk_summaries: Vec<String> = Vec::new();
        let mut current_hunk: Vec<&str> = Vec::new();

        for line in &diff_lines {
            if line.starts_with("@@") {
                if !current_hunk.is_empty() {
                    hunks.push(current_hunk.clone());
                }
                current_hunk = vec![line];
            } else if !current_hunk.is_empty() {
                current_hunk.push(line);
            }
        }
        if !current_hunk.is_empty() {
            hunks.push(current_hunk);
        }

        // Build hunk summaries for the LLM
        for (i, hunk_lines) in hunks.iter().enumerate() {
            let header = hunk_lines.first().unwrap_or(&"");
            let additions = hunk_lines.iter().filter(|l| l.starts_with('+') && !l.starts_with("+++")).count();
            let deletions = hunk_lines.iter().filter(|l| l.starts_with('-') && !l.starts_with("---")).count();
            // Show a few content lines for context
            let preview: Vec<&str> = hunk_lines.iter()
                .skip(1) // skip @@ header
                .filter(|l| l.starts_with('+') || l.starts_with('-'))
                .take(3)
                .copied()
                .collect();
            hunk_summaries.push(format!(
                "Hunk {}: {} — {} additions, {} deletions\n  Preview: {}",
                i, header, additions, deletions,
                preview.join(" | ")
            ));
        }

        // If only 1 hunk, skip the LLM grouping call — it's one chunk
        let chunk_groups: Vec<Vec<usize>> = if hunks.len() <= 1 {
            vec![(0..hunks.len()).collect()]
        } else {
            // PHASE 1: Ask LLM to group hunks semantically
            let grouping_prompt = format!(
                "You are grouping code changes for a narrated walkthrough.\n\n\
                PROJECT: {} - {}\n\
                COMMIT: \"{}\"\n\
                FILE: {}\n\n\
                HUNKS:\n{}\n\n\
                Group these hunks into 1-4 semantic chunks. Each chunk should cover a coherent change \
                (e.g. imports, a new function, config updates). Keep related hunks together.\n\n\
                Respond with ONLY JSON: {{\"chunks\": [[0, 1], [2], [3, 4]]}}",
                project_context.repo_name,
                &project_context.description.chars().take(300).collect::<String>(),
                commit_message,
                filename,
                hunk_summaries.join("\n")
            );

            let openai_config = OpenAIConfig::new().with_api_key(api_key);
            let client = Client::with_config(openai_config);

            let grouping_request = CreateChatCompletionRequestArgs::default()
                .model("gpt-5.2")
                .messages(vec![
                    ChatCompletionRequestMessage::User(
                        ChatCompletionRequestUserMessageArgs::default()
                            .content(grouping_prompt)
                            .build()?
                    )
                ])
                .temperature(0.3)
                .max_completion_tokens(256u32)
                .build()?;

            let grouping_response = client
                .chat()
                .create(grouping_request)
                .await
                .context("Failed to get hunk groupings")?;

            let grouping_content = grouping_response
                .choices
                .first()
                .and_then(|choice| choice.message.content.as_ref())
                .context("No content in grouping response")?;

            // Parse the response
            match serde_json::from_str::<serde_json::Value>(grouping_content.trim()) {
                Ok(parsed) => {
                    if let Some(chunks_arr) = parsed["chunks"].as_array() {
                        let mut groups: Vec<Vec<usize>> = Vec::new();
                        let mut used = std::collections::HashSet::new();
                        for group in chunks_arr {
                            if let Some(indices) = group.as_array() {
                                let valid: Vec<usize> = indices.iter()
                                    .filter_map(|v| v.as_u64().map(|n| n as usize))
                                    .filter(|&idx| idx < hunks.len() && used.insert(idx))
                                    .collect();
                                if !valid.is_empty() {
                                    groups.push(valid);
                                }
                            }
                        }
                        // Add any hunks the LLM missed
                        let missed: Vec<usize> = (0..hunks.len()).filter(|i| !used.contains(i)).collect();
                        if !missed.is_empty() {
                            groups.push(missed);
                        }
                        groups
                    } else {
                        // Fallback: one chunk per hunk
                        (0..hunks.len()).map(|i| vec![i]).collect()
                    }
                }
                Err(_) => {
                    // Fallback: all hunks in one chunk
                    vec![(0..hunks.len()).collect()]
                }
            }
        };

        // PHASE 2: For each chunk group, calculate animation time → derive word count → generate explanation
        let openai_config = OpenAIConfig::new().with_api_key(api_key);
        let client = Client::with_config(openai_config);
        let mut chunks = Vec::new();

        for (idx, hunk_indices) in chunk_groups.iter().enumerate() {
            // Collect all diff lines for hunks in this chunk
            let chunk_lines: Vec<&str> = hunk_indices.iter()
                .flat_map(|&hi| hunks.get(hi).map(|h| h.as_slice()).unwrap_or(&[]))
                .copied()
                .collect();

            // Calculate animation duration from the diff content
            let animation_secs = calculate_animation_duration(&chunk_lines, speed_ms);
            let target_words = words_for_duration(animation_secs);
            let chunk_diff = chunk_lines.join("\n");

            // Generate explanation with derived word count
            let explanation_prompt = format!(
                "You are narrating live code changes for a developer teaching stream.\n\n\
                PROJECT: {} - {}\n\
                COMMIT: \"{}\"\n\
                FILE: {}\n\n\
                CODE CHANGES:\n{}\n\n\
                Write a {}-word narration explaining these changes.\n\
                This narration will be spoken by text-to-speech while the code is being typed on screen.\n\
                The typing animation for this section lasts {:.0} seconds, so the narration MUST fill that time.\n\n\
                RULES:\n\
                - Explain WHAT changed, WHY it matters for this project, and HOW it works\n\
                - Be semantically rich: describe the purpose and design decisions, not just surface changes\n\
                - OPTIMIZE FOR SPEECH: Say 'Node' not 'Node.js', 'React' not 'React.js', 'TypeScript' not 'TS'\n\
                - No symbols, no file extensions, no code syntax. Write how developers actually talk.\n\n\
                Respond with ONLY the narration text.",
                project_context.repo_name,
                project_context.description,
                commit_message,
                filename,
                chunk_diff,
                target_words,
                animation_secs
            );

            tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;

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
                .max_completion_tokens((target_words * 2).max(200) as u32)
                .build()?;

            let explanation_response = client
                .chat()
                .create(explanation_request)
                .await
                .context("Failed to generate explanation")?;

            let explanation = explanation_response
                .choices
                .first()
                .and_then(|choice| choice.message.content.as_ref())
                .context("No content in explanation response")?
                .trim()
                .to_string();

            let actual_words = explanation.split_whitespace().count();
            let audio_secs = (actual_words as f32) / 2.5;

            chunks.push(DiffChunk {
                chunk_id: idx,
                file_path: filename.to_string(),
                hunk_indices: hunk_indices.clone(),
                explanation,
                audio_data: None,
                has_audio: false,
                estimated_duration_secs: animation_secs,
                audio_duration_secs: audio_secs,
            });
        }

        Ok(chunks)
    }

    /// Generate audio chunks for all files in a commit
    pub fn generate_audio_chunks(
        &self,
        _commit_hash: String,
        _author: String,
        message: String,
        file_changes: Vec<(String, String, FileStatus)>, // (filename, diff_text, status)
        speed_ms: u64, // Base typing speed in ms per character (for animation duration calc)
    ) -> Vec<DiffChunk> {
        if !self.config.enabled || self.config.api_key.is_none() {
            return Vec::new();
        }

        let config = self.config.clone();
        let chunks_map = self.chunks.clone();

        // Generate ALL audio chunks synchronously BEFORE returning
        let rt = match tokio::runtime::Runtime::new() {
            Ok(rt) => rt,
            Err(_) => {
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
                    match generate_project_context_with_llm(&config).await {
                        Ok(llm_description) => {
                            project_context.description = llm_description;
                        }
                        Err(_) => {
                            return Vec::new();
                        }
                    }
                } else {
                    return Vec::new();
                }

                // Filter boring files, limit to 5
                let max_files = 5;
                let important_files: Vec<(String, String, FileStatus)> = file_changes.into_iter()
                    .filter(|(filename, _, _)| {
                        !filename.contains("package-lock.json") &&
                        !filename.contains("yarn.lock") &&
                        !filename.contains("pnpm-lock.yaml") &&
                        !filename.ends_with(".lock") &&
                        !filename.ends_with(".json")
                    })
                    .take(max_files)
                    .collect();

                // Ask LLM to order files by logical development flow
                let ordered_files = Self::order_files_by_development_flow(
                    &config, &project_context, &message, &important_files
                ).await;

                for (i, (filename, diff, _file_status)) in ordered_files.iter().enumerate() {
                    // Add delay between API calls to avoid rate limits
                    if i > 0 {
                        tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
                    }

                    // Split this file's diff into logical chunks
                    match Self::split_diff_into_chunks(&config, &project_context, &message, filename, diff, speed_ms).await {
                        Ok(mut file_chunks) => {
                            // Generate audio for each chunk (explanations already generated with target word counts)
                            for chunk in &mut file_chunks {
                                // Assign global chunk ID
                                chunk.chunk_id = global_chunk_id;
                                global_chunk_id += 1;

                                let word_count = chunk.explanation.split_whitespace().count();

                                // Add delay before TTS call
                                tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;

                                match Self::synthesize_speech_from_text(&config, &chunk.explanation).await {
                                    Ok(audio_data) => {
                                        // Measure real audio duration from decoded audio
                                        let real_duration = {
                                            let cursor = std::io::Cursor::new(audio_data.clone());
                                            Decoder::new(cursor)
                                                .ok()
                                                .and_then(|s| s.total_duration())
                                                .map(|d| d.as_secs_f32())
                                        };
                                        let fallback = (word_count as f32) / 2.5;
                                        chunk.audio_duration_secs = real_duration.unwrap_or(fallback);
                                        chunk.audio_data = Some(audio_data);
                                        chunk.has_audio = true;
                                    }
                                    Err(_) => {}
                                }
                            }

                            all_chunks.extend(file_chunks);
                        }
                        Err(_) => {}
                    }
                }

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
            return;
        }

        let segment_queue = self.segment_queue.clone();
        let sink = self.sink.clone();

        thread::spawn(move || {
            // Find matching segment
            let segment = {
                if let Ok(mut queue) = segment_queue.lock() {
                    let pos = queue.iter().position(|s| s.trigger_type == trigger_type);
                    if let Some(index) = pos {
                        Some(queue.remove(index).unwrap())
                    } else {
                        None
                    }
                } else {
                    None
                }
            };

            if let Some(seg) = segment {
                if let Some(audio_data) = seg.audio_data {
                    if let Some(sink_arc) = sink {
                        if let Ok(sink_guard) = sink_arc.lock() {
                            let cursor = std::io::Cursor::new(audio_data);
                            if let Ok(source) = Decoder::new(cursor) {
                                sink_guard.append(source);
                                sink_guard.play();
                            }
                        }
                    }
                }
            }
        });
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

    /// Pause audio playback (sink keeps its queue, resumes from same position)
    pub fn pause(&self) {
        if let Some(sink_arc) = &self.sink {
            if let Ok(sink) = sink_arc.lock() {
                sink.pause();
            }
        }
    }

    /// Resume audio playback
    pub fn resume(&self) {
        if let Some(sink_arc) = &self.sink {
            if let Ok(sink) = sink_arc.lock() {
                sink.play();
            }
        }
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
