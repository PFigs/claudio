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

const SKIP_DIRS: &[&str] = &[
    "node_modules",
    "target",
    "__pycache__",
    ".venv",
    "venv",
    "dist",
    "build",
    ".next",
    ".cache",
    ".git",
];

pub struct FileTree {
    pub roots: Vec<PathBuf>,
    pub expanded: HashSet<PathBuf>,
    pub visible: bool,
    pub search_query: String,
    pub search_active: bool,
    pub show_files: bool,
    pub show_folders: bool,
}

impl FileTree {
    pub fn new() -> Self {
        Self {
            roots: Vec::new(),
            expanded: HashSet::new(),
            visible: true,
            search_query: String::new(),
            search_active: false,
            show_files: true,
            show_folders: true,
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
        let header = div()
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
            );

        let search_bar = self.render_file_tree_search(cx);

        let mut content = div()
            .id("file-tree-content")
            .flex_1()
            .min_h(px(0.0))
            .overflow_y_scroll();

        if self.file_tree.roots.is_empty() {
            content = content.child(
                div()
                    .px(px(8.0))
                    .py(px(16.0))
                    .child("No folders added")
                    .text_color(rgb(theme::OVERLAY0))
                    .text_size(px(12.0)),
            );
        } else {
            let query = self.file_tree.search_query.trim();
            if query.is_empty() {
                for root in &self.file_tree.roots {
                    content = content.child(self.render_root_node(root, cx));
                }
            } else {
                let query_lower = query.to_lowercase();
                let mut has_results = false;
                for root in &self.file_tree.roots {
                    if let Some(el) = self.render_filtered_root_node(root, &query_lower, cx) {
                        content = content.child(el);
                        has_results = true;
                    }
                }
                if !has_results {
                    content = content.child(
                        div()
                            .px(px(8.0))
                            .py(px(16.0))
                            .child("No matches")
                            .text_color(rgb(theme::OVERLAY0))
                            .text_size(px(12.0)),
                    );
                }
            }
        }

        div()
            .id("file-tree")
            .w(px(self.file_tree_width))
            .h_full()
            .flex()
            .flex_col()
            .bg(rgb(theme::MANTLE))
            .child(header)
            .child(search_bar)
            .child(content)
            .into_any_element()
    }

    fn render_root_node(&self, root: &PathBuf, cx: &mut Context<Self>) -> AnyElement {
        let is_expanded = self.file_tree.expanded.contains(root);
        let name = root
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| root.to_string_lossy().to_string());

        let arrow = if is_expanded { "\u{25be} " } else { "\u{25b8} " };
        let label = format!("{arrow}{name}");

        let path_toggle = root.clone();
        let path_open = root.clone();
        let path_plus = root.clone();
        let path_editor = root.clone();
        let path_remove = root.clone();

