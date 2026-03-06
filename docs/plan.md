# ok_claude -- Voice-Activated Terminal Session Manager

## Context

Current IDE/terminal workflows for Claude Code are clunky: multiple terminal splits, constant window juggling, everything text-based. ok_claude solves this by adding a voice layer on top of Claude Code sessions. You speak to the focused session, hear responses via TTS, and manage multiple sessions with independent audio modes (speaking, listening, muted). It's a personal productivity tool built for fast iteration.

## Architecture Overview

Polyglot design using the best language per component:

```
                         ┌──────────────────────────────┐
                         │       Rust Daemon             │
                         │                               │
  Microphone ──► cpal ──►│  Audio Capture                │
                         │       │                       │
  Keyboard ──► evdev ──► │  PTT Hotkey ──► gate ──┐     │
                         │                         │     │
                         │       ▼                 │     │
                         │  ┌─────────────────┐    │     │
                         │  │ Python ML Svc   │◄───┘     │
                         │  │ (child process) │          │
                         │  │                 │          │
                         │  │ VAD ► STT ► txt │──────┐   │
                         │  │ txt ► TTS ► pcm │◄──┐  │   │
                         │  └─────────────────┘   │  │   │
                         │                        │  │   │
                         │  Session Manager       │  │   │
                         │    ├─ session 1 ──► claude -p  │
                         │    ├─ session 2 ──► claude -p  │
                         │    └─ session 3 ──► claude -p  │
                         │         │              │  │   │
                         │         ▼              │  │   │
                         │  tmux display ◄────────┘  │   │
                         │  (presentation layer)     │   │
                         │                           │   │
  Speaker ◄── cpal ◄────│  Audio Playback ◄──────────┘   │
                         │                               │
  CLI ◄──► Unix Socket ◄─│  IPC Server                   │
                         └──────────────────────────────┘
```

## Language Split

### Rust (daemon + CLI binary)
- **Why**: Real-time audio capture (cpal), async event loop (tokio), global hotkey (evdev), subprocess management, IPC server. Ships as a single binary.
- **Crates**: `tokio`, `cpal`, `evdev`, `clap`, `serde_json`, `libtmux` (or shell out to tmux CLI)

### Python (ML inference service)
- **Why**: faster-whisper, silero-vad, and piper-tts all have first-class Python APIs. No need to fight ONNX bindings in Rust when Python wraps it cleanly.
- **Packages**: `faster-whisper`, `piper-tts`, `silero-vad` (or `torch` + `torch.hub`), `soundfile`, `numpy`
- **Managed by**: uv (per your preference)

### Communication: Unix socket with binary framed protocol
- Rust daemon starts the Python service as a child process
- Length-prefixed binary frames over Unix socket (no JSON overhead for audio)
- Audio data sent inline as raw bytes -- no temp files, no serialization

## Session Modes

Each session operates in one of three modes:

| Mode | Voice In | TTS Out | Text Display |
|------|----------|---------|-------------|
| **Speaking** | Yes (when focused + PTT) | Yes | Yes |
| **Listening** | No | Yes | Yes |
| **Muted** | No | No | Yes |

Only one session can be "focused" for voice input at a time. Multiple sessions can output TTS simultaneously.

## CLI Commands

```
ok_claude start              # Start the daemon (foreground or -d for detach)
ok_claude stop               # Stop the daemon
ok_claude new [--name NAME] [--mode speaking|listening|muted]
ok_claude list               # Show sessions with mode, focus, busy state
ok_claude focus <session>    # Set voice input target
ok_claude mode <session> <speaking|listening|muted>
ok_claude kill <session>     # Destroy session
ok_claude shell <session>    # Next voice input runs as shell command
ok_claude config             # Show/edit configuration
```

## Push-to-Talk

- Default key: Right Ctrl (`KEY_RIGHTCTRL`)
- Captured via Linux evdev (`/dev/input/`) -- works on X11 and Wayland
- User must be in `input` group (daemon checks at startup)
- Key press starts audio capture into STT pipeline
- Key release force-flushes any buffered audio to STT
- Configurable via `~/.config/ok_claude/config.toml`

## Claude Code Integration

Sessions use `claude -p` (pipe mode) with `--output-format stream-json --verbose`:
- Daemon owns Claude as subprocesses, not tmux
- Session continuity via `--resume <session_id>`
- NDJSON streaming output parsed for sentence-by-sentence TTS
- tmux is the **presentation layer** only -- daemon writes session activity to tmux panes for visual monitoring

## Audio Pipeline Detail

