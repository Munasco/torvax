# Installation Guide

This guide provides detailed instructions for installing gitlogue on different platforms.

## Prerequisites

- Git must be installed on your system
- For building from source: Rust 1.70 or later

## Installation Methods

### Method 1: Using Install Script (Recommended)

The quickest way to install gitlogue is using the installation script:

```bash
curl -fsSL https://raw.githubusercontent.com/unhappychoice/gitlogue/main/install.sh | bash
```

This script will:
- Automatically detect your platform (macOS Intel/ARM, Linux x64/ARM64)
- Download the latest release binary
- Install to `~/.local/bin` (customizable with `INSTALL_DIR` environment variable)

**Custom installation directory:**
```bash
INSTALL_DIR=/usr/local/bin curl -fsSL https://raw.githubusercontent.com/unhappychoice/gitlogue/main/install.sh | bash
```

### Method 2: Using Homebrew

For macOS and Linux users with Homebrew:

```bash
brew install unhappychoice/tap/gitlogue
```

**Updating via Homebrew:**
```bash
brew upgrade gitlogue
```

### Method 3: Using pacman (Arch Linux)

gitlogue is available in the [official Arch Linux repositories](https://archlinux.org/packages/extra/x86_64/gitlogue/):

```bash
sudo pacman -Syu gitlogue
```

This keeps you on the latest packaged release, integrates with system upgrades, and installs the binary into `/usr/bin`.

### Method 4: Using Cargo

Install from [crates.io](https://crates.io) using Cargo:

```bash
cargo install gitlogue
```

This will download, compile, and install the latest version.

### Method 5: Download Pre-built Binaries

Pre-built binaries for multiple platforms are available in the [Releases](https://github.com/unhappychoice/gitlogue/releases) section.

**Supported platforms:**
- macOS (Intel: x86_64, Apple Silicon: aarch64)
- Linux (x64: x86_64, ARM64: aarch64)
- Windows (x64: x86_64)

**Manual installation:**
1. Download the appropriate archive for your platform
2. Extract the binary:
   ```bash
   # For Linux/macOS
   tar -xzf gitlogue-v*.tar.gz

   # For Windows
   # Use your preferred extraction tool for .zip files
   ```
3. Move to a directory in your PATH:
   ```bash
   sudo mv gitlogue /usr/local/bin/
   ```

### Method 6: From Source

If you want to build the latest development version or contribute to the project:

1. Clone the repository:
   ```bash
   git clone https://github.com/unhappychoice/gitlogue.git
   cd gitlogue
   ```

2. Build and install:
   ```bash
   cargo install --path .
   ```

   Or just build for development:
   ```bash
   cargo build --release
   ```

   The binary will be located at `target/release/gitlogue`.

## Platform-Specific Notes

### macOS

On macOS, you may need to install Xcode Command Line Tools if you haven't already:

```bash
xcode-select --install
```

### Linux

Make sure you have the build essentials installed:

**Ubuntu/Debian:**
```bash
sudo apt update
sudo apt install build-essential git
```

**Fedora/RHEL:**
```bash
sudo dnf install gcc git
```

**Arch Linux:**
```bash
sudo pacman -S base-devel git
```

### Windows

1. Install [Rust](https://www.rust-lang.org/tools/install) using rustup
2. Install [Git for Windows](https://git-scm.com/download/win)
3. Follow the Cargo installation method above

## Verifying Installation

After installation, verify that gitlogue is correctly installed:

```bash
gitlogue --help
```

You should see the help message with available commands and options.

## Updating

### Install Script

Re-run the installation script to update to the latest version:

```bash
curl -fsSL https://raw.githubusercontent.com/unhappychoice/gitlogue/main/install.sh | bash
```

### Homebrew

```bash
brew upgrade gitlogue
```

### Cargo Installation

To update to the latest version:

```bash
cargo install gitlogue
```

Cargo will automatically update to the newest version.

### Source Installation

```bash
cd gitlogue
git pull origin main
cargo install --path .
```

## Uninstalling

### Install Script

If installed via the install script, manually remove the binary:

```bash
rm ~/.local/bin/gitlogue
# Or if you used a custom INSTALL_DIR
rm /path/to/your/install/dir/gitlogue
```

### Homebrew

```bash
brew uninstall gitlogue
```

### Cargo

```bash
cargo uninstall gitlogue
```

## Configuration

gitlogue supports configuration via `~/.config/gitlogue/config.toml`. See the [Configuration Guide](configuration.md) for detailed options and the [Usage Guide](usage.md) for command-line options.

## Troubleshooting

### Command Not Found

If you get a "command not found" error after installation, make sure the installation directory is in your PATH:

**For Install Script (default: `~/.local/bin`):**
```bash
export PATH="$HOME/.local/bin:$PATH"
```

**For Cargo:**
```bash
export PATH="$HOME/.cargo/bin:$PATH"
```

Add the appropriate line to your shell configuration file (`~/.bashrc`, `~/.zshrc`, etc.) to make it permanent.

### Build Errors

If you encounter build errors:

1. Ensure you have the latest version of Rust:
   ```bash
   rustup update
   ```

2. Clean the build directory and try again:
   ```bash
   cargo clean
   cargo build --release
   ```

### Permission Errors

On Linux/macOS, if you encounter permission errors during installation, you may need to use `sudo`:

```bash
sudo cargo install gitlogue
```

However, it's generally recommended to use cargo without sudo and ensure your user has proper permissions.

### GLIBC Version Errors (Linux)

If you see an error like:

```
gitlogue: /lib/x86_64-linux-gnu/libc.so.6: version `GLIBC_2.38' not found (required by gitlogue)
```

This means your system's glibc version is older than what the pre-built binary requires.

**Solutions:**

1. **Upgrade your OS** (Recommended)

   The safest way is to upgrade to a newer version of your Linux distribution:
   - Ubuntu 24.04+ includes glibc 2.39
   - Debian 13 (Trixie)+ includes glibc 2.38+
   - Fedora 39+ includes glibc 2.38+

2. **Build from source**

   Install via Cargo to compile for your system:
   ```bash
   cargo install gitlogue
   ```

3. **Use Nix**

   If you have Nix installed:
   ```bash
   nix run github:unhappychoice/gitlogue
   ```

## Next Steps

- Read the [Usage Guide](usage.md) to learn how to use gitlogue
- Explore [Theme Customization](themes.md) to personalize your experience
- Check out the [Contributing Guidelines](CONTRIBUTING.md) if you want to contribute

## Getting Help

If you encounter issues during installation:

- Check the [GitHub Issues](https://github.com/unhappychoice/gitlogue/issues) for known problems
- Open a new issue if your problem isn't already reported
- Include your OS, Rust version (`rustc --version`), and error messages
