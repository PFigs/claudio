use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::Duration;

use gpui::*;

use super::actions::*;
use super::file_tree::FileTree;
use super::ipc_bridge;
use super::session_state::SessionState;
use super::terminal_grid::GridResize;
use crate::config::AppState;
use crate::ipc::events::DaemonEvent;
use crate::ipc::protocol::Request;
use crate::session::mode::SessionMode;

pub struct ClaudioApp {
    pub sessions: Vec<SessionState>,
    pub focused_session_id: Option<String>,
    pub ptt_active: bool,
    pub vad_speaking: bool,
    pub last_transcription: Option<String>,
    socket_path: PathBuf,
    focus_handle: FocusHandle,
    needs_focus_sync: bool,
    pub file_tree: FileTree,
    pending_session_cwds: VecDeque<PathBuf>,
    pending_shell_modes: VecDeque<bool>,
    pub renaming_session_id: Option<String>,
    pub rename_input: String,
    pub pending_worktree_repo: Option<PathBuf>,
    pub worktree_name_input: String,
    pub file_tree_width: f32,
    file_tree_resizing: bool,
    pub grid_col_ratios: Vec<f32>,
    pub grid_row_ratios: Vec<f32>,
    pub grid_resize: Option<GridResize>,
}

impl ClaudioApp {
    pub fn new(
        socket_path: &Path,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus_handle = cx.focus_handle();

        // Start IPC subscription on background thread
        let (tx, rx) = flume::unbounded::<DaemonEvent>();
        let socket = socket_path.to_path_buf();
        std::thread::spawn(move || {
            ipc_bridge::run_subscription(&socket, tx);
        });

        // Poll IPC events
        cx.spawn(async move |this: gpui::WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(16))
                    .await;

                // Drain all available events
                while let Ok(event) = rx.try_recv() {
                    let _ = this.update(cx, |app, cx| {
                        app.handle_daemon_event(event, cx);
                    });
                }
            }
        })
        .detach();

        let mut file_tree = FileTree::new();
        let state = AppState::load();
        for root in &state.gui.folder_roots {
            file_tree.add_root(root.clone());
        }
        for root in &state.gui.worktree_roots {
            file_tree.add_worktree_root(root.clone());
        }

        // Restore sessions from previous run
        let mut pending_session_cwds = VecDeque::new();
        let mut pending_shell_modes = VecDeque::new();
        let restore_socket = socket_path.to_path_buf();
        let persisted_sessions = state.gui.sessions.clone();
        if !persisted_sessions.is_empty() {
            let default_cwd = file_tree.roots.first().cloned()
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
            for ps in &persisted_sessions {
                pending_session_cwds.push_back(ps.cwd.clone().unwrap_or_else(|| default_cwd.clone()));
                pending_shell_modes.push_back(ps.shell_mode);
            }
            cx.background_executor()
                .spawn(async move {
                    for ps in persisted_sessions {
                        let _ = ipc_bridge::send_command(
                            &restore_socket,
                            &Request::New {
                                name: Some(ps.name),
                                mode: SessionMode::Speaking,
                                cwd: None,
                                command: None,
                            },
                        );
                    }
                })
                .detach();
        }

        Self {
            sessions: Vec::new(),
            focused_session_id: None,
            ptt_active: false,
            vad_speaking: false,
            last_transcription: None,
            socket_path: socket_path.to_path_buf(),
            focus_handle,
            needs_focus_sync: false,
            file_tree,
            pending_session_cwds,
            pending_shell_modes,
            renaming_session_id: None,
            rename_input: String::new(),
            pending_worktree_repo: None,
            worktree_name_input: String::new(),
            file_tree_width: 250.0,
            file_tree_resizing: false,
            grid_col_ratios: Vec::new(),
            grid_row_ratios: Vec::new(),
            grid_resize: None,
        }
    }

    fn handle_daemon_event(&mut self, event: DaemonEvent, cx: &mut Context<Self>) {
        match event {
            DaemonEvent::Snapshot {
                sessions: _,
                focused_id: _,
                ptt_active,
            } => {
                // Only restore PTT state from daemon. Don't restore sessions
                // because the GUI owns the PTYs and we can't reconnect to old ones.
                self.ptt_active = ptt_active;
                cx.notify();
            }
            DaemonEvent::PttState { active } => {
                self.ptt_active = active;
                cx.notify();
            }
            DaemonEvent::VadState { speaking } => {
                self.vad_speaking = speaking;
                cx.notify();
            }
            DaemonEvent::Transcription { session_id, text, .. } => {
                if !text.is_empty() {
                    self.last_transcription = Some(text.clone());
                }
                if let Some(session) = self.sessions.iter().find(|s| s.id == session_id) {
                    if !text.is_empty() {
                        // Write text to terminal without auto-submitting.
                        // User reviews and presses Enter manually.
                        session.write_raw(&text);
                    }
                }
                cx.notify();
            }
            DaemonEvent::SessionCreated { session } => {
                if !self.sessions.iter().any(|s| s.id == session.id) {
                    let cwd = self.pending_session_cwds.pop_front();
                    let shell_mode = self.pending_shell_modes.pop_front().unwrap_or(false);
                    let state = SessionState::new_with_cwd(session, cwd, shell_mode, self.socket_path.clone(), cx);
                    self.sessions.push(state);
                    self.save_state();
                }
                self.needs_focus_sync = true;
                cx.notify();
            }
            DaemonEvent::SessionDestroyed { session_id } => {
                self.sessions.retain(|s| s.id != session_id);
                if self.focused_session_id.as_deref() == Some(&session_id) {
                    self.focused_session_id = self.sessions.first().map(|s| s.id.clone());
                }
                self.save_state();
                cx.notify();
            }
            DaemonEvent::SessionFocused { session_id } => {
                self.focused_session_id = Some(session_id.clone());
                for s in &mut self.sessions {
                    s.focused = s.id == session_id;
                }
                self.needs_focus_sync = true;
                cx.notify();
            }
            DaemonEvent::SessionRenamed { session_id, new_name } => {
                if let Some(s) = self.sessions.iter_mut().find(|s| s.id == session_id) {
                    s.set_name(new_name);
                }
                cx.notify();
            }
            DaemonEvent::SessionModeChanged { session_id, mode } => {
                if let Some(s) = self.sessions.iter_mut().find(|s| s.id == session_id) {
                    s.mode = mode;
                }
                cx.notify();
            }
            DaemonEvent::SessionBusyChanged { session_id, busy } => {
                if let Some(s) = self.sessions.iter_mut().find(|s| s.id == session_id) {
                    s.busy = busy;
                }
                cx.notify();
            }
            // Events we don't need in the GUI
            DaemonEvent::PaneContent { .. }
            | DaemonEvent::ResponseChunk { .. }
            | DaemonEvent::ResponseDone { .. }
            | DaemonEvent::ShellOutput { .. }
            | DaemonEvent::TtsSpeaking { .. } => {}
        }
    }

    fn sync_terminal_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.needs_focus_sync {
            return;
        }
        self.needs_focus_sync = false;
        if let Some(ref id) = self.focused_session_id {
            if let Some(session) = self.sessions.iter().find(|s| &s.id == id) {
                let handle = session.terminal_view.read(cx).focus_handle().clone();
                window.focus(&handle);
            }
        }
    }

    pub fn focus_session_by_id(&mut self, id: &str, cx: &mut Context<Self>) {
        let socket = self.socket_path.clone();
        let session_id = id.to_string();
        cx.background_executor()
            .spawn(async move {
                let _ = ipc_bridge::send_command(
                    &socket,
                    &Request::Focus {
                        session: session_id,
                    },
                );
            })
            .detach();
    }

    // Action handlers

    fn new_session(&mut self, _: &NewSession, _window: &mut Window, cx: &mut Context<Self>) {
        self.pending_session_cwds.push_back(self.default_cwd());
        self.pending_shell_modes.push_back(false);
        let socket = self.socket_path.clone();
        cx.background_executor()
            .spawn(async move {
                let _ = ipc_bridge::send_command(
                    &socket,
                    &Request::New {
                        name: None,
                        mode: SessionMode::Speaking,
                        cwd: None,
                        command: None,
                    },
                );
            })
            .detach();
    }

    fn new_shell_session(&mut self, _: &NewShellSession, _window: &mut Window, cx: &mut Context<Self>) {
        self.create_shell_session(cx);
    }

    pub fn create_shell_session(&mut self, cx: &mut Context<Self>) {
        self.pending_session_cwds.push_back(self.default_cwd());
        self.pending_shell_modes.push_back(true);
        let socket = self.socket_path.clone();
        cx.background_executor()
            .spawn(async move {
                let _ = ipc_bridge::send_command(
                    &socket,
                    &Request::New {
                        name: None,
                        mode: SessionMode::Speaking,
                        cwd: None,
                        command: None,
                    },
                );
            })
            .detach();
    }

    fn kill_focused(&mut self, _: &KillFocusedSession, _window: &mut Window, cx: &mut Context<Self>) {
        if let Some(id) = self.focused_session_id.clone() {
            self.kill_session(&id, cx);
        }
    }

    pub fn kill_session(&mut self, id: &str, cx: &mut Context<Self>) {
        let socket = self.socket_path.clone();
        let session_id = id.to_string();
        cx.background_executor()
            .spawn(async move {
                let _ = ipc_bridge::send_command(
                    &socket,
                    &Request::Kill { session: session_id },
                );
            })
            .detach();
    }

    fn cycle_focus(&mut self, _: &CycleFocus, window: &mut Window, cx: &mut Context<Self>) {
        if self.sessions.is_empty() {
            return;
        }
        let current_idx = self
            .focused_session_id
            .as_ref()
            .and_then(|id| self.sessions.iter().position(|s| &s.id == id))
            .unwrap_or(0);
        let next_idx = (current_idx + 1) % self.sessions.len();
        let next_id = self.sessions[next_idx].id.clone();
        self.focus_session_by_id(&next_id, cx);
        let handle = self.sessions[next_idx].terminal_view.read(cx).focus_handle().clone();
        window.focus(&handle);
    }

    fn toggle_mode(&mut self, _: &ToggleMode, _window: &mut Window, cx: &mut Context<Self>) {
        if let Some(ref id) = self.focused_session_id.clone() {
            let current = self
                .sessions
                .iter()
                .find(|s| &s.id == id)
                .map(|s| s.mode.as_str())
                .unwrap_or("speaking");
            let next = match current {
                "speaking" => SessionMode::Listening,
                "listening" => SessionMode::Muted,
                _ => SessionMode::Speaking,
            };
            let socket = self.socket_path.clone();
            let session_id = id.clone();
            cx.background_executor()
                .spawn(async move {
                    let _ = ipc_bridge::send_command(
                        &socket,
                        &Request::Mode {
                            session: session_id,
                            mode: next,
                        },
                    );
                })
                .detach();
        }
    }

    fn minimize_session(&mut self, _: &MinimizeSession, _window: &mut Window, cx: &mut Context<Self>) {
        if let Some(ref id) = self.focused_session_id.clone() {
            if let Some(s) = self.sessions.iter_mut().find(|s| &s.id == id) {
                s.minimized = true;
            }
            cx.notify();
        }
    }

    fn quit(&mut self, _: &Quit, _window: &mut Window, cx: &mut Context<Self>) {
        cx.quit();
    }

    fn stop_daemon(&mut self, _: &StopDaemon, _window: &mut Window, cx: &mut Context<Self>) {
        let socket = self.socket_path.clone();
        cx.background_executor()
            .spawn(async move {
                let _ = ipc_bridge::send_command(&socket, &Request::Stop);
            })
            .detach();
        cx.quit();
    }

    fn restart_daemon_action(&mut self, _: &RestartDaemon, _window: &mut Window, cx: &mut Context<Self>) {
        self.restart_daemon(cx);
    }

    pub fn restart_daemon(&mut self, cx: &mut Context<Self>) {
        let socket = self.socket_path.clone();
        cx.spawn(async move |this: gpui::WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
            // Stop the daemon
            let stop_socket = socket.clone();
            cx.background_executor()
                .spawn(async move {
                    let _ = ipc_bridge::send_command(&stop_socket, &Request::Stop);
                })
                .await;

            // Wait for the daemon to die (socket becomes unconnectable)
            for _ in 0..40 {
                cx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await;
                let probe_socket = socket.clone();
                let still_alive = cx
                    .background_executor()
                    .spawn(async move {
                        ipc_bridge::send_command(&probe_socket, &Request::Ping).is_ok()
                    })
                    .await;
                if !still_alive {
                    break;
                }
            }

            // Force cleanup stale files
            let config = crate::config::Config::load().ok();
            if let Some(ref config) = config {
                let pid_path = config.pid_path();
                if let Ok(pid_str) = std::fs::read_to_string(&pid_path) {
                    if let Ok(pid) = pid_str.trim().parse::<i32>() {
                        unsafe {
                            libc::kill(pid, libc::SIGTERM);
                        }
                    }
                }
                let _ = std::process::Command::new("pkill")
                    .args(["-f", "ok-claude-ml"])
                    .status();
                let _ = std::fs::remove_file(&pid_path);
                let _ = std::fs::remove_file(config.socket_path());
                let ml_sock = config
                    .socket_path()
                    .parent()
                    .unwrap_or(&PathBuf::from("/tmp"))
                    .join("ok_claude_ml.sock");
                let _ = std::fs::remove_file(&ml_sock);
            }

            // Start daemon in background
            let exe = std::env::current_exe().ok();
            if let Some(exe) = exe {
                let log_path = crate::config::Config::runtime_dir().join("claudio.log");
                if let Some(parent) = log_path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                if let Ok(log_file) = std::fs::File::create(&log_path) {
                    if let Ok(log_file2) = log_file.try_clone() {
                        let _ = std::process::Command::new(exe)
                            .args(["start", "--foreground"])
                            .stdout(std::process::Stdio::from(log_file2))
                            .stderr(std::process::Stdio::from(log_file))
                            .stdin(std::process::Stdio::null())
                            .spawn();
                    }
                }
            }

            // Wait for daemon to come up
            let mut started = false;
            for _ in 0..60 {
                cx.background_executor()
                    .timer(Duration::from_millis(500))
                    .await;
                let probe_socket = socket.clone();
                let alive = cx
                    .background_executor()
                    .spawn(async move {
                        ipc_bridge::send_command(&probe_socket, &Request::Ping).is_ok()
                    })
                    .await;
                if alive {
                    started = true;
                    break;
                }
            }

            if !started {
                tracing::error!("Daemon did not start after restart");
                return;
            }

            // Reconnect IPC subscription
            let (tx, rx) = flume::unbounded::<DaemonEvent>();
            let sub_socket = socket.clone();
            std::thread::spawn(move || {
                ipc_bridge::run_subscription(&sub_socket, tx);
            });

            // Re-create sessions from current state
            let session_names: Vec<(String, bool)> = this
                .update(cx, |app, cx| {
                    app.sessions.clear();
                    app.focused_session_id = None;
                    app.ptt_active = false;
                    app.vad_speaking = false;
                    app.last_transcription = None;
                    cx.notify();

                    let state = AppState::load();
                    state
                        .gui
                        .sessions
                        .iter()
                        .map(|ps| (ps.name.clone(), ps.shell_mode))
                        .collect()
                })
                .unwrap_or_default();

            // Queue cwds/shell modes and request sessions from daemon
            if !session_names.is_empty() {
                let state = AppState::load();
                let _ = this.update(cx, |app, _cx| {
                    let default_cwd = app.default_cwd();
                    for ps in &state.gui.sessions {
                        app.pending_session_cwds.push_back(
                            ps.cwd.clone().unwrap_or_else(|| default_cwd.clone()),
                        );
                        app.pending_shell_modes.push_back(ps.shell_mode);
                    }
                });
                let create_socket = socket.clone();
                cx.background_executor()
                    .spawn(async move {
                        for (name, _shell) in session_names {
                            let _ = ipc_bridge::send_command(
                                &create_socket,
                                &Request::New {
                                    name: Some(name),
                                    mode: SessionMode::Speaking,
                                    cwd: None,
                                    command: None,
                                },
                            );
                        }
                    })
                    .await;
            }

            // Poll new subscription for events
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(16))
                    .await;
                while let Ok(event) = rx.try_recv() {
                    let _ = this.update(cx, |app, cx| {
                        app.handle_daemon_event(event, cx);
                    });
                }
            }
        })
        .detach();
    }

    fn toggle_file_tree(&mut self, _: &ToggleFileTree, _window: &mut Window, cx: &mut Context<Self>) {
        self.file_tree.toggle_visible();
        cx.notify();
    }

    fn focus_file_tree_search(
        &mut self,
        _: &FocusFileTreeSearch,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.file_tree.visible {
            self.file_tree.visible = true;
        }
        self.file_tree.search_active = !self.file_tree.search_active;
        cx.notify();
    }

    fn add_folder(&mut self, _: &AddFolder, _window: &mut Window, cx: &mut Context<Self>) {
        self.open_add_folder_dialog(cx);
    }

    fn toggle_autopilot(&mut self, _: &ToggleAutopilot, _window: &mut Window, cx: &mut Context<Self>) {
        if let Some(ref id) = self.focused_session_id {
            if let Some(session) = self.sessions.iter().find(|s| &s.id == id) {
                let prev = session.autopilot.load(Ordering::Relaxed);
                session.autopilot.store(!prev, Ordering::Relaxed);
                cx.notify();
            }
        }
    }

    fn stop_speech(&mut self, _: &StopSpeech, _window: &mut Window, _cx: &mut Context<Self>) {
        let _ = std::process::Command::new("pkill")
            .arg("-f")
            .arg("pw-play.*/tmp/claudio_tts")
            .output();
        let _ = std::process::Command::new("pkill")
            .arg("-f")
            .arg("piper.*claudio")
            .output();
    }

    fn read_aloud(&mut self, _: &ReadAloud, _window: &mut Window, cx: &mut Context<Self>) {
        let raw_text = if let Some(ref id) = self.focused_session_id {
            self.sessions
                .iter()
                .find(|s| &s.id == id)
                .map(|s| s.terminal_view.read(cx).screen_text())
        } else {
            None
        };

        if let Some(raw_text) = raw_text {
            if !raw_text.trim().is_empty() {
                // Kill any ongoing speech
                let _ = std::process::Command::new("pkill").arg("-f").arg("pw-play.*/tmp/claudio_tts").output();

                let config = crate::config::Config::load().ok();
                let model_path = config
                    .and_then(|c| c.tts.model)
                    .unwrap_or_else(|| {
                        dirs::data_dir()
                            .unwrap_or_else(|| PathBuf::from("/home"))
                            .join("piper/voices/en_US-lessac-medium.onnx")
                            .to_string_lossy()
                            .to_string()
                    });

                let piper_bin = crate::daemon::find_ml_service_dir()
                    .map(|d| d.join(".venv/bin/piper"))
                    .unwrap_or_else(|_| PathBuf::from("piper"));

                std::thread::spawn(move || {
                    use std::process::{Command, Stdio};
                    let wav_path = "/tmp/claudio_tts.wav";

                    let text = summarize_screen_for_tts(&raw_text)
                        .unwrap_or_else(|| clean_for_tts(&raw_text));

                    if text.trim().is_empty() {
                        return;
                    }

                    let status = Command::new(&piper_bin)
                        .arg("--model")
                        .arg(&model_path)
                        .arg("--output_file")
                        .arg(wav_path)
                        .stdin(Stdio::piped())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .spawn()
                        .and_then(|mut child| {
                            if let Some(mut stdin) = child.stdin.take() {
                                use std::io::Write;
                                let _ = stdin.write_all(text.as_bytes());
                                drop(stdin);
                            }
                            child.wait()
                        });

                    if status.is_ok_and(|s| s.success()) {
                        let _ = Command::new("pw-play")
                            .arg(wav_path)
                            .stdout(Stdio::null())
                            .stderr(Stdio::null())
                            .status();
                    } else {
                        // Fallback to spd-say
                        let _ = Command::new("spd-say").arg(&text).spawn();
                    }
                });
            }
        }
    }

    fn default_cwd(&self) -> PathBuf {
        self.file_tree.roots.first().cloned()
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
    }

    pub fn save_state(&self) {
        let state = AppState {
            gui: crate::config::GuiState {
                folder_roots: self.file_tree.roots.clone(),
                worktree_roots: self.file_tree.worktree_roots.clone(),
                sessions: self
                    .sessions
                    .iter()
                    .map(|s| crate::config::PersistedSession {
                        name: s.name.clone(),
                        cwd: s.cwd.clone(),
                        shell_mode: s.shell_mode,
                    })
                    .collect(),
            },
        };
        state.save();
    }

    pub fn open_add_folder_dialog(&mut self, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: true,
            prompt: None,
        });
        cx.spawn(async move |this: gpui::WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
            if let Ok(Ok(Some(paths))) = receiver.await {
                let _ = this.update(cx, |app, cx| {
                    for path in paths {
                        app.file_tree.add_root(path);
                    }
                    app.save_state();
                    cx.notify();
                });
            }
        })
        .detach();
    }

    pub fn inject_text_to_focused(&self, text: &str) {
        if let Some(ref id) = self.focused_session_id {
            if let Some(session) = self.sessions.iter().find(|s| &s.id == id) {
                session.write_raw(text);
            }
        }
    }

    pub fn open_in_editor(&self, dir: &Path) {
        let config = crate::config::Config::load().unwrap_or_default();
        let editor = config.gui.resolve_editor();
        match std::process::Command::new(&editor).arg(dir).spawn() {
            Ok(_) => {}
            Err(e) => eprintln!("Failed to open editor '{}': {}", editor, e),
        }
    }

    pub fn start_rename(&mut self, session_id: String, cx: &mut Context<Self>) {
        let current_name = self
            .sessions
            .iter()
            .find(|s| s.id == session_id)
            .map(|s| s.name.clone())
            .unwrap_or_default();
        self.renaming_session_id = Some(session_id);
        self.rename_input = current_name;
        cx.notify();
    }

    pub fn commit_rename(&mut self, cx: &mut Context<Self>) {
        if let Some(session_id) = self.renaming_session_id.take() {
            let new_name = self.rename_input.clone();
            self.rename_input.clear();
            if !new_name.is_empty() {
                // Update local state immediately
                if let Some(s) = self.sessions.iter_mut().find(|s| s.id == session_id) {
                    s.set_name(new_name.clone());
                }
                let socket = self.socket_path.clone();
                cx.background_executor()
                    .spawn(async move {
                        let _ = ipc_bridge::send_command(
                            &socket,
                            &Request::Rename {
                                session: session_id,
                                new_name,
                            },
                        );
                    })
                    .detach();
            }
            cx.notify();
        }
    }

    pub fn cancel_rename(&mut self, cx: &mut Context<Self>) {
        self.renaming_session_id = None;
        self.rename_input.clear();
        cx.notify();
    }

    fn handle_rename_key(&mut self, ev: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if self.pending_worktree_repo.is_some() {
            let key = ev.keystroke.key.as_str();
            match key {
                "enter" => self.commit_worktree_naming(cx),
                "escape" => self.cancel_worktree_naming(cx),
                "backspace" => {
                    self.worktree_name_input.pop();
                    cx.notify();
                }
                _ => {
                    if let Some(ch) = &ev.keystroke.key_char {
                        if !ev.keystroke.modifiers.control && !ev.keystroke.modifiers.alt {
                            self.worktree_name_input.push_str(ch);
                            cx.notify();
                        }
                    }
                }
            }
            return;
        }
        if self.renaming_session_id.is_some() {
            let key = ev.keystroke.key.as_str();
            match key {
                "enter" => self.commit_rename(cx),
                "escape" => self.cancel_rename(cx),
                "backspace" => {
                    self.rename_input.pop();
                    cx.notify();
                }
                _ => {
                    if let Some(ch) = &ev.keystroke.key_char {
                        if !ev.keystroke.modifiers.control && !ev.keystroke.modifiers.alt {
                            self.rename_input.push_str(ch);
                            cx.notify();
                        }
                    }
                }
            }
            return;
        }
        if self.file_tree.search_active {
            let key = ev.keystroke.key.as_str();
            match key {
                "escape" => {
                    self.file_tree.search_active = false;
                    cx.notify();
                }
                "backspace" => {
                    self.file_tree.search_query.pop();
                    cx.notify();
                }
                _ => {
                    if let Some(ch) = &ev.keystroke.key_char {
                        if !ev.keystroke.modifiers.control && !ev.keystroke.modifiers.alt {
                            self.file_tree.search_query.push_str(ch);
                            cx.notify();
                        }
                    }
                }
            }
        }
    }

    pub fn new_session_in_dir(&mut self, dir: PathBuf, cx: &mut Context<Self>) {
        self.pending_session_cwds.push_back(dir);
        self.pending_shell_modes.push_back(false);
        let socket = self.socket_path.clone();
        cx.background_executor()
            .spawn(async move {
                let _ = ipc_bridge::send_command(
                    &socket,
                    &Request::New {
                        name: None,
                        mode: SessionMode::Speaking,
                        cwd: None,
                        command: None,
                    },
                );
            })
            .detach();
    }

    pub fn start_worktree_naming(&mut self, repo_dir: PathBuf, cx: &mut Context<Self>) {
        self.renaming_session_id = None;
        self.rename_input.clear();
        self.pending_worktree_repo = Some(repo_dir);
        self.worktree_name_input.clear();
        cx.notify();
    }

    pub fn commit_worktree_naming(&mut self, cx: &mut Context<Self>) {
        if let Some(repo_dir) = self.pending_worktree_repo.take() {
            let name = self.worktree_name_input.clone();
            self.worktree_name_input.clear();
            if !name.is_empty() {
                self.new_worktree_session_in_dir(repo_dir, name, cx);
            }
            cx.notify();
        }
    }

    pub fn cancel_worktree_naming(&mut self, cx: &mut Context<Self>) {
        self.pending_worktree_repo = None;
        self.worktree_name_input.clear();
        cx.notify();
    }

    pub fn new_worktree_session_in_dir(&mut self, repo_dir: PathBuf, session_name: String, cx: &mut Context<Self>) {
        let worktree_dir = repo_dir.join(".worktree").join(&session_name);
        let branch_name = session_name.clone();
        let socket = self.socket_path.clone();
        let name = session_name.clone();
        let wt_dir_for_cleanup = worktree_dir.clone();

        self.pending_session_cwds.push_back(worktree_dir.clone());
        self.pending_shell_modes.push_back(false);

        cx.spawn(async move |this: gpui::WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
            let git_ok = cx.background_executor()
                .spawn(async move {
                    let result = std::process::Command::new("git")
                        .args(["worktree", "add", "-b", &branch_name])
                        .arg(&worktree_dir)
                        .arg("HEAD")
                        .current_dir(&repo_dir)
                        .output();

                    match result {
                        Ok(out) if out.status.success() => true,
                        Ok(out) => {
                            let stderr = String::from_utf8_lossy(&out.stderr);
                            tracing::warn!("git worktree add -b failed: {stderr}");
                            // Branch might already exist, retry without -b
                            match std::process::Command::new("git")
                                .args(["worktree", "add"])
                                .arg(&worktree_dir)
                                .arg(&branch_name)
                                .current_dir(&repo_dir)
                                .output()
                            {
                                Ok(out2) if out2.status.success() => true,
                                Ok(out2) => {
                                    let stderr2 = String::from_utf8_lossy(&out2.stderr);
                                    tracing::error!("git worktree add fallback failed: {stderr2}");
                                    false
                                }
                                Err(e) => {
                                    tracing::error!("git worktree add fallback error: {e}");
                                    false
                                }
                            }
                        }
                        Err(e) => {
                            tracing::error!("Failed to run git: {e}");
                            false
                        }
                    }
                })
                .await;

            if git_ok {
                let wt_dir_for_tree = wt_dir_for_cleanup.clone();
                let _ = this.update(cx, |app, cx| {
                    app.file_tree.add_worktree_root(wt_dir_for_tree);
                    app.save_state();
                    cx.notify();
                });
                cx.background_executor()
                    .spawn(async move {
                        if let Err(e) = ipc_bridge::send_command(
                            &socket,
                            &Request::New {
                                name: Some(name),
                                mode: SessionMode::Speaking,
                                cwd: None,
                                command: None,
                            },
                        ) {
                            tracing::error!("IPC send_command failed for worktree session: {e}");
                        }
                    })
                    .detach();
            } else {
                let _ = this.update(cx, |app, _cx| {
                    app.pending_session_cwds.retain(|p| p != &wt_dir_for_cleanup);
                });
            }
        })
        .detach();
    }
}

