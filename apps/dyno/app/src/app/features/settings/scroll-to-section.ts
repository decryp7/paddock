import { afterWindowActive, type ActiveWindow } from '@decryp7/paddock-ui/shell/dialogs';

/** Where `scrollToSection` works, and where it reports (specs pass fakes). */
export interface ScrollToSectionOptions {
  readonly root?: Document;
  /** The window that must be active before the scroll (default: the root's). */
  readonly view?: ActiveWindow | null;
  /** A line for the app's log (rule 56); none: not logged. */
  readonly log?: (message: string) => void;
}

/**
 * Brings the element `id` to the top of its scroll area and keeps it there while the content around it settles (widgets above it
 * loading, a list growing) by scrolling again on each resize, until the person scrolls, presses a key or clicks: from then on the page
 * is theirs. It starts once the window is active (`afterWindowActive`: WebKit left the page blank when an open Settings window,
 * re-targeted from the tray, scrolled before it came forward). Returns what stops it. Nothing happens when the element is not there.
 */
export function scrollToSection(id: string, options: ScrollToSectionOptions = {}): () => void {
  const root = options.root ?? document;
  const view = options.view === undefined ? (root.defaultView as ActiveWindow | null) : options.view;
  const log = options.log ?? (() => undefined);
  const el = root.getElementById(id);
  if (!el) {
    log(`Settings: section ${id} is not on this page; not scrolled`);
    return () => undefined;
  }
  const go = (): void => {
    if (typeof el.scrollIntoView === 'function') el.scrollIntoView({ block: 'start' });
  };
  let observer: ResizeObserver | null = null;
  const events = ['wheel', 'touchstart', 'keydown', 'pointerdown'] as const;
  const stop = (): void => {
    cancelStart();
    observer?.disconnect();
    for (const e of events) root.removeEventListener(e, stop, true);
  };
  const waited = view ? !view.document.hasFocus() : false;
  const cancelStart = afterWindowActive(() => {
    el.setAttribute('tabindex', '-1');
    go();
    el.focus({ preventScroll: true });
    const scroller = el.closest<HTMLElement>('main, [data-testid="settings-content"], .pd-content');
    log(`Settings: scrolled to section ${id}${waited ? ' after the window came forward' : ''} (scrollTop ${Math.round(scroller?.scrollTop ?? 0)})`);
    if (typeof ResizeObserver === 'function') {
      observer = new ResizeObserver(go);
      observer.observe(el.parentElement ?? el);
      observer.observe(scroller ?? root.body);
    }
    for (const e of events) root.addEventListener(e, stop, { capture: true, passive: true });
  }, view);
  return stop;
}
