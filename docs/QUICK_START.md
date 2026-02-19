# Quick Start Guide: Testing Voiceover with LLM

This guide helps you quickly test the LLM-powered voiceover feature after cloning the repository.

## Prerequisites

1. **API Keys** (get these first):
   - OpenAI API key from [platform.openai.com](https://platform.openai.com/)
   - ElevenLabs API key from [elevenlabs.io](https://elevenlabs.io/)

2. **Rust** (for building):
   - Install from [rustup.rs](https://rustup.rs/)

## Quick Test (5 minutes)

### Step 1: Clone and Build

```bash
# Clone the repository
git clone https://github.com/Munasco/gitlogue.git
cd gitlogue

# Build with audio support (takes 2-3 minutes)
cargo build --release --features audio
```

### Step 2: Set API Keys

```bash
# Set your OpenAI key (for intelligent explanations)
export OPENAI_API_KEY="sk-..."

# Set your ElevenLabs key (for voice synthesis)
export ELEVENLABS_API_KEY="..."
```

### Step 3: Test on This Repository

```bash
# Review the last 5 commits with voiceover
./target/release/gitlogue --voiceover --commit HEAD~5..HEAD
```

## Test on Your Own Repository

```bash
# Navigate to your project
cd /path/to/your/project

# Run gitlogue with voiceover
/path/to/gitlogue/target/release/gitlogue --voiceover
```

## What You Should Hear

Instead of basic metadata like:
> "Commit abc123 by John Doe. Fix bug. Modified 3 files."

You'll hear intelligent explanations like:
> "This commit addresses a critical authentication vulnerability by implementing token expiration checks and adding rate limiting to prevent brute force attacks."

## Troubleshooting

### No Audio Plays

Check that both API keys are set:
```bash
echo $OPENAI_API_KEY
echo $ELEVENLABS_API_KEY
```

### "Warning: Voiceover enabled but no API key configured"

Solution:
```bash
# Make sure to export in the same terminal session
export ELEVENLABS_API_KEY="your-key-here"
```

### Build Fails

If you get dependency errors:
```bash
# Update Rust
rustup update

# Clean and rebuild
cargo clean
cargo build --release --features audio
```

### LLM Explanations Not Working

Check if OpenAI key is valid:
```bash
# Test with curl
curl https://api.openai.com/v1/models \
  -H "Authorization: Bearer $OPENAI_API_KEY"
```

If this fails, your key may be invalid or expired.

## Configuration File (Optional)

For persistent settings, create `~/.config/gitlogue/config.toml`:

```toml
[voiceover]
enabled = true
provider = "elevenlabs"
use_llm_explanations = true
api_key = "your-elevenlabs-key"
openai_api_key = "your-openai-key"
voice_id = "21m00Tcm4TlvDq8ikWAM"
```

## Command Reference

```bash
# Basic usage with voiceover
gitlogue --voiceover

# Review specific commits
gitlogue --voiceover --commit abc123

# Review commit range
gitlogue --voiceover --commit HEAD~10..HEAD

# Review by author
gitlogue --voiceover --author "your-name" --after "1 week ago"

# Slower playback for presentations
gitlogue --voiceover --commit HEAD~5..HEAD --speed 100

# Loop through commits
gitlogue --voiceover --commit HEAD~10..HEAD --loop
```

## Expected Behavior

1. **First commit loads**: You'll hear a slight delay (2-3 seconds) while OpenAI generates the explanation
2. **Audio plays**: The explanation plays via ElevenLabs TTS
3. **Animation continues**: Code typing animation proceeds while audio plays
4. **Next commit**: Process repeats for each commit

## Costs

**For testing (5 commits):**
- OpenAI: ~$0.01 (GPT-3.5-turbo)
- ElevenLabs: Free tier covers testing

**Monthly usage (1000 commits):**
- OpenAI: ~$1-2
- ElevenLabs: ~$5-10 or free tier

## Disable LLM (Use Basic Narration)

To test basic voiceover without OpenAI:

```bash
# Don't set OPENAI_API_KEY
unset OPENAI_API_KEY

# Only set TTS key
export ELEVENLABS_API_KEY="..."

# Run gitlogue
./target/release/gitlogue --voiceover
```

## Next Steps

- Read [LLM-Powered Explanations Guide](llm-voiceover.md) for advanced features
- Read [Voiceover Guide](voiceover.md) for TTS customization
- Configure default settings in `~/.config/gitlogue/config.toml`

## Support

If you encounter issues:
1. Check [Troubleshooting section](llm-voiceover.md#troubleshooting)
2. Verify API keys are valid
3. Ensure internet connection is stable
4. Check terminal for error messages

## Example Session

```bash
$ git clone https://github.com/Munasco/gitlogue.git
$ cd gitlogue
$ cargo build --release --features audio
   Compiling gitlogue v0.8.0 (/path/to/gitlogue)
   Finished release [optimized] target(s) in 3m 02s

$ export OPENAI_API_KEY="sk-..."
$ export ELEVENLABS_API_KEY="..."

$ ./target/release/gitlogue --voiceover --commit HEAD~3..HEAD
# Audio plays: "This commit enhances the voiceover feature by integrating
# OpenAI's GPT model to generate intelligent explanations of code changes..."
# (Animation plays while audio continues)
```

## Feedback

The LLM-powered voiceover is a new feature! Please share:
- What works well
- What could be improved
- Example commits that generate great/poor explanations
- Feature requests

Open an issue on GitHub with your feedback.
