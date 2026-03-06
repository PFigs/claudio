use super::mode::SessionMode;

/// Daemon-side session metadata. PTYs are owned by the GUI process.
pub struct Session {
    pub id: String,
    pub name: String,
    pub mode: SessionMode,
    pub busy: bool,
    pub shell_mode: bool,
}

impl Session {
    pub fn new(id: String, name: String, mode: SessionMode) -> Self {
        Self {
            id,
            name,
            mode,
            busy: false,
            shell_mode: false,
        }
    }
}
