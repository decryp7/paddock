/** What `afterWindowActive` needs of a window (the page's `window`; a fake in specs). */
export interface ActiveWindow {
  readonly document: Pick<Document, 'hasFocus'>;
  addEventListener(type: 'focus', listener: () => void, options?: AddEventListenerOptions): void;
  removeEventListener(type: 'focus', listener: () => void): void;
  requestAnimationFrame(callback: FrameRequestCallback): number;
  cancelAnimationFrame(handle: number): void;
}

/**
 * Runs `fn` once this window is the active one and has drawn two frames since: at once plus two frames when it has focus already,
 * otherwise two frames after its next `focus` event. Returns what cancels it (then `fn` never runs).
 *
 * For programmatic scrolling in a window the window system is bringing forward (a dialog window re-targeted from the tray). WKWebView
 * (macOS 26, the Dyno 1.0 VM, 2026-11-01) left a freshly mounted page blank, although laid out, hit-testable and scrolled, whenever it
 * was scrolled before the window became key; scrolled in the same task as the `focus` event it was still blank about half the time;
 * two frames after the `focus` event it painted every time. Waiting is not a timeout: a window that never gets focus never scrolls.
 */
export function afterWindowActive(fn: () => void, view: ActiveWindow | null = typeof window === 'undefined' ? null : window): () => void {
  if (!view) {
    fn();
    return () => undefined;
  }
  let cancelled = false;
  let frame: number | null = null;
  const twoFrames = (): void => {
    frame = view.requestAnimationFrame(() => {
      frame = view.requestAnimationFrame(() => {
        frame = null;
        if (!cancelled) fn();
      });
    });
  };
  const onFocus = (): void => twoFrames();
  if (view.document.hasFocus()) twoFrames();
  else view.addEventListener('focus', onFocus, { once: true });
  return () => {
    cancelled = true;
    view.removeEventListener('focus', onFocus);
    if (frame !== null) view.cancelAnimationFrame(frame);
  };
}
