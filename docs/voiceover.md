# Voiceover Feature

The voiceover feature adds audio narration to git commit reviews, explaining the changes made in each commit. This makes git history more accessible and easier to understand, especially for code reviews and presentations.

> **✨ NEW:** [LLM-Powered Intelligent Explanations](llm-voiceover.md) - Use OpenAI to generate smart, contextual explanations instead of just reading metadata!

## Overview

Gitlogue can now narrate commit information using Text-to-Speech (TTS) services. The system supports two modes:

### Basic Mode (Default)
When a commit is displayed, the voiceover reads:
- The commit hash (shortened)
- The author name
- The commit message
- Number of files changed

### LLM-Enhanced Mode (Recommended)
With OpenAI integration, get intelligent explanations like:
- *"This commit fixes an authentication bug by adding token validation"*
- *"The changes optimize database queries, reducing response time by 60%"*

See [LLM-Powered Explanations](llm-voiceover.md) for setup and examples.

## Supported Providers

### ElevenLabs

[ElevenLabs](https://elevenlabs.io/) provides high-quality, natural-sounding AI voices with emotional depth and clarity.

**Features:**
- Multiple voice options
- Adjustable voice settings (stability, similarity_boost)
- High-quality audio output (MP3 format)
- Low latency

**Default Voice:** Rachel (`21m00Tcm4TlvDq8ikWAM`)

**Default Model:** `eleven_monolingual_v1`

### Inworld

[Inworld AI](https://www.inworld.ai/) offers character-driven AI voices suitable for interactive experiences.

**Note:** The Inworld integration is a placeholder implementation. The actual API structure may differ. Please refer to Inworld's official documentation for the correct API endpoints and authentication methods.

## Configuration

### Environment Variables

The easiest way to configure voiceover is through environment variables:

```bash
# For ElevenLabs
export ELEVENLABS_API_KEY="your-api-key-here"

# For Inworld
export INWORLD_API_KEY="your-api-key-here"
```

### Configuration File

Add voiceover settings to `~/.config/gitlogue/config.toml`:

```toml
# Voiceover settings for narrating git changes
[voiceover]
enabled = true
provider = "elevenlabs"  # Options: "elevenlabs" or "inworld"
api_key = "your-api-key-here"  # Optional: can also use environment variable
voice_id = "21m00Tcm4TlvDq8ikWAM"  # Optional: ElevenLabs voice ID
model_id = "eleven_monolingual_v1"  # Optional: ElevenLabs model ID
```

**Configuration Priority:**
1. CLI arguments (highest)
2. Environment variables
3. Configuration file
4. Default values (lowest)

## Usage

### Enable Voiceover

Enable voiceover using the `--voiceover` flag:

```bash
# Enable voiceover for commit playback
gitlogue --voiceover

# Enable voiceover for a specific commit
gitlogue --commit abc123 --voiceover

# Enable voiceover for a commit range
gitlogue --commit HEAD~5..HEAD --voiceover
```

### Disable Voiceover

If voiceover is enabled in the config file, you can disable it:

```bash
gitlogue --voiceover=false
```

### Choose Provider

Override the provider from the command line:

```bash
# Use ElevenLabs
gitlogue --voiceover --voiceover-provider elevenlabs

# Use Inworld
gitlogue --voiceover --voiceover-provider inworld
```

## Getting API Keys

### ElevenLabs

1. Visit [ElevenLabs](https://elevenlabs.io/)
2. Sign up for an account
3. Navigate to your profile settings
4. Copy your API key from the "API Keys" section

**Free Tier:** ElevenLabs offers a free tier with monthly character limits.

### Inworld

1. Visit [Inworld AI](https://www.inworld.ai/)
2. Create an account
3. Access your API credentials from the dashboard

## Voice Customization

### ElevenLabs Voice Options

You can choose from various pre-made voices or create your own. To use a different voice:

1. Browse voices at [ElevenLabs Voice Library](https://elevenlabs.io/voice-library)
2. Copy the voice ID
3. Set it in your config file or use a custom configuration

Popular voices:
- `21m00Tcm4TlvDq8ikWAM` - Rachel (default)
- `EXAVITQu4vr4xnSDxMaL` - Bella
- `ErXwobaYiN019PkySvjV` - Antoni
- `MF3mGyEYCl7XYWbV9V6O` - Elli

### Example Narration

For a commit with:
- Hash: `a1b2c3d`
- Author: `John Doe`
- Message: `Fix authentication bug`
- Files: 3 modified
- Insertions: 25 lines
- Deletions: 10 lines

The narration would be:

> "Reviewing commit a1b2c3d by John Doe. Fix authentication bug. This commit modified 3 files, adding 25 lines and removing 10 lines."

## Audio Playback

### Asynchronous Playback

Voiceovers play asynchronously in the background, allowing the commit animation to continue without blocking. The audio starts when a commit is loaded and plays while the typing animation proceeds.

### Dependencies

The audio system uses:
- `reqwest` - HTTP client for API requests
- `rodio` - Audio playback library
- `tokio` - Async runtime for background processing
- `serde_json` - JSON serialization for API communication

Audio support is now built-in by default, so you can simply:

```bash
cargo build
cargo install --path .
```

## Troubleshooting

### No Audio Output

**Problem:** Voiceover is enabled but no audio plays.

**Solutions:**
1. Check that your API key is correctly set
2. Verify you have an active internet connection
3. Check terminal output for error messages

### API Errors

**Problem:** Error messages about API authentication.

**Solutions:**
1. Verify your API key is correct
2. Check that your account has available credits/quota
3. Ensure the API key environment variable is exported in your current shell

### Audio Quality Issues

**Problem:** Audio sounds distorted or choppy.

**Solutions:**
1. Check your internet connection speed
2. For ElevenLabs, adjust voice settings in config:
   ```toml
   [voiceover]
   enabled = true
   provider = "elevenlabs"
   # Adjust these values between 0.0 and 1.0
   # Higher stability = more consistent but less dynamic
   # Lower stability = more expressive but potentially less consistent
   ```

## Privacy and Security

### API Key Security

- Never commit API keys to version control
- Use environment variables or config files with restricted permissions
- Keep your config file secure: `chmod 600 ~/.config/gitlogue/config.toml`

### Data Transmission

When voiceover is enabled:
- Commit messages are sent to the TTS provider's API
- Author names and basic commit statistics are included in narration
- No source code content is transmitted
- Consider this when working with sensitive repositories

## Examples

### Basic Usage

```bash
# Simple commit playback with voiceover
export ELEVENLABS_API_KEY="sk_..."
gitlogue --voiceover
```

### Code Review Scenario

```bash
# Review last 10 commits with voiceover
gitlogue --commit HEAD~10..HEAD --voiceover --speed 50
```

### Presentation Mode

```bash
# Loop through recent commits with slower speed and voiceover
gitlogue --commit HEAD~20..HEAD --loop --speed 80 --voiceover
```

### Diff Review with Voiceover

```bash
# Review staged changes with voiceover narration
gitlogue diff --voiceover
```

## Future Enhancements

Potential future improvements for the voiceover system:

- [ ] Local TTS option (no API required)
- [ ] Voice cloning support
- [ ] Multilingual narration
- [ ] Customizable narration templates
- [ ] Code snippet pronunciation improvements
- [ ] Keyboard shortcut to mute/unmute
- [ ] Volume control
- [ ] Audio caching to reduce API calls
- [ ] Offline mode with pre-generated audio

## Contributing

If you'd like to contribute to the voiceover feature:

1. Report bugs or issues on GitHub
2. Suggest new TTS providers
3. Improve narration templates
4. Add support for more languages
5. Optimize audio playback performance

See [CONTRIBUTING.md](CONTRIBUTING.md) for more details.

## Credits

The voiceover feature integrates with:
- [ElevenLabs](https://elevenlabs.io/) - AI voice generation
- [Inworld AI](https://www.inworld.ai/) - Character AI voices
- [rodio](https://github.com/RustAudio/rodio) - Audio playback library
