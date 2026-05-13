use gpui::*;

use super::app::ClaudioApp;
use super::session_state::SessionState;
use super::theme;

impl ClaudioApp {
    pub fn render_status_bar(&self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut bar = div()
            .h(px(32.0))
            .w_full()
            .flex()
            .flex_row()
            .items_center()
            .bg(rgb(theme::MANTLE))
            .px(px(12.0))
            .gap(px(8.0));

        // Session pills
        for session in &self.sessions {
            bar = bar.child(self.render_pill(session, cx));
        }

        // New shell session button
        bar = bar.child(
            div()
                .id("new-shell-session")
                .child("+sh")
                .text_color(rgb(theme::OVERLAY0))
                .text_size(px(12.0))
                .px(px(6.0))
                .py(px(2.0))
                .rounded(px(8.0))
                .cursor_pointer()
                .hover(|s| s.text_color(rgb(theme::GREEN)).bg(rgb(theme::SURFACE0)))
                .on_mouse_down(MouseButton::Left, cx.listener(|app, _ev, _window, cx| {
                    app.create_shell_session(cx);
                })),
        );

        // Worktree naming input
        if self.pending_worktree_repo.is_some() {
            let input_text = format!("worktree: {}|", &self.worktree_name_input);
            bar = bar.child(
                div()
                    .px(px(8.0))
                    .py(px(2.0))
                    .rounded(px(8.0))
                    .bg(rgb(theme::SURFACE1))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(4.0))
                            .text_size(px(12.0))
                            .child(
                                div()
                                    .child(input_text)
                                    .text_color(rgb(theme::TEAL)),
                            ),
                    ),
            );
        }

        // Spacer / drag handle
        bar = bar.child(
            div()
                .flex_1()
                .h_full()
                .on_mouse_down(MouseButton::Left, |_ev, window, _cx| {
                    window.start_window_move();
                }),
        );

        // PTT indicator
        let ptt_color = if self.ptt_active { theme::RED } else { theme::OVERLAY0 };
        let ptt_text = if self.ptt_active { "PTT: ACTIVE" } else { "PTT: IDLE" };
        bar = bar.child(
            div()
                .child(ptt_text)
                .text_color(rgb(ptt_color))
                .text_size(px(12.0)),
        );

        // VAD indicator
        let vad_color = if self.vad_speaking { theme::GREEN } else { theme::OVERLAY0 };
        let vad_text = if self.vad_speaking { "VAD: SPEECH" } else { "VAD: ---" };
        bar = bar.child(
            div()
                .child(vad_text)
                .text_color(rgb(vad_color))
                .text_size(px(12.0)),
        );

        // Last transcription
        if let Some(ref text) = self.last_transcription {
            let display = if text.len() > 50 {
                format!("\"{}...\"", &text[..47])
            } else {
                format!("\"{text}\"")
            };
            bar = bar.child(
                div()
                    .child(display)
                    .text_color(rgb(theme::TEAL))
                    .text_size(px(12.0))
                    .max_w(px(300.0))
                    .overflow_x_hidden(),
            );
        }

        // Test notification button
        bar = bar.child(
            div()
                .id("test-notif")
                .child("bell")
                .text_color(rgb(theme::OVERLAY0))
                .text_size(px(12.0))
                .px(px(6.0))
                .cursor_pointer()
                .hover(|s| s.text_color(rgb(theme::TEAL)))
                .on_mouse_down(MouseButton::Left, cx.listener(|_app, _ev, _window, _cx| {
                    super::session_state::send_desktop_notification("claudio: test", "test notification");
                })),
        );

        // Restart daemon
        bar = bar.child(
            div()
                .id("restart-daemon")
                .child("\u{21bb}")
                .text_color(rgb(theme::OVERLAY0))
                .text_size(px(14.0))
                .px(px(6.0))
                .cursor_pointer()
                .hover(|s| s.text_color(rgb(theme::YELLOW)))
                .on_mouse_down(MouseButton::Left, cx.listener(|app, _ev, _window, cx| {
                    app.restart_daemon(cx);
                })),
        );

        // Close app
        bar = bar.child(
            div()
                .child("x")
                .text_color(rgb(theme::OVERLAY0))
                .text_size(px(14.0))
                .px(px(6.0))
                .cursor_pointer()
                .hover(|s| s.text_color(rgb(theme::RED)))
                .on_mouse_down(MouseButton::Left, cx.listener(|_app, _ev, _window, cx| {
                    cx.quit();
                })),
        );

        bar
    }

    fn render_pill(&self, session: &SessionState, cx: &mut Context<Self>) -> impl IntoElement {
        let is_focused = self.focused_session_id.as_deref() == Some(&session.id);
        let is_renaming = self.renaming_session_id.as_deref() == Some(&session.id);

        let icon = if session.busy {
            "*"
        } else {
            "o"
        };

        let bg = if is_focused { theme::BLUE } else { theme::SURFACE0 };
        let fg = if is_focused { theme::CRUST } else { theme::TEXT };

        let session_id = session.id.clone();
        let session_id_right = session.id.clone();
        let session_id_kill = session.id.clone();
        let session_id_editor = session.id.clone();
        let is_minimized = session.minimized;

        let name_element: AnyElement = if is_renaming {
            let input_text = format!("{}|", &self.rename_input);
            div()
                .child(input_text)
                .text_color(rgb(theme::TEXT))
                .text_size(px(12.0))
                .bg(rgb(theme::SURFACE1))
                .px(px(4.0))
                .rounded(px(4.0))
                .into_any_element()
        } else {
            div()
                .max_w(px(150.0))
                .overflow_x_hidden()
                .child(session.name.clone())
                .into_any_element()
        };

        div()
            .px(px(8.0))
            .py(px(2.0))
            .rounded(px(8.0))
            .bg(rgb(bg))
            .cursor_pointer()
            .on_mouse_down(MouseButton::Left, cx.listener(move |app, _ev, _window, cx| {
                let id = session_id.clone();
                if is_minimized
                    && let Some(s) = app.sessions.iter_mut().find(|s| s.id == id) {
                        s.minimized = false;
                    }
                app.focus_session_by_id(&id, cx);
            }))
            .on_mouse_down(MouseButton::Right, cx.listener(move |app, _ev: &MouseDownEvent, _window, cx| {
                app.start_rename(session_id_right.clone(), cx);
            }))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(4.0))
                    .text_color(rgb(fg))
                    .text_size(px(12.0))
                    .child(icon)
                    .child(name_element)
                    .child(
                        div()
                            .child("\u{270e}")
                            .text_size(px(11.0))
                            .text_color(rgb(theme::OVERLAY0))
                            .pl(px(4.0))
                            .cursor_pointer()
                            .hover(|s| s.text_color(rgb(theme::BLUE)))
                            .on_mouse_down(MouseButton::Left, cx.listener(move |app, _ev, _window, _cx| {
                                if let Some(session) = app.sessions.iter().find(|s| s.id == session_id_editor)
                                    && let Some(ref cwd) = session.cwd {
                                        app.open_in_editor(cwd);
                                    }
                            })),
                    )
                    .child(
                        div()
                            .child("x")
                            .text_size(px(11.0))
                            .text_color(rgb(theme::OVERLAY0))
                            .pl(px(4.0))
                            .cursor_pointer()
                            .hover(|s| s.text_color(rgb(theme::RED)))
                            .on_mouse_down(MouseButton::Left, cx.listener(move |app, _ev, _window, cx| {
                                app.kill_session(&session_id_kill, cx);
                            })),
                    ),
            )
    }
}