        let label_id: SharedString = format!("label-{}", name).into();
        let btn_new_id: SharedString = format!("btn-new-{}", name).into();
        let btn_ed_id: SharedString = format!("btn-ed-{}", name).into();
        let btn_rm_id: SharedString = format!("btn-rm-{}", name).into();

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
            )
            .child(
                div()
                    .id(btn_new_id)
                    .child(">_")
                    .text_size(px(11.0))
                    .text_color(rgb(theme::OVERLAY0))
                    .px(px(3.0))
                    .cursor_pointer()
                    .hover(|s| s.text_color(rgb(theme::GREEN)))
                    .tooltip(build_tooltip("New session in this folder"))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |app, _ev: &MouseDownEvent, _window, cx| {
                            app.new_session_in_dir(path_plus.clone(), cx);
                        }),
                    ),
            );

        // Git worktree session button (only if this is a git repo)
        if root.join(".git").exists() {
            let path_wt = root.clone();
            let btn_wt_id: SharedString = format!("btn-wt-{}", name).into();
            node = node.child(
                div()
                    .id(btn_wt_id)
                    .child("\u{2387}")
                    .text_size(px(11.0))
                    .text_color(rgb(theme::OVERLAY0))
                    .px(px(3.0))
                    .cursor_pointer()
                    .hover(|s| s.text_color(rgb(theme::TEAL)))
                    .tooltip(build_tooltip("New worktree session"))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |app, _ev: &MouseDownEvent, _window, cx| {
                            app.start_worktree_naming(path_wt.clone(), cx);
                        }),
                    ),
            );
        }

        node = node
            .child(
                div()
                    .id(btn_ed_id)
                    .child("\u{270e}")
                    .text_size(px(11.0))
                    .text_color(rgb(theme::OVERLAY0))
                    .px(px(3.0))
                    .cursor_pointer()
                    .hover(|s| s.text_color(rgb(theme::BLUE)))
                    .tooltip(build_tooltip("Open in editor"))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |app, _ev: &MouseDownEvent, _window, _cx| {
                            app.open_in_editor(&path_editor);
                        }),
                    ),
            )
            .child(
                div()
                    .id(btn_rm_id)
                    .child("\u{00d7}")
                    .text_size(px(11.0))
                    .text_color(rgb(theme::OVERLAY0))
                    .px(px(3.0))
                    .cursor_pointer()
                    .hover(|s| s.text_color(rgb(theme::RED)))
                    .tooltip(build_tooltip("Remove folder"))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |app, _ev: &MouseDownEvent, _window, cx| {
                            app.file_tree.remove_root(&path_remove);
                            app.save_state();
                            cx.notify();
                        }),
                    ),
            );

        let mut wrapper = div().flex().flex_col().child(node);

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
        let path_inject = path.clone();
        let path_editor = path.clone();

        let inject_id: SharedString = format!("inject-{}", path.display()).into();
        let edit_id: SharedString = format!("edit-{}", path.display()).into();

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
            )
            .child(
                div()
                    .id(inject_id)
                    .child("@")
                    .text_size(px(11.0))
                    .text_color(rgb(theme::OVERLAY0))
                    .px(px(3.0))
                    .cursor_pointer()
                    .hover(|s| s.text_color(rgb(theme::GREEN)))
                    .tooltip(build_tooltip("Inject folder path"))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |app, _ev: &MouseDownEvent, _window, _cx| {
                            let text = format!("@{} ", path_inject.to_string_lossy());
                            app.inject_text_to_focused(&text);
                        }),
                    ),
            )
            .child(
                div()
                    .id(edit_id)
                    .child("\u{270e}")
                    .text_size(px(11.0))
                    .text_color(rgb(theme::OVERLAY0))
                    .px(px(3.0))
                    .cursor_pointer()
                    .hover(|s| s.text_color(rgb(theme::BLUE)))
                    .tooltip(build_tooltip("Open in editor"))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |app, _ev: &MouseDownEvent, _window, _cx| {
                            app.open_in_editor(&path_editor);
                        }),
                    ),
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
        let path_editor = path.clone();

        let inject_id: SharedString = format!("inject-{}", path.display()).into();
        let edit_id: SharedString = format!("edit-{}", path.display()).into();

        div()
            .px(px(8.0 + depth as f32 * 16.0))
            .py(px(1.0))
            .w_full()
            .flex()
            .flex_row()
            .items_center()
            .hover(|s| s.bg(rgb(theme::SURFACE0)))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .overflow_x_hidden()
                    .child(format!("  {name}"))
                    .text_color(rgb(theme::OVERLAY0))
                    .text_size(px(12.0)),
            )
            .child(
                div()
                    .id(inject_id)
                    .child("@")
                    .text_size(px(11.0))
                    .text_color(rgb(theme::OVERLAY0))
                    .px(px(3.0))
                    .cursor_pointer()
                    .hover(|s| s.text_color(rgb(theme::GREEN)))
                    .tooltip(build_tooltip("Inject file path"))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |app, _ev: &MouseDownEvent, _window, _cx| {
                            let text = format!("@{} ", path_inject.to_string_lossy());
                            app.inject_text_to_focused(&text);
                        }),
                    ),
            )
            .child(
                div()
                    .id(edit_id)
                    .child("\u{270e}")
                    .text_size(px(11.0))
                    .text_color(rgb(theme::OVERLAY0))
                    .px(px(3.0))
                    .cursor_pointer()
                    .hover(|s| s.text_color(rgb(theme::BLUE)))
                    .tooltip(build_tooltip("Open in editor"))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |app, _ev: &MouseDownEvent, _window, _cx| {
                            app.open_in_editor(&path_editor);
                        }),
                    ),
            )
            .into_any_element()
    }

    fn render_file_tree_search(&self, cx: &mut Context<Self>) -> AnyElement {
        let query = &self.file_tree.search_query;
        let is_active = self.file_tree.search_active;

        let display_text: SharedString = if query.is_empty() {
            if is_active {
                "|".into()
            } else {
                "Search...".into()
            }
        } else if is_active {
            format!("{}|", query).into()
        } else {
            query.clone().into()
        };

        let text_color = if query.is_empty() && !is_active {
            rgb(theme::OVERLAY0)
        } else {
            rgb(theme::TEXT)
        };

        let border_color = if is_active {
            rgb(theme::BLUE)
        } else {
            rgb(theme::SURFACE0)
        };

        let dir_color = if self.file_tree.show_folders {
            rgb(theme::GREEN)
        } else {
            rgb(theme::OVERLAY0)
        };

        let file_color = if self.file_tree.show_files {
            rgb(theme::GREEN)
        } else {
            rgb(theme::OVERLAY0)
        };

        let mut row = div()
            .px(px(8.0))
            .py(px(4.0))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(4.0))
            .child(
                div()
                    .id("file-tree-search-input")
                    .flex_1()
                    .min_w(px(0.0))
                    .px(px(6.0))
                    .py(px(2.0))
                    .rounded(px(3.0))
                    .border_1()
                    .border_color(border_color)
                    .bg(rgb(theme::BASE))
                    .overflow_x_hidden()
                    .child(display_text)
                    .text_color(text_color)
                    .text_size(px(12.0))
                    .cursor_text()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|app, _ev: &MouseDownEvent, _window, cx| {
                            app.file_tree.search_active = true;
                            cx.notify();
                        }),
                    ),
            );

        if !query.is_empty() {
            row = row.child(
                div()
                    .id("file-tree-search-clear")
                    .child("\u{00d7}")
                    .text_size(px(13.0))
                    .text_color(rgb(theme::OVERLAY0))
                    .px(px(2.0))
                    .cursor_pointer()
                    .hover(|s| s.text_color(rgb(theme::RED)))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|app, _ev: &MouseDownEvent, _window, cx| {
                            app.file_tree.search_query.clear();
                            app.file_tree.search_active = false;
                            cx.notify();
                        }),
                    ),
            );
        }

        row = row
            .child(
                div()
                    .id("file-tree-toggle-dirs")
                    .child("D")
                    .text_size(px(11.0))
                    .text_color(dir_color)
                    .px(px(4.0))
                    .py(px(2.0))
                    .rounded(px(2.0))
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(theme::SURFACE0)))
                    .tooltip(build_tooltip("Toggle folders"))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|app, _ev: &MouseDownEvent, _window, cx| {
                            app.file_tree.show_folders = !app.file_tree.show_folders;
                            cx.notify();
                        }),
                    ),
            )
            .child(
                div()
                    .id("file-tree-toggle-files")
                    .child("F")
                    .text_size(px(11.0))
                    .text_color(file_color)
                    .px(px(4.0))
                    .py(px(2.0))
                    .rounded(px(2.0))
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(theme::SURFACE0)))
                    .tooltip(build_tooltip("Toggle files"))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|app, _ev: &MouseDownEvent, _window, cx| {
                            app.file_tree.show_files = !app.file_tree.show_files;
                            cx.notify();
                        }),
                    ),
            );

        row.into_any_element()
    }

    fn render_filtered_root_node(
        &self,
        root: &PathBuf,
        query_lower: &str,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let name = root
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| root.to_string_lossy().to_string());

        let name_matches =
            self.file_tree.show_folders && name.to_lowercase().contains(query_lower);

        let children = self.collect_filtered_children(root, query_lower, 0, cx);

        if !name_matches && children.is_empty() {
            return None;
        }

        let label = format!("\u{25be} {name}");
        let path_open = root.clone();
        let path_plus = root.clone();
        let path_remove = root.clone();

        let label_id: SharedString = format!("search-label-{}", name).into();
        let btn_new_id: SharedString = format!("search-new-{}", name).into();
        let btn_rm_id: SharedString = format!("search-rm-{}", name).into();

        let text_color = if name_matches {
            rgb(theme::YELLOW)
        } else {
            rgb(theme::TEXT)
        };

        let node = div()
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
                    .text_color(text_color)
                    .text_size(px(13.0))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |app, ev: &MouseDownEvent, _window, cx| {
                            if ev.click_count >= 2 {
                                app.new_session_in_dir(path_open.clone(), cx);
                            }
                        }),
                    ),
            )
            .child(
                div()
                    .id(btn_new_id)
                    .child(">_")
                    .text_size(px(11.0))
                    .text_color(rgb(theme::OVERLAY0))
                    .px(px(3.0))
                    .cursor_pointer()
                    .hover(|s| s.text_color(rgb(theme::GREEN)))
                    .tooltip(build_tooltip("New session in this folder"))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |app, _ev: &MouseDownEvent, _window, cx| {
                            app.new_session_in_dir(path_plus.clone(), cx);
                        }),
                    ),
            )
            .child(
                div()
                    .id(btn_rm_id)
                    .child("\u{00d7}")
                    .text_size(px(11.0))
                    .text_color(rgb(theme::OVERLAY0))
                    .px(px(3.0))
                    .cursor_pointer()
                    .hover(|s| s.text_color(rgb(theme::RED)))
                    .tooltip(build_tooltip("Remove folder"))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |app, _ev: &MouseDownEvent, _window, cx| {
                            app.file_tree.remove_root(&path_remove);
                            app.save_state();
                            cx.notify();
                        }),
                    ),
            );

        let mut wrapper = div().flex().flex_col().child(node);
        for child in children {
            wrapper = wrapper.child(child);
        }

        Some(wrapper.into_any_element())
    }

    fn collect_filtered_children(
        &self,
        dir: &PathBuf,
        query_lower: &str,
        depth: usize,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        const MAX_DEPTH: usize = 8;

        if depth >= MAX_DEPTH {
            return Vec::new();
        }

        let (dirs, files) = FileTree::list_children(dir);
        let mut elements = Vec::new();

        for child_dir in &dirs {
            let dir_name = child_dir
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();

            if dir_name.starts_with('.') || SKIP_DIRS.contains(&dir_name.as_str()) {
                continue;
            }

            let dir_matches =
                self.file_tree.show_folders && dir_name.to_lowercase().contains(query_lower);

            let grandchildren =
                self.collect_filtered_children(child_dir, query_lower, depth + 1, cx);

            if dir_matches || !grandchildren.is_empty() {
                elements.push(self.render_filtered_dir_node(
                    child_dir,
                    depth + 1,
                    dir_matches,
                    grandchildren,
                    cx,
                ));
            }
        }

        if self.file_tree.show_files {
            for file in &files {
                let file_name = file
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();

                if file_name.to_lowercase().contains(query_lower) {
                    elements.push(self.render_filtered_file_node(file, depth + 1, cx));
                }
            }
        }

        elements
    }

    fn render_filtered_dir_node(
        &self,
        path: &PathBuf,
        depth: usize,
        name_matches: bool,
        children: Vec<AnyElement>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string_lossy().to_string());

        let label = format!("\u{25be} {name}");
        let path_open = path.clone();
        let path_inject = path.clone();
        let path_editor = path.clone();

        let inject_id: SharedString = format!("search-inject-{}", path.display()).into();
        let edit_id: SharedString = format!("search-edit-{}", path.display()).into();

        let text_color = if name_matches {
            rgb(theme::YELLOW)
        } else {
            rgb(theme::TEXT)
        };

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
                    }
                }),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .overflow_x_hidden()
                    .child(label)
                    .text_color(text_color)
                    .text_size(px(13.0)),
            )
            .child(
                div()
                    .id(inject_id)
                    .child("@")
                    .text_size(px(11.0))
                    .text_color(rgb(theme::OVERLAY0))
                    .px(px(3.0))
                    .cursor_pointer()
                    .hover(|s| s.text_color(rgb(theme::GREEN)))
                    .tooltip(build_tooltip("Inject folder path"))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |app, _ev: &MouseDownEvent, _window, _cx| {
                            let text = format!("@{} ", path_inject.to_string_lossy());
                            app.inject_text_to_focused(&text);
                        }),
                    ),
            )
            .child(
                div()
                    .id(edit_id)
                    .child("\u{270e}")
                    .text_size(px(11.0))
                    .text_color(rgb(theme::OVERLAY0))
                    .px(px(3.0))
                    .cursor_pointer()
                    .hover(|s| s.text_color(rgb(theme::BLUE)))
                    .tooltip(build_tooltip("Open in editor"))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |app, _ev: &MouseDownEvent, _window, _cx| {
                            app.open_in_editor(&path_editor);
                        }),
                    ),
            );

        let mut wrapper = div().flex().flex_col().child(node);
        for child in children {
            wrapper = wrapper.child(child);
        }

        wrapper.into_any_element()
    }

    fn render_filtered_file_node(
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
        let path_editor = path.clone();

        let inject_id: SharedString = format!("search-finject-{}", path.display()).into();
        let edit_id: SharedString = format!("search-fedit-{}", path.display()).into();

        div()
            .px(px(8.0 + depth as f32 * 16.0))
            .py(px(1.0))
            .w_full()
            .flex()
            .flex_row()
            .items_center()
            .hover(|s| s.bg(rgb(theme::SURFACE0)))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .overflow_x_hidden()
                    .child(format!("  {name}"))
                    .text_color(rgb(theme::YELLOW))
                    .text_size(px(12.0)),
            )
            .child(
                div()
                    .id(inject_id)
                    .child("@")
                    .text_size(px(11.0))
                    .text_color(rgb(theme::OVERLAY0))
                    .px(px(3.0))
                    .cursor_pointer()
                    .hover(|s| s.text_color(rgb(theme::GREEN)))
                    .tooltip(build_tooltip("Inject file path"))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |app, _ev: &MouseDownEvent, _window, _cx| {
                            let text = format!("@{} ", path_inject.to_string_lossy());
                            app.inject_text_to_focused(&text);
                        }),
                    ),
            )
            .child(
                div()
                    .id(edit_id)
                    .child("\u{270e}")
                    .text_size(px(11.0))
                    .text_color(rgb(theme::OVERLAY0))
                    .px(px(3.0))
                    .cursor_pointer()
                    .hover(|s| s.text_color(rgb(theme::BLUE)))
                    .tooltip(build_tooltip("Open in editor"))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |app, _ev: &MouseDownEvent, _window, _cx| {
                            app.open_in_editor(&path_editor);
                        }),
                    ),
            )
            .into_any_element()
    }
}