impl ClaudioApp {
    fn render_resize_borders(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let border = px(6.0);
        let corner = px(12.0);

        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            // Edges
            .child(
                div().absolute().top_0().left(corner).right(corner).h(border)
                    .cursor_ns_resize()
                    .on_mouse_down(MouseButton::Left, cx.listener(|_, _, window, _| {
                        window.start_window_resize(ResizeEdge::Top);
                    })),
            )
            .child(
                div().absolute().bottom_0().left(corner).right(corner).h(border)
                    .cursor_ns_resize()
                    .on_mouse_down(MouseButton::Left, cx.listener(|_, _, window, _| {
                        window.start_window_resize(ResizeEdge::Bottom);
                    })),
            )
            .child(
                div().absolute().left_0().top(corner).bottom(corner).w(border)
                    .cursor_ew_resize()
                    .on_mouse_down(MouseButton::Left, cx.listener(|_, _, window, _| {
                        window.start_window_resize(ResizeEdge::Left);
                    })),
            )
            .child(
                div().absolute().right_0().top(corner).bottom(corner).w(border)
                    .cursor_ew_resize()
                    .on_mouse_down(MouseButton::Left, cx.listener(|_, _, window, _| {
                        window.start_window_resize(ResizeEdge::Right);
                    })),
            )
            // Corners
            .child(
                div().absolute().top_0().left_0().w(corner).h(corner)
                    .cursor_nwse_resize()
                    .on_mouse_down(MouseButton::Left, cx.listener(|_, _, window, _| {
                        window.start_window_resize(ResizeEdge::TopLeft);
                    })),
            )
            .child(
                div().absolute().top_0().right_0().w(corner).h(corner)
                    .cursor_nesw_resize()
                    .on_mouse_down(MouseButton::Left, cx.listener(|_, _, window, _| {
                        window.start_window_resize(ResizeEdge::TopRight);
                    })),
            )
            .child(
                div().absolute().bottom_0().left_0().w(corner).h(corner)
                    .cursor_nesw_resize()
                    .on_mouse_down(MouseButton::Left, cx.listener(|_, _, window, _| {
                        window.start_window_resize(ResizeEdge::BottomLeft);
                    })),
            )
            .child(
                div().absolute().bottom_0().right_0().w(corner).h(corner)
                    .cursor_nwse_resize()
                    .on_mouse_down(MouseButton::Left, cx.listener(|_, _, window, _| {
                        window.start_window_resize(ResizeEdge::BottomRight);
                    })),
            )
    }
}

