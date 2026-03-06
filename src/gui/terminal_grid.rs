use gpui::*;

use super::app::ClaudioApp;
use super::session_state::SessionState;
use super::theme;

impl ClaudioApp {
    pub fn render_terminal_grid(
        &self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let visible: Vec<&SessionState> = self.sessions.iter().filter(|s| !s.minimized).collect();

        if visible.is_empty() {
            return div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .bg(rgb(theme::BASE))
                .child(
                    div()
                        .child("No sessions currently open.")
                        .text_color(rgb(theme::OVERLAY0))
                        .text_size(px(16.0)),
                )
                .into_any_element();
        }

        let n = visible.len();
        let cols = grid_cols(n);
        let rows = grid_rows(n);

        let mut grid = div().size_full().flex().flex_col().bg(rgb(theme::CRUST)).gap(px(2.0));

        for row_idx in 0..rows {
            let mut row = div().flex_1().flex().flex_row().gap(px(2.0));
            for col_idx in 0..cols {
                let idx = row_idx * cols + col_idx;
                if idx < n {
                    let session = visible[idx];
                    row = row.child(self.render_pane(session, cx));
                } else {
                    row = row.child(div().flex_1().bg(rgb(theme::CRUST)));
                }
            }
            grid = grid.child(row);
        }

        grid.into_any_element()
    }

    fn render_pane(&self, session: &SessionState, cx: &mut Context<Self>) -> impl IntoElement {
        let is_focused = self.focused_session_id.as_deref() == Some(&session.id);
        let border_color = if is_focused { theme::BLUE } else { theme::SURFACE1 };

        let mode_short = match session.mode.as_str() {
            "speaking" => "SPK",
            "listening" => "LSN",
            "muted" => "MUT",
            _ => "???",
        };

        let busy_text = if session.busy { " ..." } else { "" };
        let title = format!("{} [{}]{}", session.name, mode_short, busy_text);

        let session_id_focus = session.id.clone();
        let session_id_min = session.id.clone();
        let terminal_view = session.terminal_view.clone();

        div()
            .flex_1()
            .flex()
            .flex_col()
            .border_1()
            .border_color(rgb(border_color))
            .rounded(px(4.0))
            .overflow_hidden()
            .on_mouse_down(MouseButton::Left, cx.listener(move |app, _ev, window, cx| {
                app.focus_session_by_id(&session_id_focus.clone(), cx);
                let handle = terminal_view.read(cx).focus_handle().clone();
                window.focus(&handle);
            }))
            // Title bar
            .child(
                div()
                    .h(px(24.0))
                    .w_full()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .bg(rgb(theme::MANTLE))
                    .px(px(8.0))
                    .child(
                        div()
                            .child(title)
                            .text_color(rgb(if is_focused { theme::BLUE } else { theme::TEXT }))
                            .text_size(px(12.0)),
                    )
                    .child(
                        div()
                            .child("_")
                            .text_color(rgb(theme::OVERLAY0))
                            .text_size(px(12.0))
                            .cursor_pointer()
                            .px(px(4.0))
                            .on_mouse_down(MouseButton::Left, cx.listener(move |app, _ev, _window, cx| {
                                if let Some(s) = app.sessions.iter_mut().find(|s| s.id == session_id_min) {
                                    s.minimized = true;
                                }
                                cx.notify();
                            })),
                    ),
            )
            // Terminal view
            .child(
                div()
                    .flex_1()
                    .child(session.terminal_view.clone()),
            )
    }
}

/// Determine grid columns for N sessions.
pub fn grid_cols(n: usize) -> usize {
    match n {
        0 | 1 => 1,
        2 => 2,
        3 | 4 => 2,
        5 | 6 => 3,
        7..=9 => 3,
        _ => 4,
    }
}

/// Determine grid rows for N sessions.
pub fn grid_rows(n: usize) -> usize {
    if n == 0 {
        return 1;
    }
    let cols = grid_cols(n);
    (n + cols - 1) / cols
}
