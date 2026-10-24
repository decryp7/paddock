/** The longest window title the layer sets (Rust: `paddock_window::close::MAX_TITLE`). */
export const MAX_TITLE = 200;

/**
 * A window's title as "<view> - <app>": the panel's name and the app or host window it belongs to. One line, at most
 * `MAX_TITLE` characters, without control characters; the view alone when the app is unknown. Rust sets the same title
 * when it creates the window; the UI re-applies it (`pinDocumentTitle`) because a page may rewrite `document.title`.
 */
export function windowTitle(view: string, app: string | null | undefined): string {
  const clean = (s: string) => s.replace(/\p{Cc}/gu, ' ').replace(/\s+/g, ' ').trim();
  const v = clean(view);
  const a = clean(app ?? '');
  return (a ? `${v} - ${a}` : v).slice(0, MAX_TITLE);
}
