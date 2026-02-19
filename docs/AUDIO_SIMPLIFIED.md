# Audio Feature Simplified

## What Changed

Audio support is now **always enabled** by default. You no longer need to specify `--features audio` when building.

## Before (Complex)

```bash
# Users had to remember to add --features audio
cargo build --release --features audio
cargo install --path . --features audio
```

## After (Simple)

```bash
# Just build normally
cargo build --release
cargo install --path .
```

## Why This Change?

1. **Simpler for users** - No need to remember feature flags
2. **Fewer support questions** - "Why isn't audio working?" → Because they forgot `--features audio`
3. **Better experience** - The voiceover feature is a core part of gitlogue now
4. **Easier documentation** - One command instead of multiple variants

## Technical Changes

### Cargo.toml
- Removed `optional = true` from `rodio` dependency
- Removed the entire `[features]` section
- Audio dependencies are now standard

### src/audio.rs
- Removed all `#[cfg(feature = "audio")]` conditional compilation
- Removed `#[cfg(not(feature = "audio"))]` fallback code
- Simplified struct definitions
- Cleaner code without feature gates

### Documentation
- Updated all 5 documentation files
- Removed mentions of `--features audio`
- Simplified build instructions

## System Requirements

The only new requirement is ALSA on Linux systems:

```bash
# On Ubuntu/Debian
sudo apt-get install libasound2-dev

# On Fedora/RHEL
sudo dnf install alsa-lib-devel

# On Arch
sudo pacman -S alsa-lib
```

This is a common library and most systems already have it.

## Binary Size Impact

The binary size increases slightly due to rodio being always included:
- **Before:** ~15-20 MB (without audio)
- **After:** ~20-25 MB (with audio)

This is acceptable for the improved user experience.

## Building

Now it's just:

```bash
git clone https://github.com/Munasco/gitlogue.git
cd gitlogue
cargo build --release
```

That's it! No feature flags needed. 🎉
