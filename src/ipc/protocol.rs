use serde::{Deserialize, Serialize};

use crate::session::mode::SessionMode;

/// Request from CLI to daemon over Unix socket (JSON-line for CLI IPC).
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Request {
    Ping,
    Stop,
    New {
        name: Option<String>,
        mode: SessionMode,
    },
    List,
    Focus {
        session: String,
    },
    Mode {
        session: String,
        mode: SessionMode,
    },
    Kill {
        session: String,
    },
    Rename {
        session: String,
        new_name: String,
    },
    Shell {
        session: String,
    },
    Subscribe,
}

/// Response from daemon to CLI.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Response {
    Ok { data: ResponseData },
    Error { message: String },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResponseData {
    Empty {},
    Session(SessionInfo),
    Sessions { list: Vec<SessionInfo> },
    Pong {},
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub id: String,
    pub name: String,
    pub mode: String,
    pub focused: bool,
    pub busy: bool,
}

impl Response {
    pub fn ok(data: ResponseData) -> Self {
        Self::Ok { data }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self::Error {
            message: message.into(),
        }
    }

    pub fn ok_empty() -> Self {
        Self::ok(ResponseData::Empty {})
    }
}
