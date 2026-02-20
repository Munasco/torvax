pub mod types;
pub(crate) mod chunker;
pub(crate) mod llm;
pub(crate) mod tts;

pub use types::{
    DiffChunk, VoiceoverConfig, VoiceoverProvider, VoiceoverSegment,
    VoiceoverTrigger,
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

        let (_stream, stream_handle) = OutputStream::try_default()
            .context("Failed to create audio output stream")?;
        let sink =
            Sink::try_new(&stream_handle).context("Failed to create audio sink")?;
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
            .map(|g| g.values().filter(|c| c.file_path == file_path).cloned().collect())
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
                        if let Ok(sink_guard) = sink_arc.lock() {
                            let cursor = std::io::Cursor::new(audio_data);
                            if let Ok(source) = Decoder::new(cursor) {
                                sink_guard.append(source);
                                sink_guard.play();
                                let ms = (chunk.audio_duration_secs * 1000.0) as u64;
                                thread::sleep(std::time::Duration::from_millis(ms));
                                let _ = tx.send(chunk_id);
                            }
                        }
                    }
                }
            }
        });
    }

    /// Pre-generate all audio chunks for a commit's file changes (blocking)
    pub fn generate_audio_chunks(
        &self,
        _commit_hash: String,
        _author: String,
        message: String,
        file_changes: Vec<(String, String, FileStatus)>,
        speed_ms: u64,
    ) -> Vec<DiffChunk> {
        if !self.config.enabled || self.config.api_key.is_none() {
            return Vec::new();
        }

        let config = self.config.clone();
        let chunks_map = self.chunks.clone();

        let rt = match tokio::runtime::Runtime::new() {
            Ok(rt) => rt,
            Err(_) => return Vec::new(),
        };

        rt.block_on(async {
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
                    !name.contains("package-lock.json")
                        && !name.contains("yarn.lock")
                        && !name.contains("pnpm-lock.yaml")
                        && !name.ends_with(".lock")
                        && !name.ends_with(".json")
                })
                .take(5)
                .collect();

            let ordered = llm::order_files_by_development_flow(
                &config, &project_context, &message, &important_files,
            )
            .await;

            let mut all_chunks: Vec<DiffChunk> = Vec::new();
            let mut global_id = 0usize;

            for (i, (filename, diff, _)) in ordered.iter().enumerate() {
                if i > 0 {
                    tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
                }

                if let Ok(mut file_chunks) = chunker::split_diff_into_chunks(
                    &config, &project_context, &message, filename, diff, speed_ms,
                )
                .await
                {
                    for chunk in &mut file_chunks {
                        chunk.chunk_id = global_id;
                        global_id += 1;

                        let word_count = chunk.explanation.split_whitespace().count();
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

            all_chunks
        })
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
                q.iter().position(|s| s.trigger_type == trigger_type).map(|i| q.remove(i).unwrap())
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
