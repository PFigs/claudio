use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub hotkey: HotkeyConfig,
    pub audio: AudioConfig,
    pub stt: SttConfig,
    pub tts: TtsConfig,
    pub daemon: DaemonConfig,
    pub tmux: TmuxConfig,
    pub gui: GuiConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct HotkeyConfig {
    pub ptt_key: String,
    pub device: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioConfig {
    pub sample_rate: u32,
    pub input_device: Option<String>,
    pub output_device: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SttConfig {
    pub model: String,
    pub language: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TtsConfig {
    pub model: Option<String>,
    pub voice: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DaemonConfig {
    pub socket_path: Option<String>,
    pub pid_file: Option<String>,
    pub log_file: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TmuxConfig {
    pub session_name: String,
}

impl Config {
    pub fn load() -> anyhow::Result<Self> {
        let config_path = Self::config_path();
        if config_path.exists() {
            let content = std::fs::read_to_string(&config_path)?;
            Ok(toml::from_str(&content)?)
        } else {
            Ok(Self::default())
        }
    }

    pub fn config_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("~/.config"))
            .join("ok_claude")
            .join("config.toml")
    }

    pub fn socket_path(&self) -> PathBuf {
        if let Some(ref path) = self.daemon.socket_path {
            PathBuf::from(path)
        } else {
            Self::runtime_dir().join("ok_claude.sock")
        }
    }

    pub fn pid_path(&self) -> PathBuf {
        if let Some(ref path) = self.daemon.pid_file {
            PathBuf::from(path)
        } else {
            Self::runtime_dir().join("ok_claude.pid")
        }
    }

    pub fn runtime_dir() -> PathBuf {
        std::env::var("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                let uid = unsafe { libc::getuid() };
                PathBuf::from(format!("/tmp/ok_claude-{uid}"))
            })
    }
}

/// GUI-specific configuration (user-authored).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GuiConfig {
    /// Preferred editor command. Falls back to $VISUAL, then $EDITOR, then "code".
    pub editor: Option<String>,
}

impl Default for GuiConfig {
    fn default() -> Self {
        Self { editor: None }
    }
}

impl GuiConfig {
    /// Resolve the editor command: config > $VISUAL > $EDITOR > "zed"
    pub fn resolve_editor(&self) -> String {
        if let Some(ref ed) = self.editor {
            return ed.clone();
        }
        if let Ok(v) = std::env::var("VISUAL") {
            if !v.is_empty() {
                return v;
            }
        }
        if let Ok(v) = std::env::var("EDITOR") {
            if !v.is_empty() {
                return v;
            }
        }
        "zed".to_string()
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            hotkey: HotkeyConfig::default(),
            audio: AudioConfig::default(),
            stt: SttConfig::default(),
            tts: TtsConfig::default(),
            daemon: DaemonConfig::default(),
            tmux: TmuxConfig::default(),
            gui: GuiConfig::default(),
        }
    }
}

impl Default for HotkeyConfig {
    fn default() -> Self {
        Self {
            ptt_key: "KEY_RIGHTCTRL".into(),
            device: None,
        }
    }
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            sample_rate: 16000,
            input_device: None,
            output_device: None,
        }
    }
}

impl Default for SttConfig {
    fn default() -> Self {
        Self {
            model: "base".into(),
            language: None,
        }
    }
}

impl Default for TtsConfig {
    fn default() -> Self {
        Self {
            model: None,
            voice: None,
        }
    }
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            socket_path: None,
            pid_file: None,
            log_file: None,
        }
    }
}

impl Default for TmuxConfig {
    fn default() -> Self {
        Self {
            session_name: "ok_claude".into(),
        }
    }
}

/// Persisted GUI state (not user-authored config).
/// Stored at ~/.local/state/ok_claude/state.toml
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct AppState {
    pub gui: GuiState,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct GuiState {
    pub folder_roots: Vec<PathBuf>,
}

impl AppState {
    pub fn state_path() -> PathBuf {
        dirs::state_dir()
            .unwrap_or_else(|| {
                dirs::home_dir()
                    .unwrap_or_else(|| PathBuf::from("/tmp"))
                    .join(".local/state")
            })
            .join("ok_claude")
            .join("state.toml")
    }

    pub fn load() -> Self {
        let path = Self::state_path();
        if path.exists() {
            std::fs::read_to_string(&path)
                .ok()
                .and_then(|c| toml::from_str(&c).ok())
                .unwrap_or_default()
        } else {
            Self::default()
        }
    }

    pub fn save(&self) {
        let path = Self::state_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(content) = toml::to_string_pretty(self) {
            let _ = std::fs::write(&path, content);
        }
    }
}
