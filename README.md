# torvax

Git review of your diffs, like a movie.

<p align="center">
  <img src="docs/assets/demo.gif" alt="torvax demo" style="max-width: 100%; width: 800px;" />
</p>

Replays your git history as a typing animation with AI voiceover that explains what changed and why. Every commit becomes a narrated code walkthrough.

## Get API Keys

Torvax needs two API keys to run with voiceover:

**1. OpenAI** — for code explanations (GPT-5.2)
- Get yours at [platform.openai.com/api-keys](https://platform.openai.com/api-keys)

**2. Inworld** — for voice narration (TTS)
- Get yours at [inworld.ai](https://inworld.ai) → API → Basic Auth key (base64 encoded)

## Install

```bash
# Homebrew
brew tap Munasco/tap
brew install torvax
```

```bash
# macOS / Linux
curl -fsSL https://raw.githubusercontent.com/Munasco/torvax/main/install.sh | bash
```

## Setup

Run the setup command — it will ask for your keys and save them:

```bash
torvax setup
```

Or add them manually to `~/.config/torvax/config.toml`:

```toml
[voiceover]
enabled = true
provider = "inworld"
use_llm_explanations = true
openai_api_key = "sk-..."
api_key = "your-inworld-base64-key"
voice_id = "Ashley"
model_id = "inworld-tts-1.5-max"
```

Or as environment variables:

```bash
export OPENAI_API_KEY="sk-..."
export INWORLD_API_KEY="your-inworld-base64-key"
```

## Run

```bash
# First time — save your API keys
torvax setup

# Narrated replay of recent commits
torvax --voiceover --commit HEAD~3..HEAD

# HEAD@N is shorthand for HEAD~N — these are identical:
torvax --voiceover --commit HEAD@3..HEAD
torvax --voiceover --commit HEAD~3..HEAD

# Screensaver mode (no voiceover needed)
torvax

# Specific commit
torvax --voiceover --commit abc123

# Loop through a range
torvax --voiceover --commit HEAD~10..HEAD --loop
```

## How it works

1. Reads your repo and generates a project description with GPT-5.2
2. Splits each file's diff into semantic chunks by grouping hunks
3. Orders files by logical development flow (not alphabetically)
4. Calculates exact animation duration per chunk from character counts and typing speed
5. Reverse-engineers the word count so narration always covers the animation
6. Generates speech with Inworld TTS, measures real audio duration
7. Plays narration in sync — audio starts when a chunk begins animating, pauses at chunk boundaries

## Features

- **AI Voiceover** — GPT-5.2 explains your diffs while code types itself on screen
- **Semantic Chunking** — Changes grouped by meaning, not by line number
- **Smart File Ordering** — AI orders files by how they were logically developed
- **Audio-Animation Sync** — Narration duration matched to typing animation speed
- **29 Languages** — Tree-sitter syntax highlighting
- **9 Themes** — Tokyo Night, Dracula, Nord, and more
- **Screensaver Mode** — Endless random commit playback

## Key Bindings

| Key | Action |
|-----|--------|
| `Space` | Play / pause |
| `h` / `l` | Step backward / forward one line |
| `H` / `L` | Step backward / forward one change |
| `p` / `n` | Previous / next commit |
| `Esc` | Menu |
| `q` | Quit |

## Configuration

```bash
# Set default theme
torvax theme set dracula

# Adjust typing speed (ms per character)
torvax --speed 20

# Different speeds per file type
torvax --speed-rule "*.java:50" --speed-rule "*.xml:5"

# Ignore files
torvax --ignore "*.ipynb" --ignore "poetry.lock"

# Filter by author or date
torvax --author "john" --after "2024-01-01"
```

Full config at `~/.config/torvax/config.toml`.

## Credits

Torvax grew out of [gitlogue](https://github.com/Munasco/gitlogue) — the original git history screensaver that laid the foundation for the terminal animation engine, syntax highlighting, and commit replay system this project is built on.

## License

ISC
