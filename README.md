# claudio

Voice-activated terminal session manager for [Claude Code](https://docs.anthropic.com/en/docs/claude-code). Speak to your focused session, hear responses read aloud, manage multiple sessions with independent audio modes.

**Linux only** | MIT License

## Features

- **Voice input** -- hold a push-to-talk key (default: Right Ctrl) and speak. Voice is transcribed via Whisper and injected into the focused terminal session for review before submission.
- **Screen reader** -- press Ctrl+R to read the focused terminal aloud. Uses `claude -p --model haiku` to summarize the screen content into natural speech (plans, explanations, status updates), then speaks it via Piper TTS. Falls back to a heuristic text filter if the summarizer is unavailable.
- **Pipe mode** -- when stdout is piped, claudio acts as a voice-to-text passthrough. Keyboard input from `/dev/tty` and voice transcriptions are merged to stdout, making it composable with other CLI tools (e.g., `claudio | claude -p "review this"`).
- **Multiple sessions** -- create, focus, rename, minimize, and kill terminal sessions. Each session gets its own PTY. The GUI arranges them in an auto-sized grid (1x1 up to 2x3+).
- **Session modes** -- each session can be in speaking (voice in + TTS out), listening (TTS out only), or muted mode.
- **File browser** -- resizable sidebar with multi-root folder navigation. Click files to open in your editor (`$VISUAL` / `$EDITOR` / `zed`).
- **Desktop notifications** -- terminal output is scanned for approval prompts ("do you want to proceed", "allow once", etc.) and triggers a `notify-send` notification so you don't miss them.
- **Git worktrees** -- right-click a repo folder to create a named worktree with its own session and branch.
- **Resizable grid** -- drag borders between terminal panes to resize them.

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

### Pipe mode

When stdout is not a terminal, claudio runs as a voice-to-text passthrough. Keyboard input from `/dev/tty` and voice transcriptions are merged to stdout:

```bash
# Dictate text and pipe it to another program
claudio | wc -w

# Voice-driven prompt for Claude
claudio | claude -p "answer this question"

# Record voice notes to a file
claudio > notes.txt
```

Status messages (recording, transcribing) go to stderr so they don't pollute the pipe.

## Keybindings

| Key | Action |
|-----|--------|
| Ctrl+N | New session |
| Ctrl+W | Kill focused session |
| Ctrl+Tab | Cycle focus to next session |
| Ctrl+M | Toggle session mode (speaking / listening / muted) |
| Ctrl+Shift+M | Minimize focused session (hide from grid) |
| Ctrl+B | Toggle file tree sidebar |
| Ctrl+R | Read screen aloud (summarize + TTS) |
| Ctrl+Shift+R | Stop ongoing speech |
| Ctrl+Q | Quit GUI (daemon keeps running) |
| Ctrl+Shift+Q | Stop daemon and quit GUI |

## Session modes

| Mode | Voice in | TTS out |
|------|----------|---------|
| speaking | yes (when focused + PTT held) | yes |
| listening | no | yes |
| muted | no | no |

## Configuration

Stored at `~/.config/ok_claude/config.toml`. Created automatically on first run.

```toml
[hotkey]
ptt_key = "KEY_RIGHTCTRL"     # PTT key (see below for options)
# device = "/dev/input/event5" # specific evdev device (auto-detected if omitted)

[audio]
sample_rate = 16000
# input_device = "..."        # CPAL input device name
# output_device = "..."       # CPAL output device name

[stt]
model = "base"                # whisper model: tiny, base, small, medium, large
# language = "en"             # ISO 639-1 code (auto-detect if omitted)

[tts]
# model = "/path/to/voice.onnx"  # Piper TTS model (auto-detected if omitted)

[daemon]
# socket_path = "..."
# pid_file = "..."
# log_file = "..."

[tmux]
session_name = "ok_claude"
```

Supported PTT keys: `KEY_RIGHTCTRL`, `KEY_LEFTCTRL`, `KEY_RIGHTALT`, `KEY_LEFTALT`, `KEY_RIGHTSHIFT`, `KEY_LEFTSHIFT`, `KEY_CAPSLOCK`, `KEY_SCROLLLOCK`, `KEY_PAUSE`, `KEY_F13`-`KEY_F15`.

## Architecture

```
claudio (no args)
  |
  +-- setup          check deps, download models
  +-- daemon         session metadata + voice pipeline (PTT -> VAD -> STT)
  |     |
  |     +-- ML service (Python)    Silero VAD + faster-Whisper + Piper TTS
  |     +-- IPC server             Unix socket, JSON-line protocol
  |
  +-- GUI (GPUI)     owns PTYs, terminal rendering, file browser
        |
        +-- IPC client             subscribes to daemon events
        +-- terminal grid          alacritty_terminal via gpui-terminal
        +-- screen reader          claude summarizer + piper TTS
```

The daemon holds no PTYs -- the GUI owns them via `portable-pty`. This means the GUI can detach and reattach without losing session metadata. Voice transcriptions flow from the daemon to the GUI over IPC, where they are injected into the focused session's PTY for review.

## Uninstall

```bash
curl -sSL https://raw.githubusercontent.com/PFigs/claudio/main/scripts/uninstall.sh | bash
```

Or if installed via .deb: `sudo apt remove claudio`

## License

MIT
