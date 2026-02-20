pub(crate) mod chunker;
pub(crate) mod llm;
pub(crate) mod tts;
pub mod types;

pub use types::{
    DiffChunk, VoiceoverConfig, VoiceoverProvider, VoiceoverSegment, VoiceoverTrigger,
};

use anyhow::{Context, Result};
use rodio::{Decoder, OutputStream, Sink, Source};
use std::collections::VecDeque;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

use crate::git::FileStatus;

/// Handles pre-generated audio chunks and synced playback during animation
pub struct AudioPlayer {
    config: VoiceoverConfig,
    _stream: Option<OutputStream>,
    sink: Option<Arc<Mutex<Sink>>>,
    segment_queue: Arc<Mutex<VecDeque<VoiceoverSegment>>>,
    chunks: Arc<Mutex<std::collections::HashMap<usize, DiffChunk>>>,
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

        let (_stream, stream_handle) =
            OutputStream::try_default().context("Failed to create audio output stream")?;
        let sink = Sink::try_new(&stream_handle).context("Failed to create audio sink")?;
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

    /// Drain finished chunk IDs (non-blocking)
    pub fn poll_finished_chunks(&self) -> Vec<usize> {
        let mut finished = Vec::new();
        if let Ok(rx) = self.chunk_finished_rx.lock() {
            while let Ok(id) = rx.try_recv() {
                finished.push(id);
            }
        }
        finished
    }

