use serde::{Deserialize, Serialize};

use super::protocol::SessionInfo;

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
