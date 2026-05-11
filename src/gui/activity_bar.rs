use gpui::*;

use super::app::ClaudioApp;
use super::theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Activity {
    Files,
    Orchestrator,
}

impl Activity {
    pub fn label(self) -> &'static str {
        match self {
            Self::Files => "Files",
            Self::Orchestrator => "Orchestrator",
        }
    }

    /// Single-character icon. ASCII-friendly to avoid font fallback paths.
    pub fn icon(self) -> &'static str {
        match self {
            Self::Files => "F",
            Self::Orchestrator => "O",
        }
    }

    pub const ALL: [Activity; 2] = [Activity::Files, Activity::Orchestrator];
}

impl ClaudioApp {
    pub fn render_activity_bar(
        &self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let mut bar = div()
            .w(px(48.0))
            .h_full()
            .flex()
            .flex_col()
            .items_center()
            .py(px(8.0))
            .gap(px(4.0))
            .bg(rgb(theme::CRUST));

        for activity in Activity::ALL {
            let is_active = self.active_activity == activity;
            let bg = if is_active { theme::SURFACE0 } else { theme::CRUST };
            let fg = if is_active { theme::BLUE } else { theme::OVERLAY0 };
            let id: SharedString = format!("activity-{}", activity.label()).into();

            bar = bar.child(
                div()
                    .id(id)
                    .w(px(40.0))
                    .h(px(40.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(4.0))
                    .bg(rgb(bg))
                    .text_color(rgb(fg))
                    .text_size(px(16.0))
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(theme::SURFACE0)))
                    .child(activity.icon())
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |app, _ev: &MouseDownEvent, _window, cx| {
                            app.set_active_activity(activity, cx);
                        }),
                    ),
            );
        }

        bar
    }
}
