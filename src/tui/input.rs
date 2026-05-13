// Input handling - key binding definitions
// Actual handling is in TuiApp::handle_key

#[allow(dead_code)]
pub struct KeyBinding {
    pub key: &'static str,
    pub description: &'static str,
}

#[allow(dead_code)]
pub const KEY_BINDINGS: &[KeyBinding] = &[
    KeyBinding { key: "Enter", description: "Send message" },
    KeyBinding { key: "Shift+Enter", description: "Insert newline" },
    KeyBinding { key: "Ctrl+C", description: "Interrupt/Exit" },
    KeyBinding { key: "Ctrl+D", description: "Exit TUI" },
    KeyBinding { key: "Tab", description: "Cycle panel focus" },
    KeyBinding { key: "Ctrl+L", description: "Clear conversation" },
    KeyBinding { key: "Ctrl+S", description: "Save session" },
];
