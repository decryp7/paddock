// Ids and keys of the window commands (no imports: the registration file reads them).

export const NEW_WINDOW_KEYBINDING = 'Mod+Shift+N';
/** Debrief ▸ Quit Debrief on macOS (on Windows and Linux, Ctrl+Q and Alt+F4 stay per window: File ▸ Exit has none). */
export const QUIT_KEYBINDING = 'Mod+Q';
/** The quit command (the native macOS menu puts it last in the app menu; see `menu.rs`). */
export const QUIT_COMMAND = 'app.quit';
