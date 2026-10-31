/**
 * Brings the element `id` to the top of its scroll area and keeps it there while the content around it settles (widgets above it
 * loading, a list growing) by scrolling again on each resize, until the person scrolls, presses a key or clicks: from then on the page
 * is theirs. Returns what stops it. Nothing happens when the element is not there.
 */
export function scrollToSection(id: string, root: Document = document): () => void {
  const el = root.getElementById(id);
  if (!el) return () => undefined;
  const go = (): void => {
    if (typeof el.scrollIntoView === 'function') el.scrollIntoView({ block: 'start' });
  };
  el.setAttribute('tabindex', '-1');
  go();
  el.focus({ preventScroll: true });
  let observer: ResizeObserver | null = null;
  const events = ['wheel', 'touchstart', 'keydown', 'pointerdown'] as const;
  const stop = (): void => {
    observer?.disconnect();
    for (const e of events) root.removeEventListener(e, stop, true);
  };
  if (typeof ResizeObserver === 'function') {
    observer = new ResizeObserver(go);
    observer.observe(el.parentElement ?? el);
    observer.observe(el.closest('main, [data-testid="settings-content"], .pd-content') ?? root.body);
  }
  for (const e of events) root.addEventListener(e, stop, { capture: true, passive: true });
  return stop;
}
