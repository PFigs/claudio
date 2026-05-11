use std::time::Duration;

use gpui::*;

use super::app::ClaudioApp;
use super::orchestrator_index::{self, FeatureSummary, HandoffStatus};
use super::theme;

pub struct OrchestratorState {
    pub features: Vec<FeatureSummary>,
    pub show_completed: bool,
}

impl OrchestratorState {
    pub fn new() -> Self {
        Self {
            features: Vec::new(),
            show_completed: false,
        }
    }
}

impl ClaudioApp {
    /// Kick off the background polling task that refreshes
    /// `self.orchestrator.features` every 2 seconds.
    pub fn start_orchestrator_poll(&self, cx: &mut Context<Self>) {
        cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
            loop {
                let features = cx
                    .background_executor()
                    .spawn(async { orchestrator_index::scan_all() })
                    .await;
                let _ = this.update(cx, |app, cx| {
                    app.orchestrator.features = features;
                    cx.notify();
                });
                cx.background_executor()
                    .timer(Duration::from_secs(2))
                    .await;
            }
        })
        .detach();
    }

    pub fn render_orchestrator_sidebar(
        &self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let header = div()
            .px(px(8.0))
            .py(px(6.0))
            .flex()
            .flex_row()
            .items_center()
            .child(
                div()
                    .flex_1()
                    .child("Orchestrator")
                    .text_color(rgb(theme::OVERLAY0))
                    .text_size(px(12.0)),
            )
            .child(
                div()
                    .id("orch-toggle-completed")
                    .child(if self.orchestrator.show_completed {
                        "all"
                    } else {
                        "active"
                    })
                    .text_color(rgb(theme::OVERLAY0))
                    .text_size(px(11.0))
                    .px(px(4.0))
                    .cursor_pointer()
                    .hover(|s| s.text_color(rgb(theme::BLUE)))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|app, _ev: &MouseDownEvent, _window, cx| {
                            app.orchestrator.show_completed = !app.orchestrator.show_completed;
                            cx.notify();
                        }),
                    ),
            );

        let mut content = div()
            .id("orch-content")
            .flex_1()
            .min_h(px(0.0))
            .overflow_y_scroll();

        let visible: Vec<&FeatureSummary> = self
            .orchestrator
            .features
            .iter()
            .filter(|f| self.orchestrator.show_completed || !f.is_completed())
            .collect();

        if visible.is_empty() {
            content = content.child(
                div()
                    .px(px(8.0))
                    .py(px(16.0))
                    .child("No orchestrator features.")
                    .text_color(rgb(theme::OVERLAY0))
                    .text_size(px(12.0)),
            );
        } else {
            for feature in visible {
                content = content.child(self.render_feature_row(feature, cx));
            }
        }

        div()
            .id("orchestrator-sidebar")
            .w(px(self.file_tree_width))
            .h_full()
            .flex()
            .flex_col()
            .bg(rgb(theme::MANTLE))
            .child(header)
            .child(content)
            .into_any_element()
    }

    fn render_feature_row(
        &self,
        feature: &FeatureSummary,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let state_color = match feature.state.as_str() {
            "in_progress" | "running" | "implementing" => theme::BLUE,
            "blocked" => theme::RED,
            "pending" | "spawned" => theme::YELLOW,
            "done" | "completed" | "complete" | "merged" | "shipped" => theme::GREEN,
            _ => theme::OVERLAY0,
        };

        let handoff_badge = match feature.handoff {
            HandoffStatus::Blocker => Some(("BLOCK", theme::RED)),
            HandoffStatus::Question => Some(("Q", theme::YELLOW)),
            HandoffStatus::Done => Some(("DONE", theme::GREEN)),
            HandoffStatus::None => None,
        };

        let log_preview = feature
            .last_log_line
            .clone()
            .unwrap_or_else(|| String::from("(no log entries)"));
        let log_preview = if log_preview.len() > 64 {
            format!("{}...", &log_preview[..61])
        } else {
            log_preview
        };

        let feature_slug = feature.feature_slug.clone();
        let worktree = feature.worktree.clone();
        let row_id: SharedString = format!("orch-row-{}", feature.feature_slug).into();
        let has_session = self.sessions.iter().any(|s| s.name == feature.feature_slug);

        let mut row = div()
            .id(row_id)
            .px(px(8.0))
            .py(px(6.0))
            .w_full()
            .flex()
            .flex_col()
            .gap(px(2.0))
            .cursor_pointer()
            .hover(|s| s.bg(rgb(theme::SURFACE0)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |app, _ev: &MouseDownEvent, _window, cx| {
                    app.handle_orchestrator_row_click(&feature_slug, worktree.as_deref(), cx);
                }),
            );

        let mut top = div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(6.0))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .overflow_x_hidden()
                    .child(feature.feature_slug.clone())
                    .text_color(rgb(theme::TEXT))
                    .text_size(px(13.0)),
            )
            .child(
                div()
                    .child(feature.state.clone())
                    .text_color(rgb(state_color))
                    .text_size(px(10.0))
                    .px(px(4.0))
                    .rounded(px(2.0))
                    .bg(rgb(theme::SURFACE0)),
            );

        if let Some((label, color)) = handoff_badge {
            top = top.child(
                div()
                    .child(label)
                    .text_color(rgb(theme::CRUST))
                    .text_size(px(10.0))
                    .px(px(4.0))
                    .rounded(px(2.0))
                    .bg(rgb(color)),
            );
        }

        if !has_session {
            top = top.child(
                div()
                    .child("(no PTY)")
                    .text_color(rgb(theme::OVERLAY0))
                    .text_size(px(10.0)),
            );
        }

        row = row.child(top).child(
            div()
                .child(log_preview)
                .text_color(rgb(theme::OVERLAY0))
                .text_size(px(11.0))
                .overflow_x_hidden(),
        );

        row.into_any_element()
    }

    /// Click handler for an orchestrator feature row.
    /// - If a session named after the feature slug exists, focus it.
    /// - Otherwise, spawn a new claudio session pointed at the worktree
    ///   running `claude "Read <feature_dir>/starter.md and follow it."`.
    fn handle_orchestrator_row_click(
        &mut self,
        feature_slug: &str,
        worktree: Option<&std::path::Path>,
        cx: &mut Context<Self>,
    ) {
        if let Some(session) = self.sessions.iter().find(|s| s.name == feature_slug) {
            let id = session.id.clone();
            self.focus_session_by_id(&id, cx);
            return;
        }

        let Some(worktree) = worktree else {
            tracing::warn!(
                "orchestrator: no worktree recorded for {feature_slug}; cannot spawn"
            );
            return;
        };

        let starter = self
            .orchestrator
            .features
            .iter()
            .find(|f| f.feature_slug == feature_slug)
            .and_then(|f| f.status_path.parent())
            .map(|p| p.join("starter.md"));

        let Some(starter) = starter else {
            tracing::warn!(
                "orchestrator: cannot derive starter.md path for {feature_slug}"
            );
            return;
        };

        let socket = self.socket_path.clone();
        let name = feature_slug.to_string();
        let cwd = worktree.to_path_buf();
        let argv = vec![
            "claude".to_string(),
            format!("Read {} and follow it.", starter.display()),
        ];

        self.pending_session_cwds.push_back(cwd.clone());
        self.pending_shell_modes.push_back(false);
        self.pending_commands.push_back(Some(argv.clone()));

        let req = crate::ipc::protocol::Request::New {
            name: Some(name),
            mode: crate::session::mode::SessionMode::Listening,
            cwd: Some(cwd),
            command: Some(argv),
        };

        cx.background_executor()
            .spawn(async move {
                let _ = super::ipc_bridge::send_command(&socket, &req);
            })
            .detach();
    }
}