impl Render for ClaudioApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_terminal_focus(window, cx);
        div()
            .key_context("ClaudioApp")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::new_session))
            .on_action(cx.listener(Self::new_shell_session))
            .on_action(cx.listener(Self::kill_focused))
            .on_action(cx.listener(Self::cycle_focus))
            .on_action(cx.listener(Self::toggle_mode))
            .on_action(cx.listener(Self::minimize_session))
            .on_action(cx.listener(Self::toggle_file_tree))
            .on_action(cx.listener(Self::focus_file_tree_search))
            .on_action(cx.listener(Self::add_folder))
            .on_action(cx.listener(Self::read_aloud))
            .on_action(cx.listener(Self::stop_speech))
            .on_action(cx.listener(Self::quit))
            .on_action(cx.listener(Self::stop_daemon))
            .on_action(cx.listener(Self::restart_daemon_action))
            .on_action(cx.listener(Self::toggle_autopilot))
            .on_key_down(cx.listener(Self::handle_rename_key))
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .bg(rgb(super::theme::BASE))
            .text_color(rgb(super::theme::TEXT))
            .child(self.render_status_bar(window, cx))
            .on_mouse_move(cx.listener(|app, ev: &MouseMoveEvent, _window, cx| {
                if app.file_tree_resizing {
                    let x: f32 = ev.position.x.into();
                    app.file_tree_width = x.clamp(120.0, 600.0);
                    cx.notify();
                } else if app.grid_resize.is_some() {
                    let resize = app.grid_resize.as_ref().unwrap();
                    let sensitivity = 0.005;
                    let min_ratio = 0.1;
                    match resize.axis {
                        super::terminal_grid::ResizeAxis::Column(idx) => {
                            let delta: f32 = f32::from(ev.position.x) - resize.start_position;
                            let ratio_delta = delta * sensitivity;
                            let (r0, r1) = resize.start_ratios;
                            let sum = r0 + r1;
                            let new_r0 = (r0 + ratio_delta).clamp(min_ratio, sum - min_ratio);
                            let new_r1 = sum - new_r0;
                            app.grid_col_ratios[idx] = new_r0;
                            app.grid_col_ratios[idx + 1] = new_r1;
                        }
                        super::terminal_grid::ResizeAxis::Row(idx) => {
                            let delta: f32 = f32::from(ev.position.y) - resize.start_position;
                            let ratio_delta = delta * sensitivity;
                            let (r0, r1) = resize.start_ratios;
                            let sum = r0 + r1;
                            let new_r0 = (r0 + ratio_delta).clamp(min_ratio, sum - min_ratio);
                            let new_r1 = sum - new_r0;
                            app.grid_row_ratios[idx] = new_r0;
                            app.grid_row_ratios[idx + 1] = new_r1;
                        }
                    }
                    cx.notify();
                }
            }))
            .on_mouse_up(MouseButton::Left, cx.listener(|app, _ev: &MouseUpEvent, _window, _cx| {
                app.file_tree_resizing = false;
                app.grid_resize = None;
            }))
            .child({
                let mut content = div().flex_1().min_h(px(0.0)).flex().flex_row();
                if self.file_tree.visible {
                    content = content.child(self.render_file_tree(window, cx));
                    // Resize handle
                    content = content.child(
                        div()
                            .w(px(4.0))
                            .h_full()
                            .cursor_col_resize()
                            .bg(rgb(super::theme::SURFACE0))
                            .hover(|s| s.bg(rgb(super::theme::BLUE)))
                            .on_mouse_down(MouseButton::Left, cx.listener(|app, _ev: &MouseDownEvent, _window, _cx| {
                                app.file_tree_resizing = true;
                            })),
                    );
                }
                content = content.child(
                    div().flex_1().child(self.render_terminal_grid(window, cx)),
                );
                content
            })
            .child(self.render_resize_borders(cx))
    }
}

