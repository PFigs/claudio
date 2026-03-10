use std::collections::HashSet;
use std::path::PathBuf;

use gpui::*;

use super::app::ClaudioApp;
use super::theme;

struct Tooltip(SharedString);

impl Render for Tooltip {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .child(self.0.clone())
            .text_size(px(12.0))
            .text_color(rgb(theme::TEXT))
            .bg(rgb(theme::SURFACE0))
            .px(px(8.0))
            .py(px(4.0))
            .rounded(px(4.0))
    }
}

fn build_tooltip(text: &'static str) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    move |_window, cx| cx.new(|_| Tooltip(text.into())).into()
}

pub struct FileTree {
    pub roots: Vec<PathBuf>,
    pub expanded: HashSet<PathBuf>,
    pub visible: bool,
    pub menu_open: Option<PathBuf>,
}

impl FileTree {
    pub fn new() -> Self {
        Self {
            roots: Vec::new(),
            expanded: HashSet::new(),
            visible: true,
            menu_open: None,
        }
    }

    pub fn toggle_visible(&mut self) {
        self.visible = !self.visible;
    }

    pub fn toggle_dir(&mut self, path: &PathBuf) {
        if self.expanded.contains(path) {
            self.expanded.remove(path);
        } else {
            self.expanded.insert(path.clone());
        }
    }

    pub fn add_root(&mut self, path: PathBuf) {
        if !self.roots.contains(&path) {
            self.roots.push(path);
            self.roots.sort();
        }
    }

    pub fn remove_root(&mut self, path: &PathBuf) {
        self.roots.retain(|r| r != path);
        self.expanded.retain(|p| !p.starts_with(path));
    }

    pub fn list_children(dir: &PathBuf) -> (Vec<PathBuf>, Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return (Vec::new(), Vec::new());
        };
        let mut dirs = Vec::new();
        let mut files = Vec::new();
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            let hidden = path
                .file_name()
                .map(|n| n.to_string_lossy().starts_with('.'))
                .unwrap_or(true);
            if hidden {
                continue;
            }
            if path.is_dir() {
                dirs.push(path);
            } else {
                files.push(path);
            }
        }
        dirs.sort();
        files.sort();
        (dirs, files)
    }
}

