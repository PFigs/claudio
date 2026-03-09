use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui::*;

use super::actions::*;
use super::file_tree::FileTree;
use super::ipc_bridge;
use super::session_state::SessionState;
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
    pub renaming_session_id: Option<String>,
    pub rename_input: String,
    pub file_tree_width: f32,
    file_tree_resizing: bool,
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
            pending_session_cwds: VecDeque::new(),
            renaming_session_id: None,
            rename_input: String::new(),
            file_tree_width: 250.0,
            file_tree_resizing: false,
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
                    let state = SessionState::new_with_cwd(session, cwd, cx);
                    self.sessions.push(state);
                }
                self.needs_focus_sync = true;
                cx.notify();
            }
            DaemonEvent::SessionDestroyed { session_id } => {
                self.sessions.retain(|s| s.id != session_id);
                if self.focused_session_id.as_deref() == Some(&session_id) {
                    self.focused_session_id = self.sessions.first().map(|s| s.id.clone());
                }
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
                    s.name = new_name;
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
        let socket = self.socket_path.clone();
        cx.background_executor()
            .spawn(async move {
                let _ = ipc_bridge::send_command(
                    &socket,
                    &Request::New {
                        name: None,
                        mode: SessionMode::Speaking,
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

    fn toggle_file_tree(&mut self, _: &ToggleFileTree, _window: &mut Window, cx: &mut Context<Self>) {
        self.file_tree.toggle_visible();
        cx.notify();
    }

    fn add_folder(&mut self, _: &AddFolder, _window: &mut Window, cx: &mut Context<Self>) {
        self.open_add_folder_dialog(cx);
    }

    pub fn save_folder_state(&self) {
        let state = AppState {
            gui: crate::config::GuiState {
                folder_roots: self.file_tree.roots.clone(),
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
                    app.save_folder_state();
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
                    s.name = new_name.clone();
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
        if self.renaming_session_id.is_none() {
            return;
        }
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
    }

    pub fn new_session_in_dir(&mut self, dir: PathBuf, cx: &mut Context<Self>) {
        self.pending_session_cwds.push_back(dir);
        let socket = self.socket_path.clone();
        cx.background_executor()
            .spawn(async move {
                let _ = ipc_bridge::send_command(
                    &socket,
                    &Request::New {
                        name: None,
                        mode: SessionMode::Speaking,
                    },
                );
            })
            .detach();
    }

    pub fn new_worktree_session_in_dir(&mut self, repo_dir: PathBuf, cx: &mut Context<Self>) {
        let session_name = worktree_session_name(
            &repo_dir,
            &self.sessions.iter().map(|s| s.name.clone()).collect::<Vec<_>>(),
        );
        let worktree_dir = repo_dir.join(".worktree").join(format!("claudio-{session_name}"));
        let branch_name = format!("claudio-{session_name}");
        let socket = self.socket_path.clone();
        let name = session_name.clone();

        self.pending_session_cwds.push_back(worktree_dir.clone());

        cx.background_executor()
            .spawn(async move {
                let result = std::process::Command::new("git")
                    .args(["worktree", "add", "-b", &branch_name])
                    .arg(&worktree_dir)
                    .arg("HEAD")
                    .current_dir(&repo_dir)
                    .output();

                let ok = match result {
                    Ok(out) if out.status.success() => true,
                    Ok(_) => {
                        // Branch might already exist, retry without -b
                        std::process::Command::new("git")
                            .args(["worktree", "add"])
                            .arg(&worktree_dir)
                            .arg(&branch_name)
                            .current_dir(&repo_dir)
                            .output()
                            .map(|o| o.status.success())
                            .unwrap_or(false)
                    }
                    Err(_) => false,
                };

                if ok {
                    let _ = ipc_bridge::send_command(
                        &socket,
                        &Request::New {
                            name: Some(name),
                            mode: SessionMode::Speaking,
                        },
                    );
                } else {
                    tracing::error!("Failed to create git worktree in {}", repo_dir.display());
                }
            })
            .detach();
    }
}

fn worktree_session_name(repo_dir: &Path, existing_sessions: &[String]) -> String {
    let repo_name = repo_dir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "repo".to_string());

    let prefix = format!("{repo_name}-wt-");
    let max_idx = existing_sessions
        .iter()
        .filter_map(|name| name.strip_prefix(&prefix)?.parse::<u32>().ok())
        .max()
        .unwrap_or(0);

    format!("{prefix}{}", max_idx + 1)
}

impl Render for ClaudioApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_terminal_focus(window, cx);
        div()
            .key_context("ClaudioApp")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::new_session))
            .on_action(cx.listener(Self::kill_focused))
            .on_action(cx.listener(Self::cycle_focus))
            .on_action(cx.listener(Self::toggle_mode))
            .on_action(cx.listener(Self::minimize_session))
            .on_action(cx.listener(Self::toggle_file_tree))
            .on_action(cx.listener(Self::add_folder))
            .on_action(cx.listener(Self::quit))
            .on_action(cx.listener(Self::stop_daemon))
            .on_key_down(cx.listener(Self::handle_rename_key))
            .size_full()
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
                }
            }))
            .on_mouse_up(MouseButton::Left, cx.listener(|app, _ev: &MouseUpEvent, _window, _cx| {
                app.file_tree_resizing = false;
            }))
            .child({
                let mut content = div().flex_1().flex().flex_row();
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
    }
}
