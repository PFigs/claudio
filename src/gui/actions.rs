use gpui::*;

actions!(
    claudio,
    [
        NewSession,
        KillFocusedSession,
        CycleFocus,
        ToggleMode,
        MinimizeSession,
        ToggleFileTree,
        AddFolder,
        Quit,
        StopDaemon,
    ]
);

pub fn register(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("ctrl-n", NewSession, Some("ClaudioApp")),
        KeyBinding::new("ctrl-w", KillFocusedSession, Some("ClaudioApp")),
        KeyBinding::new("ctrl-tab", CycleFocus, Some("ClaudioApp")),
        KeyBinding::new("ctrl-m", ToggleMode, Some("ClaudioApp")),
        KeyBinding::new("ctrl-shift-m", MinimizeSession, Some("ClaudioApp")),
        KeyBinding::new("ctrl-b", ToggleFileTree, Some("ClaudioApp")),
        KeyBinding::new("ctrl-q", Quit, Some("ClaudioApp")),
        KeyBinding::new("ctrl-shift-q", StopDaemon, Some("ClaudioApp")),
    ]);
}
