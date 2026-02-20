use anyhow::{Context, Result};
use base64::{Engine as _, engine::general_purpose};
use super::types::{VoiceoverConfig, VoiceoverProvider};

/// Dispatch TTS to the configured provider
pub async fn synthesize_speech_from_text(config: &VoiceoverConfig, text: &str) -> Result<Vec<u8>> {
    match config.provider {
        VoiceoverProvider::ElevenLabs => synthesize_elevenlabs(config, text).await,
        VoiceoverProvider::Inworld => synthesize_inworld(config, text).await,
    }
}

async fn synthesize_elevenlabs(config: &VoiceoverConfig, text: &str) -> Result<Vec<u8>> {
    let api_key = config
        .api_key
        .as_ref()
        .context("ElevenLabs API key not configured")?;

    let voice_id = config.voice_id.as_deref().unwrap_or("21m00Tcm4TlvDq8ikWAM");
    let model_id = config.model_id.as_deref().unwrap_or("eleven_flash_v2_5");

    let url = format!("https://api.elevenlabs.io/v1/text-to-speech/{}", voice_id);

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

    response
        .bytes()
        .await
        .context("Failed to read audio response")
        .map(|b| b.to_vec())
}

async fn synthesize_inworld(config: &VoiceoverConfig, text: &str) -> Result<Vec<u8>> {
    let api_key = config
        .api_key
        .as_ref()
        .context("Inworld API key not configured (Basic auth base64)")?;

    let voice_id = config.voice_id.as_deref().unwrap_or("Ashley");
    let model_id = config.model_id.as_deref().unwrap_or("inworld-tts-1.5-max");

    let client = reqwest::Client::new();
    let response = client
        .post("https://api.inworld.ai/tts/v1/voice")
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

    let audio_base64 = response_json["audioContent"]
        .as_str()
        .context("Failed to extract audioContent from Inworld response")?;

    general_purpose::STANDARD
        .decode(audio_base64)
        .context("Failed to decode base64 audio from Inworld")
}