/// Use `claude -p` to summarize terminal screen content for TTS.
/// Returns `None` on failure (timeout, missing binary, non-zero exit).
fn summarize_screen_for_tts(screen_text: &str) -> Option<String> {
    use std::process::{Command, Stdio};

    let prompt = format!(
        "You are Claude Code narrating your own terminal screen to a visually impaired user. \
         Speak in first person as if you are the one working -- e.g. 'I'm currently editing...', \
         'I just finished running the tests...'. \
         Summarize what's on screen naturally and conversationally. \
         Skip file paths, box-drawing characters, command prompts, tool call headers, \
         progress spinners, and code blocks. Focus on what you are communicating: \
         plans, explanations, status updates, errors. Keep it concise -- this will be spoken aloud.\n\n\
         Terminal screen content:\n{screen_text}"
    );

    let mut child = Command::new("claude")
        .args(["-p", "--model", "haiku"])
        .arg(&prompt)
        .env_remove("CLAUDECODE")
        .env_remove("CLAUDE_CODE_ENTRY_TOOL")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;

    // 15-second timeout
    let start = std::time::Instant::now();
    let timeout = Duration::from_secs(15);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(_) => return None,
        }
    }

    let output = child.wait_with_output().ok()?;

    if !output.status.success() {
        return None;
    }

    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if text.is_empty() {
        return None;
    }

    Some(text)
}

