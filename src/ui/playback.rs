use anyhow::Result;

use crate::animation::StepMode;
use crate::git::{CommitMetadata, GitRepository};
use crate::PlaybackOrder;

use super::{PlaybackState, UIState, UI};

impl<'a> UI<'a> {
    pub(super) fn open_menu(&mut self) {
        self.prev_state = Some(Box::new(self.state.clone()));
        self.menu_index = 0;
        self.state = UIState::Menu;
        self.engine.pause();
    }

    pub(super) fn close_menu(&mut self) {
        let restored = self
            .prev_state
            .take()
            .map(|s| *s)
            .unwrap_or(UIState::Playing);
        self.state = match restored {
            UIState::WaitingForNext { .. } => UIState::Playing,
            other => other,
        };
        if self.playback_state == PlaybackState::Playing {
            self.engine.resume();
        }
    }

    pub(super) fn play_commit(&mut self, metadata: CommitMetadata, record_history: bool) {
        eprintln!(
            "[UI] play_commit called, has_audio_player={}",
            self.audio_player.is_some()
        );
        if record_history {
            self.record_history(&metadata);
        }

        // If audio is enabled, generate chunks in a background thread
        // and WAIT for completion before starting the video.
        // Show progress modal during generation.
        if let Some(audio_player) = &self.audio_player {
            eprintln!("[UI] Starting audio generation in background thread...");
            let config = audio_player.voiceover_config().clone();
            let chunks_map = audio_player.chunks_handle();
            let file_changes: Vec<(String, String, crate::git::FileStatus)> = metadata
                .changes
                .iter()
                .filter(|c| !c.is_excluded)
                .map(|c| (c.path.clone(), Self::build_diff_text(c), c.status.clone()))
                .collect();
            let message = metadata.message.clone();
            let speed_ms = self.speed_ms;
            let progress = self.audio_progress.clone();

            self.pending_metadata = Some(metadata);
            self.state = UIState::GeneratingAudio;
            self.audio_gen_handle = Some(std::thread::spawn(move || {
                crate::audio::generate_audio_chunks_with_progress(
                    config,
                    chunks_map,
                    message,
                    file_changes,
                    speed_ms,
                    progress,
                );
            }));
            return;
        }

        // No audio - start video immediately
        self.finish_play_commit(metadata);
    }

    /// Called once audio generation is done (or skipped) to actually start
    /// the animation with whatever audio chunks are available.
    pub(super) fn finish_play_commit(&mut self, metadata: CommitMetadata) {
        self.engine.load_commit(&metadata);
        match self.playback_state {
            PlaybackState::Playing => self.engine.resume(),
            PlaybackState::Paused => self.engine.pause(),
        }
        self.state = UIState::Playing;
    }

    /// Build a text representation of file diff (including @@ hunk headers for duration calculation)
    fn build_diff_text(change: &crate::git::FileChange) -> String {
        let mut diff = String::new();

        for hunk in &change.hunks {
            // Include hunk header so calculate_animation_duration can parse it
            diff.push_str(&format!(
                "@@ -{},{} +{},{} @@\n",
                hunk.old_start, hunk.old_lines, hunk.new_start, hunk.new_lines
            ));
            for line in &hunk.lines {
                match line.change_type {
                    crate::git::LineChangeType::Addition => {
                        diff.push_str(&format!("+{}\n", line.content));
                    }
                    crate::git::LineChangeType::Deletion => {
                        diff.push_str(&format!("-{}\n", line.content));
                    }
                    crate::git::LineChangeType::Context => {
                        diff.push_str(&format!(" {}\n", line.content));
                    }
                }
            }
        }

        diff
    }

    pub(super) fn record_history(&mut self, metadata: &CommitMetadata) {
        if let Some(index) = self.history_index {
            if index + 1 < self.history.len() {
                self.history.truncate(index + 1);
            }
        } else {
            self.history.clear();
        }

        self.history.push(metadata.clone());
        self.history_index = Some(self.history.len() - 1);
    }

    pub(super) fn play_history_commit(&mut self, index: usize) -> bool {
        if let Some(metadata) = self.history.get(index).cloned() {
            self.history_index = Some(index);
            self.play_commit(metadata, false);
            return true;
        }

        false
    }

    pub(super) fn toggle_pause(&mut self) {
        match self.playback_state {
            PlaybackState::Playing => {
                self.playback_state = PlaybackState::Paused;
                self.engine.pause();
            }
            PlaybackState::Paused => {
                self.playback_state = PlaybackState::Playing;
                self.engine.resume();
            }
        }
    }

    pub(super) fn ensure_manual_pause(&mut self) {
        if self.playback_state != PlaybackState::Paused {
            self.playback_state = PlaybackState::Paused;
            self.engine.pause();
        }
    }

    pub(super) fn step_line(&mut self) {
        self.ensure_manual_pause();
        let _ = self.engine.manual_step(StepMode::Line);
    }

    pub(super) fn step_change(&mut self) {
        self.ensure_manual_pause();
        let _ = self.engine.manual_step(StepMode::Change);
    }

    pub(super) fn step_line_back(&mut self) {
        self.ensure_manual_pause();
        let _ = self.engine.restore_line_checkpoint();
    }

    pub(super) fn step_change_back(&mut self) {
        self.ensure_manual_pause();
        let _ = self.engine.restore_change_checkpoint();
    }

    pub(super) fn handle_prev(&mut self) {
        if let Some(index) = self.history_index {
            if index > 0 {
                let target = index - 1;
                self.play_history_commit(target);
            }
        }
    }

    pub(super) fn handle_next(&mut self) {
        if let Some(index) = self.history_index {
            if index + 1 < self.history.len() {
                let target = index + 1;
                if self.play_history_commit(target) {
                    return;
                }
            }
        }

        if self.repo.is_none() && self.diff_mode.is_none() {
            return;
        }

        self.advance_to_next_commit();
    }

    pub(super) fn advance_to_next_commit(&mut self) -> bool {
        if let Some(diff_mode) = self.diff_mode {
            if let Some(repo) = self.repo {
                match repo.get_working_tree_diff(diff_mode) {
                    Ok(metadata) if !metadata.changes.is_empty() => {
                        self.load_commit(metadata);
                        return true;
                    }
                    _ => {
                        self.state = UIState::Finished;
                        return false;
                    }
                }
            }
            self.state = UIState::Finished;
            return false;
        }

        let Some(repo) = self.repo else {
            self.state = UIState::Finished;
            return false;
        };

        match self.fetch_repo_commit(repo) {
            Ok(metadata) => {
                self.load_commit(metadata);
                true
            }
            Err(_) => {
                if self.loop_playback {
                    repo.reset_index();
                    if let Ok(metadata) = self.fetch_repo_commit(repo) {
                        self.load_commit(metadata);
                        true
                    } else {
                        self.state = UIState::Finished;
                        false
                    }
                } else {
                    self.state = UIState::Finished;
                    false
                }
            }
        }
    }

    pub(super) fn fetch_repo_commit(&self, repo: &GitRepository) -> Result<CommitMetadata> {
        if self.is_range_mode {
            return match self.order {
                PlaybackOrder::Random => repo.random_range_commit(),
                PlaybackOrder::Asc => repo.next_range_commit_asc(),
                PlaybackOrder::Desc => repo.next_range_commit_desc(),
            };
        }

        if let Some(spec) = &self.commit_spec {
            return repo.get_commit(spec);
        }

        match self.order {
            PlaybackOrder::Random => repo.random_commit(),
            PlaybackOrder::Asc => repo.next_asc_commit(),
            PlaybackOrder::Desc => repo.next_desc_commit(),
        }
    }
}