```
PTT pressed
  -> cpal InputStream captures 16kHz mono i16 PCM
  -> raw audio chunks sent inline via binary frame (VAD_FEED, 0x01)
  -> Python service runs Silero-VAD, returns speech boundary events
  -> Rust daemon accumulates audio chunks in memory while speech active
  -> On speech_end (or PTT release): full utterance sent as TRANSCRIBE (0x02)
  -> faster-whisper transcribes, returns UTF-8 text
  -> Routed to focused session's Claude subprocess

Claude responds (stream-json)
  -> Rust daemon accumulates text, splits on sentence boundaries
  -> Each sentence sent as SYNTHESIZE (0x03) to Python ML service
  -> Piper returns raw PCM audio inline in response frame
  -> Played via cpal OutputStream
  -> Text also written to tmux pane
```

## Python ML Service Protocol (Binary Framed)

Length-prefixed binary frames over Unix socket. No JSON, no temp files. Audio bytes travel inline.

### Frame format

```
+----------+----------+------------------+
| msg_type | body_len |      body        |
|  1 byte  | 4 bytes  |  body_len bytes  |
|          | (u32 LE) |                  |
+----------+----------+------------------+
```

### Message types (request -> response)

```
0x01  VAD_FEED     -> VAD_RESULT
      Request body:  raw i16 LE PCM audio chunk (512 samples = 1024 bytes)
      Response body: 1 byte status:
                     0x00 = no event
                     0x01 = speech_start
                     0x02 = speech_end

0x02  TRANSCRIBE   -> TRANSCRIPTION
      Request body:  raw i16 LE PCM audio (full utterance, variable length)
      Response body: UTF-8 text (no length prefix needed, body_len covers it)

0x03  SYNTHESIZE   -> AUDIO_OUT
      Request body:  UTF-8 text to speak
      Response body: 4 bytes sample_rate (u32 LE) + raw i16 LE PCM audio

0x04  PING         -> PONG
      Request body:  empty
      Response body: empty

0xFF  ERROR (response only)
      Response body: UTF-8 error message
```

### Why binary, not JSON
- Audio chunks are 1KB+ of raw PCM. JSON would base64-encode them (33% bloat) or require temp files (syscall overhead + disk I/O).
- VAD runs on every 32ms audio chunk. At ~30 calls/sec during speech, even small per-message overhead adds up.
- The protocol has exactly 4 message types. A schema language is overkill.
- Both Rust (`tokio::io`) and Python (`asyncio.StreamReader`) handle length-prefixed binary trivially.

## Project Structure

```
ok_claude/
├── Cargo.toml                    # Rust workspace
├── src/
│   ├── main.rs                   # Entry point: CLI dispatch or daemon
│   ├── cli.rs                    # clap command definitions
│   ├── daemon.rs                 # tokio main loop, component orchestration
│   ├── config.rs                 # Config loading (~/.config/ok_claude/config.toml)
│   ├── ipc/
│   │   ├── mod.rs
│   │   ├── protocol.rs           # Request/Response types (serde)
│   │   ├── server.rs             # Unix socket server (daemon side)
│   │   └── client.rs             # Unix socket client (CLI side)
│   ├── audio/
│   │   ├── mod.rs
│   │   ├── capture.rs            # cpal InputStream, ring buffer
│   │   └── playback.rs           # cpal OutputStream for TTS audio
│   ├── session/
│   │   ├── mod.rs
│   │   ├── manager.rs            # Session registry, focus, routing
│   │   ├── session.rs            # Single session: Claude subprocess + state
│   │   ├── mode.rs               # SessionMode enum
│   │   └── tmux.rs               # tmux pane management (shells out to tmux CLI)
│   ├── hotkey/
│   │   ├── mod.rs
│   │   └── evdev.rs              # PTT via evdev async_read_loop
│   └── ml_bridge/
│       ├── mod.rs
│       └── client.rs             # Talks to Python ML service over Unix socket
├── ml_service/                   # Python ML service
│   ├── pyproject.toml
│   └── src/
│       └── ok_claude_ml/
│           ├── __init__.py
│           ├── server.py         # Unix socket server, dispatches to STT/TTS/VAD
│           ├── stt.py            # faster-whisper wrapper
│           ├── tts.py            # Piper TTS wrapper
│           └── vad.py            # Silero-VAD wrapper
└── tests/
    ├── rust/                     # Rust integration tests
    └── python/                   # Python ML service tests
        └── ok_claude_ml/
            ├── test_stt.py
            ├── test_tts.py
            └── test_vad.py
```

## Implementation Phases

### Phase 0: Scaffolding
- `cargo init ok_claude`, add workspace deps
- `uv init ml_service` inside the project
- Directory structure, CI basics