/// Filter raw terminal screen text down to readable prose for TTS.
/// Keeps natural language lines (plans, explanations, bullets) and drops
/// code, file paths, box-drawing borders, diffs, prompts, and other noise.
fn clean_for_tts(raw: &str) -> String {
    let mut out = Vec::new();

    for line in raw.lines() {
        let trimmed = line.trim();

        if trimmed.is_empty() {
            // Preserve paragraph breaks (collapsed later)
            if out.last().is_some_and(|l: &String| !l.is_empty()) {
                out.push(String::new());
            }
            continue;
        }

        // Skip box-drawing / decorative lines
        if trimmed.chars().all(|c| "─│┌┐└┘├┤┬┴┼╭╮╰╯━┃╋═║╔╗╚╝╠╣╦╩╬▀▄█▌▐░▒▓ -_=+*".contains(c)) {
            continue;
        }

        // Skip lines that are mostly box-drawing (borders with some text)
        let box_chars: usize = trimmed.chars().filter(|c| "─│┌┐└┘├┤┬┴┼╭╮╰╯━┃╋═║╔╗╚╝╠╣╦╩╬".contains(*c)).count();
        if box_chars > 3 {
            continue;
        }

        // Skip file paths
        if trimmed.starts_with('/') || trimmed.starts_with("./") || trimmed.starts_with("src/") {
            continue;
        }

        // Skip diff lines
        if (trimmed.starts_with('+') || trimmed.starts_with('-')) && trimmed.len() > 1 && !trimmed.starts_with("- ") {
            continue;
        }
        if trimmed.starts_with("@@") || trimmed.starts_with("diff ") || trimmed.starts_with("index ") {
            continue;
        }

        // Skip prompt-like lines
        if trimmed.starts_with("$ ") || trimmed.starts_with("❯ ") || trimmed.starts_with("> ") {
            continue;
        }

        // Skip tool-call headers and status lines from Claude
        if trimmed.starts_with("Read(") || trimmed.starts_with("Edit(") || trimmed.starts_with("Write(")
            || trimmed.starts_with("Bash(") || trimmed.starts_with("Glob(") || trimmed.starts_with("Grep(")
        {
            continue;
        }

        // Skip progress indicators and spinners
        if trimmed.contains("⠋") || trimmed.contains("⠙") || trimmed.contains("⠹")
            || trimmed.contains("⠸") || trimmed.contains("⠼") || trimmed.contains("⠴")
            || trimmed.contains("[====") || trimmed.contains("...")
        {
            continue;
        }

        // Heuristic: skip lines that look like code (high density of code punctuation)
        let code_chars: usize = trimmed.chars().filter(|c| "{}();=><&|\\@#$^`~[]".contains(*c)).count();
        let alpha_chars: usize = trimmed.chars().filter(|c| c.is_alphabetic()).count();
        if trimmed.len() > 5 && code_chars as f32 / trimmed.len() as f32 > 0.15 && alpha_chars < code_chars * 3 {
            continue;
        }

        // Skip lines that are mostly non-alphabetic
        if trimmed.len() > 3 && alpha_chars as f32 / (trimmed.len() as f32) < 0.4 {
            continue;
        }

        // Clean up markdown-style formatting for speech
        let mut cleaned = trimmed.to_string();
        // Strip leading bullets/numbers but keep the text
        if let Some(rest) = cleaned.strip_prefix("- ") {
            cleaned = rest.to_string();
        }
        // Strip bold/italic markers
        cleaned = cleaned.replace("**", "").replace("__", "");
        // Strip backticks
        cleaned = cleaned.replace('`', "");
        // Strip heading markers
        while cleaned.starts_with("# ") || cleaned.starts_with("## ") || cleaned.starts_with("### ") {
            if let Some(pos) = cleaned.find(' ') {
                cleaned = cleaned[pos + 1..].to_string();
            } else {
                break;
            }
        }

        if !cleaned.trim().is_empty() {
            out.push(cleaned);
        }
    }

    // Collapse multiple blank lines
    let mut result = String::new();
    let mut prev_blank = false;
    for line in &out {
        if line.is_empty() {
            if !prev_blank {
                result.push('\n');
            }
            prev_blank = true;
        } else {
            if prev_blank && !result.is_empty() {
                result.push('\n');
            }
            if !result.is_empty() && !prev_blank {
                result.push('\n');
            }
            result.push_str(line);
            prev_blank = false;
        }
    }

    result
}
