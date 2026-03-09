use serde::{Deserialize, Serialize};

use super::protocol::SessionInfo;

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum DaemonEvent {
    Snapshot {
        sessions: Vec<SessionInfo>,
        focused_id: Option<String>,
        ptt_active: bool,
    },
    PttState {
        active: bool,
    },
    VadState {
        speaking: bool,
    },
    Transcription {
        session_id: String,
        text: String,
        /// When true, append carriage return to submit the text.
        /// When false, just insert the text without submitting.
        #[serde(default = "default_true")]
        enter: bool,
    },
    ResponseChunk {
        session_id: String,
        text: String,
    },
    ResponseDone {
        session_id: String,
    },
    SessionCreated {
        session: SessionInfo,
    },
    SessionDestroyed {
        session_id: String,
    },
    SessionFocused {
        session_id: String,
    },
    SessionRenamed {
        session_id: String,
        new_name: String,
    },
    SessionModeChanged {
        session_id: String,
        mode: String,
    },
    SessionBusyChanged {
        session_id: String,
        busy: bool,
    },
    ShellOutput {
        session_id: String,
        command: String,
        output: String,
    },
    TtsSpeaking {
        session_id: String,
        text: String,
    },
    PaneContent {
        session_id: String,
        lines: Vec<String>,
    },
}
