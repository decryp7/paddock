/**
 * Copies text to the clipboard. Returns whether it worked: a clipboard that is not available (a webview without
 * permission, an insecure origin) answers false instead of throwing, so the caller can say "select it and press
 * Ctrl/Cmd+C" rather than fail.
 */
export async function copyText(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    return false;
  }
}
