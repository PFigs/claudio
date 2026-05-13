mod activity_bar;
mod app;
mod file_tree;
mod ipc_bridge;
pub mod orchestrator_index;
mod orchestrator_sidebar;
mod session_state;
mod status_bar;
mod terminal_grid;
mod actions;
mod theme;

use std::path::Path;

use anyhow::Result;
use gpui::AppContext;

use self::app::ClaudioApp;

pub fn run(socket_path: &Path) -> Result<()> {
    let socket = socket_path.to_path_buf();

    force_x11_on_gnome();

    let app = gpui::Application::new();
    app.run(move |cx: &mut gpui::App| {
        actions::register(cx);

        cx.open_window(
            gpui::WindowOptions {
                titlebar: Some(gpui::TitlebarOptions {
                    title: Some("claudio".into()),
                    ..Default::default()
                }),
                window_min_size: Some(gpui::size(gpui::px(800.0), gpui::px(600.0))),
                window_decorations: Some(gpui::WindowDecorations::Server),
                app_id: Some("claudio".into()),
                ..Default::default()
            },
            |window, cx| {
                cx.new(|cx| {
                    let app = ClaudioApp::new(&socket, window, cx);
                    app.on_mount(cx);
                    app
                })
            },
        )
        .expect("Failed to open window");
    });

    Ok(())
}

// GNOME/Mutter refuses xdg-decoration server-side mode and GPUI does not draw
// its own CSD, leaving the window chromeless. Routing through XWayland gets
// SSD from mutter-x11-frames. Other Wayland compositors honor SSD natively.
fn force_x11_on_gnome() {
    if std::env::var_os("CLAUDIO_FORCE_WAYLAND").is_some() {
        return;
    }
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    let on_gnome = desktop.split(':').any(|s| s.eq_ignore_ascii_case("GNOME"));
    let on_wayland = std::env::var_os("WAYLAND_DISPLAY")
        .map(|v| !v.is_empty())
        .unwrap_or(false);
    let has_x11 = std::env::var_os("DISPLAY")
        .map(|v| !v.is_empty())
        .unwrap_or(false);
    if on_gnome && on_wayland && has_x11 {
        // SAFETY: called before gpui (and therefore any threads) start.
        unsafe { std::env::remove_var("WAYLAND_DISPLAY") };
    }
}
