use serde::{Deserialize, Serialize};

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

impl Default for VoiceoverConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            provider: VoiceoverProvider::Inworld,
            api_key: None,
            voice_id: None,
            model_id: None,
            openai_api_key: None,
            use_llm_explanations: false,
        }
    }
}

/// Project context used to give LLM repository awareness
#[derive(Debug, Clone)]
pub struct ProjectContext {
    pub repo_name: String,
    pub description: String,
}

/// A single voiceover chunk covering a portion of a file diff
#[derive(Debug, Clone)]
pub struct DiffChunk {
    pub chunk_id: usize,
    pub file_path: String,
    pub hunk_indices: Vec<usize>,
    pub explanation: String,
    pub audio_data: Option<Vec<u8>>,
    pub has_audio: bool,
    pub audio_duration_secs: f32,
}

/// A queued voiceover segment (for file-open triggers)
#[derive(Debug, Clone)]
pub struct VoiceoverSegment {
    pub audio_data: Option<Vec<u8>>,
    pub trigger_type: VoiceoverTrigger,
}

/// When to trigger a voiceover segment
#[derive(Debug, Clone, PartialEq)]
pub enum VoiceoverTrigger {
    FileOpen(String),
}