impl ClaudioApp {
    pub fn render_file_tree(&self, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let mut container = div()
            .id("file-tree")
            .w(px(self.file_tree_width))
            .h_full()
            .flex()
            .flex_col()
            .bg(rgb(theme::MANTLE))
            .overflow_y_scroll()
            .child(
                div()
                    .px(px(8.0))
                    .py(px(6.0))
                    .flex()
                    .flex_row()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .child("Folders")
                            .text_color(rgb(theme::OVERLAY0))
                            .text_size(px(12.0)),
                    )
                    .child(
                        div()
                            .child("+ Add")
                            .text_color(rgb(theme::OVERLAY0))
                            .text_size(px(12.0))
                            .cursor_pointer()
                            .hover(|s| s.text_color(rgb(theme::GREEN)))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|app, _ev: &MouseDownEvent, _window, cx| {
                                    app.open_add_folder_dialog(cx);
                                }),
                            ),
                    ),
            );

        if self.file_tree.roots.is_empty() {
            container = container.child(
                div()
                    .px(px(8.0))
                    .py(px(16.0))
                    .child("No folders added")
                    .text_color(rgb(theme::OVERLAY0))
                    .text_size(px(12.0)),
            );
        } else {
            for root in &self.file_tree.roots {
                container = container.child(self.render_root_node(root, cx));
            }
        }

        container.into_any_element()
    }

    fn render_root_node(&self, root: &PathBuf, cx: &mut Context<Self>) -> AnyElement {
        let is_expanded = self.file_tree.expanded.contains(root);
        let menu_open = self.file_tree.menu_open.as_ref() == Some(root);
        let name = root
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| root.to_string_lossy().to_string());

        let arrow = if is_expanded { "\u{25be} " } else { "\u{25b8} " };
        let label = format!("{arrow}{name}");

        let path_toggle = root.clone();
        let path_open = root.clone();
        let path_menu = root.clone();

        let label_id: SharedString = format!("label-{}", name).into();
        let mut node = div()
            .px(px(8.0))
            .py(px(2.0))
            .w_full()
            .flex()
            .flex_row()
            .items_center()
            .hover(|s| s.bg(rgb(theme::SURFACE0)))
            .child(
                div()
                    .id(label_id)
                    .flex_1()
                    .min_w(px(0.0))
                    .overflow_x_hidden()
                    .cursor_pointer()
                    .child(label)
                    .text_color(rgb(theme::TEXT))
                    .text_size(px(13.0))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |app, ev: &MouseDownEvent, _window, cx| {
                            if ev.click_count >= 2 {
                                app.new_session_in_dir(path_open.clone(), cx);
                            } else {
                                app.file_tree.toggle_dir(&path_toggle);
                                cx.notify();
                            }
                        }),
                    ),
            );

        // Hamburger menu toggle
        let hamburger_id: SharedString = format!("hamburger-{}", name).into();
        node = node.child(
            div()
                .id(hamburger_id)
                .child("\u{2261}") // hamburger icon
                .text_size(px(14.0))
                .text_color(if menu_open { rgb(theme::TEXT) } else { rgb(theme::OVERLAY0) })
                .px(px(4.0))
                .cursor_pointer()
                .hover(|s| s.text_color(rgb(theme::TEAL)))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |app, _ev: &MouseDownEvent, _window, cx| {
                        if app.file_tree.menu_open.as_ref() == Some(&path_menu) {
                            app.file_tree.menu_open = None;
                        } else {
                            app.file_tree.menu_open = Some(path_menu.clone());
                        }
                        cx.notify();
                    }),
                ),
        );

        let mut wrapper = div().flex().flex_col().child(node);

        // Action buttons row (shown when hamburger menu is open)
        if menu_open {
            let path_plus = root.clone();
            let path_editor = root.clone();
            let path_remove = root.clone();

            let btn_new_id: SharedString = format!("btn-new-{}", name).into();
            let btn_ed_id: SharedString = format!("btn-ed-{}", name).into();
            let btn_rm_id: SharedString = format!("btn-rm-{}", name).into();

            let mut actions = div()
                .px(px(20.0))
                .py(px(2.0))
                .flex()
                .flex_row()
                .gap(px(4.0))
                .child(
                    div()
                        .id(btn_new_id)
                        .child(">_ new")
                        .text_color(rgb(theme::OVERLAY0))
                        .text_size(px(12.0))
                        .px(px(4.0))
                        .cursor_pointer()
                        .hover(|s| s.text_color(rgb(theme::GREEN)))
                        .tooltip(build_tooltip("New session in this folder"))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |app, _ev: &MouseDownEvent, _window, cx| {
                                app.new_session_in_dir(path_plus.clone(), cx);
                                app.file_tree.menu_open = None;
                            }),
                        ),
                );

            // Git worktree session button (only if this is a git repo)
            if root.join(".git").exists() {
                let path_wt = root.clone();
                let btn_wt_id: SharedString = format!("btn-wt-{}", name).into();
                actions = actions.child(
                    div()
                        .id(btn_wt_id)
                        .child("\u{2387} worktree")
                        .text_color(rgb(theme::OVERLAY0))
                        .text_size(px(12.0))
                        .px(px(4.0))
                        .cursor_pointer()
                        .hover(|s| s.text_color(rgb(theme::TEAL)))
                        .tooltip(build_tooltip("New worktree session"))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |app, _ev: &MouseDownEvent, _window, cx| {
                                app.start_worktree_naming(path_wt.clone(), cx);
                                app.file_tree.menu_open = None;
                            }),
                        ),
                );
            }

            actions = actions
                .child(
                    div()
                        .id(btn_ed_id)
                        .child("\u{270e} edit")
                        .text_size(px(12.0))
                        .text_color(rgb(theme::OVERLAY0))
                        .px(px(4.0))
                        .cursor_pointer()
                        .hover(|s| s.text_color(rgb(theme::BLUE)))
                        .tooltip(build_tooltip("Open in editor"))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |app, _ev: &MouseDownEvent, _window, _cx| {
                                app.open_in_editor(&path_editor);
                                app.file_tree.menu_open = None;
                            }),
                        ),
                )
                .child(
                    div()
                        .id(btn_rm_id)
                        .child("\u{00d7} remove")
                        .text_size(px(12.0))
                        .text_color(rgb(theme::OVERLAY0))
                        .px(px(4.0))
                        .cursor_pointer()
                        .hover(|s| s.text_color(rgb(theme::RED)))
                        .tooltip(build_tooltip("Remove folder"))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |app, _ev: &MouseDownEvent, _window, cx| {
                                app.file_tree.remove_root(&path_remove);
                                app.file_tree.menu_open = None;
                                app.save_state();
                                cx.notify();
                            }),
                        ),
                );

            wrapper = wrapper.child(actions);
        }

        if is_expanded {
            let (dirs, files) = FileTree::list_children(root);
            for child in dirs {
                wrapper = wrapper.child(self.render_tree_node(&child, 1, cx));
            }
            for file in files {
                wrapper = wrapper.child(self.render_file_node(&file, 1, cx));
            }
        }

        wrapper.into_any_element()
    }

    fn render_tree_node(
        &self,
        path: &PathBuf,
        depth: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let is_expanded = self.file_tree.expanded.contains(path);
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string_lossy().to_string());

        let arrow = if is_expanded { "\u{25be} " } else { "\u{25b8} " };
        let label = format!("{arrow}{name}");

        let path_toggle = path.clone();
        let path_open = path.clone();

        let node = div()
            .px(px(8.0 + depth as f32 * 16.0))
            .py(px(2.0))
            .w_full()
            .flex()
            .flex_row()
            .items_center()
            .cursor_pointer()
            .hover(|s| s.bg(rgb(theme::SURFACE0)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |app, ev: &MouseDownEvent, _window, cx| {
                    if ev.click_count >= 2 {
                        app.new_session_in_dir(path_open.clone(), cx);
                    } else {
                        app.file_tree.toggle_dir(&path_toggle);
                        cx.notify();
                    }
                }),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .overflow_x_hidden()
                    .child(label)
                    .text_color(rgb(theme::TEXT))
                    .text_size(px(13.0)),
            );

        let mut wrapper = div().flex().flex_col().child(node);

        if is_expanded {
            let (dirs, files) = FileTree::list_children(path);
            for child in dirs {
                wrapper = wrapper.child(self.render_tree_node(&child, depth + 1, cx));
            }
            for file in files {
                wrapper = wrapper.child(self.render_file_node(&file, depth + 1, cx));
            }
        }

        wrapper.into_any_element()
    }

    fn render_file_node(
        &self,
        path: &PathBuf,
        depth: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string_lossy().to_string());

        let path_inject = path.clone();

        div()
            .px(px(8.0 + depth as f32 * 16.0))
            .py(px(1.0))
            .w_full()
            .flex()
            .flex_row()
            .items_center()
            .cursor_pointer()
            .hover(|s| s.bg(rgb(theme::SURFACE0)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |app, _ev: &MouseDownEvent, _window, _cx| {
                    let text = format!("@{} ", path_inject.to_string_lossy());
                    app.inject_text_to_focused(&text);
                }),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .overflow_x_hidden()
                    .child(format!("  {name}"))
                    .text_color(rgb(theme::OVERLAY0))
                    .text_size(px(12.0)),
            )
            .into_any_element()
    }
}
