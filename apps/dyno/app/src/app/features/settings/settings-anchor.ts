const KEY = 'dyno.settings.anchor';

/**
 * Hands a section of Dyno's Settings page from the window that opens Settings to the Settings window, which may be another
 * webview of the same origin: "Customize widgets…" asks for Menu bar > Popup widgets. The page takes it once when it has loaded.
 */
export function rememberSettingsAnchor(anchor: string | null): void {
  try {
    if (anchor === null) localStorage.removeItem(KEY);
    else localStorage.setItem(KEY, anchor);
  } catch {
    // Storage blocked: Settings opens at the top.
  }
}

/** The section waiting to be shown, once. */
export function takeSettingsAnchor(): string | null {
  try {
    const anchor = localStorage.getItem(KEY);
    localStorage.removeItem(KEY);
    return anchor;
  } catch {
    return null;
  }
}
