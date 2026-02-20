use super::llm::{calculate_animation_duration, words_for_duration};
use super::types::{DiffChunk, ProjectContext, VoiceoverConfig};
use anyhow::{Context, Result};
use async_openai::{
    config::OpenAIConfig,
    types::{
        ChatCompletionRequestMessage, ChatCompletionRequestUserMessageArgs,
        CreateChatCompletionRequestArgs,
    },
    Client,
};

/// Split a file diff into semantic chunks, each with an LLM explanation sized to match
/// the animation duration for that chunk.
pub async fn split_diff_into_chunks(
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

    // Parse diff into hunk groups
    let (hunks, hunk_summaries) = parse_hunks(diff);

    let chunk_groups: Vec<Vec<usize>> = if hunks.len() <= 1 {
        vec![(0..hunks.len()).collect()]
    } else {
        llm_group_hunks(
            api_key,
            config,
            project_context,
            commit_message,
            filename,
            &hunk_summaries,
            &hunks,
        )
        .await?
    };

    let cfg = OpenAIConfig::new().with_api_key(api_key);
    let client = Client::with_config(cfg);
    let mut chunks = Vec::new();

    for (idx, hunk_indices) in chunk_groups.iter().enumerate() {
        let chunk_lines: Vec<&str> = hunk_indices
            .iter()
            .flat_map(|&hi| hunks.get(hi).map(|h| h.as_slice()).unwrap_or(&[]))
            .copied()
            .collect();

        let animation_secs = calculate_animation_duration(&chunk_lines, speed_ms);
        let target_words = words_for_duration(animation_secs);
        let chunk_diff = chunk_lines.join("\n");

        let prompt = format!(
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

        let request = CreateChatCompletionRequestArgs::default()
            .model("gpt-5.2")
            .messages(vec![ChatCompletionRequestMessage::User(
                ChatCompletionRequestUserMessageArgs::default()
                    .content(prompt)
                    .build()?,
            )])
            .temperature(0.7)
            .max_completion_tokens((target_words * 2).max(200) as u32)
            .build()?;

        let response = client
            .chat()
            .create(request)
            .await
            .context("Failed to generate explanation")?;

        let explanation = response
            .choices
            .first()
            .and_then(|c| c.message.content.as_ref())
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
            audio_duration_secs: audio_secs,
        });
    }

    Ok(chunks)
}

// --- helpers -----------------------------------------------------------------

fn parse_hunks(diff: &str) -> (Vec<Vec<&str>>, Vec<String>) {
    let mut hunks: Vec<Vec<&str>> = Vec::new();
    let mut current: Vec<&str> = Vec::new();

    for line in diff.lines() {
        if line.starts_with("@@") {
            if !current.is_empty() {
                hunks.push(current.clone());
            }
            current = vec![line];
        } else if !current.is_empty() {
            current.push(line);
        }
    }
    if !current.is_empty() {
        hunks.push(current);
    }

    let summaries = hunks
        .iter()
        .enumerate()
        .map(|(i, lines)| {
            let header = lines.first().unwrap_or(&"");
            let adds = lines
                .iter()
                .filter(|l| l.starts_with('+') && !l.starts_with("+++"))
                .count();
            let dels = lines
                .iter()
                .filter(|l| l.starts_with('-') && !l.starts_with("---"))
                .count();
            let preview: Vec<&str> = lines
                .iter()
                .skip(1)
                .filter(|l| l.starts_with('+') || l.starts_with('-'))
                .take(3)
                .copied()
                .collect();
            format!(
                "Hunk {}: {} — {} additions, {} deletions\n  Preview: {}",
                i,
                header,
                adds,
                dels,
                preview.join(" | ")
            )
        })
        .collect();

    (hunks, summaries)
}

async fn llm_group_hunks(
    api_key: &str,
    _config: &VoiceoverConfig,
    project_context: &ProjectContext,
    commit_message: &str,
    filename: &str,
    hunk_summaries: &[String],
    hunks: &[Vec<&str>],
) -> Result<Vec<Vec<usize>>> {
    let prompt = format!(
        "You are grouping code changes for a narrated walkthrough.\n\n\
        PROJECT: {} - {}\n\
        COMMIT: \"{}\"\n\
        FILE: {}\n\n\
        HUNKS:\n{}\n\n\
        Group these hunks into 1-4 semantic chunks. Each chunk should cover a coherent change \
        (e.g. imports, a new function, config updates). Keep related hunks together.\n\n\
        Respond with ONLY JSON: {{\"chunks\": [[0, 1], [2], [3, 4]]}}",
        project_context.repo_name,
        &project_context
            .description
            .chars()
            .take(300)
            .collect::<String>(),
        commit_message,
        filename,
        hunk_summaries.join("\n")
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
        .temperature(0.3)
        .max_completion_tokens(256u32)
        .build()?;

    let response = client
        .chat()
        .create(request)
        .await
        .context("Failed to get hunk groupings")?;

    let content = response
        .choices
        .first()
        .and_then(|c| c.message.content.as_ref())
        .context("No content in grouping response")?;

    match serde_json::from_str::<serde_json::Value>(content.trim()) {
        Ok(parsed) => {
            if let Some(arr) = parsed["chunks"].as_array() {
                let mut groups: Vec<Vec<usize>> = Vec::new();
                let mut used = std::collections::HashSet::new();
                for group in arr {
                    if let Some(indices) = group.as_array() {
                        let valid: Vec<usize> = indices
                            .iter()
                            .filter_map(|v| v.as_u64().map(|n| n as usize))
                            .filter(|&i| i < hunks.len() && used.insert(i))
                            .collect();
                        if !valid.is_empty() {
                            groups.push(valid);
                        }
                    }
                }
                let missed: Vec<usize> = (0..hunks.len()).filter(|i| !used.contains(i)).collect();
                if !missed.is_empty() {
                    groups.push(missed);
                }
                return Ok(groups);
            }
            Ok((0..hunks.len()).map(|i| vec![i]).collect())
        }
        Err(_) => Ok(vec![(0..hunks.len()).collect()]),
    }
}