    /// Get all chunks pre-generated for a specific file
    pub fn get_chunks_for_file(&self, file_path: &str) -> Vec<DiffChunk> {
        self.chunks
            .lock()
            .map(|g| {
                g.values()
                    .filter(|c| c.file_path == file_path)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Start playing a pre-generated audio chunk (non-blocking)
    pub fn trigger_chunk(&self, chunk_id: usize) {
        if !self.config.enabled || self.sink.is_none() {
            return;
        }
        let chunks = self.chunks.clone();
        let sink = self.sink.clone();
        let tx = self.chunk_finished_tx.clone();

        thread::spawn(move || {
            let chunk = chunks.lock().ok().and_then(|g| g.get(&chunk_id).cloned());
            if let Some(chunk) = chunk {
                if let Some(audio_data) = chunk.audio_data {
                    if let Some(sink_arc) = sink {
                        // Append source and release the lock immediately so
                        // pause()/resume() on the main thread are never blocked.
                        let duration_ms = {
                            let Ok(guard) = sink_arc.lock() else { return };
                            let cursor = std::io::Cursor::new(audio_data);
                            let Ok(source) = Decoder::new(cursor) else {
                                return;
                            };
                            guard.append(source);
                            guard.play();
                            (chunk.audio_duration_secs * 1000.0) as u64
                        }; // lock released

                        thread::sleep(std::time::Duration::from_millis(duration_ms));
                        let _ = tx.send(chunk_id);
                    }
                }
            }
        });
    }

    /// Access the voiceover config (for use outside the player).
    pub fn voiceover_config(&self) -> &VoiceoverConfig {
        &self.config
    }

    /// Clone the shared chunks map (Send-safe, unlike AudioPlayer itself).
    pub fn chunks_handle(&self) -> Arc<Mutex<std::collections::HashMap<usize, DiffChunk>>> {
        self.chunks.clone()
    }

    /// Trigger a queued voiceover segment (e.g. on file open)
    pub fn trigger_voiceover(&self, trigger_type: VoiceoverTrigger) {
        if !self.config.enabled || self.sink.is_none() {
            return;
        }
        let queue = self.segment_queue.clone();
        let sink = self.sink.clone();

        thread::spawn(move || {
            let segment = queue.lock().ok().and_then(|mut q| {
                q.iter()
                    .position(|s| s.trigger_type == trigger_type)
                    .map(|i| q.remove(i).unwrap())
            });

            if let Some(seg) = segment {
                if let Some(audio_data) = seg.audio_data {
                    if let Some(sink_arc) = sink {
                        if let Ok(guard) = sink_arc.lock() {
                            let cursor = std::io::Cursor::new(audio_data);
                            if let Ok(source) = Decoder::new(cursor) {
                                guard.append(source);
                                guard.play();
                            }
                        }
                    }
                }
            }
        });
    }

    pub fn pause(&self) {
        if let Some(arc) = &self.sink {
            if let Ok(sink) = arc.lock() {
                sink.pause();
            }
        }
    }

    pub fn resume(&self) {
        if let Some(arc) = &self.sink {
            if let Ok(sink) = arc.lock() {
                sink.play();
            }
        }
    }
}

/// Pre-generate all audio chunks with progress reporting.
pub fn generate_audio_chunks_with_progress(
    config: VoiceoverConfig,
    chunks_map: Arc<Mutex<std::collections::HashMap<usize, DiffChunk>>>,
    message: String,
    file_changes: Vec<(String, String, FileStatus)>,
    speed_ms: u64,
    progress: Arc<Mutex<(String, f32)>>,
) -> Vec<DiffChunk> {
    let _ = progress
        .lock()
        .map(|mut p| *p = ("Analyzing repository...".to_string(), 0.0));
    generate_audio_chunks_impl(
        config,
        chunks_map,
        message,
        file_changes,
        speed_ms,
        Some(progress),
    )
}

/// Pre-generate all audio chunks for a commit's file changes (blocking, Send-safe).
///
/// This is a free function instead of a method on `AudioPlayer` because
/// `AudioPlayer` contains `OutputStream` which is `!Send`. The caller can
/// extract the sendable parts via `voiceover_config()` and `chunks_handle()`
/// and run this on a background thread.
#[allow(dead_code)]
pub fn generate_audio_chunks(
    config: VoiceoverConfig,
    chunks_map: Arc<Mutex<std::collections::HashMap<usize, DiffChunk>>>,
    message: String,
    file_changes: Vec<(String, String, FileStatus)>,
    speed_ms: u64,
) -> Vec<DiffChunk> {
    generate_audio_chunks_impl(config, chunks_map, message, file_changes, speed_ms, None)
}

fn generate_audio_chunks_impl(
    config: VoiceoverConfig,
    chunks_map: Arc<Mutex<std::collections::HashMap<usize, DiffChunk>>>,
    message: String,
    file_changes: Vec<(String, String, FileStatus)>,
    speed_ms: u64,
    progress: Option<Arc<Mutex<(String, f32)>>>,
) -> Vec<DiffChunk> {
    if !config.enabled || config.api_key.is_none() {
        return Vec::new();
    }

    // Clear stale chunks from any previous commit
    if let Ok(mut guard) = chunks_map.lock() {
        guard.clear();
    }

    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(_) => return Vec::new(),
    };

    rt.block_on(async {
        if let Some(ref p) = progress {
            let _ = p
                .lock()
                .map(|mut s| *s = ("Generating project context with GPT...".to_string(), 0.05));
        }

        let mut project_context = llm::extract_project_context();

        if config.use_llm_explanations && config.openai_api_key.is_some() {
            match llm::generate_project_context_with_llm(&config).await {
                Ok(desc) => project_context.description = desc,
                Err(_) => return Vec::new(),
            }
        } else {
            return Vec::new();
        }

        let important_files: Vec<(String, String, FileStatus)> = file_changes
            .into_iter()
            .filter(|(name, _, _)| {
                // Exclude lock files
                !name.contains("package-lock.json")
                    && !name.contains("yarn.lock")
                    && !name.contains("pnpm-lock.yaml")
                    && !name.ends_with(".lock")
                    // Exclude JSON config/data files
                    && !name.ends_with(".json")
                    // Exclude Xcode project files
                    && !name.ends_with(".xcodeproj")
                    && !name.ends_with(".pbxproj")
                    && !name.ends_with(".xcworkspace")
                    // Exclude IDE/editor config
                    && !name.contains(".vscode/")
                    && !name.contains(".idea/")
                    // Exclude build artifacts
                    && !name.contains("/dist/")
                    && !name.contains("/build/")
                    && !name.contains("/target/")
            })
            .collect();

        if let Some(ref p) = progress {
            let _ = p.lock().map(|mut s| {
                *s = (
                    format!(
                        "Ordering {} files by development flow...",
                        important_files.len()
                    ),
                    0.1,
                )
            });
        }

        let ordered = llm::order_files_by_development_flow(
            &config,
            &project_context,
            &message,
            &important_files,
        )
        .await;

        let mut all_chunks: Vec<DiffChunk> = Vec::new();
        let mut global_id = 0usize;
        let total_files = ordered.len();

        for (i, (filename, diff, _)) in ordered.iter().enumerate() {
            // Progress: 15% to 95% based on file processing
            let file_progress = 0.15 + (0.80 * (i as f32 / total_files.max(1) as f32));

            if let Some(ref p) = progress {
                let _ = p.lock().map(|mut s| {
                    *s = (
                        format!(
                            "Processing file {}/{}: {}",
                            i + 1,
                            total_files,
                            filename.rsplit('/').next().unwrap_or(filename)
                        ),
                        file_progress,
                    )
                });
            }
            if i > 0 {
                tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
            }

            if let Ok(mut file_chunks) = chunker::split_diff_into_chunks(
                &config,
                &project_context,
                &message,
                filename,
                diff,
                speed_ms,
            )
            .await
            {
                for chunk in &mut file_chunks {
                    chunk.chunk_id = global_id;
                    global_id += 1;

                    let word_count = chunk.explanation.split_whitespace().count();

                    if let Some(ref p) = progress {
                        let _ = p.lock().map(|mut s| {
                            *s = (
                                format!(
                                    "Synthesizing audio {}/{}: {} (chunk {})",
                                    i + 1,
                                    total_files,
                                    filename.rsplit('/').next().unwrap_or(filename),
                                    chunk.chunk_id + 1
                                ),
                                file_progress,
                            )
                        });
                    }

                    tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;

                    if let Ok(audio_data) =
                        tts::synthesize_speech_from_text(&config, &chunk.explanation).await
                    {
                        let real_duration = {
                            let cursor = std::io::Cursor::new(audio_data.clone());
                            Decoder::new(cursor)
                                .ok()
                                .and_then(|s| s.total_duration())
                                .map(|d| d.as_secs_f32())
                        };
                        chunk.audio_duration_secs =
                            real_duration.unwrap_or((word_count as f32) / 2.5);
                        chunk.audio_data = Some(audio_data);
                        chunk.has_audio = true;
                    }
                }
                all_chunks.extend(file_chunks);
            }
        }

        if let Ok(mut guard) = chunks_map.lock() {
            for chunk in &all_chunks {
                guard.insert(chunk.chunk_id, chunk.clone());
            }
        }

        if let Some(ref p) = progress {
            let _ = p.lock().map(|mut s| *s = ("Complete!".to_string(), 1.0));
        }

        all_chunks
    })
}
