use std::sync::atomic::Ordering;

use gpui::*;

use super::app::ClaudioApp;
use super::session_state::SessionState;
use super::theme;

pub enum ResizeAxis {
    Column(usize),
    Row(usize),
}

pub struct GridResize {
    pub axis: ResizeAxis,
    pub start_position: f32,
    pub start_ratios: (f32, f32),
}

fn div_with_flex_grow(ratio: f32) -> Div {
    let mut d = div();
    d.style().flex_grow = Some(ratio);
    d.style().flex_shrink = Some(1.);
    d.style().flex_basis = Some(relative(0.).into());
    d.overflow_hidden()
}

impl ClaudioApp {
    pub fn ensure_grid_ratios(&mut self, rows: usize, cols: usize) {
        if self.grid_col_ratios.len() != cols {
            self.grid_col_ratios = vec![1.0; cols];
        }
        if self.grid_row_ratios.len() != rows {
            self.grid_row_ratios = vec![1.0; rows];
        }
    }

    pub fn render_terminal_grid(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let n = self.sessions.iter().filter(|s| !s.minimized).count();

        if n == 0 {
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

        let cols = grid_cols(n);
        let rows = grid_rows(n);

        self.ensure_grid_ratios(rows, cols);

        let visible: Vec<&SessionState> = self.sessions.iter().filter(|s| !s.minimized).collect();

        let mut grid = div().size_full().flex().flex_col().bg(rgb(theme::CRUST));

        for row_idx in 0..rows {
            if row_idx > 0 {
                grid = grid.child(self.render_row_divider(row_idx - 1, cx));
            }

            let mut row = div_with_flex_grow(self.grid_row_ratios[row_idx])
                .flex()
                .flex_row();

            for col_idx in 0..cols {
                if col_idx > 0 {
                    row = row.child(self.render_col_divider(col_idx - 1, cx));
                }

                let idx = row_idx * cols + col_idx;
                if idx < n {
                    let session = visible[idx];
                    row = row.child(
                        div_with_flex_grow(self.grid_col_ratios[col_idx])
                            .child(self.render_pane(session, cx)),
                    );
                } else {
                    row = row.child(
                        div_with_flex_grow(self.grid_col_ratios[col_idx]),
                    );
                }
            }
            grid = grid.child(row);
        }

        grid.into_any_element()
    }

    fn render_col_divider(&self, idx: usize, cx: &mut Context<Self>) -> impl IntoElement {
        let is_active = matches!(&self.grid_resize, Some(r) if matches!(r.axis, ResizeAxis::Column(i) if i == idx));
        let bg = if is_active { theme::BLUE } else { theme::CRUST };

        let divider_idx = idx;
        div()
            .w(px(4.0))
            .h_full()
            .flex_shrink_0()
            .cursor_col_resize()
            .bg(rgb(bg))
            .hover(|s| s.bg(rgb(theme::BLUE)))
            .on_mouse_down(MouseButton::Left, cx.listener(move |app, ev: &MouseDownEvent, _window, cx| {
                if app.grid_col_ratios.len() > divider_idx + 1 {
                    if ev.click_count == 2 {
                        app.grid_col_ratios[divider_idx] = 1.0;
                        app.grid_col_ratios[divider_idx + 1] = 1.0;
                        cx.notify();
                    } else {
                        app.grid_resize = Some(GridResize {
                            axis: ResizeAxis::Column(divider_idx),
                            start_position: ev.position.x.into(),
                            start_ratios: (
                                app.grid_col_ratios[divider_idx],
                                app.grid_col_ratios[divider_idx + 1],
                            ),
                        });
                    }
                }
            }))
    }

    fn render_row_divider(&self, idx: usize, cx: &mut Context<Self>) -> impl IntoElement {
        let is_active = matches!(&self.grid_resize, Some(r) if matches!(r.axis, ResizeAxis::Row(i) if i == idx));
        let bg = if is_active { theme::BLUE } else { theme::CRUST };

        let divider_idx = idx;
        div()
            .h(px(4.0))
            .w_full()
            .flex_shrink_0()
            .cursor_ns_resize()
            .bg(rgb(bg))
            .hover(|s| s.bg(rgb(theme::BLUE)))
            .on_mouse_down(MouseButton::Left, cx.listener(move |app, ev: &MouseDownEvent, _window, cx| {
                if app.grid_row_ratios.len() > divider_idx + 1 {
                    if ev.click_count == 2 {
                        app.grid_row_ratios[divider_idx] = 1.0;
                        app.grid_row_ratios[divider_idx + 1] = 1.0;
                        cx.notify();
                    } else {
                        app.grid_resize = Some(GridResize {
                            axis: ResizeAxis::Row(divider_idx),
                            start_position: ev.position.y.into(),
                            start_ratios: (
                                app.grid_row_ratios[divider_idx],
                                app.grid_row_ratios[divider_idx + 1],
                            ),
                        });
                    }
                }
            }))
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
        let session_id_ap = session.id.clone();
        let terminal_view = session.terminal_view.clone();

        let autopilot_on = session.autopilot.load(Ordering::Relaxed);
        let autopilot_clone = session.autopilot.clone();
        let ap_label = if autopilot_on { "AP" } else { "ap" };
        let ap_color = if autopilot_on { theme::GREEN } else { theme::OVERLAY0 };

        div()
            .size_full()
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
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(4.0))
                            // Autopilot toggle
                            .child(
                                div()
                                    .id(SharedString::from(format!("ap-{}", session_id_ap)))
                                    .child(ap_label)
                                    .text_color(rgb(ap_color))
                                    .text_size(px(12.0))
                                    .cursor_pointer()
                                    .px(px(4.0))
                                    .hover(|s| s.text_color(rgb(theme::GREEN)))
                                    .on_mouse_down(MouseButton::Left, cx.listener(move |_app, _ev, _window, cx| {
                                        let prev = autopilot_clone.load(Ordering::Relaxed);
                                        autopilot_clone.store(!prev, Ordering::Relaxed);
                                        cx.notify();
                                    })),
                            )
                            // Minimize button
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
    n.div_ceil(cols)
}
