use anyhow::{Context, Result};
use async_openai::{
    Client,
    config::OpenAIConfig,
    types::{
        ChatCompletionRequestMessage,
        ChatCompletionRequestUserMessageArgs,
        CreateChatCompletionRequestArgs,
    },
};
use crate::git::FileStatus;
use super::types::{ProjectContext, VoiceoverConfig};

/// Build a ProjectContext from the local repo (repo_name filled, description empty until LLM runs)
pub fn extract_project_context() -> ProjectContext {
    let repo_name = extract_repo_name().unwrap_or_else(|| {
        std::env::current_dir()
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
            .unwrap_or_else(|| "repository".to_string())
    });
    ProjectContext { repo_name, description: String::new() }
}

/// Generate a TTS-friendly project description via GPT
pub async fn generate_project_context_with_llm(config: &VoiceoverConfig) -> Result<String> {
    let api_key = config
        .openai_api_key
        .as_ref()
        .context("OpenAI API key not configured")?;

    let key_files = [
        ("Cargo.toml", 5000),
        ("package.json", 5000),
        ("src/main.rs", 8000),
        ("src/lib.rs", 8000),
        ("src/index.ts", 8000),
        ("main.py", 8000),
        ("README.md", 3000),
    ];

    let context_files: Vec<String> = key_files
        .iter()
        .filter_map(|(path, max)| {
            std::fs::read_to_string(path).ok().map(|content| {
                let preview = content.chars().take(*max).collect::<String>();
                format!("File: {}\n{}", path, preview)
            })
        })
        .collect();

    if context_files.is_empty() {
        anyhow::bail!("No key files found for context extraction");
    }

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
        context_files.join("\n\n---\n\n")
    );

    let cfg = OpenAIConfig::new().with_api_key(api_key);
    let client = Client::with_config(cfg);

    let request = CreateChatCompletionRequestArgs::default()
        .model("gpt-5.2")
        .messages(vec![ChatCompletionRequestMessage::User(
            ChatCompletionRequestUserMessageArgs::default()
                .content(prompt)
                .build()?,
        )])
        .temperature(0.5)
        .max_completion_tokens(2048u32)
        .build()?;

    let response = client
        .chat()
        .create(request)
        .await
        .context("Failed to call OpenAI API")?;

    response
        .choices
        .first()
        .and_then(|c| c.message.content.as_ref())
        .context("No content in OpenAI response")
        .map(|s| s.trim().to_string())
}

/// Extract repo name from .git/config remote URL
fn extract_repo_name() -> Option<String> {
    let config = std::fs::read_to_string(".git/config").ok()?;
    for line in config.lines() {
        if line.contains("url = ") {
            let url = line.split("url = ").nth(1)?.trim();
            return Some(url.trim_end_matches(".git").rsplit('/').next()?.to_string());
        }
    }
    None
}

/// Calculate animation duration from diff lines, mirroring animation.rs timing constants.
/// speed_ms: base typing delay per character in milliseconds.
pub fn calculate_animation_duration(diff_lines: &[&str], speed_ms: u64) -> f32 {
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
            if hunk_count > 0 {
                total_ms += HUNK_PAUSE * speed;
            }
            total_ms += CURSOR_MOVE_PAUSE * speed * 5.0;
            hunk_count += 1;
            in_hunk = true;
            continue;
        }
        if !in_hunk {
            continue;
        }
        if line.starts_with('+') && !line.starts_with("+++") {
            let char_count = line.len().saturating_sub(1);
            total_ms += (char_count as f64) * speed;
            total_ms += INSERT_LINE_PAUSE * speed;
        } else if line.starts_with('-') && !line.starts_with("---") {
            total_ms += DELETE_LINE_PAUSE * speed;
        }
    }

    (total_ms / 1000.0).max(5.0) as f32
}

/// Target word count so narration outlasts the animation (2× buffer, clamp 40–400).
pub fn words_for_duration(animation_secs: f32) -> usize {
    ((animation_secs * 2.5 * 2.0) as usize).clamp(40, 400)
}

/// Order files by logical development flow using GPT. Falls back to original order on error.
pub async fn order_files_by_development_flow(
    config: &VoiceoverConfig,
    project_context: &ProjectContext,
    commit_message: &str,
    files: &[(String, String, FileStatus)],
) -> Vec<(String, String, FileStatus)> {
    if files.len() <= 1 {
        return files.to_vec();
    }
    let api_key = match config.openai_api_key.as_ref() {
        Some(k) => k,
        None => return files.to_vec(),
    };

    let file_list: Vec<String> = files
        .iter()
        .enumerate()
        .map(|(i, (name, diff, status))| {
            let s = match status {
                FileStatus::Added => "new file",
                FileStatus::Deleted => "deleted",
                FileStatus::Modified => "modified",
                FileStatus::Renamed => "renamed",
                FileStatus::Copied => "copied",
                FileStatus::Unmodified => "unchanged",
            };
            format!("{}: {} ({}, {} diff lines)", i, name, s, diff.lines().count())
        })
        .collect();

    let prompt = format!(
        "You are ordering files for a code walkthrough narration.\n\n\
        PROJECT: {} - {}\nCOMMIT: \"{}\"\n\nFILES:\n{}\n\n\
        Order these files by how a developer would logically write them:\n\
        - Config/setup files first\n- Type definitions and data models next\n\
        - Core logic and business rules\n- Integration points (API calls, database)\n\
        - UI/presentation last\n- New files before modifications\n- Dependencies before dependents\n\n\
        Respond with ONLY a JSON array of the file indices. Example: [2, 0, 3, 1]",
        project_context.repo_name,
        &project_context.description.chars().take(200).collect::<String>(),
        commit_message,
        file_list.join("\n")
    );

    let cfg = OpenAIConfig::new().with_api_key(api_key);
    let client = Client::with_config(cfg);
    let request = match CreateChatCompletionRequestArgs::default()
        .model("gpt-5.2")
        .messages(vec![ChatCompletionRequestMessage::User(
            match ChatCompletionRequestUserMessageArgs::default().content(prompt).build() {
                Ok(m) => m,
                Err(_) => return files.to_vec(),
            },
        )])
        .temperature(0.2)
        .max_completion_tokens(128u32)
        .build()
    {
        Ok(r) => r,
        Err(_) => return files.to_vec(),
    };

    match client.chat().create(request).await {
        Ok(response) => {
            if let Some(content) = response.choices.first().and_then(|c| c.message.content.as_ref()) {
                if let Ok(indices) = serde_json::from_str::<Vec<usize>>(content.trim()) {
                    let mut ordered = Vec::with_capacity(files.len());
                    let mut used = std::collections::HashSet::new();
                    for &idx in &indices {
                        if idx < files.len() && used.insert(idx) {
                            ordered.push(files[idx].clone());
                        }
                    }
                    for (i, file) in files.iter().enumerate() {
                        if !used.contains(&i) { ordered.push(file.clone()); }
                    }
                    return ordered;
                }
            }
            files.to_vec()
        }
        Err(_) => files.to_vec(),
    }
}
