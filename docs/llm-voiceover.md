# LLM-Powered Voiceover Explanations

The voiceover feature now includes **intelligent code explanations** powered by OpenAI's GPT models. Instead of just reading commit metadata, the system generates contextual explanations that describe what the code changes actually do.

## What's the Difference?

### Basic Voiceover (Without LLM)
```
"Reviewing commit a1b2c3d by John Doe. Fix authentication bug. This commit modified 3 files."
```

### LLM-Enhanced Voiceover (With OpenAI)
```
"This commit fixes an authentication vulnerability by adding proper token validation 
in the login handler. The changes ensure that expired tokens are rejected before 
database queries, preventing unauthorized access. Additionally, error handling has 
been improved to log security events."
```

## How It Works

1. **Context Building**: The system collects commit metadata and file diffs
2. **LLM Analysis**: OpenAI's GPT analyzes the changes and generates a conversational explanation
3. **Speech Synthesis**: The explanation is converted to speech using your chosen TTS provider (ElevenLabs/Inworld)
4. **Async Playback**: Audio plays in the background while the animation continues

## Setup

### 1. Get Your API Keys

**OpenAI (for explanations):**
- Visit [OpenAI Platform](https://platform.openai.com/)
- Sign up or log in
- Go to API Keys section
- Create a new API key

**ElevenLabs or Inworld (for voice synthesis):**
- See [voiceover.md](voiceover.md) for TTS provider setup

### 2. Set Environment Variables

```bash
# Required: OpenAI for intelligent explanations
export OPENAI_API_KEY="sk-..."

# Required: TTS provider (choose one)
export ELEVENLABS_API_KEY="..."
# OR
export INWORLD_API_KEY="..."
```

### 3. Enable Voiceover

```bash
# Clone the repo
git clone https://github.com/Munasco/gitlogue.git
cd gitlogue

# Build
cargo build --release

# Run with voiceover on your repository
cd /path/to/your/repo
/path/to/gitlogue/target/release/gitlogue --voiceover
```

## Configuration File

Add to `~/.config/gitlogue/config.toml`:

```toml
[voiceover]
enabled = true
provider = "elevenlabs"
use_llm_explanations = true

# Optional: Store keys in config (or use environment variables)
# openai_api_key = "sk-..."
# api_key = "elevenlabs-key..."

# Optional: Customize voice
voice_id = "21m00Tcm4TlvDq8ikWAM"  # Rachel voice (default)
```

## Usage Examples

### Review Recent Commits

```bash
# Export API keys
export OPENAI_API_KEY="sk-..."
export ELEVENLABS_API_KEY="..."

# Review last 10 commits with intelligent explanations
gitlogue --voiceover --commit HEAD~10..HEAD
```

### Review Specific Commit

```bash
# Deep dive into a specific commit
gitlogue --voiceover --commit abc123
```

### Review Your Work

```bash
# Check what you've been working on
gitlogue --voiceover --author "your-name" --after "1 week ago"
```

### Code Review Session

```bash
# Review pull request changes with slower animation
gitlogue --voiceover --commit main..feature-branch --speed 100
```

## Quick Start Command

After cloning gitlogue and setting up API keys:

```bash
# Build
cargo build --release

# Test on the gitlogue repo itself
export OPENAI_API_KEY="sk-..."
export ELEVENLABS_API_KEY="..."
./target/release/gitlogue --voiceover --commit HEAD~5..HEAD
```

## What Gets Analyzed?

The LLM receives:
- Commit hash, author, and message
- File names that changed
- Diff content (additions, deletions, context lines)
- Up to 5 files with 50 lines each (to stay within token limits)

The LLM is instructed to:
- Explain what the code does, not just what changed
- Use conversational language suitable for text-to-speech
- Keep explanations concise (2-3 sentences)
- Focus on the "why" and impact, not just the "what"

## Fallback Behavior

If LLM explanation fails:
- Falls back to simple metadata narration
- Logs error message
- Continues without interruption

## Privacy & Security

**What's Sent to OpenAI:**
- Commit metadata (hash, author, message)
- File names
- Code diffs (limited to first 5 files, 50 lines each)

**What's NOT Sent:**
- Full repository content
- File content outside of diffs
- Binary files
- Git history

**Security Considerations:**
- Use environment variables for API keys
- Don't commit keys to version control
- Consider disabling for sensitive/proprietary code
- Review OpenAI's data usage policy

## Cost Estimate

**OpenAI Costs:**
- Model: GPT-3.5-turbo (fastest, cheapest)
- Average cost: ~$0.001-0.002 per commit explanation
- 1000 commits ≈ $1-2

**ElevenLabs Costs:**
- Free tier: 10,000 characters/month
- Average narration: 100-200 characters
- Free tier ≈ 50-100 commits/month

## Troubleshooting

### No LLM Explanations

**Problem:** Still getting basic narration despite setting OpenAI key.

**Solution:**
```bash
# Verify key is set
echo $OPENAI_API_KEY

# Check config
cat ~/.config/gitlogue/config.toml

# Enable explicitly in config
use_llm_explanations = true
```

### API Errors

**Problem:** "OpenAI API error" messages.

**Solutions:**
1. Verify API key is valid
2. Check account has credits
3. Ensure internet connection is stable
4. Check OpenAI service status

### Poor Explanations

**Problem:** Explanations are too generic or inaccurate.

**Current Behavior:**
- System uses GPT-3.5-turbo for speed/cost
- Limited to 200 tokens output
- First 5 files only (to stay within context window)

**Future Improvements:**
- Use GPT-4 for better analysis (configurable)
- Increase token limits
- Add more context about repository structure

## Disabling LLM Explanations

To use basic voiceover without LLM:

```bash
# Don't set OPENAI_API_KEY
unset OPENAI_API_KEY

# Or in config:
[voiceover]
enabled = true
use_llm_explanations = false
```

## Advanced: Customizing the Prompt

The LLM receives a system prompt that instructs it to:
- Provide clear, conversational explanations
- Explain what code does and why it matters
- Be concise (2-3 sentences)
- Use natural language suitable for TTS

Future versions may allow custom prompts in the config file.

## Examples of LLM Explanations

**Authentication Fix:**
```
"This commit strengthens authentication by adding token expiration checks 
and implementing rate limiting to prevent brute force attacks."
```

**Performance Optimization:**
```
"The code introduces database query caching which reduces response time 
by 60% for frequently accessed user data."
```

**UI Enhancement:**
```
"This change adds dark mode support with automatic theme detection based 
on system preferences, improving accessibility for users."
```

## Comparison: Basic vs LLM

| Feature | Basic Voiceover | LLM-Enhanced |
|---------|----------------|--------------|
| Setup | TTS API only | TTS + OpenAI |
| Cost | TTS only | TTS + OpenAI |
| Output | Metadata summary | Intelligent explanation |
| Speed | Instant | ~2-3 seconds delay |
| Privacy | Minimal data sent | Code diffs sent to OpenAI |
| Value | Quick overview | Deep understanding |

## Best For

**Use LLM-Enhanced When:**
- Learning a new codebase
- Code review sessions
- Educational presentations
- Understanding complex changes
- Onboarding new team members

**Use Basic When:**
- Privacy is critical
- Minimizing costs
- Quick overviews needed
- Offline operation required

## Future Enhancements

Planned improvements:
- [ ] Support for other LLM providers (Anthropic Claude, local models)
- [ ] Customizable prompts via config
- [ ] GPT-4 option for better analysis
- [ ] Context caching to reduce API calls
- [ ] Pre-generated explanations mode
- [ ] Interactive Q&A about commits

## Contributing

Ideas for improving LLM integration:
1. Better prompts for specific types of changes
2. Context-aware explanations based on file types
3. Integration with commit message conventions
4. Support for multiple languages (i18n)

See [CONTRIBUTING.md](../CONTRIBUTING.md) for details.
