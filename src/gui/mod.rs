mod activity_bar;
mod app;
mod file_tree;
mod ipc_bridge;
pub mod orchestrator_index;
mod session_state;
mod status_bar;
mod terminal_grid;
mod actions;
mod theme;

use std::path::Path;

use anyhow::Result;
use gpui;
use gpui::AppContext;

use self::app::ClaudioApp;

pub fn run(socket_path: &Path) -> Result<()> {
    let socket = socket_path.to_path_buf();

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
            |window, cx| cx.new(|cx| ClaudioApp::new(&socket, window, cx)),
        )
        .expect("Failed to open window");
    });

    Ok(())
}
