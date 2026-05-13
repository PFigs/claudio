use std::path::PathBuf;

use super::mode::SessionMode;

/// Daemon-side session metadata. PTYs are owned by the GUI process.
#[allow(dead_code)]
pub struct Session {
    pub id: String,
    pub name: String,
    pub mode: SessionMode,
    pub busy: bool,
    pub shell_mode: bool,
    pub cwd: Option<PathBuf>,
    pub command: Option<Vec<String>>,
}

impl Session {
    pub fn new(
        id: String,
        name: String,
        mode: SessionMode,
        cwd: Option<PathBuf>,
        command: Option<Vec<String>>,
    ) -> Self {
        Self {
            id,
            name,
            mode,
            busy: false,
            shell_mode: false,
            cwd,
            command,
        }
    }
}