### Phase 1: IPC + Daemon Shell
- Unix socket server/client in Rust (tokio)
- CLI with clap: `start`, `stop` commands
- Daemon runs, accepts connections, responds to ping
- PID file management, signal handling

### Phase 2: Python ML Service
- Python Unix socket server with asyncio
- STT endpoint: load faster-whisper base model, transcribe WAV files
- TTS endpoint: load Piper model, synthesize text to raw PCM
- VAD endpoint: load Silero-VAD, process audio chunks
- Rust `ml_bridge::client` talks to it
- Daemon starts/stops Python service as child process

### Phase 3: Audio Capture + PTT
- cpal InputStream at 16kHz mono
- evdev listener for Right Ctrl
- When PTT held: capture audio, write chunks to temp files, feed to ML service VAD
- On speech end or PTT release: send accumulated audio to ML service STT
- Return transcribed text to daemon

### Phase 4: Session Management
- Session struct: id, name, mode, claude_session_id, busy flag
- SessionManager: create, destroy, focus, mode change, list
- Claude subprocess: `claude -p <text> --output-format stream-json --resume <id>`
- Parse NDJSON stream, extract text, detect sentence boundaries
- Wire up: transcription -> focused session -> Claude -> response text

### Phase 5: TTS Output
- Send response sentences to ML service TTS endpoint
- Receive PCM audio, play via cpal OutputStream
- Respect session mode (only play for SPEAKING/LISTENING sessions)
- Sentence-by-sentence streaming: start playing first sentence while Claude generates the rest

### Phase 6: tmux Display
- Create tmux session `ok_claude` with a window per session
- Write user input and Claude responses to panes
- Show session status (mode, focus, busy) in pane title
- Clean up panes on session destroy

### Phase 7: Shell Escape Hatch
- `ok_claude shell <session>` sets one-shot flag
- Next voice transcription executes as shell command instead of Claude prompt
- Output displayed in tmux pane and optionally spoken via TTS

## Configuration

`~/.config/ok_claude/config.toml`:
```toml
[hotkey]
ptt_key = "KEY_RIGHTCTRL"
# device = "/dev/input/event3"  # auto-detect if omitted

[audio]
sample_rate = 16000
# input_device = "default"
# output_device = "default"

[stt]
model = "base"  # whisper model size: tiny, base, small, medium, large
# language = "en"

[tts]
# model = "/path/to/piper/model.onnx"
# voice = "en_US-lessac-medium"

[daemon]
# socket_path auto-derived from XDG_RUNTIME_DIR
# log_file = "~/.local/state/ok_claude/daemon.log"

[tmux]
session_name = "ok_claude"
```

## Error Handling

| Error | Response |
|-------|----------|
| STT returns empty/garbage | Log warning, show "could not understand" in tmux pane, skip |
| Claude subprocess fails | Parse stderr, display error in tmux, mark session not busy |
| Claude timeout (120s) | Kill process, notify via TTS "timed out", mark not busy |
| evdev device disappears | Retry detection every 5s, log warning |
| Audio device not found | Clear error at startup with device list |
| User not in input group | Startup check with actionable error message |
| Python ML service crashes | Daemon restarts it automatically, log the crash |
| Stale socket file | Check PID liveness, clean up if dead |

## Post-MVP Enhancements (not in scope now)

- PipeWire per-session audio routing (virtual sinks per session)
- Multiple PTT keys bound to different sessions directly
- Different Piper voices per session (distinguish by voice)
- GPU acceleration for STT/TTS
- Audio feedback sounds (beep on PTT press/release)
- Session persistence across daemon restarts
- Systemd user service for auto-start
- `ok_claude attach <session>` for interactive terminal access

## Verification

1. **Phase 1**: `ok_claude start` launches daemon, `ok_claude stop` kills it, `ok_claude list` returns empty list
2. **Phase 2**: `echo "hello" | python -m ok_claude_ml.test_stt` transcribes correctly; TTS produces audible speech
3. **Phase 3**: Hold Right Ctrl, speak, release -- daemon logs transcribed text
4. **Phase 4**: `ok_claude new --name test`, speak a question, see Claude's response in tmux pane
5. **Phase 5**: Hear Claude's response spoken back
6. **Phase 6**: `ok_claude list` shows sessions, tmux windows reflect state
7. **End-to-end**: Start daemon, create 3 sessions (speaking, listening, muted), focus session 1, speak a question, hear response from session 1, switch focus to session 2, speak again, verify session 2 responds and session 1 stays quiet for input but still shows TTS if in listening mode
