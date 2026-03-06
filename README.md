# claudio

Voice-activated terminal session manager for [Claude Code](https://docs.anthropic.com/en/docs/claude-code). Speak to your focused session, hear responses via TTS, manage multiple sessions with independent audio modes.

**Linux only** | MIT License

## Install

### One-liner (recommended)

```bash
curl -sSL https://raw.githubusercontent.com/PFigs/claudio/main/scripts/install.sh | bash
```

This will:
- Install [uv](https://docs.astral.sh/uv/) if not present
- Download the latest release binary (or build from source as fallback)
- Set up the Python ML service for voice features
- Print any remaining manual steps

### From .deb (Debian/Ubuntu)

```bash
# Download the latest .deb from GitHub Releases
sudo dpkg -i claudio_*.deb
claudio setup
```

### Docker

```bash
docker pull ghcr.io/PFigs/claudio:latest
docker run --privileged --network host \
  --device /dev/snd \
  -v /dev/input:/dev/input:ro \
  -v ~/.config/ok_claude:/root/.config/ok_claude \
  ghcr.io/PFigs/claudio
```

Or with docker compose:

```bash
curl -O https://raw.githubusercontent.com/PFigs/claudio/main/docker-compose.yml
docker compose up -d
```

### From source

```bash
# Prerequisites
sudo apt install pkg-config libasound2-dev libxkbcommon-dev libxkbcommon-x11-dev libvulkan-dev mesa-vulkan-drivers

git clone https://github.com/PFigs/claudio.git
cd claudio

# If using rust-lld on x86_64, you may need:
mkdir -p .cargo && echo -e '[target.x86_64-unknown-linux-gnu]\nrustflags = ["-L", "/usr/lib/x86_64-linux-gnu"]' > .cargo/config.toml

cargo build --release

# Set up ML service
cd ml_service && uv sync && cd ..

# Run
./target/release/claudio
```

## Prerequisites

- Linux (uses evdev for hotkeys, Vulkan for GUI)
- [claude](https://docs.anthropic.com/en/docs/claude-code) CLI in PATH
- User must be in the `input` group for push-to-talk:
  ```bash
  sudo usermod -aG input $USER
  # then log out and back in
  ```

## Usage

### Quick start

```bash
claudio
```

Runs preflight checks, downloads ML models if missing (Whisper, Silero VAD, Piper TTS), starts the daemon, and opens the GUI. Hold Right Ctrl to talk.

### Setup only (no daemon)

```bash
claudio setup
```

Checks system requirements and downloads models without starting anything.

### Session management

```bash
claudio new --name work --mode speaking   # create a session
claudio list                              # show all sessions
claudio focus work                        # set voice input target
claudio mode work listening               # change mode
claudio kill work                         # destroy session
claudio shell work                        # next voice input runs as shell command
```

### Daemon control

```bash
claudio start -f    # start daemon in foreground (without setup)
claudio stop        # stop running daemon
```

### GUI

```bash
claudio gui         # open the GUI (daemon must be running)
```

## Session modes

| Mode | Voice in | TTS out |
|------|----------|---------|
| speaking | yes (when focused + PTT held) | yes |
| listening | no | yes |
| muted | no | no |

## Configuration

Stored at `~/.config/ok_claude/config.toml`. Created automatically on first run. Covers hotkey binding, audio devices, STT/TTS model paths, and tmux session name.

## Uninstall

```bash
curl -sSL https://raw.githubusercontent.com/PFigs/claudio/main/scripts/uninstall.sh | bash
```

Or if installed via .deb: `sudo apt remove claudio`

## License

MIT
