# Answer to Your Question

## What Does the Audio Do Now?

The audio system now has **two modes**:

### 1. Basic Mode (What It Was Before)
Just reads commit metadata:
> "Reviewing commit abc123 by John Doe. Fix bug. Modified 3 files."

### 2. LLM Mode (What You Asked For) ✨
Uses **OpenAI ChatGPT** to understand the code and explain what it does:
> "This commit fixes an authentication vulnerability by adding proper token validation in the login handler. The changes ensure that expired tokens are rejected before database queries, preventing unauthorized access."

## You Were Right!

You said:
- ❌ "Reading code out loud is daft"
- ✅ Need "something that walks through the code"
- ✅ Use an LLM to understand context
- ✅ Use OpenAI with ChatGPT key

**That's exactly what I built!** 

## How It Works

1. **Context Building**: The system sends the commit message, file names, and code diffs to OpenAI
2. **LLM Analysis**: ChatGPT (GPT-3.5-turbo) analyzes what the code does and generates a conversational explanation
3. **Voice Synthesis**: The explanation is narrated using ElevenLabs or Inworld
4. **Async Playback**: Audio plays while the code animation continues

## Command to Run After Cloning

```bash
# 1. Clone the repo
git clone https://github.com/Munasco/gitlogue.git
cd gitlogue

# 2. Build with audio support
cargo build --release --features audio

# 3. Set your API keys
export OPENAI_API_KEY="sk-..."        # Get from platform.openai.com
export ELEVENLABS_API_KEY="..."       # Get from elevenlabs.io

# 4. Test on your own repo
cd /path/to/your/repo
/path/to/gitlogue/target/release/gitlogue --voiceover --commit HEAD~5..HEAD
```

That's it! The voiceover will now explain what each commit does in natural language.

## What Gets Sent to OpenAI

The LLM receives:
- Commit message
- Author name
- File names
- Code diffs (additions/deletions with context)

The prompt instructs ChatGPT to:
- Explain what the code does and why
- Use conversational language suitable for speech
- Be concise (2-3 sentences)
- Focus on the "why" and impact

## Example Output

**For a commit that adds authentication:**
> "This commit strengthens security by implementing JWT token validation. It adds middleware to verify tokens before allowing access to protected routes, preventing unauthorized users from accessing sensitive data."

**For a performance optimization:**
> "The code introduces database query caching which reduces API response time by 60%. Frequently accessed user data is now stored in Redis, eliminating redundant database calls."

**For a UI change:**
> "This update adds dark mode support with automatic theme detection based on system preferences. The implementation uses CSS variables for easy theming and includes smooth transition animations."

## Fallback Behavior

If OpenAI API fails or the key isn't set:
- Automatically falls back to basic narration
- Logs a warning message
- Continues without interruption

## Privacy Note

When using LLM mode:
- Commit messages are sent to OpenAI
- Code diffs are sent to OpenAI (limited to first 5 files, 50 lines each)
- Full repository content is NOT sent
- Consider this for proprietary/sensitive code

## Cost

Very affordable:
- **OpenAI**: ~$0.001 per commit (GPT-3.5-turbo)
- **ElevenLabs**: Free tier covers 50-100 commits/month
- **Total for 1000 commits**: ~$1-2

## Quick Test Without Your Repo

Test on the gitlogue repository itself:

```bash
cd gitlogue
./target/release/gitlogue --voiceover --commit HEAD~5..HEAD
```

This will review the last 5 commits and explain what changes were made to gitlogue itself.

## Turn Off LLM (If You Want)

To use basic voiceover without OpenAI:

```bash
unset OPENAI_API_KEY
gitlogue --voiceover
```

Or in config file:
```toml
[voiceover]
enabled = true
use_llm_explanations = false
```

## Full Documentation

- [Quick Start Guide](QUICK_START.md) - Step-by-step setup
- [LLM Feature Guide](llm-voiceover.md) - Detailed explanation
- [Voiceover Guide](voiceover.md) - TTS provider setup

## Summary

✅ Audio now uses ChatGPT to explain code changes  
✅ Not reading code literally, explaining what it does  
✅ Uses OpenAI API (set OPENAI_API_KEY)  
✅ One command to test after cloning  
✅ Fallback to basic narration if LLM unavailable  
✅ Low cost (~$0.001 per commit)  

The feature does exactly what you described - it "walks through the code" and explains what's happening using an LLM for context!
