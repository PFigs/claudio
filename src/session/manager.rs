use std::collections::HashMap;

use anyhow::{bail, Result};

use super::mode::SessionMode;
use super::session::Session;

pub struct SessionManager {
    sessions: HashMap<String, Session>,
    focused_id: Option<String>,
}

impl SessionManager {
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
            focused_id: None,
        }
    }

    pub fn create_session(&mut self, name: Option<String>, mode: SessionMode) -> &Session {
        let id = uuid::Uuid::new_v4().to_string()[..8].to_string();
        let name = name.unwrap_or_else(|| {
            let mut idx = 0;
            loop {
                let candidate = format!("session-{idx}");
                if !self.sessions.values().any(|s| s.name == candidate) {
                    break candidate;
                }
                idx += 1;
            }
        });

        let session = Session::new(id.clone(), name, mode);
        self.sessions.insert(id.clone(), session);

        // Auto-focus if this is the first session
        if self.focused_id.is_none() {
            self.focused_id = Some(id.clone());
        }

        self.sessions.get(&id).unwrap()
    }

    pub fn destroy_session(&mut self, name_or_id: &str) -> Result<()> {
        let id = self.resolve_id(name_or_id)?;
        self.sessions.remove(&id);
        if self.focused_id.as_deref() == Some(&id) {
            self.focused_id = self.sessions.keys().next().cloned();
        }
        Ok(())
    }

    pub fn set_focus(&mut self, name_or_id: &str) -> Result<()> {
        let id = self.resolve_id(name_or_id)?;
        self.focused_id = Some(id);
        Ok(())
    }

    pub fn set_mode(&mut self, name_or_id: &str, mode: SessionMode) -> Result<()> {
        let id = self.resolve_id(name_or_id)?;
        self.sessions.get_mut(&id).unwrap().mode = mode;
        Ok(())
    }

    pub fn rename_session(&mut self, name_or_id: &str, new_name: String) -> Result<()> {
        let id = self.resolve_id(name_or_id)?;
        self.sessions.get_mut(&id).unwrap().name = new_name;
        Ok(())
    }

    pub fn set_shell_mode(&mut self, name_or_id: &str) -> Result<()> {
        let id = self.resolve_id(name_or_id)?;
        self.sessions.get_mut(&id).unwrap().shell_mode = true;
        Ok(())
    }

    pub fn is_focused(&self, id: &str) -> bool {
        self.focused_id.as_deref() == Some(id)
    }

    pub fn focused_session(&self) -> Option<&Session> {
        self.focused_id
            .as_ref()
            .and_then(|id| self.sessions.get(id))
    }

    #[allow(dead_code)]
    pub fn get_session_mut(&mut self, id: &str) -> Option<&mut Session> {
        self.sessions.get_mut(id)
    }

    pub fn list_sessions(&self) -> Vec<&Session> {
        let mut sessions: Vec<&Session> = self.sessions.values().collect();
        sessions.sort_by_key(|s| &s.name);
        sessions
    }

    fn resolve_id(&self, name_or_id: &str) -> Result<String> {
        // Try name match first
        for session in self.sessions.values() {
            if session.name == name_or_id {
                return Ok(session.id.clone());
            }
        }
        // Try ID prefix match
        let matches: Vec<&str> = self
            .sessions
            .keys()
            .filter(|id| id.starts_with(name_or_id))
            .map(|id| id.as_str())
            .collect();

        match matches.len() {
            0 => bail!("session not found: {name_or_id}"),
            1 => Ok(matches[0].to_string()),
            _ => bail!("ambiguous session identifier: {name_or_id}"),
        }
    }
}
